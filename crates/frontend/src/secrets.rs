//! Small secrets (chat sign-ins, the SteamGridDB key) in Windows Credential Manager, next to
//! Flux's Microsoft sign-in. Fixed "FluxLauncher_*" names; one entry holds at most 2,560 bytes.

pub use imp::{delete, read, write};

/// Where the SteamGridDB key is kept.
pub const STEAMGRIDDB: &str = "FluxLauncher_SteamGridDB";

/// A saved text secret.
pub fn read_string(target: &str) -> Option<String> {
    read(target).and_then(|b| String::from_utf8(b).ok()).filter(|s| !s.is_empty())
}

/// Save a text secret, or forget it (None / empty).
pub fn write_string(target: &str, value: Option<&str>) {
    match value.filter(|v| !v.is_empty()) {
        Some(value) => {
            if !write(target, value.as_bytes()) {
                log::warn!("Couldn't save {target} to Windows Credential Manager");
            }
        },
        None => {
            delete(target);
        },
    }
}

#[cfg(windows)]
mod imp {
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
mod imp {
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

