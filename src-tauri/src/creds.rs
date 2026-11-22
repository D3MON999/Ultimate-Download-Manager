//! Secrets in Windows Credential Manager (generic credentials, per user): account keys never
//! sit in Ultimate Download Manager's files.

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW, CredWriteW,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn put(target: &str, secret: &str) -> Result<(), String> {
        let target_w = wide(target);
        let user_w = wide("Ultimate Download Manager");
        let blob = secret.as_bytes();
        let cred = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target_w.as_ptr() as *mut u16,
            Comment: std::ptr::null_mut(),
            LastWritten: FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 },
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_ptr() as *mut u8,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: std::ptr::null_mut(),
            UserName: user_w.as_ptr() as *mut u16,
        };
        // SAFETY: every pointer in `cred` points into buffers alive for the call.
        let ok = unsafe { CredWriteW(&cred, 0) };
        if ok == 0 { Err(format!("Windows Credential Manager refused it ({})", std::io::Error::last_os_error())) } else { Ok(()) }
    }

    pub fn get(target: &str) -> Option<String> {
        let target_w = wide(target);
        let mut out: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: `out` receives a block the system allocates, freed with CredFree below.
        let ok = unsafe { CredReadW(target_w.as_ptr(), CRED_TYPE_GENERIC, 0, &mut out) };
        if ok == 0 || out.is_null() {
            return None;
        }
        // SAFETY: CredReadW succeeded: `out` is a valid CREDENTIALW with its blob.
        let secret = unsafe {
            let c = &*out;
            let bytes = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize);
            String::from_utf8(bytes.to_vec()).ok()
        };
        // SAFETY: allocated by CredReadW.
        unsafe { CredFree(out as _) };
        secret
    }

    pub fn delete(target: &str) {
        let target_w = wide(target);
        // SAFETY: a valid, NUL-terminated wide string.
        unsafe { CredDeleteW(target_w.as_ptr(), CRED_TYPE_GENERIC, 0) };
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn put(_: &str, _: &str) -> Result<(), String> {
        Err("no keychain on this system".into())
    }
    pub fn get(_: &str) -> Option<String> {
        None
    }
    pub fn delete(_: &str) {}
}

pub use imp::{delete, get, put};

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn round_trip() {
        let t = format!("UltimateDownloadManager/test/{}", std::process::id());
        super::put(&t, "s3cret").unwrap();
        assert_eq!(super::get(&t).as_deref(), Some("s3cret"));
        super::delete(&t);
        assert!(super::get(&t).is_none());
    }
}
