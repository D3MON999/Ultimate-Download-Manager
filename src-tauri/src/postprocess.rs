//! What happens to a finished file: archives are unpacked (zip, tar, gz and xz by Ultimate Download Manager
//! itself; 7z and RAR with 7-Zip when it's installed), safely (no file lands outside the folder,
//! size and count limits stop archive bombs), and, when chosen, Microsoft Defender scans it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use udm_extract::{Format, Limits, Policy, Subfolder, volume_key};

use crate::manager::Manager;

pub fn after_download(m: Arc<Manager>, path: PathBuf, password: Option<String>) {
    // A .torrent file (a browser's download of one): open it as a torrent, to pick its files.
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("torrent")) {
        m.show_torrent(&path);
        return;
    }
    tauri::async_runtime::spawn_blocking(move || {
        let s = m.settings();
        if s.sound {
            ding();
        }
        if s.scan_downloads {
            scan(&m, &path);
        }
        if s.extract_archives {
            // The download's own password first, then the saved list.
            let mut passwords: Vec<String> = password.into_iter().collect();
            passwords.extend(s.archive_passwords.iter().filter(|p| !p.is_empty()).cloned());
            extract(&m, &path, s.delete_archives_after_extract, &passwords);
        }
    });
}

/// The volume to start from for `path` (itself, or a split archive's first volume beside it).
fn first_volume(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let Some(v) = volume_key(&name) else {
        return Some(path.to_owned());
    };
    if v.is_first() {
        return Some(path.to_owned());
    }
    let dir = path.parent()?;
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.file_name()
            .and_then(|n| volume_key(&n.to_string_lossy()))
            .is_some_and(|o| o.base == v.base && o.kind == v.kind && o.is_first())
    })
}

/// Another volume of `path`'s set is still in the list, not finished: unpack when it is.
fn set_unfinished(m: &Manager, path: &Path) -> bool {
    let Some(v) = path.file_name().and_then(|n| volume_key(&n.to_string_lossy())) else {
        return false;
    };
    let Some(dir) = path.parent() else { return false };
    m.all_items().iter().any(|i| {
        i.state != crate::store::State::Done
            && Path::new(&i.dir) == dir
            && volume_key(&i.display_name()).is_some_and(|o| o.base.eq_ignore_ascii_case(&v.base) && o.kind == v.kind)
    })
}

/// Every volume of `first`'s set beside it.
fn volumes_of(first: &Path) -> Vec<PathBuf> {
    let Some(v) = first.file_name().and_then(|n| volume_key(&n.to_string_lossy())) else {
        return vec![first.to_owned()];
    };
    let Some(dir) = first.parent() else { return vec![first.to_owned()] };
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![first.to_owned()] };
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| volume_key(&n.to_string_lossy()))
                    .is_some_and(|o| o.base == v.base && o.kind == v.kind)
        })
        .collect()
}

/// Sets being unpacked now: two volumes finishing together start one unpack, not two.
fn unpacking() -> &'static std::sync::Mutex<std::collections::HashSet<PathBuf>> {
    static S: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> = std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

fn extract(m: &Arc<Manager>, path: &Path, delete_after: bool, passwords: &[String]) {
    if set_unfinished(m, path) {
        return;
    }
    let Some(archive) = first_volume(path) else {
        return;
    };
    if !unpacking().lock().unwrap().insert(archive.clone()) {
        return;
    }
    extract_set(m, &archive, delete_after, passwords);
    unpacking().lock().unwrap().remove(&archive);
}

fn extract_set(m: &Arc<Manager>, archive: &Path, delete_after: bool, passwords: &[String]) {
    let archive = archive.to_owned();
    if !matches!(Format::detect(&archive), Ok(Some(_))) {
        return;
    }
    let Some(dest) = archive.parent().map(Path::to_owned) else {
        return;
    };
    let size = std::fs::metadata(&archive).map(|x| x.len()).unwrap_or(0);
    let host = format!("file:///{}", archive.display());
    let name = archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // No password first; then each one, while the archive says it's the wrong one.
    let tries: Vec<Option<&String>> = std::iter::once(None).chain(passwords.iter().map(Some)).collect();
    let mut result = Err(udm_extract::Error::NeedsPassword);
    for pw in tries {
        let mut mark = |f: &Path| crate::manager::mark_of_the_web(f, &host, None);
        let mut policy = Policy {
            dest: dest.clone(),
            subfolder: Subfolder::Smart,
            password: pw.cloned(),
            limits: Limits::for_archive(size),
            tool: udm_extract::find_7zip(),
            on_file: &mut mark,
            cancel: None,
        };
        result = udm_extract::extract(&archive, &mut policy);
        if !matches!(result, Err(udm_extract::Error::NeedsPassword | udm_extract::Error::WrongPassword)) {
            break;
        }
    }
    match result {
        Ok(report) => {
            m.notify_pub(
                "Archive unpacked",
                &format!("{name}: {} files into {}", report.files, report.folder.display()),
            );
            if delete_after {
                for v in volumes_of(&archive) {
                    let _ = trash::delete(&v);
                }
            }
        }
        // A later volume isn't there yet: unpacked when it arrives.
        Err(udm_extract::Error::MissingVolume(_)) => {}
        Err(udm_extract::Error::NeedsPassword | udm_extract::Error::WrongPassword) => m.notify_pub(
            "Archive not unpacked",
            &format!("{name} needs a password: add it in the download's details, or to Settings → Archives"),
        ),
        Err(udm_extract::Error::NoTool) => m.notify_pub(
            "Archive not unpacked",
            &format!("{name}: 7z and RAR archives need 7-Zip (7-zip.org) installed"),
        ),
        Err(e) => m.notify_pub("Archive not unpacked", &format!("{name}: {e}")),
    }
}

/// Microsoft Defender's command-line scanner on one file: exit code 2 means it found a threat.
fn scan(m: &Arc<Manager>, path: &Path) {
    let program = std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
        .join(r"Windows Defender\MpCmdRun.exe");
    if !program.is_file() {
        return;
    }
    let mut cmd = std::process::Command::new(program);
    cmd.args(["-Scan", "-ScanType", "3", "-File"]).arg(path).arg("-DisableRemediation");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    if let Ok(out) = cmd.output()
        && out.status.code() == Some(2)
    {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        m.notify_pub("Threat found", &format!("Microsoft Defender flagged {name}. Don't open it."));
    }
}

/// A short sound (Settings → sound when a download finishes).
fn ding() {
    let wav = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join(r"Media\Windows Notify System Generic.wav");
    if !wav.is_file() {
        return;
    }
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(format!("(New-Object Media.SoundPlayer '{}').PlaySync()", wav.display()));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let _ = cmd.spawn();
}

/// What a finished file can be converted to, with FFmpeg.
pub const PRESETS: [(&str, &str); 6] = [
    ("mp3", "Audio: MP3 (320 kbps)"),
    ("m4a", "Audio: M4A (AAC 256 kbps)"),
    ("flac", "Audio: FLAC (lossless)"),
    ("mp4", "Video: MP4 (H.264 + AAC, plays everywhere)"),
    ("mp4-720", "Video: MP4 720p (smaller, for phones)"),
    ("gif", "Animated GIF (first 15 s, 480 px wide)"),
];

/// Convert `input` with preset `preset`, beside it (a new file; the original stays). The new
/// path, or why it failed.
pub fn convert(input: &Path, preset: &str) -> Result<PathBuf, String> {
    let ffmpeg = crate::video::ffmpeg().ok_or("FFmpeg isn't installed yet: Settings → Video tools → Install")?;
    let (ext, args): (&str, Vec<&str>) = match preset {
        "mp3" => ("mp3", vec!["-vn", "-c:a", "libmp3lame", "-b:a", "320k"]),
        "m4a" => ("m4a", vec!["-vn", "-c:a", "aac", "-b:a", "256k"]),
        "flac" => ("flac", vec!["-vn", "-c:a", "flac"]),
        "mp4" => ("mp4", vec!["-c:v", "libx264", "-preset", "medium", "-crf", "20", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"]),
        "mp4-720" => ("mp4", vec!["-vf", "scale=-2:'min(720,ih)'", "-c:v", "libx264", "-preset", "medium", "-crf", "23", "-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart"]),
        "gif" => ("gif", vec!["-t", "15", "-vf", "fps=12,scale=480:-1:flags=lanczos", "-loop", "0"]),
        _ => return Err("Unknown conversion".into()),
    };
    let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "converted".into());
    let dir = input.parent().ok_or("No folder")?;
    let suffix = if preset == "mp4-720" { " (720p)" } else { "" };
    let out = udm_engine::naming::unique_path(dir, &format!("{stem}{suffix}.{ext}"));
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"]).arg(input).args(&args).arg(&out);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let o = cmd.output().map_err(|e| format!("FFmpeg didn't start: {e}"))?;
    if !o.status.success() {
        let _ = std::fs::remove_file(&out);
        let msg = String::from_utf8_lossy(&o.stderr);
        let line = msg.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("FFmpeg failed");
        return Err(format!("Conversion failed: {line}"));
    }
    Ok(out)
}

/// Settings → Rules → Convert: a finished video or song from a matching site is converted (a
/// new file beside it), the original sent to the Recycle Bin when asked.
pub fn auto_convert(m: Arc<Manager>, item: &crate::store::Item, path: PathBuf) {
    let host = url::Url::parse(&item.url).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase)).unwrap_or_default();
    let category = crate::store::category_of(&path.to_string_lossy()).to_owned();
    let Some(rule) = m.settings().auto_convert.into_iter().find(|r| {
        r.enabled
            && (r.host.trim().is_empty() || host.contains(&r.host.trim().to_ascii_lowercase()))
            && (r.category.is_empty() || r.category == category)
            && matches!(category.as_str(), "video" | "music")
    }) else {
        return;
    };
    tauri::async_runtime::spawn_blocking(move || {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match convert(&path, &rule.preset) {
            Ok(out) => {
                if rule.delete_original {
                    let _ = trash::delete(&path);
                }
                m.notify_pub(
                    "Converted",
                    &format!("{name} → {}", out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                );
            }
            Err(e) => m.notify_pub("Not converted", &format!("{name}: {e}")),
        }
    });
}
