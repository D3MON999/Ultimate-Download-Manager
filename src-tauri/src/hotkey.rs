//! A shortcut that works in any program (Settings → General, Ctrl+Shift+D by default): the link
//! just copied opens in the right window: a video page in the video window, a magnet link in the
//! torrent window, one file in Download file info, several links in the link grabber.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager as _};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

static CURRENT: Mutex<String> = Mutex::new(String::new());

/// The plugin, with what the shortcut does.
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                let app = app.clone();
                std::thread::spawn(move || copied(&app));
            }
        })
        .build()
}

/// Use `keys` as the shortcut (empty: none); a taken or unreadable one is left out.
pub fn apply(app: &AppHandle, keys: &str) {
    let keys = keys.trim().to_owned();
    let mut cur = CURRENT.lock().unwrap();
    if *cur == keys {
        return;
    }
    let gs = app.global_shortcut();
    if !cur.is_empty() {
        let _ = gs.unregister(cur.as_str());
    }
    *cur = String::new();
    if !keys.is_empty() {
        match gs.register(keys.as_str()) {
            Ok(()) => *cur = keys,
            Err(e) => eprintln!("udm: the shortcut {keys} can't be used: {e}"),
        }
    }
}

fn copied(app: &AppHandle) {
    let text = arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default();
    let links = crate::grabber::links_in(&text);
    let show = |what: serde_json::Value| {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
        let _ = app.emit(crate::bridge::EV_SHOW, what.to_string());
    };
    match links.as_slice() {
        [] => {
            let _ = app.emit(
                crate::bridge::EV_TOAST,
                serde_json::json!({ "text": "Copy a link first, then press the shortcut.", "bad": true }),
            );
            show(serde_json::json!({}));
        }
        [one] => match crate::grabber::kind_of(one) {
            "torrent" => show(serde_json::json!({ "torrent": one })),
            "video" => {
                let _ = crate::newdl::ask_video(app, one, None);
            }
            _ => {
                let asked = crate::newdl::Asked {
                    url: one.clone(),
                    name: None,
                    size: None,
                    mime: None,
                    referer: None,
                    ctx: Default::default(),
                    at: std::time::Instant::now(),
                };
                if crate::newdl::ask(app, asked).is_err() {
                    show(serde_json::json!({ "grab": one }));
                }
            }
        },
        many => show(serde_json::json!({ "grab": many.join("\n") })),
    }
}
