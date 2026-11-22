//! The site-check window: a real browser (WebView2) where a person passes a site's browser
//! check (Cloudflare's "checking your browser", an "I'm human" box) or signs in. Ultimate Download Manager then reads
//! that window's cookies and keeps them, with the window's User-Agent, as the site's session, so
//! every connection and every queued file from the site reuses it. Nothing here solves a check:
//! the person (or the browser itself) does.
//!
//! The fallback, for sites whose clearance won't carry over: the same kind of window downloads
//! the file itself, over one connection, into Ultimate Download Manager's list and folder.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use tauri::webview::{DownloadEvent, WebviewWindowBuilder};
use tauri::{AppHandle, Emitter, Manager as _, WebviewUrl};

use crate::manager::{BROWSER_AGENT, Manager, same_site};
use crate::sessions::Session;

/// Sent when a site's session was captured: `{host}`.
pub const EV_SESSION: &str = "udm://session";

static NEXT: AtomicU32 = AtomicU32::new(1);

/// Cookies that show a check was passed (Cloudflare's clearance, and its bot-management one).
const CLEARANCE: [&str; 2] = ["cf_clearance", "__cf_bm"];

fn host_of(url: &url::Url) -> String {
    url.host_str().unwrap_or_default().to_ascii_lowercase()
}

/// Open the site-check window on `url`. When the site's clearance cookie appears, or when the
/// person closes the window after signing in, its cookies become the host's session.
pub fn open(app: &AppHandle, url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("That isn't a web address ({e})"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("The site check opens web pages only.".into());
    }
    let host = host_of(&parsed);
    let label = format!("check-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parsed.clone()))
        .title(format!("Site check · {host} · close this window when the page has loaded"))
        .inner_size(1100.0, 800.0)
        .user_agent(BROWSER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut latest: Vec<(String, String, Option<i64>)> = Vec::new();
        let mut saved = false;
        loop {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let Some(w) = app.get_webview_window(&label) else {
                break;
            };
            let Ok(cookies) = w.cookies_for_url(parsed.clone()) else {
                continue;
            };
            latest = cookies
                .iter()
                .map(|c| {
                    let exp = c.expires_datetime().map(|d| d.unix_timestamp());
                    (c.name().to_owned(), c.value().to_owned(), exp)
                })
                .collect();
            if !saved && latest.iter().any(|(n, _, _)| CLEARANCE.contains(&n.as_str())) {
                saved = true;
                save(&app, &host, &latest);
                let _ = w.set_title(&format!("Site check · {host} · passed: Ultimate Download Manager is downloading"));
                tokio::time::sleep(Duration::from_millis(1200)).await;
                let _ = w.close();
                break;
            }
        }
        // Closed by the person (after signing in, say): keep what the site set.
        if !saved && !latest.is_empty() {
            save(&app, &host, &latest);
        }
    });
    let _ = window.set_focus();
    Ok(())
}

fn save(app: &AppHandle, host: &str, cookies: &[(String, String, Option<i64>)]) {
    let Some(m) = app.try_state::<Arc<Manager>>() else {
        return;
    };
    let header = cookies
        .iter()
        .map(|(n, v, _)| format!("{n}={v}"))
        .collect::<Vec<_>>()
        .join("; ");
    let expires = cookies.iter().filter_map(|(_, _, e)| *e).min();
    m.vault.put(
        host,
        Session {
            cookies: header,
            user_agent: BROWSER_AGENT.to_owned(),
            at: crate::store::now(),
            expires,
        },
    );
    m.retry_host(host);
    let _ = app.emit(EV_SESSION, serde_json::json!({ "host": host }));
}

/// Let a browser window download the file of download `id` itself (one connection), saving it
/// in the download's folder; Ultimate Download Manager's list follows it.
pub fn browser_download(app: &AppHandle, id: i64) -> Result<(), String> {
    let m = app.try_state::<Arc<Manager>>().ok_or("Ultimate Download Manager isn't ready")?.inner().clone();
    let item = m.item(id)?;
    let url = url::Url::parse(&item.url).map_err(|e| e.to_string())?;
    let dir = PathBuf::from(&item.dir);
    let label = format!("fetch-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let host = host_of(&url);
    m.external_started(id);
    let m2 = m.clone();
    let wanted = url.clone();
    WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .title(format!("Downloading from {host} in a browser window"))
        .inner_size(900.0, 640.0)
        .user_agent(BROWSER_AGENT)
        .on_download(move |webview, event| {
            match event {
                DownloadEvent::Requested { url, destination } => {
                    // Only the file asked for (or one the site's own page leads to on the same site).
                    let ours = url == wanted
                        || url.host_str().is_some_and(|h| same_site(h, &host_of(&wanted)));
                    if !ours {
                        return false;
                    }
                    let name = destination
                        .file_name()
                        .map(|n| n.to_owned())
                        .unwrap_or_else(|| "download".into());
                    *destination = udm_engine::naming::unique_path(&dir, &name.to_string_lossy());
                    m2.external_progress(id, destination);
                    true
                }
                DownloadEvent::Finished { path, success, .. } => {
                    m2.external_finished(id, path.as_deref(), success);
                    let _ = webview.window().close();
                    true
                }
                _ => true,
            }
        })
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Get a file host's file in a browser window: the host's page opens, the person waits out its
/// timer and solves its captcha there (nothing here solves anything), and when the site starts
/// the file, Ultimate Download Manager takes the file's address and the window's cookies and downloads it itself
/// (every connection, resumable). The browser's own download is cancelled.
pub fn assist(app: &AppHandle, id: i64) -> Result<(), String> {
    let m = app.try_state::<Arc<Manager>>().ok_or("Ultimate Download Manager isn't ready")?.inner().clone();
    let item = m.item(id)?;
    let url = url::Url::parse(&item.url).map_err(|e| e.to_string())?;
    let host = host_of(&url);
    let label = format!("assist-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let app2 = app.clone();
    let label2 = label.clone();
    let taken = Arc::new(std::sync::atomic::AtomicBool::new(false));
    WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .title(format!(
            "{host} · wait for the timer, solve the captcha, click Download: Ultimate Download Manager takes it from there"
        ))
        .inner_size(1100.0, 820.0)
        .user_agent(BROWSER_AGENT)
        .on_download(move |_webview, event| {
            let DownloadEvent::Requested { url, .. } = event else { return true };
            if taken.swap(true, Ordering::SeqCst) {
                return false;
            }
            let (app, label, found) = (app2.clone(), label2.clone(), url.clone());
            // Cookies are read off the main thread (reading them there deadlocks WebView2).
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                let Some(w) = app.get_webview_window(&label) else { return };
                let cookies = w
                    .cookies_for_url(found.clone())
                    .ok()
                    .map(|cs| cs.iter().map(|c| format!("{}={}", c.name(), c.value())).collect::<Vec<_>>().join("; "))
                    .filter(|c| !c.is_empty());
                if let Some(m) = app.try_state::<Arc<Manager>>() {
                    m.inner().assisted(id, found, cookies, BROWSER_AGENT.to_owned());
                }
                let _ = w.set_title("Got it: Ultimate Download Manager is downloading");
                tokio::time::sleep(Duration::from_millis(800)).await;
                let _ = w.close();
            });
            // The browser doesn't download it: Ultimate Download Manager does.
            false
        })
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}
