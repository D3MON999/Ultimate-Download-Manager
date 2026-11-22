//! Ultimate Download Manager's command channel: the token-protected loopback socket (`udm_ipc::serve`) through
//! which the browser's native host (`udm-nmhost`), the command line and scripts hand downloads
//! to the running app. Also keeps `capture.json`, the rules the native host reads, and registers
//! the native host with the browsers.

use std::sync::Arc;
use std::sync::mpsc;

use udm_ipc::capture::{CAPTURE_FILE, CaptureRules};
use udm_ipc::{BrowserCookie, Command, Incoming};
use tauri::{AppHandle, Emitter, Manager as _};

use crate::manager::{Context, Manager};

/// Sent to the UI for a short message: `{text, bad}`.
pub const EV_TOAST: &str = "udm://toast";
/// Sent to the UI to open one of its dialogs: `"settings"`.
pub const EV_SHOW: &str = "udm://show";

/// Start listening, on a thread of its own. Returns the channel's sender, for the remote control.
pub fn start(app: AppHandle) -> std::io::Result<mpsc::Sender<Incoming>> {
    let (tx, rx) = mpsc::channel::<Incoming>();
    udm_ipc::serve(tx.clone(), || {})?;
    std::thread::Builder::new()
        .name("udm-commands".into())
        .spawn(move || {
            for incoming in rx {
                let reply = handle(&app, incoming.cmd);
                if let Some(r) = incoming.reply {
                    let _ = r.send(reply);
                }
            }
        })?;
    Ok(tx)
}

fn toast(app: &AppHandle, text: impl Into<String>, bad: bool) {
    let _ = app.emit(EV_TOAST, serde_json::json!({ "text": text.into(), "bad": bad }));
}

/// The browser's cookies for `url`: its cookie jar's (domain, path and security honoured) or,
/// from an older extension, the plain header it sent.
fn cookie_header(cmd: &Command, url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let path = parsed.path();
    let https = parsed.scheme() == "https";
    let from_jar: Vec<String> = cmd
        .cookie_jar
        .iter()
        .filter(|c: &&BrowserCookie| {
            let d = c.domain.trim_start_matches('.').to_ascii_lowercase();
            let domain_ok = if c.host_only { host == d } else { host == d || host.ends_with(&format!(".{d}")) };
            domain_ok && path.starts_with(if c.path.is_empty() { "/" } else { &c.path }) && (https || !c.secure)
        })
        .map(|c| format!("{}={}", c.name, c.value))
        .collect();
    if !from_jar.is_empty() {
        return Some(from_jar.join("; "));
    }
    cmd.cookies.clone().filter(|c| !c.is_empty())
}

fn context(cmd: &Command, url: &str) -> Context {
    Context { cookies: cookie_header(cmd, url), user_agent: cmd.user_agent.clone() }
}

fn handle(app: &AppHandle, cmd: Command) -> String {
    let Some(m) = app.try_state::<Arc<Manager>>().map(|s| s.inner().clone()) else {
        return "starting".into();
    };
    // The automation API (scripts, the command line) and the phone remote: JSON answers.
    if cmd.wants_reply() || cmd.remote {
        return api_answer(app, &m, cmd);
    }
    if let Some(page) = cmd.video_page.as_deref() {
        // The video window, on the page (or the video's own file), with what the browser sent
        // for it: Ultimate Download Manager looks it up and offers its qualities.
        let ctx = context(&cmd, page);
        m.set_pending(page, ctx);
        // A small window of its own (the app stays where it is), shown at once.
        if let Err(e) = crate::newdl::ask_video(app, page, cmd.video_title.as_deref()) {
            show(app);
            let _ = app.emit(EV_SHOW, serde_json::json!({ "video": page, "title": cmd.video_title }).to_string());
            toast(app, e, true);
        }
        return "ok".into();
    }
    // A magnet link (clicked, or among the links sent): the torrent dialog, to pick its files.
    let magnet = cmd
        .url
        .iter()
        .chain(cmd.links.iter())
        .find(|l| l.trim_start().to_ascii_lowercase().starts_with("magnet:?"))
        .cloned();
    if let Some(magnet) = magnet {
        show(app);
        let _ = app.emit(EV_SHOW, serde_json::json!({ "torrent": magnet }).to_string());
        return "ok".into();
    }
    if !cmd.links.is_empty() {
        // The link grabber, to choose from them (as the browser's "all links" menu means).
        show(app);
        let _ = app.emit(EV_SHOW, serde_json::json!({ "grab": cmd.links.join("\n") }).to_string());
        return "ok".into();
    }
    if let Some(url) = cmd.url.as_deref() {
        // The address the person clicked, not where the browser's redirects ended (often a
        // short-lived signed link): the engine follows the redirects again, so a later resume
        // gets a fresh link. Cookies for both hosts go with it.
        let mut ctx = context(&cmd, url);
        if let Some(end) = cmd.final_url.as_deref()
            && let Some(more) = cookie_header(&cmd, end)
        {
            ctx.cookies = Some(match ctx.cookies.take() {
                Some(c) if !c.contains(&more) => format!("{c}; {more}"),
                Some(c) => c,
                None => more,
            });
        }
        // A stuck download of the same file: it takes this fresh address and goes on.
        if let Some(id) = m.refresh_from_browser(url, cmd.name.as_deref(), cmd.size, ctx.clone()) {
            let name = m.item(id).map(|i| i.display_name()).unwrap_or_default();
            toast(app, format!("Fresh link for {name}: it carries on from where it stopped"), false);
            return "ok".into();
        }
        // "Download file info": the person picks where it goes (Settings → General).
        if !cmd.silent && crate::newdl::should_ask(&m, cmd.name.as_deref(), url) {
            let asked = crate::newdl::Asked {
                url: url.to_owned(),
                name: cmd.name.clone(),
                size: cmd.size,
                mime: cmd.mime.clone(),
                referer: cmd.referer.clone(),
                ctx,
                at: std::time::Instant::now(),
            };
            if let Err(e) = crate::newdl::ask(app, asked) {
                toast(app, e, true);
            }
            return "ok".into();
        }
        match m.add(url, None, cmd.name.clone(), cmd.referer.clone(), !cmd.later, Some(ctx)) {
            Ok(_) if !cmd.silent => {}
            Ok(_) => {}
            Err(e) => toast(app, e, true),
        }
        return "ok".into();
    }
    if let Some(site) = cmd.exclude_site.as_deref() {
        edit_rules(|r| add_word(&mut r.excluded_sites, site));
        toast(app, format!("Downloads from {site} now stay in the browser"), false);
        return "ok".into();
    }
    if let Some(site) = cmd.panel_hide_site.as_deref() {
        edit_rules(|r| add_word(&mut r.video_panel_sites, site));
        return "ok".into();
    }
    if cmd.show.as_deref().is_some_and(|s| s.starts_with("options")) {
        show(app);
        let _ = app.emit(EV_SHOW, "settings");
        return "ok".into();
    }
    // An empty command: bring the window up (the toolbar button's "Open Ultimate Download Manager").
    show(app);
    "ok".into()
}

fn status_word(state: crate::store::State) -> &'static str {
    use crate::store::State::*;
    match state {
        Queued => "Queued",
        Downloading => "Downloading",
        Paused => "Stopped",
        Done => "Complete",
        Failed => "Error",
    }
}

/// The automation API's answers (`udm_ipc::api`): the list, actions, adds, pairing.
fn api_answer(app: &AppHandle, m: &Arc<Manager>, cmd: Command) -> String {
    use udm_ipc::api::*;
    let err = |e: &str| line(&ErrorAnswer { error: e.to_owned(), code: None });
    if cmd.query.as_deref() == Some("list") {
        let rows = m.list().unwrap_or_default();
        let ids: Vec<u64> = rows.iter().filter(|r| r.item.state == crate::store::State::Queued).map(|r| r.item.id as u64).collect();
        let answer = ListAnswer {
            speed: rows.iter().map(|r| r.speed).sum(),
            downloads: rows
                .into_iter()
                .map(|r| DownloadInfo {
                    id: r.item.id as u64,
                    file: r.display.clone(),
                    url: r.item.url.clone(),
                    path: r.item.file.clone().unwrap_or_else(|| r.item.dir.clone()),
                    status: status_word(r.item.state).into(),
                    size: r.item.size,
                    downloaded: r.item.done,
                    speed: r.speed,
                    added: Some(crate::store::now().min(r.item.created_at).to_string()),
                    category: r.item.category.clone(),
                    error: r.item.error.clone(),
                })
                .collect(),
            queues: vec![QueueInfo { name: "Main".into(), running: !m.held(), ids }],
            categories: ["other", "video", "music", "docs", "archives", "programs", "images", "torrents"]
                .iter()
                .map(|k| CategoryInfo { key: (*k).into(), name: (*k).into() })
                .collect(),
            free: None,
        };
        return line(&answer);
    }
    if let Some(action) = cmd.action.as_deref() {
        let ids: Vec<i64> = cmd.ids.iter().map(|i| *i as i64).collect();
        let all: Vec<i64> = m.list().unwrap_or_default().iter().map(|r| r.item.id).collect();
        let r = match action {
            "pause" => m.pause(&ids),
            "start" => m.resume(&ids),
            "remove" => m.remove(&ids, false),
            "delete" => m.remove(&ids, true),
            "start_queue" => m.resume(&all),
            "stop_queue" => m.pause(&all),
            _ => return err("unknown action"),
        };
        return match r {
            Ok(()) => line(&ActAnswer { done: cmd.ids.clone(), skipped: vec![] }),
            Err(e) => err(&e),
        };
    }
    if cmd.api.is_some() {
        return line(&ApiAnswer { api: true });
    }
    if cmd.pair {
        return match app.try_state::<crate::RemoteControl>() {
            Some(r) => {
                let code = r.0.start_pairing();
                line(&PairAnswer { code, minutes: 5, urls: r.0.urls(), qr: String::new() })
            }
            None => err("the remote control is off"),
        };
    }
    // From the phone: a .torrent file or a magnet link starts with all its files.
    let magnet = cmd.url.as_deref().filter(|u| u.trim_start().to_ascii_lowercase().starts_with("magnet:?"));
    if cmd.torrent_file.is_some() || magnet.is_some() {
        let source = match (&cmd.torrent_file, magnet) {
            (Some(bytes), _) => {
                let p = std::env::temp_dir().join(format!("udm-remote-{}.torrent", crate::store::now()));
                if std::fs::write(&p, bytes).is_err() {
                    return err("couldn't keep the .torrent file");
                }
                p.display().to_string()
            }
            (None, Some(m)) => m.to_owned(),
            _ => unreachable!(),
        };
        let m = m.clone();
        tauri::async_runtime::spawn(async move {
            if let Ok(meta) = crate::torrent::meta(&m, &source).await {
                let _ = crate::torrent::add(&m, &meta.hash, None, None).await;
            }
        });
        return line(&AddAnswer { id: None, pending: true });
    }
    if let Some(url) = cmd.url.as_deref() {
        return match m.add(url, None, None, None, !cmd.later, None) {
            Ok(id) => line(&AddAnswer { id: Some(id as u64), pending: false }),
            Err(e) => err(&e),
        };
    }
    err("unknown question")
}

fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn add_word(list: &mut String, word: &str) {
    let word = word.trim().to_ascii_lowercase();
    if !word.is_empty() && !list.split_whitespace().any(|w| w == word) {
        if !list.is_empty() {
            list.push(' ');
        }
        list.push_str(&word);
    }
}

/// Change the capture rules the native host reads, and save them.
pub fn edit_rules(f: impl FnOnce(&mut CaptureRules)) {
    let mut rules = CaptureRules::load();
    f(&mut rules);
    save_rules(&rules);
}

pub fn save_rules(rules: &CaptureRules) {
    let Some(dir) = udm_ipc::data_dir() else {
        return;
    };
    if let Ok(json) = serde_json::to_vec_pretty(rules) {
        let _ = udm_ipc::private_write(&dir.join(CAPTURE_FILE), &json);
    }
}

/// Make sure `capture.json` exists (the defaults), so the host and Ultimate Download Manager agree from the start.
pub fn ensure_rules() {
    if let Some(dir) = udm_ipc::data_dir()
        && !dir.join(CAPTURE_FILE).exists()
    {
        save_rules(&CaptureRules::default());
    }
}

/// Register the native host beside Ultimate Download Manager's program with every browser, per user (no admin
/// rights). Quietly does nothing when the host isn't there (a development build without it).
pub fn register_host() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let host = exe.with_file_name("udm-nmhost.exe");
    if !host.is_file() {
        return Err(format!("{} is missing", host.display()));
    }
    let mut cmd = std::process::Command::new(&host);
    cmd.arg("--register");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jar_cookies_follow_domain_path_and_https() {
        let cmd = Command {
            cookie_jar: vec![
                BrowserCookie { name: "a".into(), value: "1".into(), domain: ".example.com".into(), path: "/".into(), ..Default::default() },
                BrowserCookie { name: "b".into(), value: "2".into(), domain: "www.example.com".into(), host_only: true, path: "/".into(), ..Default::default() },
                BrowserCookie { name: "s".into(), value: "3".into(), domain: ".example.com".into(), path: "/".into(), secure: true, ..Default::default() },
                BrowserCookie { name: "p".into(), value: "4".into(), domain: ".example.com".into(), path: "/private".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        assert_eq!(cookie_header(&cmd, "https://cdn.example.com/f.zip").unwrap(), "a=1; s=3");
        assert_eq!(cookie_header(&cmd, "http://www.example.com/f.zip").unwrap(), "a=1; b=2");
        assert!(cookie_header(&cmd, "https://other.com/f.zip").is_none());
    }

    #[test]
    fn words() {
        let mut s = String::from("a.com");
        add_word(&mut s, "B.com");
        add_word(&mut s, "a.com");
        assert_eq!(s, "a.com b.com");
    }
}
