//! Diagnostic: download one address and print the engine's event log and progress.
//! `cargo run -p udm-engine --example get -- <url> <folder>`

use std::time::Duration;

use udm_engine::{Destination, Download, Options, Request};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let url = url::Url::parse(&args.next().expect("url")).expect("a valid url");
    let dir = std::path::PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
    let dl = Download::start(
        Request { url, headers: Default::default(), dest: Destination::Dir(dir) },
        Options {
            user_agent: std::env::var("UDM_UA").unwrap_or_else(|_| "UltimateDownloadManager/diag".into()),
            ..Options::default()
        },
    );
    let mut seen = 0;
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let s = dl.snapshot();
        let from = seen;
        for e in s.events.iter().filter(|e| e.seq >= from) {
            println!("event: {}", e.text);
            seen = e.seq + 1;
        }
        println!("{:?} {}/{:?} conns={}", s.state, s.downloaded, s.size, s.connections.len());
        if matches!(
            s.state,
            udm_engine::download::State::Done
                | udm_engine::download::State::Failed
                | udm_engine::download::State::Cancelled
        ) {
            break;
        }
    }
    match dl.wait().await {
        Ok(o) => println!("done: {} ({} bytes)", o.path.display(), o.size),
        Err(e) => println!("error: {e} / {e:?}"),
    }
}
