//! Ultimate Download Manager's desktop app: the Tauri shell around the download crates in `crates/`.

mod accounts;
mod autostart;
mod bridge;
mod clipboard;
mod crash;
mod cnl;
mod creds;
mod grabber;
mod hosts;
mod hotkey;
mod winint;
mod newdl;
mod check;
mod manager;
mod postprocess;
mod preview;
mod rss;
mod scheduler;
mod sessions;
mod settings;
mod store;
mod subscriptions;
mod torrent;
mod update;
mod video;

use std::path::PathBuf;
use std::sync::Arc;

use manager::{Manager, RowView};
use serde::Serialize;
use settings::Settings;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager as _, State, WindowEvent};

type Mgr<'a> = State<'a, Arc<Manager>>;

#[derive(Serialize)]
struct Status {
    version: &'static str,
    /// When this build was made (Unix seconds).
    built: u64,
    data_dir: Option<String>,
}

#[tauri::command]
fn core_status() -> Status {
    Status {
        version: env!("CARGO_PKG_VERSION"),
        built: env!("UDM_BUILT").parse().unwrap_or(0),
        data_dir: udm_ipc::data_dir().map(|d| d.display().to_string()),
    }
}

#[derive(Serialize)]
struct LinkInfo {
    supported: bool,
    kind: &'static str,
    host: Option<String>,
    name: Option<String>,
    size: Option<u64>,
    category: Option<&'static str>,
}

/// A quick look at an address before adding it: its kind and, for web and FTP addresses, the
/// file's name and size as the server tells them.
#[tauri::command]
async fn probe_link(url: String) -> Result<LinkInfo, String> {
    let text = url.trim();
    if text.starts_with("magnet:?") {
        return Ok(LinkInfo {
            supported: false,
            kind: "magnet link: torrents arrive in Phase 4",
            host: None,
            name: None,
            size: None,
            category: Some("torrents"),
        });
    }
    let parsed = url::Url::parse(text).map_err(|e| format!("That isn't a web address ({e})"))?;
    let host = parsed.host_str().map(str::to_owned);
    if !udm_engine::supported(&parsed) {
        return Ok(LinkInfo {
            supported: false,
            kind: "an address type Ultimate Download Manager doesn't download yet",
            host,
            name: None,
            size: None,
            category: None,
        });
    }
    let kind = if udm_engine::ftp::is_ftp(&parsed) { "FTP file" } else { "web file" };
    let opts = udm_engine::Options {
        user_agent: manager::UDM_AGENT.to_owned(),
        agent_fallback: Some(manager::BROWSER_AGENT.to_owned()),
        ..udm_engine::Options::default()
    };
    match udm_engine::probe(parsed, reqwest::header::HeaderMap::new(), &opts).await {
        Ok(info) => Ok(LinkInfo {
            supported: true,
            kind,
            host,
            category: Some(store::category_of(&info.file_name)),
            name: Some(info.file_name),
            size: info.size,
        }),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn list_downloads(m: Mgr) -> Result<Vec<RowView>, String> {
    m.list()
}

#[tauri::command]
fn add_download(
    m: Mgr,
    url: String,
    dir: Option<String>,
    name: Option<String>,
    referer: Option<String>,
    start: Option<bool>,
) -> Result<i64, String> {
    m.add(
        &url,
        dir.filter(|d| !d.trim().is_empty()).map(PathBuf::from),
        name,
        referer,
        start.unwrap_or(true),
        None,
    )
}

#[tauri::command]
fn pause_downloads(m: Mgr, ids: Vec<i64>) -> Result<(), String> {
    m.pause(&ids)
}

#[tauri::command]
fn resume_downloads(m: Mgr, ids: Vec<i64>) -> Result<(), String> {
    m.resume(&ids)
}

#[tauri::command]
fn remove_downloads(m: Mgr, ids: Vec<i64>, delete_files: bool) -> Result<(), String> {
    m.remove(&ids, delete_files)
}

#[tauri::command]
fn open_download(m: Mgr, id: i64) -> Result<(), String> {
    let item = m.item(id)?;
    let file = item.file.ok_or("This download has no file yet.")?;
    tauri_plugin_opener::open_path(file, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn reveal_download(m: Mgr, id: i64) -> Result<(), String> {
    let item = m.item(id)?;
    match item.file {
        Some(f) if std::path::Path::new(&f).exists() => {
            tauri_plugin_opener::reveal_item_in_dir(f).map_err(|e| e.to_string())
        }
        Some(f) => {
            let part = udm_engine::naming::part_path(std::path::Path::new(&f));
            if part.exists() {
                tauri_plugin_opener::reveal_item_in_dir(part).map_err(|e| e.to_string())
            } else {
                tauri_plugin_opener::open_path(item.dir, None::<&str>).map_err(|e| e.to_string())
            }
        }
        None => tauri_plugin_opener::open_path(item.dir, None::<&str>).map_err(|e| e.to_string()),
    }
}

/// The links in `text`, sorted and checked online (the link grabber).
#[tauri::command]
async fn grab_links(m: State<'_, Arc<Manager>>, text: String) -> Result<Vec<grabber::Grabbed>, String> {
    let links = grabber::links_in(&text);
    if links.is_empty() {
        return Err("No links found. Paste text with http(s), ftp or magnet addresses.".into());
    }
    Ok(grabber::check(m.inner(), links).await)
}

#[derive(serde::Deserialize)]
struct Pick {
    url: String,
    kind: String,
    name: String,
}

/// Queue the grabber's chosen links: files and videos (best quality); torrents are left out
/// here (each needs its files chosen: the torrent dialog).
#[tauri::command]
fn add_grabbed(m: Mgr, picks: Vec<Pick>, dir: Option<String>, start: bool) -> Result<usize, String> {
    let dir = dir.filter(|d| !d.trim().is_empty()).map(PathBuf::from);
    let mut added = 0;
    // The volumes of one split archive (x.part1.rar, x.part2.rar…) go into one folder named
    // after it, where they're unpacked once the last one arrives.
    let mut parts: std::collections::HashMap<String, usize> = Default::default();
    for p in picks.iter().filter(|p| p.kind == "file") {
        if let Some(v) = udm_extract::volume_key(p.name.trim()) {
            *parts.entry(v.base.to_lowercase()).or_default() += 1;
        }
    }
    let folder_of = |p: &Pick| -> Option<PathBuf> {
        let v = udm_extract::volume_key(p.name.trim())?;
        if parts.get(&v.base.to_lowercase()).copied().unwrap_or(0) < 2 {
            return dir.clone();
        }
        let sub = udm_engine::naming::sanitize(v.base.trim().trim_end_matches('.'));
        if sub.is_empty() {
            return dir.clone();
        }
        let base = dir.clone().unwrap_or_else(|| m.default_dir("archives"));
        let f = base.join(sub);
        let _ = std::fs::create_dir_all(&f);
        Some(f)
    };
    // Files of the same name from several sites are one download, the others its mirrors.
    let mut first_of: std::collections::HashMap<String, i64> = Default::default();
    for p in &picks {
        let key = p.name.trim().to_ascii_lowercase();
        if p.kind == "file" && key.contains('.') {
            if let Some(&id) = first_of.get(&key) {
                let _ = m.add_mirrors(id, std::slice::from_ref(&p.url));
                continue;
            }
            let into = folder_of(p).or_else(|| dir.clone());
            if let Ok(id) = m.add(&p.url, into, None, None, start, None) {
                first_of.insert(key, id);
                added += 1;
            }
            continue;
        }
        let ok = match p.kind.as_str() {
            "video" => {
                let opts = video::VideoOpts { selector: "bv*+ba/b".into(), ext: "mp4".into(), ..Default::default() };
                let title = if p.name.trim().is_empty() { "video" } else { p.name.as_str() };
                m.add_video(&p.url, title, &opts, dir.clone(), None).is_ok()
            }
            "file" => m.add(&p.url, dir.clone(), None, None, start, None).is_ok(),
            _ => false,
        };
        if ok {
            added += 1;
        }
    }
    Ok(added)
}

/// A magnet link's or a .torrent file's file list.
#[tauri::command]
async fn torrent_meta(m: State<'_, Arc<Manager>>, source: String) -> Result<torrent::MetaView, String> {
    torrent::meta(m.inner(), &source).await
}

/// What's inside a download, for the details panel: a torrent's files, peers, trackers and
/// pieces; a file's parts and connections while it runs.
#[tauri::command]
async fn download_details(m: State<'_, Arc<Manager>>, id: i64) -> Result<serde_json::Value, String> {
    let item = m.item(id)?;
    if item.kind == "torrent" {
        let d = torrent::details(m.inner(), &item).await?;
        let seq = torrent::sequential(m.inner(), &item).await;
        return serde_json::to_value(serde_json::json!({ "kind": "torrent", "torrent": d, "sequential": seq, "item": item_extras(&item) }))
            .map_err(|e| e.to_string());
    }
    Ok(serde_json::json!({ "kind": "file", "file": m.file_details(id), "item": item_extras(&item) }))
}

/// A row's own settings, for the details panel.
fn item_extras(item: &store::Item) -> serde_json::Value {
    let (seed_ratio, seed_minutes) = if item.kind == "torrent" { torrent::seeding_of(item) } else { (None, None) };
    serde_json::json!({
        "seed_ratio": seed_ratio,
        "seed_minutes": seed_minutes,
        "mirrors": manager::mirrors_of(item),
        "rule": item
            .options
            .as_deref()
            .filter(|_| item.kind == "file")
            .and_then(|o| serde_json::from_str::<serde_json::Value>(o).ok())
            .and_then(|v| v.get("rule").and_then(|r| r.as_str()).map(str::to_owned)),
        "limit_kib": item.limit_kib,
        "priority": item.priority,
        "checksum": item.checksum,
        "password": item.password.is_some(),
    })
}

/// Download only these files of a torrent from now on.
#[tauri::command]
async fn torrent_choose(m: State<'_, Arc<Manager>>, id: i64, files: Vec<usize>) -> Result<(), String> {
    let item = m.item(id)?;
    torrent::choose(m.inner(), &item, files).await
}

/// Open a file host's page in a browser window to get its file (you solve the wait/captcha).
#[tauri::command]
async fn assist_download(app: AppHandle, id: i64) -> Result<(), String> {
    check::assist(&app, id)
}

/// Download an image gallery (gallery-dl): at most `limit` files (0: all).
#[tauri::command]
fn add_gallery(m: Mgr, url: String, dir: Option<String>, limit: u64) -> Result<i64, String> {
    m.inner().add_gallery(&url, dir.filter(|d| !d.trim().is_empty()).map(PathBuf::from), limit)
}

#[tauri::command]
fn asked_download(m: Mgr, token: String) -> Result<newdl::AskView, String> {
    newdl::view(&m, &token)
}

#[tauri::command]
fn confirm_download(
    app: AppHandle,
    m: Mgr,
    token: String,
    path: String,
    category: String,
    start: bool,
    skip_type: bool,
    replace: Option<bool>,
) -> Result<i64, String> {
    newdl::confirm(&app, m.inner(), &token, &path, &category, start, skip_type, replace.unwrap_or(false))
}

#[tauri::command]
async fn torrent_seeding(m: State<'_, Arc<Manager>>, id: i64, ratio: Option<f64>, minutes: Option<u64>) -> Result<(), String> {
    let item = m.item(id)?;
    torrent::set_seeding(m.inner(), &item, ratio, minutes)
}

#[tauri::command]
async fn create_torrent(
    m: State<'_, Arc<Manager>>,
    source: String,
    trackers: String,
    out: String,
    seed: bool,
) -> Result<String, String> {
    torrent::create(m.inner(), std::path::Path::new(&source), &trackers, None, std::path::Path::new(&out), seed).await
}

#[tauri::command]
async fn torrent_port_check(m: State<'_, Arc<Manager>>) -> Result<serde_json::Value, String> {
    torrent::port_check(m.inner()).await
}

#[tauri::command]
fn clear_list(m: Mgr, which: String, delete_files: bool) -> Result<usize, String> {
    m.inner().clear_list(&which, delete_files)
}

#[tauri::command]
fn redownload(m: Mgr, ids: Vec<i64>) -> Result<(), String> {
    m.inner().redownload(&ids)
}

#[tauri::command]
fn open_with(m: Mgr, id: i64) -> Result<(), String> {
    m.open_with(id)
}

#[tauri::command]
fn set_queue(m: Mgr, ids: Vec<i64>, queue: String) -> Result<(), String> {
    m.inner().set_queue(&ids, &queue)
}

#[tauri::command]
fn set_tags(m: Mgr, ids: Vec<i64>, tags: String) -> Result<(), String> {
    m.set_tags(&ids, &tags)
}

#[tauri::command]
fn add_mirrors(m: Mgr, id: i64, urls: Vec<String>) -> Result<(), String> {
    m.add_mirrors(id, &urls)
}

#[tauri::command]
fn path_info(path: String, size: Option<u64>) -> newdl::PathInfo {
    newdl::path_info(&path, size)
}

/// One download's row (the progress window follows it).
#[tauri::command]
fn download_row(m: Mgr, id: i64) -> Result<RowView, String> {
    m.row(id)
}

/// The progress view in this window follows download `id` (no second window for it).
#[tauri::command]
fn watch_progress(window: tauri::Window, id: i64) {
    newdl::watch(id, window.label());
}

#[tauri::command]
async fn show_progress(app: AppHandle, id: i64) -> Result<(), String> {
    newdl::show_progress(&app, id)
}

#[tauri::command]
fn cancel_download(app: AppHandle, token: String) {
    newdl::cancel(&app, &token);
}

#[tauri::command]
fn set_item_limit(m: Mgr, ids: Vec<i64>, kib: u64) -> Result<(), String> {
    m.inner().set_item_limit(&ids, kib)
}

#[tauri::command]
fn move_in_queue(m: Mgr, ids: Vec<i64>, top: bool) -> Result<(), String> {
    m.inner().move_in_queue(&ids, top)
}

#[tauri::command]
fn nudge_download(m: Mgr, id: i64, up: bool) -> Result<(), String> {
    m.inner().nudge(id, up)
}

/// The checksum a download must match; for a finished one, whether it does (None: not checked yet).
#[tauri::command]
async fn set_checksum(m: State<'_, Arc<Manager>>, id: i64, text: String) -> Result<Option<bool>, String> {
    let m = m.inner().clone();
    tauri::async_runtime::spawn_blocking(move || m.set_checksum(id, &text)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn set_archive_password(m: Mgr, id: i64, password: String) -> Result<(), String> {
    m.set_password(id, &password)
}

#[tauri::command]
fn refresh_address(m: Mgr, id: i64, url: String) -> Result<(), String> {
    m.inner().refresh_address(id, &url)
}

#[tauri::command]
fn move_file(m: Mgr, id: i64, to: String) -> Result<String, String> {
    m.move_file(id, &to)
}

#[tauri::command]
fn convert_presets() -> Vec<(String, String)> {
    postprocess::PRESETS.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
}

/// Convert a finished download with FFmpeg; the new file's path.
#[tauri::command]
async fn convert_file(m: State<'_, Arc<Manager>>, id: i64, preset: String) -> Result<String, String> {
    let item = m.item(id)?;
    let file = item.file.ok_or("It has no file yet.")?;
    let out = tauri::async_runtime::spawn_blocking(move || postprocess::convert(std::path::Path::new(&file), &preset))
        .await
        .map_err(|e| e.to_string())??;
    Ok(out.to_string_lossy().into_owned())
}

/// A torrent's action: `recheck`, `reannounce`, `sequential-on`, `sequential-off`.
#[tauri::command]
async fn torrent_action(m: State<'_, Arc<Manager>>, id: i64, action: String) -> Result<(), String> {
    let item = m.item(id)?;
    match action.as_str() {
        "recheck" => torrent::recheck(m.inner(), &item).await,
        "reannounce" => torrent::reannounce(m.inner(), &item).await,
        "sequential-on" => torrent::set_sequential(m.inner(), &item, true).await,
        "sequential-off" => torrent::set_sequential(m.inner(), &item, false).await,
        other => Err(format!("Unknown torrent action {other}")),
    }
}

#[tauri::command]
async fn torrent_add_trackers(m: State<'_, Arc<Manager>>, id: i64, text: String) -> Result<usize, String> {
    let item = m.item(id)?;
    torrent::add_trackers(m.inner(), &item, &text).await
}

/// What's been downloaded: totals, per month, per type.
/// Read a feed now (Settings → Torrents → RSS → Check now): the titles added.
#[tauri::command]
async fn check_feed(m: State<'_, Arc<Manager>>, url: String) -> Result<Vec<String>, String> {
    let feed = m.settings().feeds.into_iter().find(|f| f.url == url).ok_or("Save the feed first.")?;
    rss::check(m.inner(), &feed).await
}

/// A feed's items now, to see what a filter would take.
#[tauri::command]
async fn preview_feed(url: String) -> Result<Vec<rss::FeedItem>, String> {
    rss::fetch(&url).await
}

#[tauri::command]
fn download_stats(m: Mgr) -> Result<serde_json::Value, String> {
    let rows = m.list()?;
    let done: Vec<_> = rows.iter().filter(|r| r.item.state == store::State::Done).collect();
    let mut months: std::collections::BTreeMap<String, (u64, u64)> = Default::default();
    let mut types: std::collections::BTreeMap<String, (u64, u64)> = Default::default();
    for r in &done {
        let at = r.item.finished_at.unwrap_or(r.item.created_at);
        let month = month_of(at);
        let size = r.item.size.unwrap_or(r.item.done);
        let e = months.entry(month).or_default();
        e.0 += 1;
        e.1 += size;
        let t = types.entry(r.item.category.clone()).or_default();
        t.0 += 1;
        t.1 += size;
    }
    Ok(serde_json::json!({
        "files": done.len(),
        "bytes": done.iter().map(|r| r.item.size.unwrap_or(r.item.done)).sum::<u64>(),
        "months": months.into_iter().rev().take(12).map(|(k, (n, b))| serde_json::json!({"month": k, "files": n, "bytes": b})).collect::<Vec<_>>(),
        "types": types.into_iter().map(|(k, (n, b))| serde_json::json!({"type": k, "files": n, "bytes": b})).collect::<Vec<_>>(),
    }))
}

/// `YYYY-MM` of a Unix time (UTC).
fn month_of(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}", if mo <= 2 { y + 1 } else { y }, mo)
}

/// Save the settings and the list (unfinished downloads) to a file.
#[tauri::command]
fn export_backup(m: Mgr, path: String) -> Result<usize, String> {
    let items: Vec<_> = m
        .list()?
        .into_iter()
        .filter(|r| r.item.state != store::State::Done && r.item.kind == "file")
        .map(|r| serde_json::json!({ "url": r.item.url, "name": r.item.name, "dir": r.item.dir }))
        .collect();
    let n = items.len();
    let v = serde_json::json!({ "app": "Ultimate Download Manager", "version": 1, "settings": m.settings(), "downloads": items });
    std::fs::write(&path, serde_json::to_vec_pretty(&v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(n)
}

/// Restore settings from a backup and add its downloads (paused).
#[tauri::command]
fn import_backup(m: Mgr, path: String) -> Result<usize, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| "That isn't an Ultimate Download Manager backup.".to_string())?;
    if let Some(s) = v.get("settings").cloned()
        && let Ok(s) = serde_json::from_value::<Settings>(s)
    {
        m.inner().set_settings(s)?;
    }
    let known = m.known_urls();
    let mut added = 0;
    for d in v.get("downloads").and_then(|d| d.as_array()).cloned().unwrap_or_default() {
        let Some(url) = d.get("url").and_then(|u| u.as_str()) else { continue };
        if known.contains(url) {
            continue;
        }
        let name = d.get("name").and_then(|n| n.as_str()).map(str::to_owned);
        let dir = d.get("dir").and_then(|n| n.as_str()).map(std::path::PathBuf::from);
        if m.inner().add(url, dir, name, None, false, None).is_ok() {
            added += 1;
        }
    }
    Ok(added)
}

#[tauri::command]
async fn add_torrent(
    m: State<'_, Arc<Manager>>,
    hash: String,
    files: Option<Vec<usize>>,
    dir: Option<String>,
) -> Result<i64, String> {
    let dir = dir.filter(|d| !d.trim().is_empty()).map(PathBuf::from);
    torrent::add(m.inner(), &hash, files, dir).await
}

/// Play what's there of a download in VLC.
#[tauri::command]
async fn preview(m: State<'_, Arc<Manager>>, id: i64) -> Result<(), String> {
    preview::open(m.inner(), id).await
}

#[tauri::command]
fn list_subs(m: Mgr) -> Vec<subscriptions::Sub> {
    subscriptions::all(&m)
}

/// Follow a channel or playlist; `latest` of its current videos are fetched now.
#[tauri::command]
async fn add_sub(
    m: State<'_, Arc<Manager>>,
    url: String,
    quality: String,
    every_min: i64,
    latest: usize,
) -> Result<subscriptions::Sub, String> {
    subscriptions::add(m.inner(), &url, &quality, every_min, latest).await
}

#[tauri::command]
fn remove_sub(m: Mgr, id: i64) -> Result<(), String> {
    subscriptions::remove(&m, id)
}

#[tauri::command]
async fn check_sub(m: State<'_, Arc<Manager>>, id: i64) -> Result<usize, String> {
    let sub = subscriptions::all(m.inner()).into_iter().find(|s| s.id == id).ok_or("No such subscription")?;
    subscriptions::check(m.inner(), &sub).await
}

/// Open a web page in the person's own browser (https only: a service's key page, help).
#[tauri::command]
async fn open_web(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("Only https pages open from here.".into());
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_accounts(m: Mgr) -> Vec<accounts::AccountView> {
    m.accounts.list()
}

/// Check a debrid service's API key and keep it (in Windows Credential Manager).
#[tauri::command]
async fn add_account(m: State<'_, Arc<Manager>>, service: String, key: String) -> Result<accounts::AccountView, String> {
    let kind = accounts::service_named(&service).ok_or("Unknown service")?;
    m.accounts.add(kind, &key).await
}

#[tauri::command]
fn remove_account(m: Mgr, id: u64) -> Result<(), String> {
    m.accounts.remove(id)
}

/// The computer's network adapters (for the VPN choice).
#[tauri::command]
fn list_adapters() -> Vec<scheduler::AdapterView> {
    scheduler::adapters()
}

#[tauri::command]
fn tools_status() -> video::ToolsStatus {
    video::status()
}

/// Install or update yt-dlp and FFmpeg.
#[tauri::command]
async fn install_tools() -> Result<video::ToolsStatus, String> {
    video::install().await
}

/// Look a page up with yt-dlp: one video's qualities and subtitles, or a playlist's entries.
#[tauri::command]
async fn video_lookup(m: State<'_, Arc<Manager>>, url: String) -> Result<serde_json::Value, String> {
    // The same page again within ten minutes: the answer at once (yt-dlp takes seconds).
    static CACHE: std::sync::Mutex<Vec<(String, std::time::Instant, serde_json::Value)>> = std::sync::Mutex::new(Vec::new());
    let url = url.trim().to_owned();
    {
        let mut c = CACHE.lock().unwrap();
        c.retain(|(_, at, _)| at.elapsed() < std::time::Duration::from_secs(600));
        if let Some((_, _, v)) = c.iter().find(|(u, _, _)| *u == url) {
            return Ok(v.clone());
        }
    }
    let exe = video::ytdlp().await?;
    let ctx = m.pending(&url);
    let browser = video::from_browser(None, &ctx);
    let page = url.clone();
    let view = tauri::async_runtime::spawn_blocking(move || video::lookup(&exe, &page, &browser))
        .await
        .map_err(|e| e.to_string())??;
    let v = serde_json::to_value(&view).map_err(|e| e.to_string())?;
    let mut c = CACHE.lock().unwrap();
    if c.len() >= 30 {
        c.remove(0);
    }
    c.push((url, std::time::Instant::now(), v.clone()));
    Ok(v)
}

#[tauri::command]
fn add_video(
    m: Mgr,
    page: String,
    title: String,
    opts: video::VideoOpts,
    dir: Option<String>,
) -> Result<i64, String> {
    m.add_video(&page, &title, &opts, dir.filter(|d| !d.trim().is_empty()).map(PathBuf::from), None)
}

#[derive(serde::Deserialize)]
struct PlaylistEntry {
    url: String,
    title: String,
}

/// Queue a playlist's chosen entries at one quality (`best`, `1080`, `720`, `mp3`).
#[tauri::command]
fn add_playlist(
    m: Mgr,
    entries: Vec<PlaylistEntry>,
    quality: String,
    dir: Option<String>,
) -> Result<usize, String> {
    let opts = video::quality_opts(&quality);
    let dir = dir.filter(|d| !d.trim().is_empty()).map(PathBuf::from);
    let mut added = 0;
    for e in &entries {
        if m.add_video(&e.url, &e.title, &opts, dir.clone(), None).is_ok() {
            added += 1;
        }
    }
    Ok(added)
}

/// Open the site-check window on `url` (a page whose downloads need a real browser first).
#[tauri::command]
async fn open_site_check(app: AppHandle, url: String) -> Result<(), String> {
    check::open(&app, url.trim())
}

/// Let a browser window download `id` itself (the fallback when a site's check won't carry over).
#[tauri::command]
async fn browser_download(app: AppHandle, id: i64) -> Result<(), String> {
    check::browser_download(&app, id)
}

#[derive(Serialize)]
struct ExtensionInfo {
    chromium: Option<String>,
    firefox: Option<String>,
    host: Result<String, String>,
}

/// Where the browser extension's folders are, and whether the browsers know the native host
/// (registered again: harmless, and it repairs a moved install).
#[tauri::command]
async fn extension_info(app: AppHandle) -> ExtensionInfo {
    let base = app.path().resource_dir().ok();
    let find = |name: &str| {
        base.as_ref()
            .map(|b| b.join(name))
            .filter(|p| p.join("manifest.json").is_file())
            .map(|p| p.display().to_string())
    };
    ExtensionInfo {
        chromium: find("extension-chromium"),
        firefox: find("extension-firefox"),
        host: bridge::register_host(),
    }
}

/// The folder crash reports are written to (made if it isn't there yet).
#[tauri::command]
fn open_crash_reports() -> Result<(), String> {
    let data = udm_ipc::data_dir().ok_or("no data folder")?;
    let dir = crash::folder(&data);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    tauri_plugin_opener::open_path(dir.to_string_lossy().into_owned(), None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
async fn open_folder(path: String) -> Result<(), String> {
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_settings(m: Mgr) -> Settings {
    m.settings()
}

#[tauri::command]
fn set_settings(app: AppHandle, m: Mgr, settings: Settings) -> Result<Settings, String> {
    let saved = m.set_settings(settings)?;
    apply_remote(&app, &saved);
    Ok(saved)
}

#[tauri::command]
async fn check_update(app: AppHandle, m: State<'_, Arc<Manager>>) -> Result<Option<update::UpdateInfo>, String> {
    update::check(&app, m.inner()).await
}

#[tauri::command]
async fn install_update(app: AppHandle, m: State<'_, Arc<Manager>>) -> Result<(), String> {
    update::install(&app, m.inner()).await
}

/// The phone remote (the reused `udm-remote` crate), managed as app state.
pub struct RemoteControl(pub udm_remote::Remote);

fn apply_remote(app: &AppHandle, s: &Settings) {
    if let Some(r) = app.try_state::<RemoteControl>() {
        r.0.apply(udm_remote::Config { on: s.remote_on, port: s.remote_port, ..Default::default() });
    }
}

#[derive(Serialize)]
struct RemoteInfo {
    status: String,
    urls: Vec<String>,
    fingerprint: Option<String>,
    devices: Vec<(String, String)>,
    code: Option<String>,
}

#[tauri::command]
fn remote_info(r: State<'_, RemoteControl>) -> RemoteInfo {
    let status = match r.0.status() {
        udm_remote::Status::Off => "off".to_owned(),
        udm_remote::Status::Listening(a) => format!("on ({} address{})", a.len(), if a.len() == 1 { "" } else { "es" }),
        udm_remote::Status::NoAddress => "on, but no home network address now".to_owned(),
        udm_remote::Status::PortInUse(p) => format!("port {p} is taken by another program"),
        udm_remote::Status::Failed(e) => format!("failed: {e}"),
    };
    RemoteInfo {
        status,
        urls: r.0.urls(),
        fingerprint: r.0.fingerprint(),
        devices: r.0.devices().into_iter().map(|d| (d.id.clone(), d.name.clone())).collect(),
        code: r.0.pairing().0.map(|(c, _)| c),
    }
}

/// A new pairing code for a phone (works once, for a few minutes).
#[tauri::command]
fn remote_pair(r: State<'_, RemoteControl>) -> String {
    r.0.start_pairing()
}

#[tauri::command]
fn remote_forget(r: State<'_, RemoteControl>, id: String) {
    r.0.remove(&id);
}

/// `udm.exe --add <url> [--add <url>…]`: queue each address (also when Ultimate Download Manager is already
/// running: the second launch hands its arguments to the first).
fn add_from_args(app: &AppHandle, args: &[String]) {
    let Some(m) = app.try_state::<Arc<Manager>>() else {
        return;
    };
    // A magnet link or a .torrent file Windows opened with Ultimate Download Manager: the torrent window. At the
    // first start the window isn't listening yet: a moment later.
    let opened: Vec<String> = args
        .iter()
        .skip(1)
        .filter(|a| {
            let l = a.to_ascii_lowercase();
            l.starts_with("magnet:?") || (l.ends_with(".torrent") && std::path::Path::new(a.as_str()).is_file())
        })
        .cloned()
        .collect();
    let grab_file = args.windows(2).find(|w| w[0] == "--grab-file").map(|w| w[1].clone());
    if !opened.is_empty() || grab_file.is_some() {
        let (app, m) = (app.clone(), m.inner().clone());
        tauri::async_runtime::spawn(async move {
            let ready = app.get_webview_window("main").is_some_and(|w| w.is_visible().unwrap_or(false));
            tokio::time::sleep(std::time::Duration::from_millis(if ready { 50 } else { 2500 })).await;
            for o in opened {
                m.show_torrent(std::path::Path::new(&o));
            }
            if let Some(f) = grab_file {
                match std::fs::read_to_string(&f) {
                    Ok(text) => {
                        show_main(&app);
                        let _ = app.emit(bridge::EV_SHOW, serde_json::json!({ "grab": text }).to_string());
                    }
                    Err(e) => eprintln!("udm: {f}: {e}"),
                }
            }
        });
    }
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--add-video"
            && let Some(page) = it.next()
        {
            // Best quality, named after the page's title once yt-dlp has looked it up.
            let opts = video::VideoOpts { selector: "bv*+ba/b".into(), ext: "mp4".into(), ..Default::default() };
            let (m, page) = (m.inner().clone(), page.clone());
            tauri::async_runtime::spawn(async move {
                let title = match video::ytdlp().await {
                    Ok(exe) => {
                        let p = page.clone();
                        tauri::async_runtime::spawn_blocking(move || {
                            match video::lookup(&exe, &p, &Default::default()) {
                                Ok(video::LookupView::Video { title, .. }) => title,
                                _ => "video".into(),
                            }
                        })
                        .await
                        .unwrap_or_else(|_| "video".into())
                    }
                    Err(_) => "video".into(),
                };
                if let Err(e) = m.add_video(&page, &title, &opts, None, None) {
                    eprintln!("udm: {page}: {e}");
                }
            });
        }
        if a == "--subscribe"
            && let Some(page) = it.next()
        {
            let (m, page) = (m.inner().clone(), page.clone());
            tauri::async_runtime::spawn(async move {
                if let Err(e) = subscriptions::add(&m, &page, "480", 360, 1).await {
                    eprintln!("udm: {page}: {e}");
                }
            });
        }
        if a == "--torrent"
            && let Some(src) = it.next()
        {
            let (m, src) = (m.inner().clone(), src.clone());
            tauri::async_runtime::spawn(async move {
                let added = match torrent::meta(&m, &src).await {
                    Ok(meta) => torrent::add(&m, &meta.hash, None, None).await.map(|_| ()),
                    Err(e) => Err(e),
                };
                if let Err(e) = added {
                    eprintln!("udm: {src}: {e}");
                }
            });
        }
        if a == "--site-check"
            && let Some(url) = it.next()
            && let Err(e) = check::open(app, url)
        {
            eprintln!("udm: {url}: {e}");
        }
        if a == "--add"
            && let Some(url) = it.next()
            && let Err(e) = m.add(url, None, None, None, true, None)
        {
            eprintln!("udm: {url}: {e}");
        }
    }
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Ultimate Download Manager", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::with_id("udm")
        .tooltip("Ultimate Download Manager")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id().as_ref() {
            "open" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// This process is a running copy's crash monitor: it has watched it to its end.
pub fn crash_monitor() -> bool {
    crash::monitor_if_asked()
}

pub fn run() {
    tauri::Builder::default()
        // A second launch shows the running window instead of starting another Ultimate Download Manager.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // `udm.exe --quit` (the installer, before it replaces the files): the running copy ends.
            if args.iter().any(|a| a == "--quit") {
                app.exit(0);
                return;
            }
            add_from_args(app, &args);
            if !args.iter().any(|a| a == "--background") {
                show_main(app);
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(hotkey::plugin())
        .setup(|app| {
            // `--quit` with no copy running: nothing to end.
            if std::env::args().any(|a| a == "--quit") {
                std::process::exit(0);
            }
            let data = udm_ipc::data_dir()
                .or_else(|| app.path().app_data_dir().ok())
                .ok_or("no data folder")?;
            // A log in <data>/logs/udm.log (2 MB, three kept): the engine's, the torrent
            // session's and the app's warnings and errors, and any panic (UDM_LOG=debug for more).
            if udm_crash::logs::init(&data, "udm") {
                let default_hook = std::panic::take_hook();
                std::panic::set_hook(Box::new(move |info| {
                    tracing::error!(target: "udm", "panic: {info}");
                    default_hook(info);
                }));
                tracing::info!(target: "udm", "Ultimate Download Manager {} started", env!("CARGO_PKG_VERSION"));
            }
            crash::start(&data);
            let crashed = !crash::unseen(&data).is_empty();
            udm_geo::prepare();
            let manager = Manager::new(app.handle().clone(), &data)?;
            if crashed {
                manager.notify_pub(
                    "Ultimate Download Manager closed unexpectedly",
                    "A crash report was saved. Settings → App → Crash reports opens it.",
                );
            }
            app.manage(manager);
            build_tray(app.handle())?;
            bridge::ensure_rules();
            let channel = bridge::start(app.handle().clone())?;
            // The phone remote: its commands take the same channel as the browser's.
            let rt = tauri::async_runtime::block_on(async { tokio::runtime::Handle::current() });
            let sink = udm_ipc::Sink::new(channel, Arc::new(|| {}));
            let texts: udm_remote::Texts = Arc::new(|_| udm_remote::PageTexts {
                lang: "en".into(),
                rtl: false,
                texts: Default::default(),
            });
            let remote = udm_remote::Remote::new(rt, data.join("remote"), sink, texts);
            app.manage(RemoteControl(remote));
            apply_remote(app.handle(), &app.state::<Arc<Manager>>().settings());
            update::check_at_start(app.handle().clone(), app.state::<Arc<Manager>>().inner().clone());
            clipboard::start(app.handle().clone());
            let cnl_app = app.handle().clone();
            tauri::async_runtime::spawn(async move { cnl::run(cnl_app).await });
            // The browsers find the native host through the registry: (re)register it.
            std::thread::spawn(|| {
                let _ = bridge::register_host();
            });
            let args: Vec<String> = std::env::args().collect();
            add_from_args(app.handle(), &args);
            if let Some(m) = app.try_state::<Arc<Manager>>() {
                hotkey::apply(app.handle(), &m.settings().hotkey);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let to_tray = window
                    .app_handle()
                    .try_state::<Arc<Manager>>()
                    .map(|m| m.settings().close_to_tray)
                    .unwrap_or(false);
                if to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            assist_download,
            add_gallery,
            asked_download,
            download_row,
            show_progress,
            watch_progress,
            path_info,
            add_mirrors,
            set_tags,
            set_queue,
            clear_list,
            redownload,
            open_with,
            torrent_seeding,
            create_torrent,
            torrent_port_check,
            confirm_download,
            cancel_download,
            set_item_limit,
            move_in_queue,
            nudge_download,
            set_checksum,
            set_archive_password,
            refresh_address,
            move_file,
            convert_presets,
            convert_file,
            torrent_action,
            torrent_add_trackers,
            download_stats,
            check_feed,
            preview_feed,
            export_backup,
            import_backup,
            download_details,
            torrent_choose,
            core_status,
            probe_link,
            list_downloads,
            add_download,
            pause_downloads,
            resume_downloads,
            remove_downloads,
            open_download,
            reveal_download,
            get_settings,
            set_settings,
            open_site_check,
            tools_status,
            list_adapters,
            list_accounts,
            list_subs,
            remote_info,
            check_update,
            install_update,
            remote_pair,
            remote_forget,
            preview,
            add_sub,
            remove_sub,
            check_sub,
            open_web,
            add_account,
            remove_account,
            torrent_meta,
            grab_links,
            add_grabbed,
            add_torrent,
            install_tools,
            video_lookup,
            add_video,
            add_playlist,
            browser_download,
            extension_info,
            open_folder,
            open_crash_reports,
        ])
        .build(tauri::generate_context!())
        .expect("error while running Ultimate Download Manager")
        .run(|_, event| {
            if let tauri::RunEvent::Exit = event {
                crash::clean_exit();
            }
        });
}
