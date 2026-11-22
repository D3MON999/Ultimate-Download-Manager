//! Video and audio from web pages (YouTube and the 1,800+ sites yt-dlp knows), through the
//! reused `udm-tools` crate: installing yt-dlp and FFmpeg (signature- and checksum-checked),
//! looking a page up (one video's qualities and subtitles, or a playlist's entries), and the
//! choice a queued video download keeps.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use udm_tools::proc::FromBrowser;
use udm_tools::resolve::{self, Looked};
use serde::{Deserialize, Serialize};

use crate::manager::Context;

/// What a queued video download asks yt-dlp for (stored as JSON in its row).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoOpts {
    /// yt-dlp's `-f` selector, or an audio conversion (`mp3:320`, `opus:`, `flac:`).
    pub selector: String,
    /// The resulting container (`mp4`, `mp3`…).
    pub ext: String,
    /// The size the choice was shown with.
    pub expected: Option<u64>,
    /// Subtitle languages to save (none: no subtitles).
    pub subs: Vec<String>,
    pub automatic_subs: bool,
    /// Put the subtitles into the video file (needs FFmpeg).
    pub embed_subs: bool,
    /// Only this part, `1:20-3:45` (empty: all of it).
    #[serde(default)]
    pub section: String,
    /// Cut out sponsor segments, self-promotion and "subscribe" reminders (SponsorBlock).
    #[serde(default)]
    pub sponsorblock: bool,
    /// Also save each chapter as a file of its own.
    #[serde(default)]
    pub chapters: bool,
}

impl VideoOpts {
    /// yt-dlp's options for the part, SponsorBlock and chapters (fixed choices; the part is
    /// checked: digits, colons, a dash).
    pub fn extra(&self, dir: &Path, stem: &str) -> Vec<String> {
        let mut out = Vec::new();
        let s = self.section.trim();
        if !s.is_empty() && s.contains('-') && s.chars().all(|c| c.is_ascii_digit() || matches!(c, ':' | '-' | '.' | ' ')) {
            out.extend(["--download-sections".into(), format!("*{}", s.replace(' ', "")), "--force-keyframes-at-cuts".into()]);
        }
        if self.sponsorblock {
            out.extend(["--sponsorblock-remove".into(), "sponsor,selfpromo,interaction".into()]);
        }
        if self.chapters {
            let tpl = dir.join(format!("{} - %(section_number)02d %(section_title)s.%(ext)s", stem.replace('%', "%%")));
            out.extend(["--split-chapters".into(), "-o".into(), format!("chapter:{}", tpl.display())]);
        }
        out
    }
}

pub fn tools_dir() -> Result<PathBuf, String> {
    udm_tools::tools_dir().ok_or_else(|| "Ultimate Download Manager has no data folder".into())
}

#[derive(Serialize)]
pub struct ToolsStatus {
    pub ytdlp: Option<String>,
    pub ffmpeg: Option<String>,
}

pub fn status() -> ToolsStatus {
    let dir = tools_dir().ok();
    ToolsStatus {
        ytdlp: dir.as_deref().and_then(udm_tools::installed).map(|i| i.version),
        ffmpeg: dir.as_deref().and_then(udm_tools::ffmpeg::installed).map(|i| i.version),
    }
}

fn net() -> udm_engine::Options {
    udm_tools::net(udm_engine::Proxy::System)
}

/// Retry a tool install a few times: GitHub's release servers drop the odd connection.
async fn with_retries<T, F, Fut>(what: &str, mut f: F) -> Result<T, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, udm_tools::Error>>,
{
    let mut last = String::new();
    for wait in [0u64, 3, 10] {
        tokio::time::sleep(Duration::from_secs(wait)).await;
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("{what}: {last}"))
}

async fn install_ytdlp(dir: &Path) -> Result<(), String> {
    let net = net();
    with_retries("yt-dlp", || udm_tools::install_latest(dir, &net, None)).await.map(|_| ())
}

async fn install_ffmpeg(dir: &Path) -> Result<(), String> {
    let net = net();
    with_retries("FFmpeg", || udm_tools::ffmpeg::install_latest(dir, &net, None))
        .await
        .map(|_| ())
}

/// Install (or update) yt-dlp and FFmpeg: yt-dlp's release checked against its publisher's
/// signing key, FFmpeg's against its published checksums. Each on its own: one failing leaves
/// the other.
pub async fn install() -> Result<ToolsStatus, String> {
    let dir = tools_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let a = install_ytdlp(&dir).await;
    let b = install_ffmpeg(&dir).await;
    match (a, b) {
        (Err(a), Err(b)) => Err(format!("{a}; {b}")),
        (Err(e), _) | (_, Err(e)) => Err(e),
        _ => Ok(status()),
    }
}

/// yt-dlp, installing it first when it's missing; FFmpeg too, best-effort (needed only to
/// merge separate picture and sound, and to convert audio).
pub async fn ytdlp() -> Result<PathBuf, String> {
    let dir = tools_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if udm_tools::installed(&dir).is_none() {
        install_ytdlp(&dir).await?;
    }
    if udm_tools::ffmpeg::installed(&dir).is_none() {
        let _ = install_ffmpeg(&dir).await;
    }
    udm_tools::installed(&dir)
        .map(|i| i.exe)
        .ok_or_else(|| "yt-dlp couldn't be installed".into())
}

/// gallery-dl, installing it first when it's missing (its release checked against its
/// author's signing key).
pub async fn gallery_dl() -> Result<udm_tools::Installed, String> {
    let dir = tools_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if let Some(i) = udm_tools::gallery::installed(&dir) {
        return Ok(i);
    }
    let net = net();
    with_retries("gallery-dl", || udm_tools::gallery::install_latest(&dir, &net, None)).await
}

pub fn ffmpeg() -> Option<PathBuf> {
    tools_dir().ok().as_deref().and_then(udm_tools::ffmpeg::installed).map(|i| i.exe)
}

pub fn from_browser(referer: Option<String>, ctx: &Context) -> FromBrowser {
    FromBrowser {
        referer,
        user_agent: ctx.user_agent.clone(),
        cookies: ctx.cookies.clone(),
        jar: Vec::new(),
    }
}

#[derive(Serialize)]
pub struct ChoiceView {
    pub label: String,
    pub selector: String,
    pub ext: String,
    pub size: Option<u64>,
    pub audio: bool,
    pub holds_subs: bool,
}

#[derive(Serialize)]
pub struct SubView {
    pub code: String,
    pub name: String,
    pub automatic: bool,
}

#[derive(Serialize)]
pub struct EntryView {
    pub index: u64,
    pub url: String,
    pub title: String,
    pub duration: Option<f64>,
    pub unavailable: bool,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LookupView {
    Video {
        title: String,
        duration: Option<f64>,
        choices: Vec<ChoiceView>,
        subs: Vec<SubView>,
        /// FFmpeg is missing, and merging would give a better picture.
        needs_ffmpeg: bool,
        thumbnail: Option<String>,
        /// The languages its sound comes in, when there are several.
        audio_langs: Vec<String>,
        /// It has chapters to split by.
        chapters: bool,
    },
    List {
        title: String,
        total: Option<u64>,
        entries: Vec<EntryView>,
        capped: bool,
    },
}

/// The longest a playlist listing runs before Ultimate Download Manager shows what it has.
const LIST_WAIT: Duration = Duration::from_secs(90);

/// Look `url` up (blocking: yt-dlp runs).
pub fn lookup(exe: &Path, url: &str, browser: &FromBrowser) -> Result<LookupView, String> {
    let can_merge = ffmpeg().is_some();
    match resolve::look_up(exe, url, browser).map_err(|e| e.to_string())? {
        Looked::Video(v) => {
            let choices = resolve::choices(&v, can_merge)
                .into_iter()
                .map(|c| ChoiceView {
                    audio: resolve::is_audio(&c),
                    holds_subs: resolve::holds_subtitles(&c),
                    label: c.label,
                    selector: c.selector,
                    ext: c.ext,
                    size: c.size,
                })
                .collect();
            let subs = resolve::sub_choices(&v, "en")
                .into_iter()
                .map(|s| SubView { code: s.code, name: s.name, automatic: s.automatic })
                .collect();
            Ok(LookupView::Video {
                needs_ffmpeg: !can_merge && resolve::needs_merging(&v),
                thumbnail: v.thumbnail.clone(),
                audio_langs: v.audio_langs.clone(),
                chapters: v.chapters,
                title: v.title,
                duration: v.duration,
                choices,
                subs,
            })
        }
        Looked::List(page) => {
            let listing = udm_tools::playlist::Listing::start(exe, url, browser, 1, None)
                .map_err(|e| e.to_string())?;
            let started = Instant::now();
            loop {
                std::thread::sleep(Duration::from_millis(300));
                let p = listing.progress();
                if p.done || started.elapsed() > LIST_WAIT {
                    if !p.done {
                        listing.cancel();
                    }
                    if let Some(e) = p.error.filter(|_| p.listed == 0) {
                        return Err(e);
                    }
                    let entries = listing
                        .entries_from(0)
                        .into_iter()
                        .map(|e| EntryView {
                            index: e.index,
                            url: e.url,
                            title: e.title,
                            duration: e.duration,
                            unavailable: e.unavailable,
                        })
                        .collect();
                    return Ok(LookupView::List {
                        title: p.title.unwrap_or(page.title),
                        total: p.total.or(page.total),
                        entries,
                        capped: p.capped || !p.done,
                    });
                }
            }
        }
    }
}

/// The choice for a quality name (`best`, `1080`, `720`, `480`, `mp3`), as playlists and
/// subscriptions use them.
pub fn quality_opts(quality: &str) -> VideoOpts {
    match quality {
        "mp3" => VideoOpts { selector: "mp3:0".into(), ext: "mp3".into(), ..Default::default() },
        "1080" | "720" | "480" => VideoOpts {
            selector: format!("bv*[height<={quality}]+ba/b[height<={quality}]"),
            ext: "mp4".into(),
            ..Default::default()
        },
        _ => VideoOpts { selector: "bv*+ba/b".into(), ext: "mp4".into(), ..Default::default() },
    }
}

/// A file name for `title` (what yt-dlp's output template gets as the stem).
pub fn stem(title: &str) -> String {
    if title.trim().is_empty() {
        return "video".into();
    }
    let s = udm_engine::naming::sanitize(title.trim());
    let s: String = s.chars().take(150).collect();
    if s.trim().is_empty() { "video".into() } else { s.trim().to_owned() }
}

/// The sidebar category of a choice.
pub fn category(opts: &VideoOpts) -> &'static str {
    match crate::store::category_of(&format!("x.{}", opts.ext)) {
        "music" => "music",
        _ if opts.selector == "ba" || opts.selector.contains(':') => "music",
        _ => "video",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_and_categories() {
        assert_eq!(stem("  A/B: clip?  "), stem("A/B: clip?"));
        assert!(!stem("A/B: clip?").contains('/'));
        assert_eq!(stem("   "), "video");
        let mp3 = VideoOpts { selector: "mp3:320".into(), ext: "mp3".into(), ..Default::default() };
        assert_eq!(category(&mp3), "music");
        let mp4 = VideoOpts { selector: "bv*+ba/b".into(), ext: "mp4".into(), ..Default::default() };
        assert_eq!(category(&mp4), "video");
    }
}
