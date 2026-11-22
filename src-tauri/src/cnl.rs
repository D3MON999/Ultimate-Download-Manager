//! Click'n'Load: link sites' "CNL" buttons send their links to a download manager listening
//! on this computer at 127.0.0.1:9666. The links,
//! plain (`/flash/add`) or AES-encrypted with a key the page gives (`/flash/addcrypted2`), open
//! in the link grabber to choose from: nothing starts by itself. Only this computer can reach the
//! port (loopback), and a request must come from a web page's form (it carries no secrets).

use aes::cipher::{BlockCipherDecrypt, KeyInit};
use axum::extract::{Form, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use base64::Engine as _;
use serde::Deserialize;
use tauri::{AppHandle, Emitter};

pub const PORT: u16 = 9666;

/// Listen until the app ends; a port already taken is left alone.
pub async fn run(app: AppHandle) {
    let router = Router::new()
        .route("/", get(|| async { "JDownloader" }))
        .route("/jdcheck.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], "jdownloader=true; var version='18507';") }))
        .route(
            "/crossdomain.xml",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/xml")],
                    "<?xml version=\"1.0\"?><cross-domain-policy><allow-access-from domain=\"*\" /></cross-domain-policy>",
                )
            }),
        )
        .route("/flash/", get(|| async { "JDownloader" }))
        .route("/flash/add", post(add))
        .route("/flash/addcrypted2", post(add_crypted))
        .with_state(app);
    let Ok(listener) = tokio::net::TcpListener::bind(("127.0.0.1", PORT)).await else {
        eprintln!("click'n'load: port {PORT} is taken (another download manager?)");
        return;
    };
    let _ = axum::serve(listener, router).await;
}

#[derive(Deserialize)]
struct Plain {
    #[serde(default)]
    urls: String,
    #[serde(default)]
    package: String,
}

#[derive(Deserialize)]
struct Crypted {
    #[serde(default)]
    jk: String,
    #[serde(default)]
    crypted: String,
    #[serde(default)]
    package: String,
}

async fn add(State(app): State<AppHandle>, headers: HeaderMap, Form(f): Form<Plain>) -> impl IntoResponse {
    hand_over(&app, &headers, &f.urls, &f.package)
}

async fn add_crypted(State(app): State<AppHandle>, headers: HeaderMap, Form(f): Form<Crypted>) -> impl IntoResponse {
    match decrypt(&f.jk, &f.crypted) {
        Some(text) => hand_over(&app, &headers, &text, &f.package),
        None => (StatusCode::BAD_REQUEST, "failed\r\n").into_response(),
    }
}

fn hand_over(app: &AppHandle, headers: &HeaderMap, text: &str, package: &str) -> axum::response::Response {
    let links = crate::grabber::links_in(text);
    if links.is_empty() {
        return (StatusCode::BAD_REQUEST, "failed\r\n").into_response();
    }
    let from = headers.get(header::REFERER).and_then(|v| v.to_str().ok()).unwrap_or("a web page");
    if let Some(w) = tauri::Manager::get_webview_window(app, "main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    let _ = app.emit(
        crate::bridge::EV_SHOW,
        serde_json::json!({ "grab": links.join("\n"), "package": package, "from": from }).to_string(),
    );
    "success\r\n".into_response()
}

/// The key a page gives: a JavaScript function returning it as hex (`function f(){ return
/// '31323334…'; }`), or the hex itself. A full browser would run the script; every CNL page seen returns
/// a constant, so the constant is read and nothing is run.
fn key_of(jk: &str) -> Option<Vec<u8>> {
    let hex_run = |s: &str| -> Option<String> {
        let mut best = String::new();
        let mut cur = String::new();
        for c in s.chars() {
            if c.is_ascii_hexdigit() {
                cur.push(c);
            } else {
                if cur.len() > best.len() {
                    best = std::mem::take(&mut cur);
                }
                cur.clear();
            }
        }
        if cur.len() > best.len() {
            best = cur;
        }
        (best.len() >= 32).then_some(best)
    };
    let text = match jk.find("return") {
        Some(i) => &jk[i + 6..],
        None => jk,
    };
    // What `return` gives: string pieces joined (`'3132' + '3334'`), or a variable set to one
    // (`var k = '…'; return k;`); else the longest hex run.
    let expr = text.split([';', '}']).next().unwrap_or("").trim();
    let pieces = quoted(expr);
    let joined: String = if !pieces.is_empty() {
        pieces.concat()
    } else if !expr.is_empty() && expr.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
        let at = jk.find(&format!("{expr} =")).or_else(|| jk.find(&format!("{expr}=")));
        at.map(|i| quoted(jk[i..].split(';').next().unwrap_or("")).concat()).unwrap_or_default()
    } else {
        String::new()
    };
    let hex = if joined.len() >= 32 && joined.chars().all(|c| c.is_ascii_hexdigit()) { joined } else { hex_run(text)? };
    let key = hex::decode(&hex[..32]).ok()?;
    Some(key)
}

/// The string literals in `s` ('…' or "…"), in order.
fn quoted(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\'' || c == '"' {
            let mut lit = String::new();
            for d in chars.by_ref() {
                if d == c {
                    break;
                }
                lit.push(d);
            }
            out.push(lit);
        }
    }
    out
}

/// `crypted` (base64) decrypted with AES-128-CBC, the key as the IV too (the protocol's way),
/// padding with zeros trimmed.
pub fn decrypt(jk: &str, crypted: &str) -> Option<String> {
    let key = key_of(jk)?;
    let data = base64::engine::general_purpose::STANDARD
        .decode(crypted.trim().replace([' ', '\n', '\r'], "").replace('-', "+").replace('_', "/"))
        .ok()?;
    if data.is_empty() || data.len() % 16 != 0 {
        return None;
    }
    let cipher = aes::Aes128::new_from_slice(&key).ok()?;
    let mut prev = [0u8; 16];
    prev.copy_from_slice(&key);
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let mut block = aes::Block::default();
        block.copy_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        out.extend(block.iter().zip(prev.iter()).map(|(a, b)| a ^ b));
        prev.copy_from_slice(chunk);
    }
    let text = String::from_utf8_lossy(&out).trim_end_matches(['\0', ' ', '\n', '\r']).to_owned();
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::BlockCipherEncrypt;

    fn encrypt(key: &[u8; 16], text: &str) -> String {
        let cipher = aes::Aes128::new_from_slice(key).unwrap();
        let mut data = text.as_bytes().to_vec();
        while data.len() % 16 != 0 {
            data.push(0);
        }
        let mut prev = *key;
        let mut out = Vec::new();
        for chunk in data.chunks(16) {
            let mut block = aes::Block::default();
            for (i, b) in chunk.iter().enumerate() {
                block[i] = b ^ prev[i];
            }
            cipher.encrypt_block(&mut block);
            prev.copy_from_slice(&block);
            out.extend_from_slice(&block);
        }
        base64::engine::general_purpose::STANDARD.encode(out)
    }

    #[test]
    fn crypted_links_open() {
        let key = *b"1234567890abcdef";
        let links = "https://a.test/file1.rar\r\nhttps://a.test/file2.rar";
        let crypted = encrypt(&key, links);
        let jk = format!("function f(){{ return '{}';}}", hex::encode(key));
        assert_eq!(decrypt(&jk, &crypted).as_deref(), Some(links));
        // The hex alone works too; a wrong key gives garbage, not the links.
        assert_eq!(decrypt(&hex::encode(key), &crypted).as_deref(), Some(links));
        assert_ne!(decrypt(&hex::encode(*b"0000000000000000"), &crypted).as_deref(), Some(links));
        assert_eq!(decrypt("function f(){ return 'zz'; }", &crypted), None);
        // Built from pieces, or kept in a variable.
        let hx = hex::encode(key);
        let joined = format!("function f(){{ return '{}' + \"{}\"; }}", &hx[..10], &hx[10..]);
        assert_eq!(decrypt(&joined, &crypted).as_deref(), Some(links));
        let var = format!("function f(){{ var k = '{hx}'; return k; }}");
        assert_eq!(decrypt(&var, &crypted).as_deref(), Some(links));
    }
}
