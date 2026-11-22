//! Ultimate Download Manager's settings: `<data>/settings.json`, written whole and atomically (temp file + rename).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Where new downloads go.
    pub download_dir: PathBuf,
    /// Downloads running at the same time; the rest wait in the queue.
    pub max_parallel: u32,
    /// Connections per download.
    pub connections: u32,
    /// Add connections (up to 32) while each one still makes a download faster: for sites
    /// that cap every connection's speed.
    pub boost_connections: bool,
    /// A cap on all downloads together, KiB/s (0: none).
    pub global_limit_kib: u64,
    /// A notification when a download finishes or fails.
    pub notify: bool,
    /// Closing the window keeps Ultimate Download Manager running in the notification area.
    pub close_to_tray: bool,
    /// Keep sharing a torrent once it has finished (off: it stops when done).
    pub seed_when_done: bool,
    /// Unpack downloaded archives (zip, 7z, rar, tar…) next to them.
    pub extract_archives: bool,
    /// Then send the archive to the Recycle Bin.
    pub delete_archives_after_extract: bool,
    /// Have Microsoft Defender scan each finished download.
    pub scan_downloads: bool,
    /// Save into a folder per type (Video, Music, Documents…) inside the download folder.
    pub sort_by_type: bool,
    /// Offer the links copied anywhere (off: Ultimate Download Manager doesn't read the clipboard).
    pub watch_clipboard: bool,
    /// The network adapter every connection goes through (a VPN's); empty: any.
    pub adapter: String,
    /// Download hours: start only inside them.
    pub schedule_enabled: bool,
    pub schedule_start: String,
    pub schedule_stop: String,
    /// Monday first.
    pub schedule_days: Vec<bool>,
    /// When the queue is done inside the hours: `nothing`, `sleep` or `shutdown`.
    pub when_done: String,
    /// A slower speed during these hours.
    pub slow_enabled: bool,
    pub slow_from: String,
    pub slow_to: String,
    pub slow_limit_kib: u64,
    /// The phone remote: a page on the home network, for paired devices only.
    pub remote_on: bool,
    pub remote_port: u16,
    /// Where updates come from: a public GitHub repository holding only releases ("name/ultimate-download-manager-releases").
    pub update_repo: String,
    /// Look for an update at each start.
    pub check_updates: bool,

    // ---- Torrents
    /// The port peers reach this computer on (0: one picked at each start).
    pub torrent_port: u16,
    /// Find peers without trackers (DHT).
    pub torrent_dht: bool,
    /// Ask the router to open the port (UPnP).
    pub torrent_upnp: bool,
    /// All torrents together, KiB/s (0: no limit).
    pub torrent_down_kib: u64,
    pub torrent_up_kib: u64,
    /// Stop sharing a finished torrent once it has sent this many times its size (0: no limit).
    pub seed_ratio: f64,
    /// ... or after sharing this many minutes (0: no limit).
    pub seed_minutes: u64,
    /// An IP blocklist (P2P or eMule .dat, gzipped or not): peers in it are refused.
    pub torrent_blocklist: String,
    /// Torrent RSS feeds checked every `rss_minutes`.
    pub feeds: Vec<Feed>,
    pub rss_minutes: u64,

    // ---- Network
    /// Empty: direct; `system`: Windows' settings; `http://host:port` or `socks5://host:port`
    /// (a login as `user:pass@`); or `pac:<address of the script>`.
    pub proxy: String,
    /// A command run when a file host says to wait (its limit for this address): a VPN's
    /// command line to switch server, for example. Empty: none.
    pub reconnect_command: String,

    // ---- Archives and rules
    /// Passwords tried, in order, on a password-protected archive.
    pub archive_passwords: Vec<String>,
    /// What happens to new links by host and name, in order (the first that matches applies).
    pub rules: Vec<Rule>,

    // ---- The app
    /// `dock` (the default), `dark` (Obsidian), `light` or `system`.
    pub theme: String,
    /// Dock became the default: a settings file from before it, still on the old
    /// default `dark`, is moved to Dock once. Missing in older files, so false there.
    #[serde(default)]
    pub theme_dock_seen: bool,
    /// Start with Windows, in the notification area.
    pub autostart: bool,
    /// A sound when a download finishes.
    pub sound: bool,
    /// Ask where to save each download the browser hands over ("Download file info").
    pub ask_where: bool,
    /// File types (extensions) never asked about.
    pub ask_skip_types: Vec<String>,
    /// The folder last chosen for each category.
    pub last_dirs: std::collections::HashMap<String, String>,
    /// The main window: `split` (a list, and the chosen download in full beside it) or `table`.
    pub layout: String,
    /// Magnet links and .torrent files open in Ultimate Download Manager.
    pub handle_torrents: bool,
    /// "Download links with Ultimate Download Manager" on text files' right-click menu.
    pub txt_menu: bool,
    /// A shortcut that works in any program: download the link just copied (empty: none).
    pub hotkey: String,
    /// Show the Download complete window when a download finishes.
    pub complete_window: bool,
    /// … for files at least this big, MB (small ones only notify).
    pub complete_min_mb: u64,
    /// The person has answered "open magnet links with Ultimate Download Manager?".
    pub asked_torrents: bool,
    /// Downloads from these sites at most this fast, each (KiB/s).
    pub site_limits: Vec<SiteLimit>,
    /// Queues besides Main, which runs `max_parallel` at once in the download hours.
    pub queues: Vec<Queue>,
    /// Main is stopped (its downloads wait).
    pub main_paused: bool,
    /// Convert finished downloads from matching sites (FFmpeg presets).
    pub auto_convert: Vec<AutoConvert>,
    /// Take finished downloads off the list after this many days (0: never). Files stay.
    pub forget_after_days: u64,
}

/// A queue of the person's: its downloads start by themselves, this many at once, inside its
/// hours when it has any ("HH:MM", both or neither).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Queue {
    pub id: String,
    pub name: String,
    pub parallel: u32,
    pub paused: bool,
    pub from: String,
    pub to: String,
}

/// The built-in queue's id.
pub const MAIN_QUEUE: &str = "main";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteLimit {
    pub host: String,
    pub kib: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoConvert {
    pub enabled: bool,
    /// Part of the site (`youtube.com`); empty: any.
    pub host: String,
    /// `video`, `music` or empty (both).
    pub category: String,
    /// A `postprocess::PRESETS` key (`mp3`, `mp4`…).
    pub preset: String,
    /// Then send the original to the Recycle Bin.
    pub delete_original: bool,
}

/// A torrent RSS feed: new items whose title matches `filter` are added.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Feed {
    pub url: String,
    pub name: String,
    /// Words that must all be in the title (case-insensitive, `*` wildcards); empty: every item.
    pub filter: String,
    /// Words none of which may be in the title.
    pub exclude: String,
    /// Where its downloads go (empty: the download folder).
    pub folder: String,
    pub enabled: bool,
}

/// A Packagizer rule: links whose host and file name match go to a folder, or aren't added.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Rule {
    pub enabled: bool,
    /// Part of the host (`1fichier.com`); empty: any.
    pub host: String,
    /// The file name, `*` and `?` wildcards (`*.nfo`); empty: any.
    pub name: String,
    /// `folder` (save in `folder`), `skip` (don't add), or `paused` (add, don't start).
    pub action: String,
    pub folder: String,
}

impl Rule {
    pub fn matches(&self, host: &str, name: &str) -> bool {
        self.enabled
            && (self.host.trim().is_empty()
                || host.to_ascii_lowercase().contains(&self.host.trim().to_ascii_lowercase()))
            && (self.name.trim().is_empty()
                || wildcard(&self.name.trim().to_ascii_lowercase(), &name.to_ascii_lowercase()))
    }
}

/// `*` any run, `?` one character.
pub fn wildcard(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0usize, 0usize, None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            pi += 1;
            mark = ti;
        } else if let Some(st) = star {
            pi = st + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// The download-what-I-copied shortcut. Ctrl+Shift+D was the first default, but browsers use it
/// (bookmark all tabs), and a shortcut taken for the whole system takes it from them.
pub const HOTKEY: &str = "CommandOrControl+Alt+D";
const OLD_HOTKEY: &str = "CommandOrControl+Shift+D";

impl Default for Settings {
    fn default() -> Self {
        Settings {
            download_dir: dirs::download_dir().unwrap_or_else(|| PathBuf::from(".")),
            max_parallel: 3,
            connections: 8,
            boost_connections: true,
            global_limit_kib: 0,
            notify: true,
            close_to_tray: true,
            seed_when_done: false,
            extract_archives: true,
            delete_archives_after_extract: false,
            scan_downloads: false,
            sort_by_type: false,
            watch_clipboard: false,
            adapter: String::new(),
            schedule_enabled: false,
            schedule_start: "01:00".into(),
            schedule_stop: "07:00".into(),
            schedule_days: vec![true; 7],
            when_done: "nothing".into(),
            slow_enabled: false,
            slow_from: "09:00".into(),
            slow_to: "18:00".into(),
            slow_limit_kib: 1024,
            remote_on: false,
            remote_port: 8463,
            update_repo: String::new(),
            check_updates: true,
            torrent_port: 0,
            torrent_dht: true,
            torrent_upnp: true,
            torrent_down_kib: 0,
            torrent_up_kib: 0,
            seed_ratio: 0.0,
            seed_minutes: 0,
            torrent_blocklist: String::new(),
            feeds: Vec::new(),
            rss_minutes: 30,
            proxy: String::new(),
            reconnect_command: String::new(),
            archive_passwords: Vec::new(),
            rules: Vec::new(),
            theme: "dock".into(),
            theme_dock_seen: true,
            autostart: false,
            sound: false,
            ask_where: true,
            ask_skip_types: Vec::new(),
            last_dirs: std::collections::HashMap::new(),
            layout: "split".into(),
            handle_torrents: false,
            txt_menu: true,
            hotkey: HOTKEY.into(),
            complete_window: true,
            complete_min_mb: 50,
            asked_torrents: false,
            site_limits: Vec::new(),
            queues: Vec::new(),
            main_paused: false,
            auto_convert: Vec::new(),
            forget_after_days: 0,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<Settings>(&t).ok())
            .map(|mut s| {
                if !s.theme_dock_seen {
                    if s.theme == "dark" {
                        s.theme = "dock".into();
                    }
                    s.theme_dock_seen = true;
                }
                s
            })
            .unwrap_or_default()
            .clamped()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)
    }

    /// Keeps hand-edited values in their ranges.
    pub fn clamped(mut self) -> Settings {
        self.max_parallel = self.max_parallel.clamp(1, 16);
        let mut ids = std::collections::HashSet::new();
        self.queues.retain(|q| !q.id.trim().is_empty() && q.id != MAIN_QUEUE && ids.insert(q.id.clone()));
        for q in &mut self.queues {
            q.parallel = q.parallel.clamp(1, 16);
            if q.name.trim().is_empty() {
                q.name = "Queue".into();
            }
        }
        self.connections = self.connections.clamp(1, udm_engine::download::MAX_CONNECTIONS);
        self.rss_minutes = self.rss_minutes.clamp(5, 24 * 60);
        if self.hotkey == OLD_HOTKEY {
            self.hotkey = HOTKEY.into();
        }
        if !self.seed_ratio.is_finite() || self.seed_ratio < 0.0 {
            self.seed_ratio = 0.0;
        }
        if !matches!(self.layout.as_str(), "split" | "table") {
            self.layout = "split".into();
        }
        if !matches!(self.theme.as_str(), "dark" | "light" | "system" | "dock") {
            self.theme = "dock".into();
        }
        self
    }

    /// The engine's proxy for `proxy`.
    pub fn engine_proxy(&self) -> udm_engine::download::Proxy {
        use udm_engine::download::Proxy;
        let p = self.proxy.trim();
        if p.is_empty() {
            return Proxy::None;
        }
        if p.eq_ignore_ascii_case("system") {
            return Proxy::System;
        }
        if let Some(script) = p.strip_prefix("pac:") {
            return Proxy::Pac { script: std::sync::Arc::from(script.trim()) };
        }
        // A login in the address goes as the engine's login.
        let (url, login) = match url::Url::parse(p) {
            Ok(mut u) if !u.username().is_empty() => {
                let user = percent_encoding::percent_decode_str(u.username()).decode_utf8_lossy().into_owned();
                let pass = percent_encoding::percent_decode_str(u.password().unwrap_or(""))
                    .decode_utf8_lossy()
                    .into_owned();
                let _ = u.set_username("");
                let _ = u.set_password(None);
                (u.to_string().trim_end_matches('/').to_owned(), Some((user, pass)))
            }
            _ => (p.to_owned(), None),
        };
        Proxy::Manual { url, login, http: true, https: true }
    }

    /// A SOCKS proxy for torrents (librqbit takes only `socks5://`).
    pub fn torrent_proxy(&self) -> Option<String> {
        let p = self.proxy.trim();
        (p.starts_with("socks5://") || p.starts_with("socks5h://")).then(|| p.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_becomes_the_theme_once() {
        let dir = std::env::temp_dir().join(format!("udm-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("settings.json");
        // A file from before Dock, on the old default: moved to Dock.
        std::fs::write(&p, r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(Settings::load(&p).theme, "dock");
        // Light stays light.
        std::fs::write(&p, r#"{"theme":"light"}"#).unwrap();
        assert_eq!(Settings::load(&p).theme, "light");
        // Obsidian chosen after the switch: kept.
        std::fs::write(&p, r#"{"theme":"dark","theme_dock_seen":true}"#).unwrap();
        assert_eq!(Settings::load(&p).theme, "dark");
        // A new install.
        assert_eq!(Settings::default().theme, "dock");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wildcards_and_rules() {
        assert!(wildcard("*.nfo", "readme.nfo"));
        assert!(!wildcard("*.nfo", "readme.nfo.txt"));
        assert!(wildcard("part?.rar", "part1.rar"));
        assert!(wildcard("*", ""));
        let r = Rule { enabled: true, host: "1fichier".into(), name: "*.rar".into(), action: "folder".into(), folder: "X".into() };
        assert!(r.matches("1fichier.com", "Game.part1.RAR"));
        assert!(!r.matches("other.com", "Game.part1.rar"));
    }

    #[test]
    fn proxies() {
        use udm_engine::download::Proxy;
        let mut s = Settings::default();
        assert!(matches!(s.engine_proxy(), Proxy::None));
        s.proxy = "socks5://me:pw@127.0.0.1:1080".into();
        match s.engine_proxy() {
            Proxy::Manual { url, login, .. } => {
                assert_eq!(url, "socks5://127.0.0.1:1080");
                assert_eq!(login, Some(("me".into(), "pw".into())));
            }
            other => panic!("{other:?}"),
        }
        assert!(s.torrent_proxy().is_some());
        s.proxy = "pac:http://wpad/wpad.dat".into();
        assert!(matches!(s.engine_proxy(), Proxy::Pac { .. }));
    }
}
