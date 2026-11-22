//! Torrent RSS feeds (Settings → Torrents → RSS): each enabled feed is read every
//! `rss_minutes`; a new item whose title matches the feed's filter is added as a torrent (its
//! magnet link, or its .torrent file), with every file. The first read of a feed only marks what
//! is already there, so adding a feed doesn't download its whole history.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use crate::manager::Manager;
use crate::settings::{Feed, wildcard};

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct FeedItem {
    pub title: String,
    /// A magnet link or a .torrent file's address.
    pub link: String,
    pub guid: String,
}

/// Every feed, forever.
pub async fn run(m: Arc<Manager>) {
    // Give the torrent session a moment to start.
    tokio::time::sleep(Duration::from_secs(20)).await;
    loop {
        let s = m.settings();
        for f in s.feeds.iter().filter(|f| f.enabled && !f.url.trim().is_empty()) {
            if let Err(e) = check(&m, f).await {
                eprintln!("rss {}: {e}", f.url);
            }
        }
        tokio::time::sleep(Duration::from_secs(s.rss_minutes.max(5) * 60)).await;
    }
}

/// Read one feed now; the titles it added.
pub async fn check(m: &Arc<Manager>, feed: &Feed) -> Result<Vec<String>, String> {
    let items = fetch(&feed.url).await?;
    let first = !m.with_db(|db| {
        db.query_row("SELECT 1 FROM rss_seen WHERE feed = ?1 LIMIT 1", [&feed.url], |_| Ok(()))
            .map(|_| true)
            .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(false) } else { Err(e) })
    })
    .map_err(|e| e.to_string())?;
    let mut added = Vec::new();
    for it in items {
        let new = m
            .with_db(|db| {
                db.execute(
                    "INSERT OR IGNORE INTO rss_seen (feed, guid, seen_at) VALUES (?1, ?2, ?3)",
                    rusqlite::params![feed.url, it.guid, crate::store::now()],
                )
            })
            .map_err(|e| e.to_string())?
            > 0;
        if !new || first || !wanted(feed, &it.title) {
            continue;
        }
        match add(m, feed, &it).await {
            Ok(()) => added.push(it.title.clone()),
            Err(e) => m.notify_pub("RSS: torrent not added", &format!("{}: {e}", it.title)),
        }
    }
    if !added.is_empty() {
        m.notify_pub(
            "New from your feeds",
            &format!("{} added from {}", added.len(), if feed.name.is_empty() { &feed.url } else { &feed.name }),
        );
    }
    Ok(added)
}

/// The filter's words all in the title, none of the exclusions.
pub fn wanted(feed: &Feed, title: &str) -> bool {
    let t = title.to_ascii_lowercase();
    let has = |w: &str| if w.contains('*') || w.contains('?') { wildcard(&format!("*{w}*"), &t) } else { t.contains(w) };
    let all = feed.filter.to_ascii_lowercase();
    let none = feed.exclude.to_ascii_lowercase();
    all.split_whitespace().all(has) && !none.split_whitespace().any(has)
}

async fn add(m: &Arc<Manager>, feed: &Feed, it: &FeedItem) -> Result<(), String> {
    let source = if it.link.to_ascii_lowercase().starts_with("magnet:") {
        it.link.clone()
    } else {
        // A .torrent file: fetched to the data folder, then read like one dropped on the app.
        let bytes = reqwest::Client::builder()
            .user_agent(crate::manager::UDM_AGENT)
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())?
            .get(&it.link)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?
            .bytes()
            .await
            .map_err(|e| e.to_string())?;
        let path = std::env::temp_dir().join(format!("udm-rss-{}.torrent", crate::store::now()));
        std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
        path.to_string_lossy().into_owned()
    };
    let meta = crate::torrent::meta(m, &source).await?;
    let dir = (!feed.folder.trim().is_empty()).then(|| std::path::PathBuf::from(feed.folder.trim()));
    crate::torrent::add(m, &meta.hash, None, dir).await.map(|_| ())
}

pub async fn fetch(url: &str) -> Result<Vec<FeedItem>, String> {
    let text = reqwest::Client::builder()
        .user_agent(crate::manager::UDM_AGENT)
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("the feed can't be read ({e})"))?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    Ok(parse(&text))
}

/// The items of an RSS (`<item>`) or Atom (`<entry>`) feed: the title, and the torrent link
/// (a magnet in `<torrent:magnetURI>`, `<link>` or the enclosure; else a .torrent enclosure or
/// link).
pub fn parse(xml: &str) -> Vec<FeedItem> {
    let mut out = Vec::new();
    for tag in ["item", "entry"] {
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        let mut rest = xml;
        while let Some(i) = rest.find(&open) {
            let after = &rest[i..];
            let Some(j) = after.find(&close) else { break };
            let body = &after[..j];
            rest = &after[j + close.len()..];
            let title = text_of(body, "title").unwrap_or_default();
            let links: Vec<String> = [
                text_of(body, "torrent:magnetURI"),
                attr_of(body, "enclosure", "url"),
                text_of(body, "link").filter(|l| !l.is_empty()),
                attr_of(body, "link", "href"),
            ]
            .into_iter()
            .flatten()
            .collect();
            let link = links
                .iter()
                .find(|l| l.to_ascii_lowercase().starts_with("magnet:"))
                .or_else(|| links.iter().find(|l| l.to_ascii_lowercase().split(['?', '#']).next().unwrap_or("").ends_with(".torrent")))
                .or_else(|| attr_of(body, "enclosure", "type").filter(|t| t.contains("bittorrent")).and(links.get(1)))
                .cloned();
            let Some(link) = link else { continue };
            let guid = text_of(body, "guid").or_else(|| text_of(body, "id")).filter(|g| !g.is_empty()).unwrap_or_else(|| link.clone());
            out.push(FeedItem { title, link, guid });
        }
    }
    out
}

/// The text inside the first `<name …>…</name>`, entities and CDATA undone.
fn text_of(body: &str, name: &str) -> Option<String> {
    let open = format!("<{name}");
    let mut from = 0;
    loop {
        let i = body[from..].find(&open)? + from;
        let after = &body[i + open.len()..];
        // `<link>` but not `<linkx>`.
        if after.starts_with(['>', ' ', '\t', '\n', '\r', '/']) {
            let gt = after.find('>')?;
            if after[..gt].ends_with('/') {
                return Some(String::new());
            }
            let inner = &after[gt + 1..];
            let end = inner.find(&format!("</{name}>"))?;
            return Some(unescape(inner[..end].trim()));
        }
        from = i + open.len();
    }
}

fn attr_of(body: &str, name: &str, attr: &str) -> Option<String> {
    let open = format!("<{name}");
    let i = body.find(&open)?;
    let tag = &body[i..i + body[i..].find('>')?];
    for q in ['"', '\''] {
        let key = format!("{attr}={q}");
        if let Some(k) = tag.find(&key) {
            let v = &tag[k + key.len()..];
            return Some(unescape(&v[..v.find(q)?]));
        }
    }
    None
}

fn unescape(s: &str) -> String {
    let s = s.strip_prefix("<![CDATA[").and_then(|x| x.strip_suffix("]]>")).unwrap_or(s);
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&#39;", "'").replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rss_and_atom_items() {
        let rss = r#"<rss><channel><title>x</title>
          <item><title>Show S01E01 1080p</title><link>https://t.test/a.torrent</link><guid>1</guid></item>
          <item><title><![CDATA[Show S01E02 720p]]></title><link>https://t.test/page</link>
            <torrent:magnetURI><![CDATA[magnet:?xt=urn:btih:abc&amp;dn=x]]></torrent:magnetURI></item>
          <item><title>No torrent</title><link>https://t.test/page</link></item>
        </channel></rss>"#;
        let items = parse(rss);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].link, "https://t.test/a.torrent");
        assert_eq!(items[0].guid, "1");
        assert_eq!(items[1].title, "Show S01E02 720p");
        assert!(items[1].link.starts_with("magnet:?xt=urn:btih:abc&dn=x"));
        let atom = r#"<feed><entry><title>Linux ISO</title><id>tag:1</id>
          <link rel="enclosure" href="https://t.test/linux.torrent"/></entry></feed>"#;
        let items = parse(atom);
        assert_eq!(items, vec![FeedItem { title: "Linux ISO".into(), link: "https://t.test/linux.torrent".into(), guid: "tag:1".into() }]);
    }

    /// A real public feed (DistroWatch's Linux torrents) reads into items with .torrent links
    /// (network: run with --ignored). Reads the feed only; nothing is downloaded.
    #[tokio::test]
    #[ignore = "reads a public feed over the network"]
    async fn a_real_feed_reads() {
        let items = fetch("https://distrowatch.com/news/torrents.xml").await.expect("the feed");
        assert!(items.len() >= 5, "{items:?}");
        assert!(items.iter().all(|i| i.link.ends_with(".torrent")), "{items:?}");
        let f = Feed { filter: "amd64".into(), ..Feed::default() };
        assert!(items.iter().any(|i| wanted(&f, &i.title)));
    }

    #[test]
    fn filters() {
        let f = Feed { filter: "show 1080p".into(), exclude: "cam".into(), ..Feed::default() };
        assert!(wanted(&f, "Show S01E01 1080p WEB"));
        assert!(!wanted(&f, "Show S01E01 720p"));
        assert!(!wanted(&f, "Show 1080p CAM"));
        assert!(wanted(&Feed::default(), "anything"));
        let w = Feed { filter: "s01e*".into(), ..Feed::default() };
        assert!(wanted(&w, "Show S01E05"));
    }
}
