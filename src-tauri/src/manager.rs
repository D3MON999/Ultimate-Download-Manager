//! The download manager: the queue, one actor task per running download (driving the engine's
//! `Download`), progress for the UI, and the list kept in `Store`.
//!
//! Lock order: `running` before `store`, never the reverse.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use udm_engine::download::{ConnStatus, State as EngineState};
use udm_engine::hosts::HostLimits;
use udm_engine::limiter::Limiter;
use udm_engine::{Destination, Download, FileHook, Options, Request};
use reqwest::header::{COOKIE, HeaderMap, HeaderValue, REFERER};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::mpsc;

use crate::sessions::Vault;
use crate::settings::Settings;
use crate::store::{self, Item, State, Store};
use crate::torrent::{self, Swarm};
use crate::video::{self, VideoOpts};

/// Sent to the UI whenever the list changes (it reloads it).
pub const EV_CHANGED: &str = "udm://changed";
/// Sent twice a second while something downloads: `Vec<Tick>`.
pub const EV_PROGRESS: &str = "udm://progress";

#[derive(Debug)]
enum Ctl {
    Pause,
    /// Outside the download hours: stop, and wait in the queue for the next window.
    Hold,
    Remove { delete_files: bool },
}

#[derive(Clone, Debug, Default)]
struct Live {
    done: u64,
    size: Option<u64>,
    /// Bytes/s, smoothed.
    speed: f64,
    conns: u32,
    /// Its parts as the engine splits them: (start, cursor, end) each.
    segments: Vec<(u64, u64, u64)>,
    /// Each connection: bytes received, bytes/s now, receiving or not.
    connections: Vec<(u64, u64, bool)>,
    resumable: Option<bool>,
    /// The server it comes from (or the proxy that carries it), as the engine saw it.
    server: Option<udm_engine::Server>,
}

/// A running file download's inside, for the details panel.
#[derive(Clone, Debug, Serialize)]
pub struct FileDetails {
    pub segments: Vec<(u64, u64, u64)>,
    pub connections: Vec<(u64, u64, bool)>,
    pub resumable: Option<bool>,
    pub server: Option<ServerView>,
}

/// Where a download's server is, for the progress window and the details panel. The country
/// is looked up on this computer (a table shipped with the app); the address goes nowhere.
#[derive(Clone, Debug, Serialize)]
pub struct ServerView {
    pub ip: String,
    /// ISO 3166-1 code ("DE"); None for a local or unplaced address.
    pub country: Option<String>,
    /// The flag's place in `/flags.png` (32×24 cells), in pixels.
    pub flag: Option<(u32, u32)>,
    pub rtt_ms: Option<u64>,
    pub proxied: bool,
    pub cdn: Option<String>,
}

impl ServerView {
    fn of(s: &udm_engine::Server) -> Option<ServerView> {
        let ip = s.ip?;
        let c = udm_geo::country(ip);
        Some(ServerView {
            ip: ip.to_string(),
            country: c.map(|c| c.code().to_owned()),
            flag: c.and_then(|c| c.flag_cell()),
            rtt_ms: s.rtt.map(|d| d.as_millis() as u64),
            proxied: s.proxied,
            cdn: s.cdn.map(str::to_owned),
        })
    }
}

struct Running {
    ctl: mpsc::UnboundedSender<Ctl>,
    live: Arc<Mutex<Live>>,
    queue: String,
}

/// A download's live numbers, for the list and the status pills.
#[derive(Clone, Debug, Serialize)]
pub struct Tick {
    pub id: i64,
    pub done: u64,
    pub size: Option<u64>,
    pub speed: u64,
    pub eta: Option<u64>,
    pub conns: u32,
}

/// One row of the list, as the UI shows it.
#[derive(Clone, Debug, Serialize)]
pub struct RowView {
    #[serde(flatten)]
    pub item: Item,
    pub display: String,
    pub speed: u64,
    pub eta: Option<u64>,
    pub conns: u32,
}

pub struct Manager {
    app: AppHandle,
    store: Mutex<Store>,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    running: Mutex<HashMap<i64, Running>>,
    global: Arc<Limiter>,
    hosts: Arc<HostLimits>,
    /// Automatic retries so far, per download (reset when it finishes or the user resumes it).
    retries: Mutex<HashMap<i64, u32>>,
    /// What the browser sent with a download (its cookies and User-Agent): in memory only,
    /// never written to the list, so no sign-in sits on disk.
    context: Mutex<HashMap<i64, Context>>,
    /// Sessions left by the site-check window, per host.
    pub vault: Vault,
    /// The browser's context for video pages it handed over, until they're queued.
    pending: Mutex<HashMap<String, Context>>,
    /// The torrent session.
    pub swarm: Swarm,
    /// Running torrents' numbers: (done, total, speed, peers).
    torrent_live: Mutex<HashMap<i64, (u64, u64, f64, u32)>>,
    /// Outside the download hours: nothing new starts.
    held: std::sync::atomic::AtomicBool,
    /// Debrid accounts.
    pub accounts: crate::accounts::Accounts,
    /// File addresses a browser window found for a download (the person got past the host's
    /// wait and captcha there): the engine downloads them instead of the host's page.
    assisted: Mutex<HashMap<i64, url::Url>>,
}

/// The browser's side of a download it handed over.
#[derive(Clone, Debug, Default)]
pub struct Context {
    /// As a Cookie header: `a=1; b=2`.
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
}

/// The start of a failed download's error when a site wants a real browser first.
pub const CHECK_NEEDED: &str = "Site check needed: ";

/// The start of a failed download's error when a file host wants a person (its wait, its
/// captcha): the browser window gets the file (`check::assist`).
pub const ASSIST_NEEDED: &str = "Needs you: ";

/// Sent when a download needs the site-check window: `{id, url}`.
pub const EV_NEEDS_CHECK: &str = "udm://needs-check";

/// Connections per download while it rides on a site-check session: gentle enough not to
/// trip the site's check again.
const SESSION_CONNECTIONS: u32 = 4;

/// How Ultimate Download Manager introduces itself by default: honestly, as curl does. Some servers (seen:
/// Hetzner's) cut off requests that claim to be a browser but aren't one.
pub const UDM_AGENT: &str = concat!("UltimateDownloadManager/", env!("CARGO_PKG_VERSION"));
/// Asked with instead when a server answers the first request with 403 (some only serve
/// browsers). Downloads handed over by the browser extension send the browser's own.
pub const BROWSER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36";

/// Waits before each automatic retry of a download that failed on the network or a busy
/// server; after the last one it stays failed for the user to retry.
const RETRY_WAITS: [u64; 5] = [10, 30, 60, 120, 300];

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl Manager {
    pub fn new(app: AppHandle, data_dir: &Path) -> Res<Arc<Manager>> {
        std::fs::create_dir_all(data_dir).map_err(err)?;
        let store = Store::open(&data_dir.join("udm.db")).map_err(err)?;
        store.requeue_interrupted().map_err(err)?;
        let settings_path = data_dir.join("settings.json");
        let settings = Settings::load(&settings_path);
        let global = Arc::new(Limiter::new(None));
        global.set((settings.global_limit_kib > 0).then(|| settings.global_limit_kib * 1024));
        let m = Arc::new(Manager {
            app,
            store: Mutex::new(store),
            settings: Mutex::new(settings),
            settings_path,
            running: Mutex::new(HashMap::new()),
            global,
            hosts: Arc::new(HostLimits::default()),
            retries: Mutex::new(HashMap::new()),
            context: Mutex::new(HashMap::new()),
            vault: Vault::open(data_dir),
            pending: Mutex::new(HashMap::new()),
            swarm: Swarm::new(data_dir),
            torrent_live: Mutex::new(HashMap::new()),
            held: std::sync::atomic::AtomicBool::new(false),
            accounts: crate::accounts::Accounts::open(data_dir),
            assisted: Mutex::new(HashMap::new()),
        });
        let hosts = m.clone();
        tauri::async_runtime::spawn(async move { hosts.accounts.refresh().await });
        m.swarm.set_settings(&m.settings());
        let ticker = m.clone();
        tauri::async_runtime::spawn(async move { ticker.tick_forever().await });
        let swarm = m.clone();
        tauri::async_runtime::spawn(async move { torrent::follow(swarm).await });
        let subs = m.clone();
        tauri::async_runtime::spawn(async move { crate::subscriptions::run(subs).await });
        let clock = m.clone();
        tauri::async_runtime::spawn(async move { crate::scheduler::run(clock).await });
        let feeds = m.clone();
        tauri::async_runtime::spawn(async move { crate::rss::run(feeds).await });
        if m.settings().autostart {
            crate::autostart::apply(true);
        }
        let s = m.settings();
        std::thread::spawn(move || crate::winint::apply(s.handle_torrents, s.txt_menu));
        m.pump();
        Ok(m)
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    pub fn set_settings(self: &Arc<Self>, new: Settings) -> Res<Settings> {
        let mut new = new.clamped();
        // Saved from the app: the one-time move to Dock is done, whatever the window sent.
        new.theme_dock_seen = true;
        new.save(&self.settings_path).map_err(err)?;
        self.global
            .set((new.global_limit_kib > 0).then(|| new.global_limit_kib * 1024));
        *self.settings.lock().unwrap() = new.clone();
        self.swarm.set_settings(&new);
        crate::autostart::apply(new.autostart);
        crate::winint::apply(new.handle_torrents, new.txt_menu);
        crate::hotkey::apply(&self.app, &new.hotkey);
        self.pump();
        Ok(new)
    }

    /// One download's row, with its live numbers (a progress window's twice-a-second look:
    /// not the whole list).
    pub fn row(&self, id: i64) -> Res<RowView> {
        let mut item = self.item(id)?;
        let live = self
            .running
            .lock()
            .unwrap()
            .get(&id)
            .map(|r| r.live.lock().unwrap().clone())
            .or_else(|| {
                self.torrent_live.lock().unwrap().get(&id).map(|(done, total, speed, peers)| Live {
                    done: *done,
                    size: Some(*total),
                    speed: *speed,
                    conns: *peers,
                    ..Live::default()
                })
            });
        let l = live.clone().unwrap_or_default();
        if live.is_some() {
            item.done = l.done.max(item.done);
            item.size = l.size.or(item.size);
        }
        let display = item.display_name();
        let eta = eta(item.size, item.done, l.speed);
        Ok(RowView { item, display, speed: l.speed as u64, eta, conns: l.conns })
    }

    pub fn list(&self) -> Res<Vec<RowView>> {
        let mut live: HashMap<i64, Live> = self
            .running
            .lock()
            .unwrap()
            .iter()
            .map(|(id, r)| (*id, r.live.lock().unwrap().clone()))
            .collect();
        for (id, (done, total, speed, peers)) in self.torrent_live.lock().unwrap().iter() {
            live.insert(*id, Live { done: *done, size: Some(*total), speed: *speed, conns: *peers, ..Live::default() });
        }
        let items = self.store.lock().unwrap().all().map_err(err)?;
        Ok(items
            .into_iter()
            .map(|mut item| {
                let l = live.get(&item.id).cloned().unwrap_or_default();
                if live.contains_key(&item.id) {
                    item.done = l.done.max(item.done);
                    item.size = l.size.or(item.size);
                }
                let display = item.display_name();
                let eta = eta(item.size, item.done, l.speed);
                RowView { item, display, speed: l.speed as u64, eta, conns: l.conns }
            })
            .collect())
    }

    /// Add a download (queued, or paused when `start` is false). Returns its id.
    pub fn add(
        self: &Arc<Self>,
        url: &str,
        dir: Option<PathBuf>,
        name: Option<String>,
        referer: Option<String>,
        start: bool,
        ctx: Option<Context>,
    ) -> Res<i64> {
        let direct = crate::accounts::direct_cloud_link(url.trim());
        let parsed = url::Url::parse(&direct).map_err(|e| format!("That isn't a web address ({e})"))?;
        if !udm_engine::supported(&parsed) {
            return Err(match parsed.scheme() {
                "magnet" => "Torrents and magnet links arrive in Phase 4.".into(),
                s => format!("Ultimate Download Manager can't download \"{s}:\" addresses yet."),
            });
        }
        // Packagizer rules (Settings → Rules): the first that matches the host and the name.
        let rule_name = name
            .clone()
            .or_else(|| crate::hosts::name_in_link(&parsed))
            .unwrap_or_else(|| parsed.path_segments().and_then(|mut s| s.next_back().map(str::to_owned)).unwrap_or_default());
        let rule = self
            .settings()
            .rules
            .into_iter()
            .find(|r| r.matches(parsed.host_str().unwrap_or_default(), &rule_name));
        let mut start = start;
        let mut dir = dir;
        if let Some(r) = &rule {
            match r.action.as_str() {
                "skip" => return Err(format!("Not added: a rule skips {rule_name}")),
                "paused" => start = false,
                _ if dir.is_none() && !r.folder.trim().is_empty() => {
                    let f = PathBuf::from(r.folder.trim());
                    let f = if f.is_absolute() { f } else { self.settings().download_dir.join(f) };
                    let _ = std::fs::create_dir_all(&f);
                    dir = Some(f);
                }
                _ => {}
            }
        }
        let dir = dir.unwrap_or_else(|| self.default_dir(store::category_of(&name.clone().unwrap_or_else(|| parsed.path().to_owned()))));
        let name = name
            .or_else(|| crate::hosts::name_in_link(&parsed))
            .map(|n| udm_engine::naming::sanitize(n.trim()))
            .filter(|n| !n.is_empty());
        let guess = name.clone().unwrap_or_else(|| {
            parsed
                .path_segments()
                .and_then(|mut s| s.next_back().map(str::to_owned))
                .unwrap_or_default()
        });
        let state = if start { State::Queued } else { State::Paused };
        let id = self
            .store
            .lock()
            .unwrap()
            .add(
                parsed.as_str(),
                name.as_deref(),
                &dir,
                store::category_of(&guess),
                referer.as_deref(),
                state,
            )
            .map_err(err)?;
        if let Some(c) = ctx.filter(|c| c.cookies.is_some() || c.user_agent.is_some()) {
            self.context.lock().unwrap().insert(id, c);
        }
        if let Some(r) = &rule {
            let what = match r.action.as_str() {
                "paused" => "added paused".to_owned(),
                _ => format!("saved in {}", r.folder.trim()),
            };
            let who = [r.host.trim(), r.name.trim()].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(" ");
            let desc = format!("{} → {what}", if who.is_empty() { "every link" } else { who.as_str() });
            if let Ok(item) = self.item(id) {
                let _ = self.set_option(&item, "rule", serde_json::json!(desc));
            }
        }
        self.changed();
        self.pump();
        Ok(id)
    }

    pub fn add_torrent_row(
        self: &Arc<Self>,
        magnet: &str,
        name: &str,
        dir: &Path,
        folder: &Path,
        size: u64,
        opts: &str,
    ) -> Res<i64> {
        let id = self
            .store
            .lock()
            .unwrap()
            .add_torrent(magnet, name, dir, folder, size, opts)
            .map_err(err)?;
        self.changed();
        Ok(id)
    }

    /// Where a download of category goes when none is given: the download folder, or its
    /// folder for that type (Settings → sort by type).
    pub fn default_dir(&self, category: &str) -> PathBuf {
        let s = self.settings();
        if !s.sort_by_type {
            return s.download_dir;
        }
        let sub = match category {
            "video" => "Video",
            "music" => "Music",
            "docs" => "Documents",
            "archives" => "Compressed",
            "programs" => "Programs",
            "images" => "Images",
            "torrents" => "Torrents",
            _ => return s.download_dir,
        };
        let dir = s.download_dir.join(sub);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// Run  on the database (subscriptions keep their own tables there).
    pub fn with_db<T>(&self, f: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<T>) -> rusqlite::Result<T> {
        f(self.store.lock().unwrap().conn())
    }

    /// Every address in the list (for the grabber's duplicate check).
    pub fn known_urls(&self) -> std::collections::HashSet<String> {
        self.store.lock().unwrap().all().unwrap_or_default().into_iter().map(|i| i.url).collect()
    }

    /// Every download in the list.
    pub fn all_items(&self) -> Vec<Item> {
        self.store.lock().unwrap().all().unwrap_or_default()
    }

    pub fn items_of_kind(&self, kind: &str) -> Vec<Item> {
        self.store.lock().unwrap().of_kind(kind).unwrap_or_default()
    }

    pub fn torrent_progress(&self, id: i64, done: u64, total: u64) {
        let _ = self.store.lock().unwrap().set_progress(id, None, Some(total), done);
    }

    pub fn torrent_done(&self, id: i64, total: u64) {
        let name = {
            let store = self.store.lock().unwrap();
            let _ = store.set_progress(id, None, Some(total), total);
            let _ = store.set_state(id, State::Done, None);
            store.get(id).ok().flatten().map(|i| i.display_name()).unwrap_or_default()
        };
        self.notify("Torrent finished", &name);
        self.changed();
    }

    pub fn torrent_failed(&self, id: i64, why: &str) {
        let _ = self.store.lock().unwrap().set_state(id, State::Failed, Some(why));
        self.changed();
    }

    /// The running torrents' numbers, for the list and the progress events.
    /// A browser window found the file of download `id` at `found` (the person passed the host's
    /// wait and captcha): it downloads from there, with the window's cookies and agent.
    pub fn assisted(self: &Arc<Self>, id: i64, found: url::Url, cookies: Option<String>, agent: String) {
        self.assisted.lock().unwrap().insert(id, found);
        self.context.lock().unwrap().insert(id, Context { cookies, user_agent: Some(agent) });
        let _ = self.store.lock().unwrap().set_state(id, State::Queued, None);
        self.retries.lock().unwrap().remove(&id);
        self.changed();
        self.pump();
    }

    /// Run the reconnect command (Settings → Network), at most once a minute.
    pub fn reconnect(&self) {
        static LAST: Mutex<Option<Instant>> = Mutex::new(None);
        let cmd = self.settings().reconnect_command;
        if cmd.trim().is_empty() {
            return;
        }
        {
            let mut last = LAST.lock().unwrap();
            if last.is_some_and(|t| t.elapsed() < Duration::from_secs(60)) {
                return;
            }
            *last = Some(Instant::now());
        }
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", cmd.trim()]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        match c.spawn() {
            Ok(_) => self.notify("Reconnecting", "A file host asked to wait: the reconnect command is running."),
            Err(e) => self.notify("Reconnect failed", &format!("The reconnect command didn't start: {e}")),
        }
    }

    /// A download's own speed limit, KiB/s (0: none); a running one restarts with it.
    pub fn set_item_limit(self: &Arc<Self>, ids: &[i64], kib: u64) -> Res<()> {
        let store = self.store.lock().unwrap();
        for id in ids {
            store.set_limit(*id, kib).map_err(err)?;
        }
        drop(store);
        self.restart_running(ids);
        self.changed();
        Ok(())
    }

    /// Running file downloads restart (keeping their data) to take a new setting.
    fn restart_running(self: &Arc<Self>, ids: &[i64]) {
        let running = self.running.lock().unwrap();
        for id in ids {
            if let Some(r) = running.get(id) {
                let _ = r.ctl.send(Ctl::Hold);
            }
        }
    }

    pub fn move_in_queue(self: &Arc<Self>, ids: &[i64], top: bool) -> Res<()> {
        self.store.lock().unwrap().move_in_queue(ids, top).map_err(err)?;
        self.changed();
        Ok(())
    }

    pub fn nudge(self: &Arc<Self>, id: i64, up: bool) -> Res<()> {
        self.store.lock().unwrap().nudge(id, up).map_err(err)?;
        self.changed();
        Ok(())
    }

    /// The checksum a download must match (checked when it finishes, or now if it has).
    pub fn set_checksum(self: &Arc<Self>, id: i64, text: &str) -> Res<Option<bool>> {
        let text = text.trim();
        let hash = if text.is_empty() {
            None
        } else {
            Some(udm_engine::verify::Hash::parse(text).ok_or(
                "That isn't a checksum Ultimate Download Manager knows (MD5, SHA-1, SHA-256 or SHA-512, in hex).",
            )?)
        };
        self.store.lock().unwrap().set_checksum(id, (!text.is_empty()).then_some(text)).map_err(err)?;
        self.changed();
        let item = self.item(id)?;
        match (hash, item.state, item.file) {
            (Some(h), State::Done, Some(f)) => {
                let ok = udm_engine::verify::matches(Path::new(&f), &h).map_err(err)?;
                Ok(Some(ok))
            }
            _ => Ok(None),
        }
    }

    pub fn set_password(&self, id: i64, password: &str) -> Res<()> {
        let p = password.trim();
        self.store.lock().unwrap().set_password(id, (!p.is_empty()).then_some(p)).map_err(err)?;
        Ok(())
    }

    /// A stopped download gets a new address for the same file (the "Refresh download
    /// address"): its data so far is kept and it resumes from the new one.
    pub fn refresh_address(self: &Arc<Self>, id: i64, url: &str) -> Res<()> {
        let item = self.item(id)?;
        if item.kind != "file" {
            return Err("Only file downloads take a new address.".into());
        }
        let parsed = url::Url::parse(url.trim()).map_err(|e| format!("That isn't a web address ({e})"))?;
        if !udm_engine::supported(&parsed) {
            return Err("Use an http, https or ftp address.".into());
        }
        if self.running.lock().unwrap().contains_key(&id) {
            return Err("Pause it first, then give it the new address.".into());
        }
        // The resume file holds the old address: point it at the new one.
        if let Some(f) = item.file.as_deref() {
            let state = udm_engine::state::state_path(Path::new(f));
            if let Ok(text) = std::fs::read_to_string(&state)
                && let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&text)
            {
                v["url"] = serde_json::Value::String(parsed.to_string());
                v["final_url"] = serde_json::Value::String(parsed.to_string());
                let _ = std::fs::write(&state, serde_json::to_vec(&v).map_err(err)?);
            }
        }
        {
            let store = self.store.lock().unwrap();
            store.set_url(id, parsed.as_str()).map_err(err)?;
            store.set_state(id, State::Queued, None).map_err(err)?;
        }
        self.retries.lock().unwrap().remove(&id);
        self.changed();
        self.pump();
        Ok(())
    }

    /// Rename or move a finished download's file (the list follows it).
    pub fn move_file(&self, id: i64, to: &str) -> Res<String> {
        let item = self.item(id)?;
        let from = item.file.clone().ok_or("It has no file yet.")?;
        if item.state != State::Done {
            return Err("Wait until it has finished.".into());
        }
        let from = PathBuf::from(from);
        let to = PathBuf::from(to.trim());
        let to = if to.is_absolute() {
            to
        } else {
            from.parent().map(|d| d.join(&to)).ok_or("No folder")?
        };
        // A folder as the target keeps the name.
        let to = if to.is_dir() { to.join(from.file_name().unwrap_or_default()) } else { to };
        if to.exists() {
            return Err(format!("{} already exists.", to.display()));
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(err)?;
        }
        if std::fs::rename(&from, &to).is_err() {
            // Another drive: copy, then remove.
            if from.is_dir() {
                return Err("Folders can only be moved on the same drive.".into());
            }
            std::fs::copy(&from, &to).map_err(err)?;
            std::fs::remove_file(&from).map_err(err)?;
        }
        self.store.lock().unwrap().set_file(id, &to).map_err(err)?;
        self.changed();
        Ok(to.to_string_lossy().into_owned())
    }

    /// A torrent's chosen files changed (`torrent::choose`).
    pub fn set_torrent_files(&self, id: i64, options: &str, size: Option<u64>) {
        let _ = self.store.lock().unwrap().set_options(id, options, size);
        self.changed();
    }

    /// A running file download's parts and connections (None: it isn't running).
    pub fn file_details(&self, id: i64) -> Option<FileDetails> {
        let running = self.running.lock().unwrap();
        let l = running.get(&id)?.live.lock().unwrap().clone();
        let server = l.server.as_ref().and_then(ServerView::of);
        Some(FileDetails { segments: l.segments, connections: l.connections, resumable: l.resumable, server })
    }

    pub fn set_torrent_live(&self, live: HashMap<i64, (u64, u64, f64, u32)>) {
        *self.torrent_live.lock().unwrap() = live;
    }

    /// A browser window took download `id` over (`check::browser_download`).
    pub fn external_started(&self, id: i64) {
        let _ = self.store.lock().unwrap().set_state(id, State::Downloading, None);
        self.changed();
    }

    /// The browser window chose where the file goes.
    pub fn external_progress(&self, id: i64, file: &Path) {
        let store = self.store.lock().unwrap();
        let _ = store.set_progress(id, Some(file), None, 0);
        if let Some(n) = file.file_name() {
            let _ = store.set_category(id, store::category_of(&n.to_string_lossy()));
        }
        drop(store);
        self.changed();
    }

    /// The browser window's download ended.
    pub fn external_finished(&self, id: i64, path: Option<&Path>, ok: bool) {
        {
            let store = self.store.lock().unwrap();
            let size = path.and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len());
            if ok {
                let _ = store.set_progress(id, path, size, size.unwrap_or(0));
                let _ = store.set_state(id, State::Done, None);
            } else {
                let _ = store.set_state(id, State::Failed, Some("The browser window's download failed"));
            }
        }
        if ok {
            self.notify("Download finished", &path.and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        }
        self.changed();
    }

    /// Requeue the downloads from `host` that failed for want of a site check, now that its
    /// session is in the vault.
    pub fn retry_host(self: &Arc<Self>, host: &str) {
        let ids: Vec<i64> = self
            .store
            .lock()
            .unwrap()
            .all()
            .unwrap_or_default()
            .into_iter()
            .filter(|it| {
                it.state == State::Failed
                    && url::Url::parse(&it.url)
                        .ok()
                        .and_then(|u| u.host_str().map(|h| same_site(h, host)))
                        .unwrap_or(false)
            })
            .map(|it| it.id)
            .collect();
        let _ = self.resume(&ids);
    }

    /// The torrent row `id`, if it is one.
    fn torrent_item(&self, id: i64) -> Option<Item> {
        self.store.lock().unwrap().get(id).ok().flatten().filter(|it| it.kind == "torrent")
    }

    pub fn pause(self: &Arc<Self>, ids: &[i64]) -> Res<()> {
        for &id in ids {
            if let Some(it) = self.torrent_item(id) {
                if it.state == State::Downloading {
                    let _ = self.store.lock().unwrap().set_state(id, State::Paused, None);
                    let me = self.clone();
                    tauri::async_runtime::spawn(async move { let _ = torrent::pause(&me, &it).await; });
                }
                continue;
            }
            let sent = self
                .running
                .lock()
                .unwrap()
                .get(&id)
                .map(|r| r.ctl.send(Ctl::Pause).is_ok())
                .unwrap_or(false);
            if !sent {
                let store = self.store.lock().unwrap();
                if let Some(it) = store.get(id).map_err(err)?
                    && it.state == State::Queued
                {
                    store.set_state(id, State::Paused, None).map_err(err)?;
                }
            }
        }
        self.changed();
        Ok(())
    }

    pub fn resume(self: &Arc<Self>, ids: &[i64]) -> Res<()> {
        for &id in ids {
            if let Some(it) = self.torrent_item(id)
                && matches!(it.state, State::Paused | State::Failed)
            {
                let _ = self.store.lock().unwrap().set_state(id, State::Downloading, None);
                let me = self.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = torrent::resume(&me, &it).await {
                        me.torrent_failed(it.id, &e);
                    }
                });
            }
        }
        {
            let store = self.store.lock().unwrap();
            for &id in ids {
                if store.get(id).ok().flatten().is_some_and(|it| it.kind == "torrent") {
                    continue;
                }
                if let Some(it) = store.get(id).map_err(err)?
                    && matches!(it.state, State::Paused | State::Failed)
                {
                    store.set_state(id, State::Queued, None).map_err(err)?;
                    self.retries.lock().unwrap().remove(&id);
                }
            }
        }
        self.changed();
        self.pump();
        Ok(())
    }

    /// Take downloads off the list; `delete_files` also deletes what they wrote (a finished
    /// file goes to the Recycle Bin, never deleted outright).
    pub fn remove(self: &Arc<Self>, ids: &[i64], delete_files: bool) -> Res<()> {
        for &id in ids {
            if let Some(it) = self.torrent_item(id) {
                let _ = self.store.lock().unwrap().remove(id);
                let me = self.clone();
                tauri::async_runtime::spawn(async move { let _ = torrent::remove(&me, &it, delete_files).await; });
                continue;
            }
            let sent = self
                .running
                .lock()
                .unwrap()
                .get(&id)
                .map(|r| r.ctl.send(Ctl::Remove { delete_files }).is_ok())
                .unwrap_or(false);
            if sent {
                continue;
            }
            let item = self.store.lock().unwrap().get(id).map_err(err)?;
            if let Some(item) = item {
                if delete_files {
                    discard_files(&item);
                }
                self.store.lock().unwrap().remove(id).map_err(err)?;
            }
        }
        self.changed();
        Ok(())
    }

    /// Clear the list: `finished`, `failed` (failed and stopped), `missing` (finished whose file is
    /// gone) or `all`; with `delete_files`, their files too (to the Recycle Bin). How many.
    pub fn clear_list(self: &Arc<Self>, which: &str, delete_files: bool) -> Res<usize> {
        let items = self.store.lock().unwrap().all().map_err(err)?;
        let ids: Vec<i64> = items
            .iter()
            .filter(|i| match which {
                "finished" => i.state == State::Done,
                "failed" => i.state == State::Failed,
                "missing" => i.state == State::Done && i.file.as_deref().is_some_and(|f| !Path::new(f).exists()),
                "all" => true,
                _ => false,
            })
            .map(|i| i.id)
            .collect();
        self.remove(&ids, delete_files && which != "missing")?;
        Ok(ids.len())
    }

    /// Start a download again from nothing (its file, if any, to the Recycle Bin first).
    pub fn redownload(self: &Arc<Self>, ids: &[i64]) -> Res<()> {
        for &id in ids {
            let item = self.item(id)?;
            if item.kind == "torrent" {
                continue;
            }
            if self.running.lock().unwrap().contains_key(&id) {
                return Err("Pause it first, then download it again.".into());
            }
            if let Some(f) = item.file.as_deref() {
                let p = Path::new(f);
                if item.kind == "file" {
                    if p.is_file() {
                        let _ = trash::delete(p);
                    }
                    for t in store::temp_files(p) {
                        let _ = std::fs::remove_file(t);
                    }
                }
            }
            self.store.lock().unwrap().reset(id).map_err(err)?;
            self.retries.lock().unwrap().remove(&id);
        }
        self.changed();
        self.pump();
        Ok(())
    }

    /// Windows' "Open with" chooser for a finished download.
    pub fn open_with(&self, id: i64) -> Res<()> {
        let item = self.item(id)?;
        let f = item.file.ok_or("It has no file yet.")?;
        if !Path::new(&f).exists() {
            return Err("The file isn't there any more.".into());
        }
        #[cfg(windows)]
        {
            std::process::Command::new("rundll32.exe")
                .arg("shell32.dll,OpenAs_RunDLL")
                .arg(&f)
                .spawn()
                .map_err(err)?;
        }
        Ok(())
    }

    pub fn item(&self, id: i64) -> Res<Item> {
        self.store
            .lock()
            .unwrap()
            .get(id)
            .map_err(err)?
            .ok_or_else(|| "That download is no longer in the list.".into())
    }

    /// Start queued downloads while there is room.
    pub fn held(&self) -> bool {
        self.held.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_held(&self, held: bool) {
        self.held.store(held, std::sync::atomic::Ordering::Relaxed);
    }

    /// The download hours have ended: what runs goes back to the queue.
    pub fn hold_running(&self) {
        for r in self.running.lock().unwrap().values() {
            let _ = r.ctl.send(Ctl::Hold);
        }
    }

    /// Downloads running, and waiting in the queue.
    pub fn activity(&self) -> (usize, usize) {
        let running = self.running.lock().unwrap().len();
        let queued = self.store.lock().unwrap().all().unwrap_or_default().iter().filter(|i| i.state == State::Queued).count();
        (running, queued)
    }

    /// The limit on all downloads together now (the scheduler's slow hours included).
    pub fn set_limit(&self, bps: Option<u64>) {
        self.global.set(bps);
    }

    pub fn pump(self: &Arc<Self>) {
        if self.held() {
            self.changed();
            return;
        }
        let s = self.settings();
        // Main (and any download whose queue is gone), then each of the person's queues: each
        // starts its own downloads up to its own number at once.
        let known: Vec<String> = s.queues.iter().map(|q| q.id.clone()).collect();
        let mut places: Vec<(Option<String>, usize)> = Vec::new();
        if !s.main_paused {
            places.push((None, s.max_parallel as usize));
        }
        for q in s.queues.iter().filter(|q| crate::scheduler::queue_open(q)) {
            places.push((Some(q.id.clone()), q.parallel as usize));
        }
        let mut running = self.running.lock().unwrap();
        // A queue stopped, or out of its hours: what it runs goes back in line (as the download
        // hours do for Main).
        for r in running.values() {
            let open = match s.queues.iter().find(|q| q.id == r.queue) {
                Some(q) => crate::scheduler::queue_open(q),
                None => !s.main_paused,
            };
            if !open {
                let _ = r.ctl.send(Ctl::Hold);
            }
        }
        for (queue, max) in places {
            loop {
                let in_queue = |id: &str| match &queue {
                    Some(q) => id == q,
                    None => !known.iter().any(|k| k == id),
                };
                if running.values().filter(|r| in_queue(&r.queue)).count() >= max {
                    break;
                }
                let busy: Vec<i64> = running.keys().copied().collect();
                let next = match self.store.lock().unwrap().next_queued_where(&busy, |it| in_queue(&it.queue)) {
                    Ok(Some(it)) => it,
                    _ => break,
                };
                if self.store.lock().unwrap().set_state(next.id, State::Downloading, None).is_err() {
                    break;
                }
                let (tx, rx) = mpsc::unbounded_channel();
                let live = Arc::new(Mutex::new(Live {
                    done: next.done,
                    size: next.size,
                    ..Live::default()
                }));
                running.insert(next.id, Running { ctl: tx, live: live.clone(), queue: next.queue.clone() });
                let me = self.clone();
                tauri::async_runtime::spawn(async move { me.drive(next, rx, live).await });
            }
        }
        drop(running);
        self.changed();
    }

    /// The engine's options for `item`, and the cookies to send with it: the browser's (handed
    /// over with it) and the site-check session's for its host.
    fn options(&self, item: &Item) -> (Options, Option<String>) {
        let s = self.settings();
        let ctx = self.context.lock().unwrap().get(&item.id).cloned().unwrap_or_default();
        let host = url::Url::parse(&item.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default();
        let session = self.vault.for_host(&host);
        // A session's clearance is tied to the browser that earned it: ask as that browser.
        let (agent, fallback) = match (&session, &ctx.user_agent) {
            (Some(sess), _) => (sess.user_agent.clone(), None),
            (None, Some(browser)) => (browser.clone(), Some(UDM_AGENT.to_owned())),
            (None, None) => (UDM_AGENT.to_owned(), Some(BROWSER_AGENT.to_owned())),
        };
        let connections = match &session {
            Some(_) => s.connections.min(SESSION_CONNECTIONS),
            None => s.connections,
        };
        // A site-check session's clearance is for a few connections only: no boost there.
        let boost_to = if s.boost_connections && session.is_none() { udm_engine::download::MAX_CONNECTIONS } else { 0 };
        let cookies = [session.map(|s| s.cookies), ctx.cookies]
            .into_iter()
            .flatten()
            .filter(|c| !c.is_empty())
            .collect::<Vec<_>>();
        let cookies = (!cookies.is_empty()).then(|| cookies.join("; "));
        let mut opts = Options {
            user_agent: agent,
            agent_fallback: fallback,
            max_connections: connections,
            boost_to,
            global: Some(self.global.clone()),
            host_limits: Some(self.hosts.clone()),
            route: crate::scheduler::route(&s),
            proxy: s.engine_proxy(),
            // Its own limit, else its site's (Settings → Network → per-site limits).
            speed_limit: (item.limit_kib > 0)
                .then(|| item.limit_kib * 1024)
                .or_else(|| {
                    let host = url::Url::parse(&item.url).ok()?.host_str()?.to_ascii_lowercase();
                    s.site_limits
                        .iter()
                        .find(|l| l.kib > 0 && !l.host.trim().is_empty() && host.contains(&l.host.trim().to_ascii_lowercase()))
                        .map(|l| l.kib * 1024)
                }),
            checksum: item.checksum.as_deref().and_then(udm_engine::verify::Hash::parse),
            // A debrid account that takes this host turns its link into a direct one.
            // Else a file host whose page's button gives the file (fuckingfast.co).
            // A browser window's find first (`assist`), then a debrid account, then a file host
            // whose page's button gives the file (fuckingfast.co, pixeldrain, MediaFire).
            unlocker: match self.assisted.lock().unwrap().get(&item.id) {
                Some(found) => Some(crate::hosts::found(found.clone())),
                None => url::Url::parse(&item.url)
                    .ok()
                    .and_then(|u| self.accounts.debrid.unlocker_for(&u).or_else(|| crate::hosts::unlocker_for(&u))),
            },
            ..Options::default()
        };
        // Mark of the Web, as browsers do: Windows then warns before running a downloaded
        // program and opens Office files in Protected View.
        let host_url = item.url.clone();
        let referer = item.referer.clone();
        opts.on_file_created = Some(FileHook::new(move |part: &Path| {
            mark_of_the_web(part, &host_url, referer.as_deref());
        }));
        (opts, cookies)
    }

    /// One download's actor: start or resume it, report its progress, obey pause and remove.
    async fn drive(
        self: Arc<Self>,
        item: Item,
        mut rx: mpsc::UnboundedReceiver<Ctl>,
        live: Arc<Mutex<Live>>,
    ) {
        if item.kind == "video" {
            return self.drive_video(item, rx, live).await;
        }
        if item.kind == "gallery" {
            return self.drive_gallery(item, rx, live).await;
        }
        let id = item.id;
        let (opts, cookies) = self.options(&item);
        let mut headers = HeaderMap::new();
        if let Some(r) = item.referer.as_deref().and_then(|r| HeaderValue::from_str(r).ok()) {
            headers.insert(REFERER, r);
        }
        if let Some(c) = cookies.as_deref().and_then(|c| HeaderValue::from_str(c).ok()) {
            headers.insert(COOKIE, c);
        }
        let known = item.file.as_ref().map(PathBuf::from);
        let resumable = known
            .as_ref()
            .map(|f| udm_engine::state::state_path(f))
            .filter(|s| s.exists());
        let dl = match resumable {
            Some(state) => match Download::resume(state, headers.clone(), opts.clone()) {
                Ok(d) => d,
                Err(_) => self.fresh(&item, headers, opts),
            },
            None => self.fresh(&item, headers, opts),
        };

        let mut tick = tokio::time::interval(Duration::from_millis(250));
        let mut last = (Instant::now(), item.done);
        let mut speed = 0.0f64;
        let mut file_saved = false;
        let mut last_save = Instant::now();
        let mut stop: Option<Ctl> = None;
        let mut ctl_open = true;
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    let s = dl.snapshot();
                    let now = Instant::now();
                    let dt = now.duration_since(last.0).as_secs_f64();
                    if dt > 0.0 {
                        let inst = s.downloaded.saturating_sub(last.1) as f64 / dt;
                        speed = if speed == 0.0 { inst } else { speed * 0.75 + inst * 0.25 };
                    }
                    last = (now, s.downloaded);
                    let conns = s
                        .connections
                        .iter()
                        .filter(|c| matches!(c.status, ConnStatus::Receiving))
                        .count() as u32;
                    *live.lock().unwrap() = Live {
                        done: s.downloaded,
                        size: s.size,
                        speed,
                        conns,
                        segments: s.segments.iter().map(|g| (g.start, g.cursor, g.end)).collect(),
                        connections: s
                            .connections
                            .iter()
                            .map(|c| (c.downloaded, c.speed, matches!(c.status, ConnStatus::Receiving)))
                            .collect(),
                        resumable: s.resumable,
                        server: s.server.clone(),
                    };
                    if !file_saved && let Some(f) = s.file.as_deref() {
                        file_saved = true;
                        let store = self.store.lock().unwrap();
                        let _ = store.set_progress(id, Some(f), s.size, s.downloaded);
                        if let Some(n) = f.file_name() {
                            let _ = store.set_category(id, store::category_of(&n.to_string_lossy()));
                        }
                        drop(store);
                        self.changed();
                    } else if last_save.elapsed() > Duration::from_secs(2) {
                        last_save = Instant::now();
                        let _ = self.store.lock().unwrap().set_progress(id, None, s.size, s.downloaded);
                    }
                    if matches!(s.state, EngineState::Done | EngineState::Failed | EngineState::Cancelled) {
                        break;
                    }
                }
                c = rx.recv(), if ctl_open => match c {
                    Some(c) => {
                        dl.cancel();
                        stop = Some(c);
                    }
                    None => ctl_open = false,
                },
            }
        }

        let snap = dl.snapshot();
        let mut try_mirror = false;
        let result = dl.wait().await;
        let name = item.display_name();
        match (result, stop) {
            (Ok(out), Some(Ctl::Remove { delete_files })) => {
                if delete_files {
                    let _ = trash::delete(&out.path);
                }
                let _ = self.store.lock().unwrap().remove(id);
            }
            (Ok(out), _) => {
                {
                    let store = self.store.lock().unwrap();
                    let _ = store.set_progress(id, Some(&out.path), Some(out.size), out.size);
                    let _ = store.set_state(id, State::Done, None);
                }
                let shown = out
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or(name);
                self.notify("Download finished", &shown);
                self.complete_window(id);
                crate::postprocess::auto_convert(self.clone(), &item, out.path.clone());
                crate::postprocess::after_download(self.clone(), out.path.clone(), item.password.clone());
            }
            (Err(_), Some(Ctl::Hold)) => {
                let store = self.store.lock().unwrap();
                let _ = store.set_progress(id, snap.file.as_deref(), snap.size, snap.downloaded);
                let _ = store.set_state(id, State::Queued, None);
            }
            (Err(_), Some(Ctl::Pause)) => {
                let store = self.store.lock().unwrap();
                let _ = store.set_progress(id, snap.file.as_deref(), snap.size, snap.downloaded);
                let _ = store.set_state(id, State::Paused, None);
            }
            (Err(_), Some(Ctl::Remove { delete_files })) => {
                if delete_files && let Some(f) = snap.file.as_deref() {
                    for t in store::temp_files(f) {
                        let _ = std::fs::remove_file(t);
                    }
                }
                let _ = self.store.lock().unwrap().remove(id);
            }
            (Err(e), None) => {
                let text = e.to_string();
                let attempt = {
                    let mut tries = self.retries.lock().unwrap();
                    let n = tries.entry(id).or_insert(0);
                    *n += 1;
                    *n
                };
                let wait = e
                    .worth_retrying_later()
                    .then(|| RETRY_WAITS.get(attempt as usize - 1).copied())
                    .flatten();
                {
                    let store = self.store.lock().unwrap();
                    let _ = store.set_progress(id, snap.file.as_deref(), snap.size, snap.downloaded);
                    let shown = match wait {
                        Some(w) => format!(
                            "{text} · trying again in {w} s (attempt {} of {})",
                            attempt + 1,
                            RETRY_WAITS.len() + 1
                        ),
                        None if needs_browser(&e) && person_host(&item.url) => format!(
                            "{ASSIST_NEEDED}this file host wants you to wait or solve a captcha: get it in a browser window"
                        ),
                        None if needs_browser(&e) => format!("{CHECK_NEEDED}{}", check_reason(&e)),
                        None => text.clone(),
                    };
                    let _ = store.set_state(id, State::Failed, Some(&shown));
                }
                // Failed for good: the next mirror, if it has one.
                if wait.is_none() && !mirrors_of(&item).is_empty() {
                    try_mirror = true;
                }
                if needs_browser(&e) && person_host(&item.url) {
                    self.notify(
                        "A file host needs you",
                        &format!("{name}: open it in Ultimate Download Manager and choose Get it in a browser window."),
                    );
                } else if needs_browser(&e) {
                    let _ = self.app.emit(
                        EV_NEEDS_CHECK,
                        serde_json::json!({ "id": id, "url": item.url }),
                    );
                    self.notify(
                        "Site check needed",
                        &format!("{name}: the site wants a real browser first. Open Ultimate Download Manager to pass its check once."),
                    );
                } else {
                    if limit_hit(&e) {
                        self.reconnect();
                    }
                    match wait {
                        Some(w) => self.retry_later(id, Duration::from_secs(w)),
                        None => self.notify("Download failed", &format!("{name}: {text}")),
                    }
                }
            }
        }
        if matches!(self.store.lock().unwrap().get(id), Ok(Some(ref it)) if it.state == State::Done) {
            self.retries.lock().unwrap().remove(&id);
        }
        self.running.lock().unwrap().remove(&id);
        if try_mirror {
            self.next_mirror(id);
        }
        self.changed();
        self.pump();
    }

    /// Move downloads to `queue` (Main, or one of the person's).
    pub fn set_queue(self: &Arc<Self>, ids: &[i64], queue: &str) -> Res<()> {
        let s = self.settings();
        if queue != crate::settings::MAIN_QUEUE && !s.queues.iter().any(|q| q.id == queue) {
            return Err("That queue no longer exists.".into());
        }
        {
            let store = self.store.lock().unwrap();
            for id in ids {
                store.set_queue(*id, queue).map_err(err)?;
            }
        }
        self.changed();
        self.pump();
        Ok(())
    }

    /// The person's tags for downloads (words; `#` optional), all set at once.
    pub fn set_tags(&self, ids: &[i64], tags: &str) -> Res<()> {
        let mut words: Vec<String> = tags
            .split(|c: char| c.is_whitespace() || c == ',')
            .map(|w| w.trim_start_matches('#').to_lowercase())
            .filter(|w| !w.is_empty())
            .collect();
        words.dedup();
        let store = self.store.lock().unwrap();
        for id in ids {
            store.set_tags(*id, &words.join(" ")).map_err(err)?;
        }
        drop(store);
        self.changed();
        Ok(())
    }

    /// Settings → General: finished downloads older than the chosen days leave the list.
    fn forget_old(&self) {
        let days = self.settings().forget_after_days;
        if days == 0 {
            return;
        }
        let before = store::now() - (days as i64) * 86_400;
        if matches!(self.store.lock().unwrap().forget_finished(before), Ok(n) if n > 0) {
            self.changed();
        }
    }

    /// Other addresses for the same file (from the grabber, or added by hand), kept with it.
    pub fn add_mirrors(&self, id: i64, urls: &[String]) -> Res<()> {
        let item = self.item(id)?;
        if item.kind != "file" {
            return Err("Only file downloads take mirrors.".into());
        }
        let mut all = mirrors_of(&item);
        for u in urls {
            let u = u.trim();
            if url::Url::parse(u).is_ok() && u != item.url && !all.iter().any(|x| x == u) {
                all.push(u.to_owned());
            }
        }
        self.set_option(&item, "mirrors", serde_json::json!(all))?;
        self.changed();
        Ok(())
    }

    /// One key of a file download's options (`mirrors`, `rule`), the others kept.
    fn set_option(&self, item: &Item, key: &str, value: serde_json::Value) -> Res<()> {
        let mut v = item
            .options
            .as_deref()
            .and_then(|o| serde_json::from_str::<serde_json::Value>(o).ok())
            .filter(|v| v.is_object())
            .unwrap_or_else(|| serde_json::json!({}));
        v[key] = value;
        self.store.lock().unwrap().set_options(item.id, &v.to_string(), None).map_err(err)?;
        self.changed();
        Ok(())
    }

    /// A failed download moves on to its next mirror (its data kept when the file is the same).
    fn next_mirror(self: &Arc<Self>, id: i64) {
        let Ok(item) = self.item(id) else { return };
        let mut rest = mirrors_of(&item);
        if rest.is_empty() {
            return;
        }
        let next = rest.remove(0);
        let _ = self.set_option(&item, "mirrors", serde_json::json!(rest));
        // Another server's copy has its own ETag and date: only the size is compared then (a
        // different size still starts over), so the part already downloaded is kept.
        if let Some(f) = item.file.as_deref() {
            let state = udm_engine::state::state_path(Path::new(f));
            if let Ok(text) = std::fs::read_to_string(&state)
                && let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&text)
            {
                v["validators"] = serde_json::json!({ "etag": null, "etag_host": null, "last_modified": null });
                if let Some(o) = v.as_object_mut() {
                    o.remove("if_range");
                }
                if let Ok(bytes) = serde_json::to_vec(&v) {
                    let _ = std::fs::write(&state, bytes);
                }
            }
        }
        let name = item.display_name();
        match self.refresh_address(id, &next) {
            Ok(()) => self.notify(
                "Trying a mirror",
                &format!("{name}: {}", url::Url::parse(&next).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or(next.clone())),
            ),
            Err(e) => self.notify("Mirror not used", &format!("{name}: {e}")),
        }
    }

    /// A stopped download of the same file the browser just handed over (same name, and the
    /// same size when both are known): it takes the fresh address and resumes, instead of a
    /// second copy starting.
    pub fn refresh_from_browser(self: &Arc<Self>, url: &str, name: Option<&str>, size: Option<u64>, ctx: Context) -> Option<i64> {
        let name = name
            .map(str::to_owned)
            .or_else(|| url::Url::parse(url).ok()?.path_segments()?.next_back().map(str::to_owned))?
            .to_ascii_lowercase();
        if name.is_empty() {
            return None;
        }
        let items = self.store.lock().unwrap().all().ok()?;
        let it = items.into_iter().find(|i| {
            i.kind == "file"
                && matches!(i.state, State::Paused | State::Failed)
                && i.url != url
                && i.done > 0
                && i.display_name().to_ascii_lowercase() == name
                && match (i.size, size) {
                    (Some(a), Some(b)) => a == b,
                    _ => true,
                }
        })?;
        self.context.lock().unwrap().insert(it.id, ctx);
        self.refresh_address(it.id, url).ok()?;
        Some(it.id)
    }

    /// Queue a video (or song) from a page, to fetch with yt-dlp.
    pub fn add_video(
        self: &Arc<Self>,
        page: &str,
        title: &str,
        opts: &VideoOpts,
        dir: Option<PathBuf>,
        referer: Option<String>,
    ) -> Res<i64> {
        let parsed = url::Url::parse(page.trim()).map_err(|e| format!("That isn't a web address ({e})"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("Videos come from web pages (http or https).".into());
        }
        let dir = dir.unwrap_or_else(|| self.default_dir(video::category(opts)));
        let name = format!("{}.{}", video::stem(title), opts.ext);
        let json = serde_json::to_string(opts).map_err(err)?;
        let id = self
            .store
            .lock()
            .unwrap()
            .add_video(parsed.as_str(), &name, &dir, video::category(opts), referer.as_deref(), &json)
            .map_err(err)?;
        if let Some(ctx) = self.pending.lock().unwrap().get(parsed.as_str()).cloned() {
            self.context.lock().unwrap().insert(id, ctx);
        }
        self.changed();
        self.pump();
        Ok(id)
    }

    /// Queue an image gallery (a page gallery-dl reads): its files go in a folder of its own.
    /// `limit`: at most this many files (0: all).
    pub fn add_gallery(self: &Arc<Self>, page: &str, dir: Option<PathBuf>, limit: u64) -> Res<i64> {
        let parsed = url::Url::parse(page.trim()).map_err(|e| format!("That isn't a web address ({e})"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("Galleries come from web pages (http or https).".into());
        }
        let dir = dir.unwrap_or_else(|| self.default_dir("images"));
        let name = udm_tools::gallery::name_of(&parsed);
        let folder = udm_engine::naming::unique_path(&dir, &udm_engine::naming::sanitize(&name));
        let opts = serde_json::json!({ "limit": limit }).to_string();
        let id = self
            .store
            .lock()
            .unwrap()
            .add_gallery(parsed.as_str(), &name, &dir, &folder, &opts)
            .map_err(err)?;
        if let Some(ctx) = self.pending.lock().unwrap().get(parsed.as_str()).cloned() {
            self.context.lock().unwrap().insert(id, ctx);
        }
        self.changed();
        self.pump();
        Ok(id)
    }

    async fn drive_gallery(self: Arc<Self>, item: Item, mut rx: mpsc::UnboundedReceiver<Ctl>, live: Arc<Mutex<Live>>) {
        let id = item.id;
        let name = item.display_name();
        let limit = item
            .options
            .as_deref()
            .and_then(|o| serde_json::from_str::<serde_json::Value>(o).ok())
            .and_then(|v| v.get("limit").and_then(|l| l.as_u64()))
            .filter(|l| *l > 0);
        let ctx = self.context.lock().unwrap().get(&id).cloned().unwrap_or_default();
        let browser = video::from_browser(item.referer.clone(), &ctx);
        let folder = item.file.clone().map(PathBuf::from).unwrap_or_else(|| PathBuf::from(&item.dir).join(&name));
        let rate = (item.limit_kib > 0).then(|| item.limit_kib * 1024);
        let started = match video::gallery_dl().await {
            Ok(tool) => udm_tools::gallery::GalleryJob::start(
                &tool,
                &item.url,
                &folder,
                &udm_tools::gallery::Ask { limit, rate },
                &browser,
            )
            .map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        let job = match started {
            Ok(j) => j,
            Err(e) => {
                let _ = self.store.lock().unwrap().set_state(id, State::Failed, Some(&e));
                self.notify("Gallery failed", &format!("{name}: {e}"));
                return self.finished(id);
            }
        };
        let mut tick = tokio::time::interval(Duration::from_millis(400));
        let mut stop: Option<Ctl> = None;
        let mut ctl_open = true;
        let end = loop {
            tokio::select! {
                _ = tick.tick() => {
                    let p = job.progress();
                    *live.lock().unwrap() = Live { done: p.downloaded, size: p.total, speed: p.speed, conns: 1, ..Live::default() };
                    if !matches!(p.state, udm_tools::video::State::Running) {
                        break p;
                    }
                }
                c = rx.recv(), if ctl_open => match c {
                    Some(c) => {
                        job.stop();
                        stop = Some(c);
                    }
                    None => ctl_open = false,
                },
            }
        };
        use udm_tools::video::State as V;
        let count = job.saved().len();
        match (end.state, stop) {
            (_, Some(Ctl::Remove { delete_files })) => {
                if delete_files {
                    let _ = trash::delete(&folder);
                }
                let _ = self.store.lock().unwrap().remove(id);
            }
            (V::Done(_), _) => {
                {
                    let store = self.store.lock().unwrap();
                    let _ = store.set_progress(id, Some(&folder), Some(end.downloaded), end.downloaded);
                    let _ = store.set_state(id, State::Done, None);
                }
                self.notify("Gallery finished", &format!("{name}: {count} new files"));
            }
            (_, Some(Ctl::Hold)) => {
                let _ = self.store.lock().unwrap().set_state(id, State::Queued, None);
            }
            (_, Some(Ctl::Pause)) | (V::Stopped, None) => {
                let _ = self.store.lock().unwrap().set_state(id, State::Paused, None);
            }
            (V::Failed(msg), None) => {
                let _ = self.store.lock().unwrap().set_state(id, State::Failed, Some(&msg));
                self.notify("Gallery failed", &format!("{name}: {msg}"));
            }
            (V::Running, None) => {}
        }
        self.finished(id);
    }

    /// What the browser sent with a video page it handed over, until the page is looked up and
    /// queued.
    pub fn set_pending(&self, page: &str, ctx: Context) {
        let mut p = self.pending.lock().unwrap();
        if p.len() > 50 {
            p.clear();
        }
        p.insert(page.to_owned(), ctx);
    }

    pub fn pending(&self, page: &str) -> Context {
        self.pending.lock().unwrap().get(page).cloned().unwrap_or_default()
    }

    /// One video's actor: yt-dlp under supervision (`udm_tools::video::VideoJob`).
    async fn drive_video(
        self: Arc<Self>,
        item: Item,
        mut rx: mpsc::UnboundedReceiver<Ctl>,
        live: Arc<Mutex<Live>>,
    ) {
        let id = item.id;
        let name = item.display_name();
        let opts: VideoOpts = item
            .options
            .as_deref()
            .and_then(|o| serde_json::from_str(o).ok())
            .unwrap_or_default();
        let ctx = self.context.lock().unwrap().get(&id).cloned().unwrap_or_default();
        let browser = video::from_browser(item.referer.clone(), &ctx);
        let dir = PathBuf::from(&item.dir);
        let stem = item
            .name
            .as_deref()
            .and_then(|n| Path::new(n).file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "video".into());
        let rate = {
            let s = self.settings();
            (s.global_limit_kib > 0).then(|| s.global_limit_kib * 1024)
        };
        let subs = udm_tools::video::Subs {
            langs: opts.subs.clone(),
            automatic: opts.automatic_subs,
            embed: opts.embed_subs,
            extra: opts.extra(&dir, &stem),
        };

        let started = match video::ytdlp().await {
            Ok(exe) => {
                let ffmpeg = video::ffmpeg();
                let (url, selector) = (item.url.clone(), opts.selector.clone());
                let expected = opts.expected;
                let dir2 = dir.clone();
                let stem2 = stem.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    udm_tools::video::VideoJob::start(
                        &exe,
                        &url,
                        &selector,
                        &dir2,
                        &stem2,
                        rate,
                        ffmpeg.as_deref(),
                        expected,
                        &browser,
                        &subs,
                    )
                    .map_err(|e| e.to_string())
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r)
            }
            Err(e) => Err(e),
        };
        let job = match started {
            Ok(j) => j,
            Err(e) => {
                let _ = self.store.lock().unwrap().set_state(id, State::Failed, Some(&e));
                self.notify("Video failed", &format!("{name}: {e}"));
                return self.finished(id);
            }
        };

        let mut tick = tokio::time::interval(Duration::from_millis(300));
        let mut stop: Option<Ctl> = None;
        let mut ctl_open = true;
        let mut last_save = Instant::now();
        let end = loop {
            tokio::select! {
                _ = tick.tick() => {
                    let p = job.progress();
                    *live.lock().unwrap() = Live { done: p.downloaded, size: p.total, speed: p.speed, conns: 1, ..Live::default() };
                    if last_save.elapsed() > Duration::from_secs(2) {
                        last_save = Instant::now();
                        let _ = self.store.lock().unwrap().set_progress(id, None, p.total, p.downloaded);
                    }
                    if !matches!(p.state, udm_tools::video::State::Running) {
                        break p;
                    }
                }
                c = rx.recv(), if ctl_open => match c {
                    Some(c) => {
                        job.stop();
                        stop = Some(c);
                    }
                    None => ctl_open = false,
                },
            }
        };

        use udm_tools::video::State as V;
        match (end.state, stop) {
            (V::Done(path), Some(Ctl::Remove { delete_files })) => {
                if delete_files {
                    let _ = trash::delete(&path);
                }
                let _ = self.store.lock().unwrap().remove(id);
            }
            (V::Done(path), _) => {
                let size = std::fs::metadata(&path).map(|m| m.len()).ok();
                {
                    let store = self.store.lock().unwrap();
                    let _ = store.set_progress(id, Some(&path), size, size.unwrap_or(end.downloaded));
                    let _ = store.set_state(id, State::Done, None);
                }
                mark_of_the_web(&path, &item.url, item.referer.as_deref());
                self.complete_window(id);
                crate::postprocess::auto_convert(self.clone(), &item, path.clone());
                self.notify(
                    "Download finished",
                    &path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(name),
                );
            }
            (_, Some(Ctl::Remove { delete_files })) => {
                if delete_files {
                    remove_video_parts(&dir, &stem);
                }
                let _ = self.store.lock().unwrap().remove(id);
            }
            (_, Some(Ctl::Hold)) => {
                let store = self.store.lock().unwrap();
                let _ = store.set_progress(id, None, end.total, end.downloaded);
                let _ = store.set_state(id, State::Queued, None);
            }
            (_, Some(Ctl::Pause)) | (V::Stopped, None) => {
                let store = self.store.lock().unwrap();
                let _ = store.set_progress(id, None, end.total, end.downloaded);
                let _ = store.set_state(id, State::Paused, None);
            }
            (V::Failed(msg), None) => {
                let _ = self.store.lock().unwrap().set_state(id, State::Failed, Some(&msg));
                self.notify("Video failed", &format!("{name}: {msg}"));
            }
            (V::Running, None) => {}
        }
        self.finished(id);
    }

    /// A download's actor has ended: make room for the next.
    fn finished(self: &Arc<Self>, id: i64) {
        self.running.lock().unwrap().remove(&id);
        self.changed();
        self.pump();
    }

    /// Queue a failed download again after `wait`, unless the user has acted on it meanwhile.
    fn retry_later(self: &Arc<Self>, id: i64, wait: Duration) {
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(wait).await;
            let still_failed = matches!(
                me.store.lock().unwrap().get(id),
                Ok(Some(ref it)) if it.state == State::Failed
            );
            if still_failed {
                let _ = me.store.lock().unwrap().set_state(id, State::Queued, None);
                me.changed();
                me.pump();
            }
        });
    }

    fn fresh(&self, item: &Item, headers: HeaderMap, opts: Options) -> Download {
        let dir = PathBuf::from(&item.dir);
        let dest = match (&item.file, &item.name) {
            // Started before but its resume file is gone: start over in the same place.
            (Some(f), _) => Destination::File(PathBuf::from(f)),
            (None, Some(n)) => Destination::File(udm_engine::naming::unique_path(&dir, n)),
            (None, None) => Destination::Dir(dir),
        };
        let url = url::Url::parse(&item.url).expect("stored addresses were parsed when added");
        Download::start(Request { url, headers, dest }, opts)
    }

    async fn tick_forever(self: Arc<Self>) {
        let mut every = tokio::time::interval(Duration::from_millis(500));
        let mut was_busy = false;
        let mut beats: u64 = 0;
        loop {
            every.tick().await;
            // Once an hour (and soon after the start): the list's old finished downloads.
            if beats % 7200 == 20 {
                self.forget_old();
            }
            beats += 1;
            let ticks: Vec<Tick> = self
                .running
                .lock()
                .unwrap()
                .iter()
                .map(|(id, r)| {
                    let l = r.live.lock().unwrap();
                    Tick {
                        id: *id,
                        done: l.done,
                        size: l.size,
                        speed: l.speed as u64,
                        eta: eta(l.size, l.done, l.speed),
                        conns: l.conns,
                    }
                })
                .collect();
            let mut ticks = ticks;
            for (id, (done, total, speed, peers)) in self.torrent_live.lock().unwrap().iter() {
                ticks.push(Tick {
                    id: *id,
                    done: *done,
                    size: Some(*total),
                    speed: *speed as u64,
                    eta: eta(Some(*total), *done, *speed),
                    conns: *peers,
                });
            }
            if !ticks.is_empty() || was_busy {
                let _ = self.app.emit(EV_PROGRESS, &ticks);
                self.taskbar(&ticks);
            }
            was_busy = !ticks.is_empty();
        }
    }

    /// The taskbar button fills with the progress of everything downloading (as the and the
    /// browsers'), and the tray icon's tooltip says how many and how fast.
    fn taskbar(&self, ticks: &[Tick]) {
        use tauri::Manager as _;
        use tauri::window::{ProgressBarState, ProgressBarStatus};
        let (done, size): (u64, u64) = ticks.iter().filter_map(|t| t.size.map(|s| (t.done, s))).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
        let speed: u64 = ticks.iter().map(|t| t.speed).sum();
        if let Some(w) = self.app.get_webview_window("main") {
            let state = if ticks.is_empty() {
                ProgressBarState { status: Some(ProgressBarStatus::None), progress: None }
            } else if size > 0 {
                ProgressBarState { status: Some(ProgressBarStatus::Normal), progress: Some((done * 100 / size).min(100)) }
            } else {
                ProgressBarState { status: Some(ProgressBarStatus::Indeterminate), progress: None }
            };
            let _ = w.set_progress_bar(state);
        }
        if let Some(tray) = self.app.tray_by_id("udm") {
            let tip = if ticks.is_empty() {
                "Ultimate Download Manager".to_owned()
            } else {
                format!(
                    "Ultimate Download Manager\n{} downloading · {}/s{}",
                    ticks.len(),
                    udm_engine::disk::human(speed),
                    if size > 0 { format!(" · {}%", (done * 100 / size).min(100)) } else { String::new() }
                )
            };
            let _ = tray.set_tooltip(Some(tip));
        }
    }

    /// The Download complete window (Settings), unless a window already follows this download.
    fn complete_window(&self, id: i64) {
        let s = self.settings();
        if !s.complete_window {
            return;
        }
        // Small files only notify; and when many finish at once, no pile of windows.
        let size = self.item(id).ok().and_then(|i| i.size).unwrap_or(0);
        if size < s.complete_min_mb.saturating_mul(1 << 20) {
            return;
        }
        {
            static RECENT: Mutex<Vec<Instant>> = Mutex::new(Vec::new());
            let mut recent = RECENT.lock().unwrap();
            recent.retain(|t| t.elapsed() < Duration::from_secs(15));
            if recent.len() >= 3 {
                return;
            }
            recent.push(Instant::now());
        }
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            if !crate::newdl::watched(&app, id) {
                let _ = crate::newdl::show_progress(&app, id);
            }
        });
    }

    fn changed(&self) {
        let _ = self.app.emit(EV_CHANGED, ());
    }

    /// Bring the window up on the torrent dialog for `source` (a .torrent file or magnet link).
    pub fn show_torrent(&self, source: &Path) {
        use tauri::Manager as _;
        if let Some(w) = self.app.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
        let _ = self.app.emit(
            crate::bridge::EV_SHOW,
            serde_json::json!({ "torrent": source.display().to_string() }).to_string(),
        );
    }

    pub fn notify_pub(&self, title: &str, body: &str) {
        self.notify(title, body);
    }

    fn notify(&self, title: &str, body: &str) {
        if self.settings().notify {
            let _ = self.app.notification().builder().title(title).body(body).show();
        }
    }
}

/// A failure only a real browser gets past: a bot check (Cloudflare and the like), or a page
/// where the file should be (a sign-in or a "click to continue").
/// A file host's limit for this address (too many requests, its wait): the moment to switch
/// address with the reconnect command.
fn limit_hit(e: &udm_engine::Error) -> bool {
    use udm_engine::{Error, Refusal, ServiceFault};
    matches!(e, Error::Throttled(..))
        || matches!(e, Error::Refused(Refusal::Service { fault: ServiceFault::TooManyRequests(_) | ServiceFault::LimitReached, .. }))
}

/// A file download's mirrors (its options' `mirrors`).
pub fn mirrors_of(item: &Item) -> Vec<String> {
    if item.kind != "file" {
        return Vec::new();
    }
    item.options
        .as_deref()
        .and_then(|o| serde_json::from_str::<serde_json::Value>(o).ok())
        .and_then(|v| v.get("mirrors").cloned())
        .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
        .unwrap_or_default()
}

fn person_host(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| crate::hosts::needs_a_person(&u))
}

fn needs_browser(e: &udm_engine::Error) -> bool {
    matches!(e, udm_engine::Error::BotCheck | udm_engine::Error::GotWebPage)
}

fn check_reason(e: &udm_engine::Error) -> &'static str {
    match e {
        udm_engine::Error::BotCheck => "the site checks for a real browser",
        _ => "the site showed a web page (a sign-in, a wait or a captcha) instead of the file",
    }
}

/// `a` is `b` or one of its subdomains, or the other way round.
pub fn same_site(a: &str, b: &str) -> bool {
    let (a, b) = (a.to_ascii_lowercase(), b.to_ascii_lowercase());
    a == b || a.ends_with(&format!(".{b}")) || b.ends_with(&format!(".{a}"))
}

fn eta(size: Option<u64>, done: u64, speed: f64) -> Option<u64> {
    let size = size?;
    (speed >= 1.0 && size > done).then(|| ((size - done) as f64 / speed).ceil() as u64)
}

/// What a removed download wrote: its part and resume files, and a finished file (to the
/// Recycle Bin).
fn discard_files(item: &Item) {
    let Some(file) = item.file.as_deref().map(Path::new) else {
        return;
    };
    for t in store::temp_files(file) {
        let _ = std::fs::remove_file(t);
    }
    if item.state == State::Done && file.exists() {
        let _ = trash::delete(file);
    }
}

/// What yt-dlp leaves of an unfinished video: `<stem>.*.part`, `<stem>.*.ytdl` and the like.
fn remove_video_parts(dir: &Path, stem: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.starts_with(stem) && (n.ends_with(".part") || n.ends_with(".ytdl") || n.contains(".part-Frag")) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Windows' Zone.Identifier stream: "this came from the Internet" (zone 3), with its addresses.
pub fn mark_of_the_web(part: &Path, host_url: &str, referer: Option<&str>) {
    #[cfg(windows)]
    {
        let mut ads = part.as_os_str().to_owned();
        ads.push(":Zone.Identifier");
        let mut text = String::from("[ZoneTransfer]\r\nZoneId=3\r\n");
        if let Some(r) = referer.filter(|r| r.starts_with("http")) {
            text.push_str(&format!("ReferrerUrl={r}\r\n"));
        }
        if host_url.starts_with("http") {
            text.push_str(&format!("HostUrl={host_url}\r\n"));
        }
        let _ = std::fs::write(PathBuf::from(ads), text);
    }
    #[cfg(not(windows))]
    let _ = (part, host_url, referer);
}

#[cfg(test)]
mod tests {
    use super::eta;

    #[test]
    fn eta_needs_size_and_speed() {
        assert_eq!(eta(Some(1000), 400, 100.0), Some(6));
        assert_eq!(eta(None, 400, 100.0), None);
        assert_eq!(eta(Some(1000), 400, 0.0), None);
        assert_eq!(eta(Some(1000), 1000, 50.0), None);
    }
}
