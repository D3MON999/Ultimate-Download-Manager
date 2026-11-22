//! File hosts whose links are a web page, not the file, and that need no account: the page's own
//! download button asks the site for the file's address. Each host is an engine `Unlocker`, asked
//! when a download starts and again when its address stops working (they expire), so a link in
//! the list always stays the host's page link.
//!
//! fuckingfast.co: a link is `https://fuckingfast.co/<id>#<file name>`. The page's button POSTs to
//! `/f/<id>/go`, and the site answers with the file's address in an `HX-Redirect` header (htmx).

use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use udm_engine::{ServiceFault, Unlocked, Unlocker};
use tokio_util::sync::CancellationToken;
use url::Url;

const FF_HOST: &str = "fuckingfast.co";
/// fuckingfast.co gives each connection about 1 MB/s and the total grows with every one
///. So a file from it
/// takes 32 connections whatever the default is; that's how other download managers fill the line.
const FF_CONNECTIONS: u32 = 32;

/// The unlocker for `url`'s host, when it is one of these hosts.
pub fn unlocker_for(url: &Url) -> Option<Arc<dyn Unlocker>> {
    if ff_id(url).is_some() {
        return Some(Arc::new(FuckingFast));
    }
    if pixeldrain_id(url).is_some() {
        return Some(Arc::new(Pixeldrain));
    }
    if is_mediafire(url) {
        return Some(Arc::new(Mediafire));
    }
    None
}

/// File hosts whose free download needs a person (a wait, a captcha): their links open in a
/// browser window (`check::assist`) when the page comes instead of the file.
pub const NEEDS_A_PERSON: [&str; 20] = [
    "1fichier.com", "ddownload.com", "deltabit.co", "fastfile.cc", "fastream.to", "filefox.cc",
    "hotlink.cc", "katfile.com", "maxstream.video", "rapidgator.net", "nitroflare.com",
    "turbobit.net", "uploaded.net", "uptobox.com", "hitfile.net", "filespace.com", "send.cm",
    "usersdrive.com", "dropapk.to", "mexa.sh",
];

/// `url` is on one of `NEEDS_A_PERSON`'s hosts.
pub fn needs_a_person(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    NEEDS_A_PERSON.iter().any(|h| host == *h || host.ends_with(&format!(".{h}")))
}

/// The address a browser window found (`Manager::assisted`), as the unlocker's answer.
pub fn found(url: Url) -> Arc<dyn Unlocker> {
    Arc::new(Found(url))
}

#[derive(Debug)]
struct Found(Url);

impl Unlocker for Found {
    fn name(&self) -> &str {
        "the browser window"
    }

    fn unlock<'a>(
        &'a self,
        _url: &'a Url,
        _note: &'a (dyn Fn(String) + Send + Sync),
        _cancel: &'a CancellationToken,
    ) -> BoxFuture<'a, Result<Option<Unlocked>, ServiceFault>> {
        Box::pin(async move { Ok(Some(Unlocked { url: self.0.clone(), file_name: None, size: None, max_connections: None, min_connections: None })) })
    }
}

// ---- pixeldrain: `pixeldrain.com/u/<id>` is a page; `/api/file/<id>?download` is the file.

fn pixeldrain_id(url: &Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    if !matches!(host.as_str(), "pixeldrain.com" | "www.pixeldrain.com" | "pixeldra.in") {
        return None;
    }
    let segs: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    match segs.as_slice() {
        ["u", id] if id.bytes().all(|b| b.is_ascii_alphanumeric()) => Some((*id).to_owned()),
        _ => None,
    }
}

#[derive(Debug)]
struct Pixeldrain;

impl Unlocker for Pixeldrain {
    fn name(&self) -> &str {
        "pixeldrain"
    }

    fn unlock<'a>(
        &'a self,
        url: &'a Url,
        _note: &'a (dyn Fn(String) + Send + Sync),
        _cancel: &'a CancellationToken,
    ) -> BoxFuture<'a, Result<Option<Unlocked>, ServiceFault>> {
        Box::pin(async move {
            let Some(id) = pixeldrain_id(url) else { return Ok(None) };
            let file = Url::parse(&format!("https://pixeldrain.com/api/file/{id}?download"))
                .map_err(|e| ServiceFault::Other(e.to_string()))?;
            Ok(Some(Unlocked { url: file, file_name: None, size: None, max_connections: None, min_connections: None }))
        })
    }
}

// ---- MediaFire: the file page holds the file's address on its download button.

fn is_mediafire(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    (host == "mediafire.com" || host == "www.mediafire.com")
        && url.path_segments().is_some_and(|mut s| matches!(s.next(), Some("file" | "file_premium" | "view")))
}

/// The download button's address in a MediaFire file page.
fn mediafire_link(page: &str) -> Option<Url> {
    // `href="https://download1234.mediafire.com/…"` (the button, `id="downloadButton"`).
    let mut rest = page;
    while let Some(i) = rest.find("https://download") {
        let tail = &rest[i..];
        let end = tail.find(['"', '\'', ' ', '<', '>']).unwrap_or(tail.len());
        let cand = &tail[..end];
        if let Ok(u) = Url::parse(cand)
            && u.host_str().is_some_and(|h| h.ends_with(".mediafire.com"))
        {
            return Some(u);
        }
        rest = &tail[16..];
    }
    None
}

#[derive(Debug)]
struct Mediafire;

impl Unlocker for Mediafire {
    fn name(&self) -> &str {
        "MediaFire"
    }

    fn unlock<'a>(
        &'a self,
        url: &'a Url,
        note: &'a (dyn Fn(String) + Send + Sync),
        _cancel: &'a CancellationToken,
    ) -> BoxFuture<'a, Result<Option<Unlocked>, ServiceFault>> {
        Box::pin(async move {
            if !is_mediafire(url) {
                return Ok(None);
            }
            note("Reading MediaFire's page for the file's address".into());
            let http = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .user_agent(crate::manager::BROWSER_AGENT)
                .build()
                .map_err(|e| ServiceFault::Other(e.to_string()))?;
            let resp = http.get(url.clone()).send().await.map_err(|_| ServiceFault::ServiceDown)?;
            let status = resp.status().as_u16();
            // A removed file redirects to MediaFire's error page.
            if status == 404 || resp.url().path().contains("error") {
                return Err(ServiceFault::FileUnavailable);
            }
            let page = resp.text().await.map_err(|_| ServiceFault::ServiceDown)?;
            match mediafire_link(&page) {
                Some(u) => Ok(Some(Unlocked { url: u, file_name: None, size: None, max_connections: None, min_connections: None })),
                None if page.contains("error.php") || page.contains("File Removed") => Err(ServiceFault::FileUnavailable),
                // No button: MediaFire wants a person (a captcha) — the engine then gets the
                // page, and the download offers the browser window.
                None => Ok(None),
            }
        })
    }
}

/// The name a host's link carries for its file (fuckingfast: the part after `#`), for the list
/// before the download starts.
pub fn name_in_link(url: &Url) -> Option<String> {
    ff_id(url)?;
    let frag = url.fragment()?;
    let name = percent_encoding::percent_decode_str(frag).decode_utf8_lossy().trim().to_owned();
    (!name.is_empty()).then_some(name)
}

/// A fuckingfast.co file link's id: `/<id>` (letters and digits), never `/dl/…` (already the file).
fn ff_id(url: &Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    if url.scheme() != "https" || (host != FF_HOST && host != format!("www.{FF_HOST}")) {
        return None;
    }
    let mut segs = url.path_segments()?.filter(|s| !s.is_empty());
    let id = segs.next()?;
    if segs.next().is_some() || !id.bytes().all(|b| b.is_ascii_alphanumeric()) || id.len() < 6 {
        return None;
    }
    Some(id.to_owned())
}

#[derive(Debug)]
struct FuckingFast;

impl Unlocker for FuckingFast {
    fn name(&self) -> &str {
        "fuckingfast.co"
    }

    fn unlock<'a>(
        &'a self,
        url: &'a Url,
        note: &'a (dyn Fn(String) + Send + Sync),
        cancel: &'a CancellationToken,
    ) -> BoxFuture<'a, Result<Option<Unlocked>, ServiceFault>> {
        Box::pin(async move {
            let Some(id) = ff_id(url) else { return Ok(None) };
            note("Asking fuckingfast.co for the file's address".into());
            let ask = ff_go(&id);
            tokio::select! {
                r = ask => r.map(|u| Some(Unlocked { url: u, file_name: name_in_link(url), size: None, max_connections: None, min_connections: Some(FF_CONNECTIONS) })),
                _ = cancel.cancelled() => Err(ServiceFault::Other("cancelled".into())),
            }
        })
    }
}

async fn ff_go(id: &str) -> Result<Url, ServiceFault> {
    // Asked as what it is, Ultimate Download Manager: Cloudflare in front of the site challenges a
    // client that claims to be Chrome but doesn't connect like one, and lets
    // the honest agent through. The page itself isn't needed (and is challenged either way).
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(crate::manager::UDM_AGENT)
        .build()
        .map_err(|e| ServiceFault::Other(e.to_string()))?;
    let page = format!("https://{FF_HOST}/{id}");
    let go = format!("https://{FF_HOST}/f/{id}/go");
    let resp = http
        .post(&go)
        .header("Accept", "*/*")
        .header("Origin", format!("https://{FF_HOST}"))
        .header("Referer", &page)
        .header("HX-Request", "true")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("")
        .send()
        .await
        .map_err(|_| ServiceFault::ServiceDown)?;
    let status = resp.status().as_u16();
    if let Some(next) = resp.headers().get("hx-redirect").and_then(|v| v.to_str().ok()) {
        let next = Url::parse(&format!("https://{FF_HOST}/"))
            .and_then(|b| b.join(next.trim()))
            .map_err(|_| ServiceFault::Other("fuckingfast.co answered with an address that can't be read".into()))?;
        return Ok(next);
    }
    Err(match status {
        404 | 410 => ServiceFault::FileUnavailable,
        429 => ServiceFault::TooManyRequests(
            resp.headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse().ok())
                .map(Duration::from_secs),
        ),
        500..=599 => ServiceFault::ServiceDown,
        s => ServiceFault::Other(format!("fuckingfast.co didn't give the file's address (HTTP {s})")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live site still hands out a file address for a link (network: run with --ignored and
    /// `UDM_FF_ID=<an id from a live link>`). Asks for the address only, never the file.
    #[tokio::test]
    #[ignore = "asks fuckingfast.co over the network"]
    async fn the_site_gives_a_file_address() {
        let Ok(id) = std::env::var("UDM_FF_ID") else { return };
        let u = ff_go(&id).await.expect("an address");
        assert_eq!(u.host_str(), Some("dl.fuckingfast.co"), "{u}");
        assert!(u.path().starts_with("/dl/"), "{u}");
        // The engine's client, as the engine asks: one byte, ranged (not the file).
        let http = reqwest::Client::builder().user_agent(crate::manager::UDM_AGENT).build().unwrap();
        let r = http.get(u.clone()).header("Range", "bytes=0-0").send().await.unwrap();
        assert_eq!(r.status().as_u16(), 206, "{:?}", r.headers());
        assert!(r.headers().get("content-disposition").is_some(), "{:?}", r.headers());
    }


    #[test]
    fn other_hosts_are_recognised() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert_eq!(pixeldrain_id(&u("https://pixeldrain.com/u/AbC123")).as_deref(), Some("AbC123"));
        assert_eq!(pixeldrain_id(&u("https://pixeldrain.com/l/AbC123")), None);
        assert!(is_mediafire(&u("https://www.mediafire.com/file/abc123/thing.zip/file")));
        assert!(!is_mediafire(&u("https://www.mediafire.com/folder/abc")));
        let page = r#"<a class="input popsok" aria-label="Download file" href="https://download2391.mediafire.com/xyz/abc123/thing.zip" id="downloadButton">"#;
        assert_eq!(mediafire_link(page).unwrap().as_str(), "https://download2391.mediafire.com/xyz/abc123/thing.zip");
        assert!(needs_a_person(&u("https://1fichier.com/?abcdef")));
        assert!(needs_a_person(&u("https://www.katfile.com/x/y.rar.html")));
        assert!(!needs_a_person(&u("https://example.com/a")));
    }

    #[test]
    fn links_are_recognised() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert_eq!(ff_id(&u("https://fuckingfast.co/abc123xyz#Game.part01.rar")).as_deref(), Some("abc123xyz"));
        assert_eq!(ff_id(&u("https://fuckingfast.co/abc123xyz")).as_deref(), Some("abc123xyz"));
        assert_eq!(ff_id(&u("https://fuckingfast.co/dl/abc123xyz")), None);
        assert_eq!(ff_id(&u("https://fuckingfast.co/")), None);
        assert_eq!(ff_id(&u("https://fuckingfast.co.evil.test/abc123xyz")), None);
        assert_eq!(ff_id(&u("http://fuckingfast.co/abc123xyz")), None);
        assert_eq!(
            name_in_link(&u("https://fuckingfast.co/abc123xyz#fg-01%20(1).bin")).as_deref(),
            Some("fg-01 (1).bin")
        );
        assert!(unlocker_for(&u("https://example.com/abc123xyz")).is_none());
    }
}
