//! Windows integration, for the current user only (HKCU, no admin rights; nothing in portable
//! mode): magnet links and .torrent files open in Ultimate Download Manager (Settings → App), and
//! text files get "Download links with Ultimate Download Manager" in their right-click menu.

#[cfg(windows)]
fn reg(args: &[&str]) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("reg").args(args).creation_flags(0x0800_0000).output();
}

const MAGNET: &str = r"HKCU\Software\Classes\magnet";
const PROGID: &str = r"HKCU\Software\Classes\UltimateDownloadManager.torrent";
const DOT_TORRENT: &str = r"HKCU\Software\Classes\.torrent";
const TXT_MENU: &str = r"HKCU\Software\Classes\SystemFileAssociations\.txt\shell\UltimateDownloadManager";

/// Match the registry to the settings (called at start and when settings are saved).
pub fn apply(torrents: bool, txt_menu: bool) {
    #[cfg(windows)]
    {
        if udm_ipc::portable_dir().is_some() {
            return;
        }
        let Ok(exe) = std::env::current_exe() else { return };
        let exe = exe.display().to_string();
        let open = format!("\"{exe}\" \"%1\"");
        let icon = format!("\"{exe}\",0");
        if torrents {
            reg(&["add", MAGNET, "/ve", "/d", "URL:Magnet link", "/f"]);
            reg(&["add", MAGNET, "/v", "URL Protocol", "/d", "", "/f"]);
            reg(&["add", &format!(r"{MAGNET}\DefaultIcon"), "/ve", "/d", &icon, "/f"]);
            reg(&["add", &format!(r"{MAGNET}\shell\open\command"), "/ve", "/d", &open, "/f"]);
            reg(&["add", PROGID, "/ve", "/d", "Torrent file", "/f"]);
            reg(&["add", &format!(r"{PROGID}\DefaultIcon"), "/ve", "/d", &icon, "/f"]);
            reg(&["add", &format!(r"{PROGID}\shell\open\command"), "/ve", "/d", &open, "/f"]);
            reg(&["add", DOT_TORRENT, "/ve", "/d", "UltimateDownloadManager.torrent", "/f"]);
            reg(&["add", &format!(r"{DOT_TORRENT}\OpenWithProgids"), "/v", "UltimateDownloadManager.torrent", "/d", "", "/f"]);
        } else {
            // Only what is ours: another program's magnet handler is left alone.
            if owned(MAGNET, &exe) {
                reg(&["delete", MAGNET, "/f"]);
            }
            reg(&["delete", PROGID, "/f"]);
        }
        if txt_menu {
            reg(&["add", TXT_MENU, "/ve", "/d", "Download links with Ultimate Download Manager", "/f"]);
            reg(&["add", TXT_MENU, "/v", "Icon", "/d", &icon, "/f"]);
            reg(&["add", &format!(r"{TXT_MENU}\command"), "/ve", "/d", &format!("\"{exe}\" --grab-file \"%1\""), "/f"]);
        } else {
            reg(&["delete", TXT_MENU, "/f"]);
        }
    }
    #[cfg(not(windows))]
    let _ = (torrents, txt_menu);
}

/// The key's open command names this program.
#[cfg(windows)]
fn owned(key: &str, exe: &str) -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("reg")
        .args(["query", &format!(r"{key}\shell\open\command"), "/ve"])
        .creation_flags(0x0800_0000)
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).to_ascii_lowercase().contains(&exe.to_ascii_lowercase()))
}
