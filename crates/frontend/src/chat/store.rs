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
    /// Xbox chat was connected.
    #[serde(default)]
    pub xbox: bool,
    /// Xbox chat's Microsoft sign-in (as the Xbox app) and this PC's key for it.
    #[serde(default)]
    pub xbox_refresh: Option<String>,
    #[serde(default)]
    pub xbox_key: Option<String>,
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
        match crate::secrets::read(&format!("{TARGET}_{i}")) {
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
        if !crate::secrets::write(&format!("{TARGET}_{i}"), part) {
            log::warn!("Couldn't save chat sign-ins to Windows Credential Manager");
            return;
        }
    }
    // Drop pieces left over from a longer save.
    let mut i = parts.len();
    while crate::secrets::delete(&format!("{TARGET}_{i}")) {
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
