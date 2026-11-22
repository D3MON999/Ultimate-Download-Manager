//! Preview while downloading: the part already there, played by VLC (which plays incomplete
//! files); a torrent switches to fetching its pieces in order first.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::manager::Manager;
use crate::store::Item;

fn vlc() -> Option<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|p| PathBuf::from(p).join(r"VideoLAN\VLC\vlc.exe"))
        .find(|p| p.is_file())
}

/// The biggest media file under `dir` (a torrent's folder).
fn biggest_in(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(u64, PathBuf)> = None;
    let mut stack = vec![dir.to_owned()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).ok()?.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(md) = e.metadata()
                && best.as_ref().is_none_or(|(s, _)| md.len() > *s)
            {
                best = Some((md.len(), p));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// What to play for `item` now.
fn target(item: &Item) -> Option<PathBuf> {
    let file = item.file.as_deref().map(PathBuf::from);
    match item.kind.as_str() {
        "torrent" => {
            let f = file?;
            if f.is_dir() { biggest_in(&f) } else { Some(f) }
        }
        "video" => {
            // yt-dlp writes `<stem>.<format>.<ext>.part`: the newest of them.
            let stem = item.name.as_deref().and_then(|n| Path::new(n).file_stem()).map(|s| s.to_string_lossy().into_owned())?;
            std::fs::read_dir(&item.dir)
                .ok()?
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with(&stem))
                .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
                .map(|e| e.path())
        }
        _ => {
            let f = file?;
            if f.exists() { Some(f) } else { Some(udm_engine::naming::part_path(&f)) }
        }
    }
}

pub async fn open(m: &Arc<Manager>, id: i64) -> Result<(), String> {
    let item = m.item(id)?;
    let player = vlc().ok_or("Previews play in VLC: install it from videolan.org.")?;
    if item.kind == "torrent" {
        crate::torrent::play_in_order(m, &item).await;
    }
    let path = target(&item)
        .filter(|p| p.exists())
        .ok_or("Nothing of it is on disk yet: try again in a moment.")?;
    std::process::Command::new(player)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("VLC didn't start: {e}"))
}
