//! Crash reports. A second copy of the program, started with `--crash-monitor`, waits beside the
//! app; when the app crashes or panics it writes a minidump and a readable report into
//! `<data>/crashes`. The next start says a report is there and the folder opens from Settings.
//!
//! The crash handler is the one place the app needs `unsafe`: it runs inside a crashing process.
#![allow(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use udm_crash::client::Reporter;
use udm_crash::crash_handler::{CrashEventResult, CrashHandler, make_crash_event};
use udm_crash::monitor::{self, Event};
use udm_crash::protocol::{Panic, Session};

static REPORTER: OnceLock<Arc<Reporter>> = OnceLock::new();

/// When this process was started as the monitor: watch the app until it ends, writing a report
/// for each dump, and say so (true) so `main` stops there.
pub fn monitor_if_asked() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let Some(m) = udm_crash::MonitorArgs::parse(&args) else {
        return false;
    };
    let _ = udm_crash::logs::init(&m.data, udm_crash::logs::MONITOR);
    let (tx, rx) = std::sync::mpsc::channel();
    let (socket, data) = (m.socket.clone(), m.data.clone());
    let failed = tx.clone();
    std::thread::spawn(move || {
        if let Err(e) = monitor::serve(&socket, &data, tx) {
            let _ = failed.send(Event::ServeFailed(e));
        }
    });
    while let Ok(e) = rx.recv() {
        match e {
            Event::Dump { path, kind, notes } => {
                udm_crash::report::make(Some(&path), kind, notes, &m.data, &[]);
            }
            Event::DumpFailed { error, .. } => tracing::warn!("crash dump failed: {error}"),
            Event::Recovered => {}
            Event::ServeFailed(e) => {
                tracing::warn!("crash monitor: {e}");
                break;
            }
            Event::Gone { .. } => break,
        }
    }
    true
}

/// Start the monitor and catch crashes and panics from here on. Without a monitor (it didn't
/// start) the app runs on without reports.
pub fn start(data: &Path) {
    let Ok(exe) = std::env::current_exe() else { return };
    let Some(rep) = Reporter::start(&exe, data) else { return };
    let rep = Arc::new(rep);
    rep.session(&Session {
        version: env!("CARGO_PKG_VERSION").into(),
        install: "The installer (setup.exe)".into(),
        os: "Windows".into(),
        arch: std::env::consts::ARCH.into(),
        language: "English".into(),
        pid: std::process::id(),
        exe: exe.display().to_string(),
        ..Default::default()
    });
    let r = rep.clone();
    // SAFETY: the closure only asks the monitor (another process) for a dump; it allocates
    // nothing of its own, as a handler running inside a crashed process must.
    let Ok(handler) = CrashHandler::attach(unsafe { make_crash_event(move |cc| CrashEventResult::Handled(r.dump(cc))) })
    else {
        return;
    };
    // Lives as long as the process.
    let handler: &'static CrashHandler = Box::leak(Box::new(handler));
    let r = rep.clone();
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        r.panic(&Panic {
            message: info.payload_as_str().unwrap_or("").to_owned(),
            location: info.location().map(|l| l.to_string()).unwrap_or_default(),
            thread: std::thread::current().name().unwrap_or("?").to_owned(),
            backtrace: std::backtrace::Backtrace::force_capture().to_string(),
        });
        handler.simulate_exception(None);
        before(info);
    }));
    let _ = REPORTER.set(rep);
}

/// The app is closing normally: the monitor ends without a report.
pub fn clean_exit() {
    if let Some(r) = REPORTER.get() {
        r.clean();
    }
}

/// The folder reports are written to.
pub fn folder(data: &Path) -> PathBuf {
    monitor::crashes_dir(data)
}

/// Reports written since the last start (the newest first), and the mark moved past them.
pub fn unseen(data: &Path) -> Vec<PathBuf> {
    let dir = folder(data);
    let mark = dir.join(".seen");
    let since = std::fs::metadata(&mark).and_then(|m| m.modified()).ok();
    let mut new: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .filter_map(|p| std::fs::metadata(&p).and_then(|m| m.modified()).ok().map(|t| (t, p)))
        .filter(|(t, _)| since.is_none_or(|s| *t > s))
        .collect();
    new.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
    if !new.is_empty() || since.is_none() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&mark, b"");
    }
    new.into_iter().map(|(_, p)| p).collect()
}
