//! The link grabber: links found in any pasted text (or sent from the
//! browser's "all links" menu, or copied), numbered patterns expanded (`file[1-20].zip`), each
//! sorted (a file, a video page, a torrent), files checked online (name, size, gone or not),
//! duplicates of the list flagged, and archive parts grouped into one package.

use std::collections::HashSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::manager::{UDM_AGENT, BROWSER_AGENT, Manager};

/// The most links one paste or one pattern may give.
const MAX_LINKS: usize = 2000;
/// Links checked at once.
const CHECKS_AT_ONCE: usize = 8;

/// Sites whose pages are videos for yt-dlp rather than files.
const VIDEO_HOSTS: [&str; 24] = [
    "youtube.com", "youtu.be", "instagram.com", "x.com", "twitter.com", "tiktok.com",
    "facebook.com", "fb.watch", "reddit.com", "redd.it", "twitch.tv", "vimeo.com",
    "dailymotion.com", "soundcloud.com", "bilibili.com", "bandcamp.com", "rumble.com",
    "kick.com", "threads.net", "pinterest.com", "linkedin.com", "streamable.com",
    "odysee.com", "mixcloud.com",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Grabbed {
    pub url: String,
    /// `file`, `video` or `torrent`.
    pub kind: String,
    pub name: String,
    pub size: Option<u64>,
    /// `online`, `offline` or `unknown` (not checked: videos and torrents).
    pub status: String,
    /// Why it's offline.
    pub note: Option<String>,
    /// Already in Ultimate Download Manager's list.
    pub duplicate: bool,
    /// The package it groups into.
    pub package: String,
}

/// Every link in `text`, numbered patterns expanded, each once, in order.
pub fn links_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw in text.split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '`' | '|')) {
        let token = raw.trim_matches(|c: char| matches!(c, ',' | ';' | ')' | '(' | '[' | ']' | '{' | '}') && raw.len() > 1);
        let lower = token.to_ascii_lowercase();
        let start = ["http://", "https://", "ftp://", "ftps://", "magnet:?"]
            .iter()
            .filter_map(|p| lower.find(p))
            .min();
        let Some(start) = start else { continue };
        let link = &token[start..];
        for l in expand(link) {
            if out.len() >= MAX_LINKS {
                return out;
            }
            if seen.insert(l.clone()) {
                out.push(l);
            }
        }
    }
    out
}

/// `file[1-3].zip` → file1.zip, file2.zip, file3.zip; `[01-10]` keeps the zeros; `[a-c]`
/// letters. A link with no pattern is itself. One pattern per link.
pub fn expand(link: &str) -> Vec<String> {
    let (Some(open), Some(close)) = (link.find('['), link.find(']')) else {
        return vec![link.to_owned()];
    };
    if close < open {
        return vec![link.to_owned()];
    }
    let inner = &link[open + 1..close];
    let Some((a, b)) = inner.split_once('-') else {
        return vec![link.to_owned()];
    };
    let (pre, post) = (&link[..open], &link[close + 1..]);
    if let (Ok(x), Ok(y)) = (a.parse::<u64>(), b.parse::<u64>()) {
        if x > y || y - x >= MAX_LINKS as u64 {
            return vec![link.to_owned()];
        }
        let width = if a.starts_with('0') && a.len() > 1 { a.len() } else { 0 };
        return (x..=y).map(|n| format!("{pre}{n:0width$}{post}")).collect();
    }
    let (ca, cb) = (a.chars().next(), b.chars().next());
    if a.len() == 1 && b.len() == 1
        && let (Some(x), Some(y)) = (ca, cb)
        && x.is_ascii_alphabetic() && y.is_ascii_alphabetic() && x <= y
    {
        return (x..=y).map(|c| format!("{pre}{c}{post}")).collect();
    }
    vec![link.to_owned()]
}

pub fn kind_of(url: &str) -> &'static str {
    if url.to_ascii_lowercase().starts_with("magnet:?") {
        return "torrent";
    }
    let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase)).unwrap_or_default();
    if VIDEO_HOSTS.iter().any(|h| crate::manager::same_site(&host, h)) {
        "video"
    } else if url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase().ends_with(".torrent") {
        "torrent"
    } else {
        "file"
    }
}

/// The package a file groups into: its name without part numbers and archive extensions
/// (`movie.part2.rar` and `movie.part1.rar` → `movie`).
pub fn package_of(kind: &str, name: &str, host: &str) -> String {
    match kind {
        "video" => format!("Videos from {host}"),
        "torrent" => "Torrents".into(),
        _ => {
            if let Some(v) = udm_extract::volume_key(name) {
                return v.base;
            }
            let base = udm_extract::base_name(std::path::Path::new(name));
            match base.rsplit_once('.') {
                Some((stem, ext)) if ext.len() <= 5 && !stem.is_empty() => stem.to_owned(),
                _ => base,
            }
        }
    }
}

fn name_from_url(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.path_segments()?
                .rev()
                .find(|s| !s.is_empty())
                .map(|s| percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned())
        })
        .unwrap_or_else(|| url.to_owned())
}

/// Sort and check `urls`: files are looked at online (name, size, gone or not), several at a
/// time; videos and torrents are named from their address.
pub async fn check(m: &Arc<Manager>, urls: Vec<String>) -> Vec<Grabbed> {
    let known: HashSet<String> = m.known_urls();
    let gate = Arc::new(Semaphore::new(CHECKS_AT_ONCE));
    let mut tasks = tokio::task::JoinSet::new();
    for (i, url) in urls.into_iter().enumerate() {
        let gate = gate.clone();
        let dup = known.contains(&url);
        tasks.spawn(async move {
            let kind = kind_of(&url);
            let host = url::Url::parse(&url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default();
            let mut g = Grabbed {
                name: if kind == "torrent" { "Magnet link".into() } else { name_from_url(&url) },
                kind: kind.into(),
                size: None,
                status: "unknown".into(),
                note: None,
                duplicate: dup,
                package: String::new(),
                url: url.clone(),
            };
            let parsed = url::Url::parse(&url).ok();
            // A file host's page link (fuckingfast.co): its page isn't the file, so it isn't
            // probed; the name is the one in the link, and the file's address is asked for when
            // the download starts (hosts.rs).
            if kind == "file"
                && let Some(p) = parsed.as_ref()
                && (crate::hosts::unlocker_for(p).is_some() || crate::hosts::needs_a_person(p))
            {
                if let Some(n) = crate::hosts::name_in_link(p) {
                    g.name = n;
                }
                g.note = Some(if crate::hosts::needs_a_person(p) {
                    format!("{host}: opens a browser window for its wait and captcha when it starts")
                } else {
                    format!("{host}: checked when it starts")
                });
            } else if kind == "file"
                && let Some(parsed) = parsed
            {
                let _permit = gate.acquire().await;
                let opts = udm_engine::Options {
                    user_agent: UDM_AGENT.to_owned(),
                    agent_fallback: Some(BROWSER_AGENT.to_owned()),
                    ..udm_engine::Options::default()
                };
                let looked = tokio::time::timeout(
                    std::time::Duration::from_secs(25),
                    udm_engine::probe(parsed, Default::default(), &opts),
                )
                .await;
                match looked {
                    Ok(Ok(info)) => {
                        g.name = info.file_name;
                        g.size = info.size;
                        g.status = "online".into();
                    }
                    Ok(Err(e)) => {
                        g.status = "offline".into();
                        g.note = Some(e.to_string());
                    }
                    Err(_) => g.note = Some("no answer in time".into()),
                }
            }
            g.package = package_of(kind, &g.name, &host);
            (i, g)
        });
    }
    let mut out: Vec<(usize, Grabbed)> = Vec::new();
    while let Some(r) = tasks.join_next().await {
        if let Ok(x) = r {
            out.push(x);
        }
    }
    out.sort_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, g)| g).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_expand() {
        assert_eq!(expand("https://x.test/f[1-3].zip").len(), 3);
        assert_eq!(expand("https://x.test/f[08-10].zip")[0], "https://x.test/f08.zip");
        assert_eq!(expand("https://x.test/[a-c].jpg")[2], "https://x.test/c.jpg");
        assert_eq!(expand("https://x.test/plain.zip"), vec!["https://x.test/plain.zip"]);
    }

    #[test]
    fn links_found_in_text() {
        let text = "see <a href=\"https://a.test/x.zip\">x</a>, also https://a.test/x.zip and\nmagnet:?xt=urn:btih:abc ftp://f.test/y.iso, (https://youtu.be/123)";
        let got = links_in(text);
        assert_eq!(got, vec!["https://a.test/x.zip", "magnet:?xt=urn:btih:abc", "ftp://f.test/y.iso", "https://youtu.be/123"]);
        assert_eq!(kind_of(&got[1]), "torrent");
        assert_eq!(kind_of(&got[3]), "video");
        assert_eq!(kind_of(&got[0]), "file");
    }

    #[test]
    fn parts_group_into_one_package() {
        assert_eq!(package_of("file", "movie.part1.rar", "h"), package_of("file", "movie.part2.rar", "h"));
        assert_eq!(package_of("file", "setup.exe", "h"), "setup");
        assert_eq!(package_of("video", "x", "youtube.com"), "Videos from youtube.com");
    }
}
