//! Start with Windows: the current user's Run key (no admin rights), starting quietly in the
//! notification area (`--background`).

const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "UltimateDownloadManager";

/// Turn it on or off to match the setting (called at start and when settings are saved).
pub fn apply(on: bool) {
    // Portable mode writes nothing to the registry.
    if on && udm_ipc::portable_dir().is_some() {
        return;
    }
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("reg");
        if on {
            let Ok(exe) = std::env::current_exe() else { return };
            let data = format!("\"{}\" --background", exe.display());
            cmd.args(["add", KEY, "/v", VALUE, "/t", "REG_SZ", "/d", &data, "/f"]);
        } else {
            if !enabled() {
                return;
            }
            cmd.args(["delete", KEY, "/v", VALUE, "/f"]);
        }
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
        let _ = cmd.output();
    }
    #[cfg(not(windows))]
    let _ = on;
}

/// Whether the Run entry is there now.
pub fn enabled() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("reg")
            .args(["query", KEY, "/v", VALUE])
            .creation_flags(0x0800_0000)
            .output()
            .is_ok_and(|o| o.status.success())
    }
    #[cfg(not(windows))]
    false
}
