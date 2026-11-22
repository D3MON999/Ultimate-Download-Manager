//! Real gallery-dl releases against the lab server, as Ultimate Download Manager starts them (#236: a distribution's
//! gallery-dl may be older than Ultimate Download Manager's own copy). Each release Ultimate Download Manager can drive is chosen, reads an
//! address (and says when it can't), and saves a picture with Ultimate Download Manager's User-Agent; one older than
//! `gallery::OLDEST` is refused when chosen.
//!
//! Ignored: it needs `UDM_TEST_GALLERYDLS`, the programs separated as in PATH (`:` or `;`):
//!
//!   UDM_TEST_GALLERYDLS=$HOME/gdl/1.20.0/bin/gallery-dl:/usr/bin/gallery-dl \
//!     cargo test -p udm-tools --test gallery_dl -- --ignored --nocapture

use std::time::{Duration, Instant};

use udm_labserver::{Config, LabServer};
use udm_tools::chosen::Helper;
use udm_tools::gallery::{Ask, GalleryJob, USER_AGENT};
use udm_tools::video::State;

#[test]
#[ignore]
fn every_release_mdm_can_drive_saves_a_gallery() {
    let list = std::env::var_os("UDM_TEST_GALLERYDLS").expect("set UDM_TEST_GALLERYDLS");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let lab = rt.block_on(LabServer::start(Config::default())).unwrap();
    let mut rows = Vec::new();
    for exe in std::env::split_paths(&list) {
        let asked = udm_tools::chosen::ask(Helper::GalleryDl, &exe)
            .unwrap_or_else(|e| panic!("{}: {e}", exe.display()));
        let v = asked.version.clone();
        let data = tempfile::tempdir().unwrap();
        let chosen = udm_tools::chosen::choose(data.path(), Helper::GalleryDl, &exe);
        if udm_tools::gallery::too_old(&v) {
            assert!(
                matches!(chosen, Err(udm_tools::Error::TooOld { .. })),
                "{v}: {chosen:?}"
            );
            assert_eq!(udm_tools::gallery::installed(data.path()), None);
            rows.push(format!("{v}: too old, refused when chosen"));
            continue;
        }
        let tool = chosen.unwrap_or_else(|e| panic!("{v}: {e}"));
        assert_eq!(
            udm_tools::gallery::installed(data.path()),
            Some(tool.clone())
        );
        let picture = lab.url(&format!("/gen/64K/picture-{}.jpg", v.replace('.', "-")));
        assert!(
            udm_tools::gallery::handles(&tool, &picture).unwrap_or_else(|e| panic!("{v}: {e}")),
            "{v} reads a picture's address"
        );
        assert!(
            !udm_tools::gallery::handles(&tool, &lab.url("/page"))
                .unwrap_or_else(|e| panic!("{v}: {e}")),
            "{v} says a page isn't one of its"
        );
        lab.clear_log();
        let folder = data.path().join("gallery");
        let job = GalleryJob::start(
            &tool,
            &picture,
            &folder,
            &Ask::default(),
            &Default::default(),
        )
        .unwrap_or_else(|e| panic!("{v}: {e}"));
        let deadline = Instant::now() + Duration::from_secs(90);
        let p = loop {
            let p = job.progress();
            if p.state != State::Running {
                break p;
            }
            assert!(Instant::now() < deadline, "{v}: still running");
            std::thread::sleep(Duration::from_millis(200));
        };
        assert!(
            matches!(&p.state, State::Done(f) if *f == folder),
            "{v}: {:?} {:?}",
            p.state,
            p.events
        );
        let saved: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(saved.len(), 1, "{v}: {saved:?}");
        assert_eq!(saved[0].metadata().unwrap().len(), 64 * 1024, "{v}");
        let log = lab.log();
        let get = log
            .iter()
            .find(|e| e.path.starts_with("/gen/64K/"))
            .unwrap_or_else(|| panic!("{v}: no request"));
        assert_eq!(get.user_agent.as_deref(), Some(USER_AGENT), "{v}");
        rows.push(format!(
            "{v}: chosen, reads and refuses addresses, saved {} with Ultimate Download Manager's User-Agent",
            saved[0].file_name().to_string_lossy()
        ));
    }
    for r in rows {
        println!("{r}");
    }
}
