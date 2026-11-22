//! Subscriptions: a channel or playlist (YouTube and the other sites yt-dlp lists) checked every
//! so often; its new videos are queued by themselves, at the quality chosen. What was there
//! when subscribing is marked as seen (only the latest few are fetched, if asked), so a
//! channel's whole history never floods the queue.

use std::sync::Arc;
use std::time::Duration;

use rusqlite::params;
use serde::Serialize;

use crate::manager::Manager;
use crate::video;

/// Entries looked at on each check (a channel lists its newest first).
const LOOK_AT: u64 = 30;

#[derive(Clone, Debug, Serialize)]
pub struct Sub {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub quality: String,
    pub every_min: i64,
    pub last_check: Option<i64>,
    pub last_error: Option<String>,
    pub enabled: bool,
}

struct Listed {
    title: Option<String>,
    entries: Vec<(String, String)>,
}

/// The newest entries of `url` (blocking: yt-dlp runs).
fn list(exe: &std::path::Path, url: &str) -> Result<Listed, String> {
    let listing = udm_tools::playlist::Listing::start(exe, url, &Default::default(), 1, Some(LOOK_AT))
        .map_err(|e| e.to_string())?;
    let started = std::time::Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(300));
        let p = listing.progress();
        if p.done || started.elapsed() > Duration::from_secs(120) {
            listing.cancel();
            if let Some(e) = p.error.filter(|_| p.listed == 0) {
                return Err(e);
            }
            let entries = listing
                .entries_from(0)
                .into_iter()
                .filter(|e| !e.unavailable)
                .map(|e| (e.url, e.title))
                .collect();
            return Ok(Listed { title: p.title, entries });
        }
    }
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Sub> {
    Ok(Sub {
        id: r.get("id")?,
        url: r.get("url")?,
        title: r.get("title")?,
        quality: r.get("quality")?,
        every_min: r.get("every_min")?,
        last_check: r.get("last_check")?,
        last_error: r.get("last_error")?,
        enabled: r.get::<_, i64>("enabled")? != 0,
    })
}

pub fn all(m: &Manager) -> Vec<Sub> {
    m.with_db(|db| {
        let mut st = db.prepare("SELECT * FROM subscription ORDER BY id")?;
        st.query_map([], row)?.collect()
    })
    .unwrap_or_default()
}

/// Follow `url`: its current videos count as seen, except the `latest` newest, which are queued.
pub async fn add(m: &Arc<Manager>, url: &str, quality: &str, every_min: i64, latest: usize) -> Result<Sub, String> {
    let exe = video::ytdlp().await?;
    let u = url.trim().to_owned();
    let listed = tauri::async_runtime::spawn_blocking(move || list(&exe, &u)).await.map_err(|e| e.to_string())??;
    if listed.entries.is_empty() {
        return Err("That page lists no videos (is it a channel or a playlist?)".into());
    }
    let title = listed.title.clone().unwrap_or_else(|| url.to_owned());
    let every = every_min.clamp(15, 7 * 24 * 60);
    let id = m
        .with_db(|db| {
            db.execute(
                "INSERT INTO subscription (url, title, quality, every_min, last_check, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![url.trim(), title, quality, every, crate::store::now()],
            )?;
            let id = db.last_insert_rowid();
            for (v, _) in &listed.entries {
                db.execute("INSERT OR IGNORE INTO sub_seen (sub_id, video_url) VALUES (?1, ?2)", params![id, v])?;
            }
            Ok(id)
        })
        .map_err(|e| if e.to_string().contains("UNIQUE") { "Already subscribed to this page.".to_owned() } else { e.to_string() })?;
    let opts = video::quality_opts(quality);
    for (v, t) in listed.entries.iter().take(latest) {
        let _ = m.add_video(v, t, &opts, Some(m.default_dir("video")), None);
    }
    all(m).into_iter().find(|s| s.id == id).ok_or_else(|| "saved but not found".into())
}

pub fn remove(m: &Manager, id: i64) -> Result<(), String> {
    m.with_db(|db| {
        db.execute("DELETE FROM sub_seen WHERE sub_id = ?1", [id])?;
        db.execute("DELETE FROM subscription WHERE id = ?1", [id])?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// Check one subscription now: queue its new videos. Returns how many.
pub async fn check(m: &Arc<Manager>, sub: &Sub) -> Result<usize, String> {
    let exe = video::ytdlp().await?;
    let u = sub.url.clone();
    let result = tauri::async_runtime::spawn_blocking(move || list(&exe, &u)).await.map_err(|e| e.to_string())?;
    let now = crate::store::now();
    let listed = match result {
        Ok(l) => l,
        Err(e) => {
            let _ = m.with_db(|db| {
                db.execute("UPDATE subscription SET last_check = ?2, last_error = ?3 WHERE id = ?1", params![sub.id, now, e])
                    .map(|_| ())
            });
            return Err(e);
        }
    };
    let fresh: Vec<(String, String)> = m
        .with_db(|db| {
            let mut out = Vec::new();
            for (v, t) in &listed.entries {
                let new = db.execute("INSERT OR IGNORE INTO sub_seen (sub_id, video_url) VALUES (?1, ?2)", params![sub.id, v])? > 0;
                if new {
                    out.push((v.clone(), t.clone()));
                }
            }
            db.execute("UPDATE subscription SET last_check = ?2, last_error = NULL WHERE id = ?1", params![sub.id, now])?;
            Ok(out)
        })
        .unwrap_or_default();
    let opts = video::quality_opts(&sub.quality);
    for (v, t) in &fresh {
        let _ = m.add_video(v, t, &opts, Some(m.default_dir("video")), None);
    }
    if !fresh.is_empty() {
        m.notify_pub(
            "New videos",
            &format!("{} new from {}", fresh.len(), sub.title),
        );
    }
    Ok(fresh.len())
}

/// Check each subscription when its time has come.
pub async fn run(m: Arc<Manager>) {
    let mut every = tokio::time::interval(Duration::from_secs(60));
    loop {
        every.tick().await;
        let now = crate::store::now();
        for s in all(&m).into_iter().filter(|s| s.enabled) {
            let due = s.last_check.is_none_or(|t| now - t >= s.every_min * 60);
            if due {
                let _ = check(&m, &s).await;
            }
        }
    }
}
