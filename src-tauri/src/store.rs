//! The download list, kept in SQLite (`<data>/udm.db`, WAL mode).
//!
//! Cookies and other request headers are never stored here (as in the engine's resume files):
//! whoever resumes a download supplies them again.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;

/// Where a download stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Queued,
    Downloading,
    Paused,
    Done,
    Failed,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Queued => "queued",
            State::Downloading => "downloading",
            State::Paused => "paused",
            State::Done => "done",
            State::Failed => "failed",
        }
    }

    fn parse(s: &str) -> State {
        match s {
            "downloading" => State::Downloading,
            "paused" => State::Paused,
            "done" => State::Done,
            "failed" => State::Failed,
            _ => State::Queued,
        }
    }
}

/// One download as stored.
#[derive(Clone, Debug, Serialize)]
pub struct Item {
    pub id: i64,
    pub url: String,
    /// The name asked for (None: the server's).
    pub name: Option<String>,
    pub dir: String,
    /// The final path, once the engine has chosen it.
    pub file: Option<String>,
    pub size: Option<u64>,
    pub done: u64,
    pub state: State,
    pub error: Option<String>,
    pub category: String,
    pub referer: Option<String>,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    /// `file` (the download engine), or `video` (yt-dlp: a page and the quality chosen).
    pub kind: String,
    /// The video's choice, as JSON (`VideoOpts`).
    pub options: Option<String>,
    /// Its own speed limit, KiB/s (0: none but the global one).
    pub limit_kib: u64,
    /// Higher starts first; the same: oldest first.
    pub priority: i64,
    /// The checksum it must match (`md5:…`, `sha256:…` or a bare hex digest), checked when done.
    pub checksum: Option<String>,
    /// The password its archive is unpacked with (else the saved list is tried).
    pub password: Option<String>,
    /// The person's tags, space-separated (`work linux`).
    pub tags: String,
    /// The queue it waits in (`settings::MAIN_QUEUE` unless moved).
    pub queue: String,
}

impl Item {
    /// The name to show: the file's, else the one asked for, else the address's last part.
    pub fn display_name(&self) -> String {
        if let Some(f) = &self.file
            && let Some(n) = Path::new(f).file_name()
        {
            return n.to_string_lossy().into_owned();
        }
        if let Some(n) = &self.name {
            return n.clone();
        }
        url::Url::parse(&self.url)
            .ok()
            .and_then(|u| u.path_segments()?.rev().find(|s| !s.is_empty()).map(str::to_owned))
            .map(|s| percent_decode(&s))
            .unwrap_or_else(|| self.url.clone())
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub struct Store {
    db: Connection,
}

const SCHEMA_VERSION: i64 = 6;

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Store> {
        let db = Connection::open(path)?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "NORMAL")?;
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            db.execute_batch(
                "CREATE TABLE IF NOT EXISTS item (
                    id INTEGER PRIMARY KEY,
                    url TEXT NOT NULL,
                    name TEXT,
                    dir TEXT NOT NULL,
                    file TEXT,
                    size INTEGER,
                    done INTEGER NOT NULL DEFAULT 0,
                    state TEXT NOT NULL,
                    error TEXT,
                    category TEXT NOT NULL,
                    referer TEXT,
                    created_at INTEGER NOT NULL,
                    finished_at INTEGER
                );
                CREATE INDEX IF NOT EXISTS item_state ON item(state);",
            )?;
            db.pragma_update(None, "user_version", 1)?;
        }
        if version < 2 {
            db.execute_batch(
                "ALTER TABLE item ADD COLUMN kind TEXT NOT NULL DEFAULT 'file';
                 ALTER TABLE item ADD COLUMN options TEXT;",
            )?;
            db.pragma_update(None, "user_version", 2)?;
        }
        if version < 3 {
            db.execute_batch(
                "CREATE TABLE IF NOT EXISTS subscription (
                    id INTEGER PRIMARY KEY,
                    url TEXT NOT NULL UNIQUE,
                    title TEXT NOT NULL,
                    quality TEXT NOT NULL,
                    every_min INTEGER NOT NULL,
                    last_check INTEGER,
                    last_error TEXT,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    created_at INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS sub_seen (
                    sub_id INTEGER NOT NULL,
                    video_url TEXT NOT NULL,
                    PRIMARY KEY (sub_id, video_url)
                );",
            )?;
            db.pragma_update(None, "user_version", 3)?;
        }
        if version < 4 {
            db.execute_batch(
                "ALTER TABLE item ADD COLUMN limit_kib INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE item ADD COLUMN priority INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE item ADD COLUMN checksum TEXT;
                 ALTER TABLE item ADD COLUMN password TEXT;
                 CREATE TABLE IF NOT EXISTS rss_seen (
                    feed TEXT NOT NULL,
                    guid TEXT NOT NULL,
                    seen_at INTEGER NOT NULL,
                    PRIMARY KEY (feed, guid)
                 );",
            )?;
            db.pragma_update(None, "user_version", 4)?;
        }
        if version < 5 {
            db.execute_batch("ALTER TABLE item ADD COLUMN tags TEXT NOT NULL DEFAULT '';")?;
            db.pragma_update(None, "user_version", 5)?;
        }
        if version < 6 {
            db.execute_batch("ALTER TABLE item ADD COLUMN queue TEXT NOT NULL DEFAULT 'main';")?;
            db.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(Store { db })
    }

    #[cfg(test)]
    pub fn in_memory() -> rusqlite::Result<Store> {
        Store::open(Path::new(":memory:"))
    }

    fn row(r: &Row<'_>) -> rusqlite::Result<Item> {
        Ok(Item {
            id: r.get("id")?,
            url: r.get("url")?,
            name: r.get("name")?,
            dir: r.get("dir")?,
            file: r.get("file")?,
            size: r.get::<_, Option<i64>>("size")?.map(|s| s as u64),
            done: r.get::<_, i64>("done")? as u64,
            state: State::parse(&r.get::<_, String>("state")?),
            error: r.get("error")?,
            category: r.get("category")?,
            referer: r.get("referer")?,
            created_at: r.get("created_at")?,
            finished_at: r.get("finished_at")?,
            kind: r.get("kind")?,
            options: r.get("options")?,
            limit_kib: r.get::<_, i64>("limit_kib")?.max(0) as u64,
            priority: r.get("priority")?,
            checksum: r.get("checksum")?,
            password: r.get("password")?,
            tags: r.get("tags")?,
            queue: r.get("queue")?,
        })
    }

    pub fn add(
        &self,
        url: &str,
        name: Option<&str>,
        dir: &Path,
        category: &str,
        referer: Option<&str>,
        state: State,
    ) -> rusqlite::Result<i64> {
        self.db.execute(
            "INSERT INTO item (url, name, dir, state, category, referer, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![url, name, dir.to_string_lossy(), state.as_str(), category, referer, now()],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// A video to fetch with yt-dlp: its page, the name to save it under, and its choice.
    pub fn add_video(
        &self,
        page: &str,
        name: &str,
        dir: &Path,
        category: &str,
        referer: Option<&str>,
        options: &str,
    ) -> rusqlite::Result<i64> {
        self.db.execute(
            "INSERT INTO item (url, name, dir, state, category, referer, created_at, kind, options)
             VALUES (?1, ?2, ?3, 'queued', ?4, ?5, ?6, 'video', ?7)",
            params![page, name, dir.to_string_lossy(), category, referer, now(), options],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// An image gallery for gallery-dl: its page and the folder its files go in (`file`).
    pub fn add_gallery(&self, page: &str, name: &str, dir: &Path, folder: &Path, options: &str) -> rusqlite::Result<i64> {
        self.db.execute(
            "INSERT INTO item (url, name, dir, file, state, category, created_at, kind, options)
             VALUES (?1, ?2, ?3, ?4, 'queued', 'images', ?5, 'gallery', ?6)",
            params![page, name, dir.to_string_lossy(), folder.to_string_lossy(), now(), options],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// A torrent started in the swarm: its magnet link, name, folder and the chosen size.
    pub fn add_torrent(
        &self,
        magnet: &str,
        name: &str,
        dir: &Path,
        folder: &Path,
        size: u64,
        options: &str,
    ) -> rusqlite::Result<i64> {
        self.db.execute(
            "INSERT INTO item (url, name, dir, file, size, state, category, created_at, kind, options)
             VALUES (?1, ?2, ?3, ?4, ?5, 'downloading', 'torrents', ?6, 'torrent', ?7)",
            params![magnet, name, dir.to_string_lossy(), folder.to_string_lossy(), size as i64, now(), options],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn of_kind(&self, kind: &str) -> rusqlite::Result<Vec<Item>> {
        let mut st = self.db.prepare("SELECT * FROM item WHERE kind = ?1")?;
        st.query_map([kind], Store::row)?.collect()
    }

    pub fn conn(&self) -> &Connection {
        &self.db
    }

    pub fn get(&self, id: i64) -> rusqlite::Result<Option<Item>> {
        self.db
            .query_row("SELECT * FROM item WHERE id = ?1", [id], Store::row)
            .optional()
    }

    /// Newest first.
    pub fn all(&self) -> rusqlite::Result<Vec<Item>> {
        let mut st = self.db.prepare("SELECT * FROM item ORDER BY id DESC")?;
        st.query_map([], Store::row)?.collect()
    }

    /// The oldest queued download, if any, skipping `except`.
    #[cfg(test)]
    pub fn next_queued(&self, except: &[i64]) -> rusqlite::Result<Option<Item>> {
        self.next_queued_where(except, |_| true)
    }

    /// The oldest queued download that `fits`, skipping `except`.
    pub fn next_queued_where(&self, except: &[i64], fits: impl Fn(&Item) -> bool) -> rusqlite::Result<Option<Item>> {
        let mut st = self
            .db
            .prepare("SELECT * FROM item WHERE state = 'queued' ORDER BY priority DESC, id ASC")?;
        let rows = st.query_map([], Store::row)?;
        for r in rows {
            let it = r?;
            if !except.contains(&it.id) && fits(&it) {
                return Ok(Some(it));
            }
        }
        Ok(None)
    }

    pub fn set_queue(&self, id: i64, queue: &str) -> rusqlite::Result<()> {
        self.db.execute("UPDATE item SET queue = ?2 WHERE id = ?1", params![id, queue])?;
        Ok(())
    }

    pub fn set_state(&self, id: i64, state: State, error: Option<&str>) -> rusqlite::Result<()> {
        let finished = (state == State::Done).then(now);
        self.db.execute(
            "UPDATE item SET state = ?2, error = ?3,
                    finished_at = COALESCE(?4, finished_at) WHERE id = ?1",
            params![id, state.as_str(), error, finished],
        )?;
        Ok(())
    }

    /// A row's options (a torrent's chosen files), and its size when that changes with them.
    pub fn set_options(&self, id: i64, options: &str, size: Option<u64>) -> rusqlite::Result<()> {
        self.db.execute(
            "UPDATE item SET options = ?2, size = COALESCE(?3, size) WHERE id = ?1",
            params![id, options, size.map(|s| s as i64)],
        )?;
        Ok(())
    }

    pub fn set_progress(
        &self,
        id: i64,
        file: Option<&Path>,
        size: Option<u64>,
        done: u64,
    ) -> rusqlite::Result<()> {
        self.db.execute(
            "UPDATE item SET file = COALESCE(?2, file), size = COALESCE(?3, size), done = ?4
             WHERE id = ?1",
            params![
                id,
                file.map(|f| f.to_string_lossy().into_owned()),
                size.map(|s| s as i64),
                done as i64
            ],
        )?;
        Ok(())
    }

    /// Its own speed limit, KiB/s (0: none).
    pub fn set_limit(&self, id: i64, kib: u64) -> rusqlite::Result<()> {
        self.db.execute("UPDATE item SET limit_kib = ?2 WHERE id = ?1", params![id, kib as i64])?;
        Ok(())
    }

    /// Put these first (`top`) or last in the queue, in the order given.
    pub fn move_in_queue(&self, ids: &[i64], top: bool) -> rusqlite::Result<()> {
        let (hi, lo): (i64, i64) = self
            .db
            .query_row("SELECT COALESCE(MAX(priority), 0), COALESCE(MIN(priority), 0) FROM item", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        let n = ids.len() as i64;
        for (i, id) in ids.iter().enumerate() {
            let p = if top { hi + n - i as i64 } else { lo - 1 - i as i64 };
            self.db.execute("UPDATE item SET priority = ?2 WHERE id = ?1", params![id, p])?;
        }
        Ok(())
    }

    /// One step up or down the queue among the queued: swaps places with its neighbour.
    pub fn nudge(&self, id: i64, up: bool) -> rusqlite::Result<()> {
        let queued: Vec<(i64, i64)> = {
            let mut st = self.db.prepare(
                "SELECT id, priority FROM item WHERE state IN ('queued', 'paused', 'failed') ORDER BY priority DESC, id ASC",
            )?;
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
        };
        let Some(at) = queued.iter().position(|(i, _)| *i == id) else { return Ok(()) };
        let other = if up { at.checked_sub(1) } else { (at + 1 < queued.len()).then_some(at + 1) };
        let Some(other) = other else { return Ok(()) };
        // Give the whole list distinct priorities in its order, then swap the two.
        let n = queued.len() as i64;
        let mut order: Vec<i64> = queued.iter().map(|(i, _)| *i).collect();
        order.swap(at, other);
        for (k, i) in order.iter().enumerate() {
            self.db.execute("UPDATE item SET priority = ?2 WHERE id = ?1", params![i, n - k as i64])?;
        }
        Ok(())
    }

    /// Back to the start: nothing downloaded, no file yet, queued.
    pub fn reset(&self, id: i64) -> rusqlite::Result<()> {
        self.db.execute(
            "UPDATE item SET file = NULL, done = 0, state = 'queued', error = NULL, finished_at = NULL WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }

    pub fn set_tags(&self, id: i64, tags: &str) -> rusqlite::Result<()> {
        self.db.execute("UPDATE item SET tags = ?2 WHERE id = ?1", params![id, tags])?;
        Ok(())
    }

    /// Finished downloads that finished before `before` (Unix time) leave the list; their files
    /// stay. How many.
    pub fn forget_finished(&self, before: i64) -> rusqlite::Result<usize> {
        self.db.execute("DELETE FROM item WHERE state = 'done' AND COALESCE(finished_at, created_at) < ?1", [before])
    }

    pub fn set_checksum(&self, id: i64, checksum: Option<&str>) -> rusqlite::Result<()> {
        self.db.execute("UPDATE item SET checksum = ?2 WHERE id = ?1", params![id, checksum])?;
        Ok(())
    }

    pub fn set_password(&self, id: i64, password: Option<&str>) -> rusqlite::Result<()> {
        self.db.execute("UPDATE item SET password = ?2 WHERE id = ?1", params![id, password])?;
        Ok(())
    }

    /// A stopped download's address, replaced (the site gave a new link for the same file).
    pub fn set_url(&self, id: i64, url: &str) -> rusqlite::Result<()> {
        self.db.execute("UPDATE item SET url = ?2, error = NULL WHERE id = ?1", params![id, url])?;
        Ok(())
    }

    /// Its file moved or renamed (done downloads).
    pub fn set_file(&self, id: i64, file: &Path) -> rusqlite::Result<()> {
        let dir = file.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
        self.db.execute(
            "UPDATE item SET file = ?2, dir = ?3 WHERE id = ?1",
            params![id, file.to_string_lossy(), dir],
        )?;
        Ok(())
    }

    pub fn set_category(&self, id: i64, category: &str) -> rusqlite::Result<()> {
        self.db
            .execute("UPDATE item SET category = ?2 WHERE id = ?1", params![id, category])?;
        Ok(())
    }

    pub fn remove(&self, id: i64) -> rusqlite::Result<()> {
        self.db.execute("DELETE FROM item WHERE id = ?1", [id])?;
        Ok(())
    }

    /// After a crash or a closed app: what was downloading goes back to the queue.
    pub fn requeue_interrupted(&self) -> rusqlite::Result<usize> {
        self.db.execute(
            "UPDATE item SET state = 'queued' WHERE state = 'downloading' AND kind != 'torrent'",
            [],
        )
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The sidebar category for a file name, by its extension.
pub fn category_of(name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "wmv" | "flv" | "m4v" | "mpg" | "mpeg" | "ts"
        | "3gp" => "video",
        "mp3" | "m4a" | "flac" | "wav" | "ogg" | "opus" | "aac" | "wma" | "alac" => "music",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "epub" | "odt"
        | "rtf" | "csv" | "md" => "docs",
        "zip" | "rar" | "7z" | "tar" | "gz" | "xz" | "bz2" | "zst" | "iso" | "cab" => "archives",
        "exe" | "msi" | "msix" | "appx" | "apk" | "dmg" | "deb" | "rpm" | "appimage" | "bat"
        | "ps1" => "programs",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" | "avif" | "heic" | "tif"
        | "tiff" => "images",
        "torrent" => "torrents",
        _ => "other",
    }
}

/// The part and resume files beside a download's final path.
pub fn temp_files(file: &Path) -> [PathBuf; 2] {
    [udm_engine::naming::part_path(file), udm_engine::state::state_path(file)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_queue_picks_its_own() {
        let s = Store::in_memory().unwrap();
        let dir = Path::new("C:/dl");
        let a = s.add("https://a.example/1", None, dir, "other", None, State::Queued).unwrap();
        let b = s.add("https://a.example/2", None, dir, "other", None, State::Queued).unwrap();
        assert_eq!(s.get(a).unwrap().unwrap().queue, "main");
        s.set_queue(b, "night").unwrap();
        let pick = |q: &str| s.next_queued_where(&[], |it| it.queue == q).unwrap().map(|it| it.id);
        assert_eq!(pick("main"), Some(a));
        assert_eq!(pick("night"), Some(b));
        assert_eq!(pick("other"), None);
    }

    #[test]
    fn add_progress_and_requeue() {
        let s = Store::in_memory().unwrap();
        let id = s
            .add("https://x.test/a%20b.zip", None, Path::new("C:/dl"), "archives", None, State::Queued)
            .unwrap();
        let it = s.get(id).unwrap().unwrap();
        assert_eq!(it.display_name(), "a b.zip");
        assert_eq!(s.next_queued(&[]).unwrap().unwrap().id, id);
        assert!(s.next_queued(&[id]).unwrap().is_none());
        s.set_state(id, State::Downloading, None).unwrap();
        s.set_progress(id, Some(Path::new("C:/dl/a b.zip")), Some(100), 40).unwrap();
        assert_eq!(s.requeue_interrupted().unwrap(), 1);
        let it = s.get(id).unwrap().unwrap();
        assert_eq!((it.state, it.done, it.size), (State::Queued, 40, Some(100)));
        s.set_state(id, State::Done, None).unwrap();
        assert!(s.get(id).unwrap().unwrap().finished_at.is_some());
        s.remove(id).unwrap();
        assert!(s.all().unwrap().is_empty());
    }

    #[test]
    fn reset_and_forget() {
        let s = Store::in_memory().unwrap();
        let a = s.add("https://x.test/a.zip", None, Path::new("C:/dl"), "archives", None, State::Queued).unwrap();
        let b = s.add("https://x.test/b.zip", None, Path::new("C:/dl"), "archives", None, State::Queued).unwrap();
        s.set_progress(a, Some(Path::new("C:/dl/a.zip")), Some(10), 10).unwrap();
        s.set_state(a, State::Done, None).unwrap();
        s.reset(a).unwrap();
        let it = s.get(a).unwrap().unwrap();
        assert_eq!((it.state, it.done, it.file, it.finished_at), (State::Queued, 0, None, None));
        s.set_state(a, State::Done, None).unwrap();
        // Finished before the cut-off leave; others stay.
        assert_eq!(s.forget_finished(now() + 10).unwrap(), 1);
        assert!(s.get(a).unwrap().is_none());
        assert!(s.get(b).unwrap().is_some());
    }

    #[test]
    fn categories() {
        assert_eq!(category_of("Movie.MKV"), "video");
        assert_eq!(category_of("setup.exe"), "programs");
        assert_eq!(category_of("noext"), "other");
    }
}
