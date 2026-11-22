//! Torrents and magnet links, through the reused `udm-torrent` crate (librqbit): a magnet's
//! or a .torrent file's file list, the files chosen, then the swarm. Torrents run beside the
//! download queue (a swarm isn't a queue slot), each a row of kind `torrent` in the list.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use udm_torrent::{Meta, Phase, Source, Torrents};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

use crate::manager::Manager;
use crate::store::{self, State};

/// The torrent session, started once in the background.
pub struct Swarm {
    session: OnceCell<Arc<Torrents>>,
    state_dir: PathBuf,
    /// Metadata looked up for the torrent dialog, until it is added (by info hash).
    metas: Mutex<HashMap<String, Meta>>,
    /// The settings the session starts with (port, DHT, UPnP, proxy, blocklist, adapter).
    settings: Mutex<crate::settings::Settings>,
    /// When each finished torrent started sharing (for the seeding time limit).
    seeding_since: Mutex<HashMap<String, std::time::Instant>>,
}

/// What a torrent row keeps (its `options`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TorrentOpts {
    pub hash: String,
    pub files: Option<Vec<usize>>,
    /// Its own seeding limits (None: the settings').
    #[serde(default)]
    pub seed_ratio: Option<f64>,
    #[serde(default)]
    pub seed_minutes: Option<u64>,
}

#[derive(Serialize)]
pub struct FileView {
    pub index: usize,
    pub path: String,
    pub size: u64,
}

#[derive(Serialize)]
pub struct MetaView {
    pub hash: String,
    pub name: String,
    pub total: u64,
    pub files: Vec<FileView>,
}

impl Swarm {
    pub fn new(data_dir: &std::path::Path) -> Swarm {
        Swarm {
            session: OnceCell::new(),
            state_dir: data_dir.join("torrents"),
            metas: Mutex::new(HashMap::new()),
            settings: Mutex::new(crate::settings::Settings::default()),
            seeding_since: Mutex::new(HashMap::new()),
        }
    }

    /// The settings the session starts with (the adapter, port, DHT, UPnP, proxy, blocklist):
    /// a change to these applies at Ultimate Download Manager's next start. Limits apply at once (`follow`).
    pub fn set_settings(&self, s: &crate::settings::Settings) {
        *self.settings.lock().unwrap() = s.clone();
    }

    pub async fn session(&self) -> Result<Arc<Torrents>, String> {
        self.session
            .get_or_try_init(|| async {
                let mut cfg = udm_torrent::Config::new(self.state_dir.clone());
                let s = self.settings.lock().unwrap().clone();
                cfg.port = (s.torrent_port > 0).then_some(s.torrent_port);
                cfg.dht = s.torrent_dht;
                cfg.upnp = s.torrent_upnp;
                cfg.proxy = s.torrent_proxy();
                cfg.blocklist = self.blocklist(&s.torrent_blocklist);
                let adapter = s.adapter.clone();
                if !adapter.trim().is_empty() {
                    // Held to the VPN's address; with the VPN off, no torrent runs at all.
                    match udm_engine::net::route_for(&adapter, &udm_engine::net::adapters()) {
                        udm_engine::Route::Bound(b) => {
                            let ip = b.v4.map(std::net::IpAddr::V4).or(b.v6.map(std::net::IpAddr::V6));
                            cfg.bind = ip.map(udm_torrent::Bind::Address);
                            cfg.upnp = false;
                            cfg.lsd = false;
                        }
                        _ => return Err(format!("The network adapter {adapter} is off: torrents wait for it (Settings → Network).")),
                    }
                }
                Torrents::start(cfg).await.map(Arc::new).map_err(|e| e.to_string())
            })
            .await
            .cloned()
    }

    /// The user's blocklist, converted to the P2P lines librqbit reads (`<state>/blocklist.p2p`).
    /// One that can't be read is left out (a bad list mustn't stop every torrent).
    fn blocklist(&self, path: &str) -> Option<PathBuf> {
        let path = path.trim();
        if path.is_empty() {
            return None;
        }
        let bytes = std::fs::read(path).ok()?;
        let converted = udm_torrent::blocklist::convert(&bytes)?;
        let out = self.state_dir.join("blocklist.p2p");
        let _ = std::fs::create_dir_all(&self.state_dir);
        std::fs::write(&out, converted.text).ok()?;
        Some(out)
    }

    fn torrent_file(&self, hash: &str) -> PathBuf {
        self.state_dir.join("udm").join(format!("{hash}.torrent"))
    }
}

/// A magnet link, or the path of a .torrent file.
fn source(text: &str) -> Result<Source, String> {
    if let Some(s) = Source::magnet(text) {
        return Ok(s);
    }
    let path = PathBuf::from(text.trim().trim_matches('"'));
    let bytes = udm_torrent::read_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Source::File(bytes))
}

/// The file list of a magnet (asking the swarm, up to a minute) or a .torrent file.
pub async fn meta(m: &Arc<Manager>, text: &str) -> Result<MetaView, String> {
    let src = source(text)?;
    let session = m.swarm.session().await?;
    let meta = session
        .metadata(&src, Duration::from_secs(60), None)
        .await
        .map_err(|e| e.to_string())?;
    let view = MetaView {
        hash: meta.info_hash.clone(),
        name: meta.name.clone(),
        total: meta.total(),
        files: meta
            .files
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.padding)
            .map(|(i, f)| FileView { index: i, path: f.path.display().to_string(), size: f.size })
            .collect(),
    };
    m.swarm.metas.lock().unwrap().insert(meta.info_hash.clone(), meta);
    Ok(view)
}

/// Start a looked-up torrent with the chosen files (None: all) in `dir`.
pub async fn add(m: &Arc<Manager>, hash: &str, files: Option<Vec<usize>>, dir: Option<PathBuf>) -> Result<i64, String> {
    let meta = m
        .swarm
        .metas
        .lock()
        .unwrap()
        .remove(hash)
        .ok_or("Look the torrent up again: its file list has expired.")?;
    // Already in the list: that row again, not a second one.
    if let Some(existing) = m
        .items_of_kind("torrent")
        .into_iter()
        .find(|it| opts_of(it).is_some_and(|o| o.hash.eq_ignore_ascii_case(hash)))
    {
        let id = existing.id;
        m.resume(&[id])?;
        return Ok(id);
    }
    let dir = dir.unwrap_or_else(|| m.default_dir("torrents"));
    let folder = meta.folder_in(&dir);
    let session = m.swarm.session().await?;
    let saved = m.swarm.torrent_file(hash);
    if let Some(parent) = saved.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&saved, &meta.torrent);
    let size: u64 = match &files {
        Some(f) => {
            let set: HashSet<usize> = f.iter().copied().collect();
            meta.files.iter().enumerate().filter(|(i, x)| set.contains(i) && !x.padding).map(|(_, x)| x.size).sum()
        }
        None => meta.total(),
    };
    session
        .add(&meta.torrent, &dir, files.clone(), meta.peers.clone(), false)
        .await
        .map_err(|e| e.to_string())?;
    // A session that kept it paused from an earlier run hands that one back: run it.
    let _ = session.resume(hash).await;
    let magnet = udm_torrent::magnet_link(&meta.torrent).unwrap_or_else(|| format!("magnet:?xt=urn:btih:{hash}"));
    let opts = serde_json::to_string(&TorrentOpts { hash: hash.to_owned(), files, ..Default::default() }).map_err(|e| e.to_string())?;
    m.add_torrent_row(&magnet, &meta.name, &dir, &folder, size, &opts)
}

fn opts_of(item: &store::Item) -> Option<TorrentOpts> {
    item.options.as_deref().and_then(|o| serde_json::from_str(o).ok())
}

pub async fn pause(m: &Arc<Manager>, item: &store::Item) -> Result<(), String> {
    let o = opts_of(item).ok_or("This torrent has lost its details")?;
    let s = m.swarm.session().await?;
    if s.has(&o.hash) {
        s.pause(&o.hash).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub async fn resume(m: &Arc<Manager>, item: &store::Item) -> Result<(), String> {
    let o = opts_of(item).ok_or("This torrent has lost its details")?;
    let s = m.swarm.session().await?;
    if s.has(&o.hash) {
        return s.resume(&o.hash).await.map_err(|e| e.to_string());
    }
    // Not in the session any more (its resume data lost): add it again from the saved file.
    let bytes = std::fs::read(m.swarm.torrent_file(&o.hash)).map_err(|e| e.to_string())?;
    s.add(&bytes, std::path::Path::new(&item.dir), o.files, Vec::new(), false)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// One file of a torrent, for the details panel.
#[derive(Serialize)]
pub struct ContentFile {
    pub index: usize,
    pub path: String,
    pub size: u64,
    /// Bytes there and checked (None: not known while it isn't running).
    pub done: Option<u64>,
    pub wanted: bool,
}

#[derive(Serialize)]
pub struct PeerView {
    pub addr: String,
    pub client: Option<String>,
    pub fetched: u64,
    pub uploaded: u64,
    pub kind: Option<String>,
}

/// A torrent's inside: its files, peers, trackers and pieces.
#[derive(Serialize)]
pub struct TorrentDetails {
    pub hash: String,
    /// The torrent is in the session now (live numbers below), else from its saved .torrent.
    pub live: bool,
    pub files: Vec<ContentFile>,
    pub peers: Vec<PeerView>,
    pub peers_seen: usize,
    pub trackers: Vec<String>,
    /// Pieces there, in up to 300 buckets: each the share (0–1) of its pieces there.
    pub pieces: Vec<f32>,
    pub piece_count: usize,
    pub piece_size: u32,
    pub uploaded: u64,
    pub up_speed: f64,
    pub phase: String,
}

const PIECE_BUCKETS: usize = 300;

pub async fn details(m: &Arc<Manager>, item: &store::Item) -> Result<TorrentDetails, String> {
    let o = opts_of(item).ok_or("This torrent has lost its details")?;
    let session = m.swarm.session().await.ok();
    let running = session.as_ref().filter(|s| s.has(&o.hash));
    if let Some(s) = running {
        let stats = s.stats(&o.hash).map_err(|e| e.to_string())?;
        let (entries, chosen, piece_size) = s.files(&o.hash).ok_or("The torrent's file list isn't ready yet")?;
        let chosen: Option<HashSet<usize>> = chosen.map(|c| c.into_iter().collect());
        let files = entries
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.padding)
            .map(|(i, f)| ContentFile {
                index: i,
                path: f.path.display().to_string(),
                size: f.size,
                done: stats.file_done.get(i).copied(),
                wanted: chosen.as_ref().is_none_or(|c| c.contains(&i)),
            })
            .collect();
        let haves = s.pieces(&o.hash);
        return Ok(TorrentDetails {
            hash: o.hash.clone(),
            live: true,
            files,
            peers: s
                .peers(&o.hash)
                .into_iter()
                .map(|p| PeerView { addr: p.addr, client: p.client, fetched: p.fetched, uploaded: p.uploaded, kind: p.kind })
                .collect(),
            peers_seen: stats.peers_seen,
            trackers: s.trackers(&o.hash),
            piece_count: haves.len(),
            pieces: buckets(&haves),
            piece_size,
            uploaded: stats.uploaded,
            up_speed: stats.up_speed,
            phase: format!("{:?}", stats.phase),
        });
    }
    // Not in the session (paused before the session restored it, or finished and let go): the
    // saved .torrent says what it holds.
    let bytes = std::fs::read(m.swarm.torrent_file(&o.hash)).map_err(|_| "The torrent's saved file is gone".to_string())?;
    let entries = udm_torrent::files_from_bytes(&bytes).map_err(|e| e.to_string())?;
    let chosen: Option<HashSet<usize>> = o.files.map(|c| c.into_iter().collect());
    let finished = item.state == State::Done;
    let files = entries
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.padding)
        .map(|(i, f)| {
            let wanted = chosen.as_ref().is_none_or(|c| c.contains(&i));
            ContentFile {
                index: i,
                path: f.path.display().to_string(),
                size: f.size,
                done: if finished { Some(if wanted { f.size } else { 0 }) } else { None },
                wanted,
            }
        })
        .collect();
    Ok(TorrentDetails {
        hash: o.hash,
        live: false,
        files,
        peers: Vec::new(),
        peers_seen: 0,
        trackers: Vec::new(),
        pieces: Vec::new(),
        piece_count: 0,
        piece_size: 0,
        uploaded: 0,
        up_speed: 0.0,
        phase: if finished { "Finished".into() } else { "Stopped".into() },
    })
}

/// `haves` squeezed into at most `PIECE_BUCKETS` shares, for the piece bar.
fn buckets(haves: &[bool]) -> Vec<f32> {
    if haves.is_empty() {
        return Vec::new();
    }
    let n = haves.len().min(PIECE_BUCKETS);
    (0..n)
        .map(|b| {
            let (from, to) = (b * haves.len() / n, ((b + 1) * haves.len() / n).max(b * haves.len() / n + 1));
            let part = &haves[from..to.min(haves.len())];
            part.iter().filter(|h| **h).count() as f32 / part.len().max(1) as f32
        })
        .collect()
}

/// Download only `files` of a torrent from now on; at least one.
pub async fn choose(m: &Arc<Manager>, item: &store::Item, files: Vec<usize>) -> Result<(), String> {
    if files.is_empty() {
        return Err("Keep at least one file: to stop the whole torrent, pause or remove it.".into());
    }
    let mut o = opts_of(item).ok_or("This torrent has lost its details")?;
    let s = m.swarm.session().await?;
    let set: HashSet<usize> = files.iter().copied().collect();
    if s.has(&o.hash) {
        s.choose(&o.hash, &set).await.map_err(|e| e.to_string())?;
    }
    // The row's size follows the chosen files.
    let size = std::fs::read(m.swarm.torrent_file(&o.hash))
        .ok()
        .and_then(|b| udm_torrent::files_from_bytes(&b).ok())
        .map(|entries| entries.iter().enumerate().filter(|(i, f)| set.contains(i) && !f.padding).map(|(_, f)| f.size).sum::<u64>());
    o.files = Some(files);
    let json = serde_json::to_string(&o).map_err(|e| e.to_string())?;
    m.set_torrent_files(item.id, &json, size);
    Ok(())
}

/// For a preview: fetch the pieces in order (each file's first and last first: a video's index
/// is often at its end), so a player can start before the torrent ends.
pub async fn play_in_order(m: &Arc<Manager>, item: &store::Item) {
    if let (Some(o), Ok(s)) = (opts_of(item), m.swarm.session().await) {
        let _ = s.set_order(&o.hash, udm_torrent::PieceOrder::InOrder { first_and_last_first: true });
    }
}

pub async fn remove(m: &Arc<Manager>, item: &store::Item, delete_files: bool) -> Result<(), String> {
    if let Some(o) = opts_of(item) {
        let s = m.swarm.session().await?;
        if s.has(&o.hash) {
            s.remove(&o.hash, delete_files).await.map_err(|e| e.to_string())?;
        }
        let _ = std::fs::remove_file(m.swarm.torrent_file(&o.hash));
    }
    Ok(())
}

/// A finished torrent has shared enough by the seeding limits (ratio, minutes).
fn seeded_enough(m: &Arc<Manager>, o: &TorrentOpts, st: &udm_torrent::Stats, s: &crate::settings::Settings) -> bool {
    let hash = o.hash.as_str();
    let since = *m.swarm.seeding_since.lock().unwrap().entry(hash.to_owned()).or_insert_with(std::time::Instant::now);
    let (ratio, minutes) = (o.seed_ratio.unwrap_or(s.seed_ratio), o.seed_minutes.unwrap_or(s.seed_minutes));
    let ratio_hit = ratio > 0.0 && st.total > 0 && st.uploaded as f64 / st.total as f64 >= ratio;
    let time_hit = minutes > 0 && since.elapsed() >= Duration::from_secs(minutes * 60);
    let done = ratio_hit || time_hit;
    if done {
        m.swarm.seeding_since.lock().unwrap().remove(hash);
    }
    done
}

/// A torrent's own seeding limits (None: the settings'). 0 means never stop.
pub fn set_seeding(m: &Arc<Manager>, item: &store::Item, ratio: Option<f64>, minutes: Option<u64>) -> Result<(), String> {
    let mut o = opts_of(item).ok_or("This torrent has lost its details")?;
    o.seed_ratio = ratio.filter(|r| r.is_finite() && *r >= 0.0);
    o.seed_minutes = minutes;
    let json = serde_json::to_string(&o).map_err(|e| e.to_string())?;
    m.set_torrent_files(item.id, &json, None);
    Ok(())
}

/// Its own seeding limits, as set.
pub fn seeding_of(item: &store::Item) -> (Option<f64>, Option<u64>) {
    opts_of(item).map(|o| (o.seed_ratio, o.seed_minutes)).unwrap_or_default()
}

/// Make a .torrent of `source` (a file or a folder), saved as `out`; with `seed`, it's shared
/// from where it is. The magnet link.
pub async fn create(m: &Arc<Manager>, source: &std::path::Path, trackers: &str, private_name: Option<String>, out: &std::path::Path, seed: bool) -> Result<String, String> {
    if !source.exists() {
        return Err("That file or folder isn't there.".into());
    }
    let trackers: Vec<String> = trackers
        .split(|c: char| c.is_whitespace() || c == ',')
        .map(str::trim)
        .filter(|t| t.starts_with("http://") || t.starts_with("https://") || t.starts_with("udp://") || t.starts_with("wss://"))
        .map(str::to_owned)
        .collect();
    let spawner = udm_torrent::librqbit::spawn_utils::BlockingSpawner::new(4);
    let opts = udm_torrent::librqbit::CreateTorrentOptions { name: private_name.as_deref(), trackers, piece_length: None };
    let made = udm_torrent::librqbit::create_torrent(source, opts, &spawner).await.map_err(|e| e.to_string())?;
    let bytes = made.as_bytes().map_err(|e| e.to_string())?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    let magnet = made.as_magnet().to_string();
    if seed {
        let meta = meta(m, &out.to_string_lossy()).await?;
        let dir = source.parent().map(std::path::Path::to_owned);
        add(m, &meta.hash, None, dir).await?;
    }
    Ok(magnet)
}

/// Whether peers on the internet can reach this computer's torrent port (asked of
/// ifconfig.co, which tries to connect to it; only when the person asks).
pub async fn port_check(m: &Arc<Manager>) -> Result<serde_json::Value, String> {
    let s = m.swarm.session().await?;
    let port = s.listen_addr().map(|a| a.port()).ok_or("Torrents aren't listening for peers (Settings → Torrents).")?;
    let v: serde_json::Value = reqwest::Client::builder()
        .user_agent(crate::manager::UDM_AGENT)
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?
        .get(format!("https://ifconfig.co/port/{port}"))
        .header("Accept", "application/json")
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("The check couldn't be done ({e})"))?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "port": port, "reachable": v.get("reachable").and_then(|r| r.as_bool()).unwrap_or(false) }))
}

/// Check every piece on disk again.
pub async fn recheck(m: &Arc<Manager>, item: &store::Item) -> Result<(), String> {
    let o = opts_of(item).ok_or("This torrent has lost its details")?;
    let s = m.swarm.session().await?;
    if !s.has(&o.hash) {
        resume(m, item).await?;
    }
    let bytes = std::fs::read(m.swarm.torrent_file(&o.hash)).map_err(|e| e.to_string())?;
    s.readd(&o.hash, &bytes, true).await.map_err(|e| e.to_string())
}

/// Ask the trackers and the DHT for peers again now.
pub async fn reannounce(m: &Arc<Manager>, item: &store::Item) -> Result<(), String> {
    let o = opts_of(item).ok_or("This torrent has lost its details")?;
    let s = m.swarm.session().await?;
    s.reannounce(&o.hash).await.map_err(|e| e.to_string())
}

/// Add trackers (one address per line) to a torrent, kept in its saved .torrent.
pub async fn add_trackers(m: &Arc<Manager>, item: &store::Item, text: &str) -> Result<usize, String> {
    let o = opts_of(item).ok_or("This torrent has lost its details")?;
    let more: Vec<String> = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .map(str::trim)
        .filter(|t| t.starts_with("http://") || t.starts_with("https://") || t.starts_with("udp://") || t.starts_with("wss://"))
        .map(str::to_owned)
        .collect();
    if more.is_empty() {
        return Err("Paste tracker addresses (http://, https:// or udp://), one per line.".into());
    }
    let path = m.swarm.torrent_file(&o.hash);
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let before = udm_torrent::trackers::listed(&bytes).len();
    let updated = udm_torrent::trackers::add(&bytes, &more).ok_or("These trackers couldn't be added to the torrent")?;
    let added = udm_torrent::trackers::listed(&updated).len().saturating_sub(before);
    std::fs::write(&path, &updated).map_err(|e| e.to_string())?;
    let s = m.swarm.session().await?;
    if s.has(&o.hash) {
        s.readd(&o.hash, &updated, false).await.map_err(|e| e.to_string())?;
    }
    Ok(added)
}

/// Fetch the pieces in order (to watch a video while it downloads), or rarest first again.
pub async fn set_sequential(m: &Arc<Manager>, item: &store::Item, on: bool) -> Result<(), String> {
    let o = opts_of(item).ok_or("This torrent has lost its details")?;
    let s = m.swarm.session().await?;
    let order = if on {
        udm_torrent::PieceOrder::InOrder { first_and_last_first: true }
    } else {
        udm_torrent::PieceOrder::default()
    };
    s.set_order(&o.hash, order).map_err(|e| e.to_string())
}

/// Whether its pieces come in order now.
pub async fn sequential(m: &Arc<Manager>, item: &store::Item) -> bool {
    match (opts_of(item), m.swarm.session().await) {
        (Some(o), Ok(s)) => s.order(&o.hash).is_some_and(|ord| ord != udm_torrent::PieceOrder::default()),
        _ => false,
    }
}

/// Follow every running torrent once a second: its progress for the list, and its end.
pub async fn follow(m: Arc<Manager>) {
    // Restore the session (and with it the torrents of the last run) at once.
    let _ = m.swarm.session().await;
    let mut every = tokio::time::interval(Duration::from_secs(1));
    loop {
        every.tick().await;
        let Ok(session) = m.swarm.session().await else {
            continue;
        };
        let s = m.settings();
        let kib = |k: u64| (k > 0).then(|| k * 1024);
        session.set_download_limit(kib(s.torrent_down_kib));
        session.set_upload_limit(kib(s.torrent_up_kib));
        let items = m.items_of_kind("torrent");
        let mut live = HashMap::new();
        for it in items.iter().filter(|it| it.state == State::Downloading) {
            let Some(o) = opts_of(it) else { continue };
            if !session.has(&o.hash) {
                let _ = resume(&m, it).await;
                continue;
            }
            let Ok(st) = session.stats(&o.hash) else { continue };
            live.insert(it.id, (st.done, st.total, st.down_speed, st.peers_live as u32));
            // Its own limit (the row's speed limit).
            let _ = session.set_torrent_limits(&o.hash, kib(it.limit_kib), None);
            match st.phase {
                Phase::Finished => {
                    m.torrent_done(it.id, st.total);
                    if !s.seed_when_done || seeded_enough(&m, &o, &st, &s) {
                        // Out of the session, files kept: the files are free to move or open
                        // (a paused torrent still holds them). Resuming adds it back from the
                        // saved .torrent and only checks what's there.
                        let _ = session.remove(&o.hash, false).await;
                    }
                }
                Phase::Error => m.torrent_failed(it.id, st.error.as_deref().unwrap_or("the torrent stopped")),
                _ => m.torrent_progress(it.id, st.done, st.total),
            }
        }
        m.set_torrent_live(live);
    }
}

#[cfg(test)]
mod tests {
    /// A .torrent made from a file reads back with that file, its size, and the tracker given.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_made_torrent_reads_back() {
        let dir = std::env::temp_dir().join(format!("udm-mk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("hello.bin");
        std::fs::write(&file, vec![7u8; 300_000]).unwrap();
        let spawner = udm_torrent::librqbit::spawn_utils::BlockingSpawner::new(2);
        let opts = udm_torrent::librqbit::CreateTorrentOptions {
            name: None,
            trackers: vec!["udp://tracker.example:1337/announce".into()],
            piece_length: None,
        };
        let made = udm_torrent::librqbit::create_torrent(&file, opts, &spawner).await.unwrap();
        let bytes = made.as_bytes().unwrap();
        let files = udm_torrent::files_from_bytes(&bytes).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].size, 300_000);
        assert!(udm_torrent::trackers::listed(&bytes).iter().any(|t| t.contains("tracker.example")));
        assert!(made.as_magnet().to_string().starts_with("magnet:?xt=urn:btih:"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
