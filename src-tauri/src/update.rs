//! Ultimate Download Manager updating itself (Tauri's updater): a release in your public releases repository on
//! GitHub (`latest.json` plus the signed installer, made by `tools/release.ps1`). Only an
//! installer signed with your private key (`keys/updater.key`, kept off GitHub) is accepted.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

use crate::manager::Manager;

/// Sent when an update is found at start: `{version, notes}`.
pub const EV_UPDATE: &str = "udm://update";

#[derive(Clone, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
}

fn endpoint(m: &Manager) -> Result<url::Url, String> {
    endpoint_for(&m.settings().update_repo)
}

/// Where `repo`'s newest release keeps `latest.json`. Also takes a pasted GitHub address.
fn endpoint_for(repo: &str) -> Result<url::Url, String> {
    let repo = repo.trim().trim_end_matches('/');
    let repo = repo.strip_prefix("https://github.com/").unwrap_or(repo).trim_matches('/').to_owned();
    if repo.split('/').count() != 2 || repo.contains(' ') {
        return Err("Set the releases repository (yourname/ultimate-download-manager-releases) in Settings → Updates.".into());
    }
    format!("https://github.com/{repo}/releases/latest/download/latest.json")
        .parse()
        .map_err(|e: url::ParseError| e.to_string())
}

async fn find(app: &AppHandle, m: &Manager) -> Result<Option<tauri_plugin_updater::Update>, String> {
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint(m)?])
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;
    updater.check().await.map_err(|e| e.to_string())
}

pub async fn check(app: &AppHandle, m: &Manager) -> Result<Option<UpdateInfo>, String> {
    Ok(find(app, m).await?.map(|u| UpdateInfo { version: u.version.clone(), notes: u.body.clone() }))
}

/// Download, check its signature, install (the installer runs on its own) and restart.
pub async fn install(app: &AppHandle, m: &Manager) -> Result<(), String> {
    let update = find(app, m).await?.ok_or("Ultimate Download Manager is up to date.")?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

/// At start (when chosen): look once, a little after the window is up.
pub fn check_at_start(app: AppHandle, m: Arc<Manager>) {
    if !m.settings().check_updates || m.settings().update_repo.trim().is_empty() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        if let Ok(Some(info)) = check(&app, &m).await {
            let _ = app.emit(EV_UPDATE, info);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::endpoint_for;

    #[test]
    fn release_feeds() {
        let want = "https://github.com/me/ultimate-download-manager-releases/releases/latest/download/latest.json";
        assert_eq!(endpoint_for("me/ultimate-download-manager-releases").unwrap().as_str(), want);
        assert_eq!(endpoint_for(" https://github.com/me/ultimate-download-manager-releases/ ").unwrap().as_str(), want);
        assert!(endpoint_for("").is_err());
        assert!(endpoint_for("just-a-name").is_err());
        assert!(endpoint_for("a/b/c").is_err());
    }
}
