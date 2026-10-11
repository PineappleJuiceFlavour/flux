//! Chat sign-ins, kept in Windows Credential Manager next to Flux's Microsoft sign-in
//! ("FluxLauncher_Chat_0", "_1", ...). An entry holds at most 2,560 bytes, so longer data is
//! split across numbered entries.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Saved {
    #[serde(default)]
    pub matrix: Option<MatrixSession>,
    /// PlayStation refresh token (lasts about two months).
    #[serde(default)]
    pub psn_refresh: Option<String>,
    /// Xbox was connected (it uses the Microsoft sign-in, nothing to keep here).
    #[serde(default)]
    pub xbox: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixSession {
    pub base_url: String,
    pub access_token: String,
    pub user_id: String,
}

const TARGET: &str = "FluxLauncher_Chat";
/// Windows' limit for one credential's data (CRED_MAX_CREDENTIAL_BLOB_SIZE).
const CHUNK: usize = 5 * 512;

static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn load() -> Saved {
    let _guard = LOCK.lock().unwrap();
    forget_old_file();
    let mut bytes = Vec::new();
    for i in 0.. {
        match cred::read(&format!("{TARGET}_{i}")) {
            Some(part) => bytes.extend(part),
            None => break,
        }
    }
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// Change the saved sign-ins.
pub fn edit(f: impl FnOnce(&mut Saved)) {
    let mut saved = load();
    f(&mut saved);
    let _guard = LOCK.lock().unwrap();
    let Ok(json) = serde_json::to_vec(&saved) else { return };
    let parts = split(&json);
    for (i, part) in parts.iter().enumerate() {
        if !cred::write(&format!("{TARGET}_{i}"), part) {
            log::warn!("Couldn't save chat sign-ins to Windows Credential Manager");
            return;
        }
    }
    // Drop pieces left over from a longer save.
    let mut i = parts.len();
    while cred::delete(&format!("{TARGET}_{i}")) {
        i += 1;
    }
}

fn split(data: &[u8]) -> Vec<&[u8]> {
    if data.is_empty() { vec![data] } else { data.chunks(CHUNK).collect() }
}

/// Flux 1.0.9 kept chat sign-ins in an encrypted `chat.dat`; that's gone (sign in to chat again).
fn forget_old_file() {
    if let Some(path) = crate::LAUNCHER_DIR.get().map(|d| d.join("chat.dat")) && path.exists() {
        _ = std::fs::remove_file(path);
    }
}

#[cfg(windows)]
mod cred {
    use windows::{
        Win32::{Foundation::ERROR_NOT_FOUND, Security::Credentials::{CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW, CredWriteW}},
        core::{HRESULT, PWSTR},
    };

    fn wide(target: &str) -> Vec<u16> {
        target.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn read(target: &str) -> Option<Vec<u8>> {
        let mut name = wide(target);
        let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
        unsafe {
            CredReadW(PWSTR::from_raw(name.as_mut_ptr()), CRED_TYPE_GENERIC, None, &mut credential).ok()?;
            let found = credential.as_ref()?;
            let bytes = std::slice::from_raw_parts(found.CredentialBlob, found.CredentialBlobSize as usize).to_vec();
            CredFree(credential as *const std::ffi::c_void);
            Some(bytes)
        }
    }

    pub fn write(target: &str, bytes: &[u8]) -> bool {
        let mut name = wide(target);
        let mut bytes = bytes.to_vec();
        let credential = CREDENTIALW {
            Flags: CRED_FLAGS(0),
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR::from_raw(name.as_mut_ptr()),
            CredentialBlobSize: bytes.len() as u32,
            CredentialBlob: bytes.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..CREDENTIALW::default()
        };
        unsafe { CredWriteW(&credential, 0).is_ok() }
    }

    /// True if there was one to delete.
    pub fn delete(target: &str) -> bool {
        let mut name = wide(target);
        match unsafe { CredDeleteW(PWSTR::from_raw(name.as_mut_ptr()), CRED_TYPE_GENERIC, None) } {
            Ok(()) => true,
            Err(e) if e.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) => false,
            Err(_) => false,
        }
    }
}

#[cfg(not(windows))]
mod cred {
    pub fn read(_: &str) -> Option<Vec<u8>> {
        None
    }
    pub fn write(_: &str, _: &[u8]) -> bool {
        false
    }
    pub fn delete(_: &str) -> bool {
        false
    }
}

#[cfg(test)]
#[test]
fn splits_into_credential_sized_pieces() {
    let data = vec![7u8; CHUNK * 2 + 10];
    let parts = split(&data);
    assert_eq!(parts.len(), 3);
    assert!(parts.iter().all(|p| p.len() <= CHUNK));
    assert_eq!(parts.concat(), data);
    assert_eq!(split(b"{}").len(), 1);
}
