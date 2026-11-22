//! Site sessions: the cookies and User-Agent a person's browser check (or sign-in) left in the
//! site-check window, kept per host so every connection and every queued file from that host
//! reuses them. Stored in `<data>/sessions.bin`, encrypted for the current Windows user (DPAPI).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    /// `name=value; name2=value2`, as a Cookie header.
    pub cookies: String,
    pub user_agent: String,
    /// When it was captured (seconds since 1970).
    pub at: i64,
    /// The earliest expiry among its cookies, if any said one.
    pub expires: Option<i64>,
}

impl Session {
    pub fn fresh(&self, now: i64) -> bool {
        // Without an expiry, a day: clearance cookies usually last minutes to hours.
        self.expires.map_or(now - self.at < 24 * 3600, |e| e > now)
    }
}

pub struct Vault {
    path: PathBuf,
    map: Mutex<HashMap<String, Session>>,
}

impl Vault {
    pub fn open(data_dir: &Path) -> Vault {
        let path = data_dir.join("sessions.bin");
        let map = std::fs::read(&path)
            .ok()
            .and_then(|b| dpapi::unprotect(&b))
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Vault { path, map: Mutex::new(map) }
    }

    /// The session for `host` (or a parent domain's: `cdn.example.com` uses `example.com`'s).
    pub fn for_host(&self, host: &str) -> Option<Session> {
        let now = crate::store::now();
        let map = self.map.lock().unwrap();
        let mut h = host;
        loop {
            if let Some(s) = map.get(h).filter(|s| s.fresh(now)) {
                return Some(s.clone());
            }
            match h.split_once('.') {
                Some((_, rest)) if rest.contains('.') => h = rest,
                _ => return None,
            }
        }
    }

    pub fn put(&self, host: &str, s: Session) {
        let mut map = self.map.lock().unwrap();
        map.insert(host.to_ascii_lowercase(), s);
        let now = crate::store::now();
        map.retain(|_, s| s.fresh(now));
        if let Ok(json) = serde_json::to_vec(&*map)
            && let Some(sealed) = dpapi::protect(&json)
        {
            let tmp = self.path.with_extension("bin.tmp");
            if std::fs::write(&tmp, sealed).is_ok() {
                let _ = std::fs::rename(&tmp, &self.path);
            }
        }
    }
}

/// Windows' Data Protection API: data only this Windows user can read back.
#[cfg(windows)]
mod dpapi {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };

    fn call(data: &[u8], protect: bool) -> Option<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: `input` points at `data`, alive for the call; `out` is allocated by the
        // system with LocalAlloc, copied out, then freed with LocalFree.
        let ok = unsafe {
            if protect {
                CryptProtectData(
                    &input,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut out,
                )
            } else {
                CryptUnprotectData(
                    &input,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut out,
                )
            }
        };
        if ok == 0 || out.pbData.is_null() {
            return None;
        }
        // SAFETY: the system wrote `cbData` bytes at `pbData`.
        let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        // SAFETY: allocated by the call above with LocalAlloc.
        unsafe { LocalFree(out.pbData as _) };
        Some(bytes)
    }

    pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
        call(data, true)
    }

    pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
        call(data, false)
    }
}

#[cfg(not(windows))]
mod dpapi {
    pub fn protect(_: &[u8]) -> Option<Vec<u8>> {
        None
    }
    pub fn unprotect(_: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_round_trip_and_parent_domains() {
        let dir = std::env::temp_dir().join(format!("udm-vault-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let v = Vault::open(&dir);
        let now = crate::store::now();
        v.put(
            "example.com",
            Session { cookies: "cf_clearance=x".into(), user_agent: "UA".into(), at: now, expires: Some(now + 60) },
        );
        let again = Vault::open(&dir);
        assert_eq!(again.for_host("cdn.example.com").unwrap().cookies, "cf_clearance=x");
        assert!(again.for_host("other.com").is_none());
        // Encrypted on disk: the cookie isn't readable in the file.
        let raw = std::fs::read(dir.join("sessions.bin")).unwrap();
        assert!(!raw.windows(12).any(|w| w == b"cf_clearance"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
