//! "Download file info": a download the browser hands over waits in a small window
//! of its own, where the person sees its name, size and site, picks the category and the folder
//! (the last one used for that category, else the type's folder), and starts it, adds it for
//! later, or cancels. A window of its own, on top, so it shows while the main window is in the
//! notification area. Off in Settings → General, or skipped for chosen file types.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Manager as _, WebviewUrl, WebviewWindowBuilder};

use crate::manager::{Context, Manager};

/// A download waiting for the person's choice.
#[derive(Clone)]
pub struct Asked {
    pub url: String,
    pub name: Option<String>,
    pub size: Option<u64>,
    pub mime: Option<String>,
    pub referer: Option<String>,
    pub ctx: Context,
    pub at: Instant,
}

fn waiting() -> &'static Mutex<HashMap<String, Asked>> {
    static W: OnceLock<Mutex<HashMap<String, Asked>>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The categories offered, with the folder each goes to (the label shown, the folder name used
/// when sorting by type).
pub const CATEGORIES: [(&str, &str, &str); 7] = [
    ("video", "Video", "Video"),
    ("music", "Music", "Music"),
    ("docs", "Documents", "Documents"),
    ("archives", "Compressed", "Compressed"),
    ("programs", "Programs", "Programs"),
    ("images", "Images", "Images"),
    ("other", "General", ""),
];

/// The extension of a download's name (or its address's last part), lower case.
pub fn ext_of(name: Option<&str>, url: &str) -> String {
    let from_url = url::Url::parse(url)
        .ok()
        .and_then(|u| u.path_segments().and_then(|mut s| s.next_back().map(str::to_owned)))
        .unwrap_or_default();
    let n = name.unwrap_or(&from_url);
    Path::new(n).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// Whether to ask about this download (Settings: ask on, its type not skipped).
pub fn should_ask(m: &Manager, name: Option<&str>, url: &str) -> bool {
    let s = m.settings();
    if !s.ask_where {
        return false;
    }
    let ext = ext_of(name, url);
    ext.is_empty() || !s.ask_skip_types.iter().any(|t| t.trim_start_matches('.').eq_ignore_ascii_case(&ext))
}

/// Hold `asked` and open its window.
pub fn ask(app: &AppHandle, asked: Asked) -> Result<(), String> {
    let token = format!("{:x}{:04x}", crate::store::now(), rand_u16());
    {
        let mut w = waiting().lock().unwrap();
        // Windows closed without an answer leave theirs behind: forget the old ones.
        w.retain(|_, a| a.at.elapsed().as_secs() < 3600);
        w.insert(token.clone(), asked);
    }
    let label = format!("newdl-{token}");
    // The token reaches the page as a global (a query in an app address may not survive).
    WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .initialization_script(format!("window.__UDM_NEW__ = {};", serde_json::to_string(&token).unwrap_or_default()))
        .title("Download file info")
        .inner_size(600.0, 430.0)
        .min_inner_size(520.0, 400.0)
        .resizable(true)
        .always_on_top(true)
        .center()
        .focused(true)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn rand_u16() -> u16 {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    (n ^ (n >> 16)) as u16
}

#[derive(Serialize)]
pub struct CategoryView {
    pub id: String,
    pub label: String,
    pub dir: String,
}

#[derive(Serialize)]
pub struct AskView {
    pub url: String,
    pub name: String,
    pub size: Option<u64>,
    pub mime: Option<String>,
    pub host: String,
    pub ext: String,
    pub category: String,
    pub categories: Vec<CategoryView>,
    pub connections: u32,
}

/// The folder a category's downloads go to now: the last one chosen for it, else its folder.
fn folder_for(m: &Manager, category: &str) -> PathBuf {
    let s = m.settings();
    if let Some(d) = s.last_dirs.get(category).filter(|d| Path::new(d).is_dir()) {
        return PathBuf::from(d);
    }
    let sub = CATEGORIES.iter().find(|c| c.0 == category).map(|c| c.2).unwrap_or("");
    if s.sort_by_type && !sub.is_empty() { s.download_dir.join(sub) } else { s.download_dir }
}

pub fn view(m: &Manager, token: &str) -> Result<AskView, String> {
    let a = waiting().lock().unwrap().get(token).cloned().ok_or("This download was already answered.")?;
    let name = a
        .name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| {
            url::Url::parse(&a.url)
                .ok()
                .and_then(|u| u.path_segments().and_then(|mut s| s.next_back().map(str::to_owned)))
                .filter(|s| !s.is_empty())
                .map(|s| percent_encoding::percent_decode_str(&s).decode_utf8_lossy().into_owned())
                .unwrap_or_else(|| "download".into())
        });
    let category = match crate::store::category_of(&name) {
        "torrents" => "other",
        c => c,
    }
    .to_owned();
    Ok(AskView {
        host: url::Url::parse(&a.url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default(),
        ext: ext_of(Some(&name), &a.url),
        url: a.url,
        size: a.size,
        mime: a.mime,
        categories: CATEGORIES
            .iter()
            .map(|(id, label, _)| CategoryView { id: (*id).into(), label: (*label).into(), dir: folder_for(m, id).to_string_lossy().into_owned() })
            .collect(),
        category,
        name,
        connections: m.settings().connections,
    })
}

/// The person's answer: save as `path` (a folder and a name), start now or later; remember the
/// folder for the category; stop asking for this type when `skip_type`.
pub fn confirm(app: &AppHandle, m: &Arc<Manager>, token: &str, path: &str, category: &str, start: bool, skip_type: bool, replace: bool) -> Result<i64, String> {
    let a = waiting().lock().unwrap().get(token).cloned().ok_or("This download was already answered.")?;
    let path = PathBuf::from(path.trim());
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty()).ok_or("Give the file a name.")?;
    let dir = path.parent().map(Path::to_owned).filter(|d| !d.as_os_str().is_empty()).ok_or("Choose a folder.")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // Replace: the file already there goes to the Recycle Bin (it can be got back).
    if replace && path.is_file() {
        trash::delete(&path).map_err(|e| format!("The file already there can't be replaced: {e}"))?;
    }
    let id = m.add(&a.url, Some(dir.clone()), Some(name.clone()), a.referer.clone(), start, Some(a.ctx.clone()))?;
    waiting().lock().unwrap().remove(token);
    // Remember the folder for the category, and the type to skip.
    let mut s = m.settings();
    s.last_dirs.insert(category.to_owned(), dir.to_string_lossy().into_owned());
    if skip_type {
        let ext = ext_of(Some(&name), &a.url);
        if !ext.is_empty() && !s.ask_skip_types.iter().any(|t| t.eq_ignore_ascii_case(&ext)) {
            s.ask_skip_types.push(ext);
        }
    }
    let _ = m.set_settings(s);
    // Started: the window stays, as its progress. Later: it closes.
    if !start {
        close(app, token);
    }
    Ok(id)
}

/// The video button's window: a small one of its own (not the whole app), shown at once with
/// the page's title; it looks the video's formats up itself and becomes the progress window.
pub fn ask_video(app: &AppHandle, page: &str, title: Option<&str>) -> Result<(), String> {
    let label = format!("video-{:x}{:04x}", crate::store::now(), rand_u16());
    let init = serde_json::json!({ "page": page, "title": title.unwrap_or("") });
    WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .initialization_script(format!("window.__UDM_VIDEO__ = {init};"))
        .title("Download video")
        .inner_size(620.0, 520.0)
        .min_inner_size(540.0, 440.0)
        .always_on_top(true)
        .center()
        .focused(true)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn watching() -> &'static Mutex<HashMap<i64, String>> {
    static W: OnceLock<Mutex<HashMap<i64, String>>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Window `label` shows download `id`'s progress (the progress view says so when it opens).
pub fn watch(id: i64, label: &str) {
    watching().lock().unwrap().insert(id, label.to_owned());
}

/// A window still open shows download `id`.
pub fn watched(app: &AppHandle, id: i64) -> bool {
    let label = watching().lock().unwrap().get(&id).cloned();
    label.is_some_and(|l| app.get_webview_window(&l).is_some()) || app.get_webview_window(&format!("progress-{id}")).is_some()
}

/// A progress window for download `id` (double-click in the list): the one already open, else a
/// new one.
pub fn show_progress(app: &AppHandle, id: i64) -> Result<(), String> {
    let label = format!("progress-{id}");
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .initialization_script(format!("window.__UDM_PROGRESS__ = {id};"))
        .title("Downloading")
        .inner_size(600.0, 430.0)
        .min_inner_size(520.0, 400.0)
        .center()
        .focused(true)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// What's at a save path: a file already there, the free space where it goes, and why the
/// download won't fit (too big for the drive, or for a FAT32 drive's 4 GB limit).
#[derive(Serialize)]
pub struct PathInfo {
    pub exists: bool,
    pub free: Option<u64>,
    pub problem: Option<String>,
    /// The name it gets when kept beside the existing file.
    pub renamed: String,
}

pub fn path_info(path: &str, size: Option<u64>) -> PathInfo {
    let p = PathBuf::from(path.trim());
    let dir = p.parent().map(Path::to_owned).unwrap_or_default();
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let vol = udm_engine::disk::volume_of(&dir);
    let problem = match (&vol, size) {
        (Some(v), Some(s)) => {
            let fs = v.file_system.to_ascii_uppercase();
            if s > udm_engine::disk::FAT32_MAX && matches!(fs.as_str(), "FAT32" | "FAT") {
                Some(format!("This drive ({}) can't hold files over 4 GB.", v.file_system))
            } else if s.saturating_add(udm_engine::disk::MARGIN) > v.free {
                Some(format!(
                    "Not enough space: it needs {}, the drive has {} free.",
                    udm_engine::disk::human(s),
                    udm_engine::disk::human(v.free)
                ))
            } else {
                None
            }
        }
        _ => None,
    };
    let renamed = udm_engine::naming::unique_path(&dir, &name)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or(name);
    PathInfo { exists: p.is_file(), free: vol.map(|v| v.free), problem, renamed }
}

pub fn cancel(app: &AppHandle, token: &str) {
    waiting().lock().unwrap().remove(token);
    close(app, token);
}

fn close(app: &AppHandle, token: &str) {
    if let Some(w) = app.get_webview_window(&format!("newdl-{token}")) {
        let _ = w.close();
    }
}
