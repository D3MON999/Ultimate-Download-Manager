//! Time and network: the download hours (downloads start only inside them, and wait for the
//! next window outside), what to do when the queue is done (sleep, shut down), a slower speed
//! during chosen hours, and the network adapter every connection is held to (a VPN's: when it
//! drops, downloads stop rather than going around it).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::{Datelike, Local, Timelike};
use serde::Serialize;

use crate::manager::Manager;
use crate::settings::Settings;

/// Minutes since midnight for "HH:MM" (None for anything else).
fn minutes(hhmm: &str) -> Option<u32> {
    let (h, m) = hhmm.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// `now` is in [from, to), the span wrapping past midnight when `to` is earlier.
fn within(now: u32, from: u32, to: u32) -> bool {
    if from == to {
        true
    } else if from < to {
        now >= from && now < to
    } else {
        now >= from || now < to
    }
}

/// Whether downloads may run now, by the download hours.
pub fn in_window(s: &Settings) -> bool {
    if !s.schedule_enabled {
        return true;
    }
    let now = Local::now();
    let day = now.weekday().num_days_from_monday() as usize;
    if !s.schedule_days.get(day).copied().unwrap_or(true) {
        return false;
    }
    match (minutes(&s.schedule_start), minutes(&s.schedule_stop)) {
        (Some(a), Some(b)) => within(now.hour() * 60 + now.minute(), a, b),
        _ => true,
    }
}

/// Whether queue `q` may start downloads now: not stopped, and inside its hours if it has any.
pub fn queue_open(q: &crate::settings::Queue) -> bool {
    if q.paused {
        return false;
    }
    let now = Local::now();
    match (minutes(&q.from), minutes(&q.to)) {
        (Some(a), Some(b)) => within(now.hour() * 60 + now.minute(), a, b),
        _ => true,
    }
}

/// The speed limit that applies now (bytes/s), with the slow hours.
pub fn limit_now(s: &Settings) -> Option<u64> {
    let base = (s.global_limit_kib > 0).then(|| s.global_limit_kib * 1024);
    if !s.slow_enabled || s.slow_limit_kib == 0 {
        return base;
    }
    let now = Local::now();
    match (minutes(&s.slow_from), minutes(&s.slow_to)) {
        (Some(a), Some(b)) if within(now.hour() * 60 + now.minute(), a, b) => {
            Some(base.map_or(s.slow_limit_kib * 1024, |b| b.min(s.slow_limit_kib * 1024)))
        }
        _ => base,
    }
}

/// The route connections take: the chosen adapter, or any.
pub fn route(s: &Settings) -> udm_engine::Route {
    udm_engine::net::route_for(&s.adapter, &udm_engine::net::adapters())
}

#[derive(Serialize)]
pub struct AdapterView {
    pub name: String,
    pub description: Option<String>,
    pub up: bool,
    pub addresses: Vec<String>,
}

pub fn adapters() -> Vec<AdapterView> {
    let described = descriptions();
    udm_engine::net::adapters()
        .into_iter()
        .map(|a| AdapterView {
            up: !(a.v4.is_empty() && a.v6.is_empty()),
            addresses: a.v4.iter().map(|v| v.to_string()).chain(a.v6.iter().map(|v| v.to_string())).collect(),
            description: described.get(&a.name).cloned(),
            name: a.name,
        })
        .collect()
}

/// Windows' descriptions of its adapters (`Local Area Connection 2` is "OpenVPN Data Channel
/// Offload for ExpressVPN"): the names alone don't tell a VPN apart.
fn descriptions() -> std::collections::HashMap<String, String> {
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Get-NetAdapter -IncludeHidden | Select-Object Name,InterfaceDescription | ConvertTo-Json -Compress",
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Row {
        name: Option<String>,
        interface_description: Option<String>,
    }
    let Ok(out) = cmd.output() else {
        return Default::default();
    };
    let rows: Vec<Row> = serde_json::from_slice::<Vec<Row>>(&out.stdout)
        .or_else(|_| serde_json::from_slice::<Row>(&out.stdout).map(|r| vec![r]))
        .unwrap_or_default();
    rows.into_iter()
        .filter_map(|r| Some((r.name?, r.interface_description?)))
        .collect()
}

/// Keep the hours, the speed and the route in force, every 20 seconds.
pub async fn run(m: Arc<Manager>) {
    let mut every = tokio::time::interval(Duration::from_secs(20));
    let worked = AtomicBool::new(false);
    let mut acted = false;
    loop {
        every.tick().await;
        let s = m.settings();
        m.set_limit(limit_now(&s));
        udm_tools::proc::set_route(route(&s));
        let open = in_window(&s);
        let was_held = m.held();
        m.set_held(!open);
        if open && was_held {
            m.pump();
        }
        if !open && !was_held {
            m.hold_running();
        }
        // The queue done inside the window: what was asked for, once per window.
        let (running, queued) = m.activity();
        if running + queued > 0 {
            worked.store(true, Ordering::Relaxed);
            acted = false;
        }
        if open && s.schedule_enabled && running + queued == 0 && worked.load(Ordering::Relaxed) && !acted {
            acted = true;
            worked.store(false, Ordering::Relaxed);
            when_done(&m, &s.when_done);
        }
    }
}

fn when_done(m: &Arc<Manager>, what: &str) {
    let mut cmd = match what {
        "sleep" => {
            m.notify_pub("Downloads finished", "The computer goes to sleep now.");
            let mut c = std::process::Command::new("rundll32.exe");
            c.args(["powrprof.dll,SetSuspendState", "0,1,0"]);
            c
        }
        "shutdown" => {
            m.notify_pub(
                "Downloads finished",
                "The computer shuts down in 60 seconds. To stop it, run: shutdown /a",
            );
            let mut c = std::process::Command::new("shutdown.exe");
            c.args(["/s", "/t", "60", "/c", "Ultimate Download Manager: downloads finished"]);
            c
        }
        _ => return,
    };
    let _ = cmd.spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_spans() {
        assert_eq!(minutes("02:30"), Some(150));
        assert_eq!(minutes("24:00"), None);
        assert!(within(60, 0, 120));
        assert!(!within(130, 0, 120));
        // 23:00 to 07:00 wraps past midnight.
        assert!(within(23 * 60 + 30, 23 * 60, 7 * 60));
        assert!(within(60, 23 * 60, 7 * 60));
        assert!(!within(12 * 60, 23 * 60, 7 * 60));
    }
}
