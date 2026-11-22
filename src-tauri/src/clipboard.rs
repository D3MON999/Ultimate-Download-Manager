//! The clipboard watcher (off unless chosen in Settings): when text with downloadable links is
//! copied anywhere, Ultimate Download Manager offers them (a banner opening the link grabber). Nothing is read
//! while it's off.

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager as _};

use crate::grabber;
use crate::manager::Manager;

/// Sent with the links found in what was copied: `Vec<String>`.
pub const EV_CLIPBOARD: &str = "udm://clipboard";

/// Worth offering: a magnet, a video page, or an address that names a file (an extension Ultimate Download Manager
/// sorts, an archive part).
fn worth_offering(url: &str) -> bool {
    match grabber::kind_of(url) {
        "torrent" | "video" => true,
        _ => {
            let name = url.split(['?', '#']).next().unwrap_or("").rsplit('/').next().unwrap_or("");
            crate::store::category_of(name) != "other" || udm_extract::volume_key(name).is_some()
        }
    }
}

pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("udm-clipboard".into())
        .spawn(move || {
            let mut last = String::new();
            let mut board: Option<arboard::Clipboard> = None;
            loop {
                std::thread::sleep(Duration::from_millis(900));
                let on = app.try_state::<Arc<Manager>>().is_some_and(|m| m.settings().watch_clipboard);
                if !on {
                    board = None;
                    continue;
                }
                if board.is_none() {
                    board = arboard::Clipboard::new().ok();
                }
                let Some(text) = board.as_mut().and_then(|b| b.get_text().ok()) else {
                    continue;
                };
                if text == last || text.len() > 200_000 {
                    continue;
                }
                let first_look = last.is_empty();
                last = text.clone();
                // What was there when watching started isn't news.
                if first_look {
                    continue;
                }
                let links: Vec<String> = grabber::links_in(&text).into_iter().filter(|l| worth_offering(l)).collect();
                if !links.is_empty() {
                    let _ = app.emit(EV_CLIPBOARD, links);
                }
            }
        })
        .expect("a clipboard thread");
}
