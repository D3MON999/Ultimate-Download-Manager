//! Debrid accounts (Real-Debrid, AllDebrid, Premiumize), through the reused `udm-debrid`
//! crate and the services' official APIs: a file host's link (1fichier, Rapidgator…) becomes a
//! direct link the engine downloads with every connection. The list of accounts is in
//! `<data>/accounts.json` (no secrets); each key is in Windows Credential Manager.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use udm_debrid::{Account, Credential, Debrid, Http, ServiceKind, Services};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stored {
    pub id: u64,
    pub service: ServiceKind,
    /// The service's user name, as it said when the key was checked.
    pub user: String,
}

#[derive(Serialize)]
pub struct AccountView {
    pub id: u64,
    pub service: String,
    pub user: String,
    pub key_page: String,
}

pub struct Accounts {
    path: PathBuf,
    list: Mutex<Vec<Stored>>,
    pub debrid: Debrid,
    http: Mutex<Option<Arc<Http>>>,
}

fn target(id: u64) -> String {
    format!("UltimateDownloadManager/debrid/{id}")
}

fn http() -> Result<Arc<Http>, String> {
    let net = udm_engine::Options::default();
    Http::new(&net, Services::default()).map(Arc::new)
}

pub fn service_named(name: &str) -> Option<ServiceKind> {
    ServiceKind::ALL.into_iter().find(|s| s.name().eq_ignore_ascii_case(name))
}

impl Accounts {
    pub fn open(data_dir: &Path) -> Accounts {
        let path = data_dir.join("accounts.json");
        let list: Vec<Stored> = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let a = Accounts { path, list: Mutex::new(list), debrid: Debrid::default(), http: Mutex::new(None) };
        a.apply();
        a
    }

    fn http(&self) -> Result<Arc<Http>, String> {
        let mut h = self.http.lock().unwrap();
        if h.is_none() {
            *h = Some(http()?);
        }
        Ok(h.clone().expect("set above"))
    }

    /// Hand the accounts (with their keys from Credential Manager) to the debrid handle.
    fn apply(&self) {
        let Ok(http) = self.http() else { return };
        let accounts: Vec<Account> = self
            .list
            .lock()
            .unwrap()
            .iter()
            .filter_map(|s| {
                let secret = crate::creds::get(&target(s.id))?;
                let credential = serde_json::from_str(&secret).ok()?;
                Some(Account { id: s.id, service: s.service, credential })
            })
            .collect();
        self.debrid.set_accounts(accounts, http);
    }

    /// Learn which file hosts each account's service takes (in the background at start).
    pub async fn refresh(&self) {
        let _ = self.debrid.refresh_hosts().await;
    }

    pub fn list(&self) -> Vec<AccountView> {
        self.list
            .lock()
            .unwrap()
            .iter()
            .map(|s| AccountView {
                id: s.id,
                service: s.service.name().into(),
                user: s.user.clone(),
                key_page: s.service.token_page().into(),
            })
            .collect()
    }

    /// Check `key` with the service, then keep it (the key in Credential Manager).
    pub async fn add(&self, service: ServiceKind, key: &str) -> Result<AccountView, String> {
        let key = key.trim();
        if key.is_empty() {
            return Err("Paste the API key from the service's page.".into());
        }
        let credential = Credential::Token { token: key.to_owned() };
        let http = self.http()?;
        let info = udm_debrid::check(&http, service, credential.clone())
            .await
            .map_err(|f| match f {
                udm_debrid::Fault::Service(udm_engine::ServiceFault::SignIn) => {
                    format!("{} didn't accept this key. Copy it again from its page.", service.name())
                }
                other => format!("{} couldn't be reached to check the key ({other:?})", service.name()),
            })?;
        let id = crate::store::now() as u64 * 1000 + (std::process::id() as u64 % 1000);
        crate::creds::put(&target(id), &serde_json::to_string(&credential).map_err(|e| e.to_string())?)?;
        let stored = Stored { id, service, user: info.user.clone() };
        {
            let mut list = self.list.lock().unwrap();
            list.push(stored);
            self.save(&list)?;
        }
        self.apply();
        self.refresh().await;
        Ok(AccountView {
            id,
            service: service.name().into(),
            user: if info.premium { info.user } else { format!("{} (not premium)", info.user) },
            key_page: service.token_page().into(),
        })
    }

    pub fn remove(&self, id: u64) -> Result<(), String> {
        crate::creds::delete(&target(id));
        let mut list = self.list.lock().unwrap();
        list.retain(|s| s.id != id);
        self.save(&list)?;
        drop(list);
        self.apply();
        Ok(())
    }

    fn save(&self, list: &[Stored]) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, json).map_err(|e| e.to_string())
    }
}

/// Cloud storage share links turned into the file's own address: Dropbox (`dl=0` → `dl=1`),
/// SharePoint and OneDrive for work (`download=1`). Google Drive's are the engine's own.
pub fn direct_cloud_link(url: &str) -> String {
    let Ok(mut u) = url::Url::parse(url) else {
        return url.to_owned();
    };
    let host = u.host_str().unwrap_or_default().to_ascii_lowercase();
    let set = |u: &mut url::Url, key: &str, value: &str| {
        let pairs: Vec<(String, String)> = u
            .query_pairs()
            .filter(|(k, _)| k != key)
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        u.query_pairs_mut().clear().extend_pairs(pairs).append_pair(key, value);
    };
    if host == "dropbox.com" || host.ends_with(".dropbox.com") {
        set(&mut u, "dl", "1");
    } else if host.ends_with(".sharepoint.com") && u.path().contains("/:") {
        set(&mut u, "download", "1");
    } else {
        return url.to_owned();
    }
    u.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real-Debrid's own API turns down a made-up key (network: run with --ignored).
    #[tokio::test]
    #[ignore = "asks Real-Debrid's API over the network"]
    async fn a_made_up_key_is_refused_by_the_service() {
        let http = http().unwrap();
        let got = udm_debrid::check(&http, ServiceKind::RealDebrid, Credential::Token { token: "UDM-TEST-NOT-A-KEY".into() }).await;
        assert!(got.is_err(), "{got:?}");
        println!("refused as expected: {got:?}");
    }

    #[test]
    fn cloud_links_become_direct() {
        assert_eq!(
            direct_cloud_link("https://www.dropbox.com/scl/fi/abc/file.zip?rlkey=x&dl=0"),
            "https://www.dropbox.com/scl/fi/abc/file.zip?rlkey=x&dl=1"
        );
        assert!(direct_cloud_link("https://contoso.sharepoint.com/:u:/g/abc").ends_with("download=1"));
        assert_eq!(direct_cloud_link("https://example.com/a.zip"), "https://example.com/a.zip");
        assert_eq!(service_named("real-debrid"), Some(ServiceKind::RealDebrid));
    }
}
