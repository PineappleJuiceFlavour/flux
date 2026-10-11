//! Small Windows helpers called directly (no PowerShell or `reg.exe` child processes, which
//! antivirus heuristics treat as suspicious): a program's version-info name, its icon as a PNG,
//! and a registry string.

use std::path::Path;

#[cfg(windows)]
fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// A string from a program's version info ("ProductName", "FileDescription", ...).
#[cfg(windows)]
pub fn version_string(exe: &Path, key: &str) -> Option<String> {
    use windows::{Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW}, core::PCWSTR};
    let path = wide(exe.as_os_str());
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(path.as_ptr()), None, size, data.as_mut_ptr() as _).ok()?;
        // The first language/code page the file lists, then the usual US English fallback.
        let mut langs = Vec::new();
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        let query = wide("\\VarFileInfo\\Translation".as_ref());
        if VerQueryValueW(data.as_ptr() as _, PCWSTR(query.as_ptr()), &mut ptr, &mut len).as_bool() && len >= 4 {
            let pair = std::slice::from_raw_parts(ptr as *const u16, 2);
            langs.push((pair[0], pair[1]));
        }
        langs.push((0x0409, 0x04B0));
        langs.push((0x0409, 0x04E4));
        for (lang, cp) in langs {
            let query = wide(format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\{key}").as_ref());
            if VerQueryValueW(data.as_ptr() as _, PCWSTR(query.as_ptr()), &mut ptr, &mut len).as_bool() && len > 0 {
                let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr as *const u16, len as usize));
                let text = text.trim_end_matches('\0').trim().to_string();
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
        None
    }
}

#[cfg(not(windows))]
pub fn version_string(_exe: &Path, _key: &str) -> Option<String> {
    None
}

/// The program's largest icon (up to 256x256) saved as a PNG.
#[cfg(windows)]
pub fn save_icon_png(exe: &Path, out: &Path) -> bool {
    use windows::Win32::{
        Graphics::Gdi::{BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDIBits, GetObjectW, HGDIOBJ},
        UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO, PrivateExtractIconsW},
    };
    let path = wide(exe.as_os_str());
    if path.len() > 260 {
        return false;
    }
    let mut name = [0u16; 260];
    name[..path.len()].copy_from_slice(&path);
    unsafe {
        let mut icons = [HICON::default()];
        if PrivateExtractIconsW(&name, 0, 256, 256, Some(&mut icons), None, 0) == 0 || icons[0].is_invalid() {
            return false;
        }
        let icon = icons[0];
        let mut info = ICONINFO::default();
        let mut result = false;
        if GetIconInfo(icon, &mut info).is_ok() && !info.hbmColor.is_invalid() {
            let mut bmp = BITMAP::default();
            GetObjectW(HGDIOBJ(info.hbmColor.0), size_of::<BITMAP>() as i32, Some(&mut bmp as *mut _ as _));
            let (w, h) = (bmp.bmWidth.max(1) as u32, bmp.bmHeight.max(1) as u32);
            let dc = CreateCompatibleDC(None);
            let mut header = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(h as i32), // top-down rows
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut pixels = vec![0u8; (w * h * 4) as usize];
            if GetDIBits(dc, info.hbmColor, 0, h, Some(pixels.as_mut_ptr() as _), &mut header, DIB_RGB_COLORS) != 0 {
                // Old icons have no alpha channel: take transparency from the mask instead.
                let has_alpha = pixels.chunks(4).any(|p| p[3] != 0);
                let mut mask = vec![0u8; (w * h * 4) as usize];
                if !has_alpha && !info.hbmMask.is_invalid() {
                    let mut mask_header = header;
                    GetDIBits(dc, info.hbmMask, 0, h, Some(mask.as_mut_ptr() as _), &mut mask_header, DIB_RGB_COLORS);
                }
                let mut image = image::RgbaImage::new(w, h);
                for (i, px) in image.pixels_mut().enumerate() {
                    let p = &pixels[i * 4..i * 4 + 4];
                    let alpha = if has_alpha { p[3] } else if mask[i * 4] == 0 { 255 } else { 0 };
                    *px = image::Rgba([p[2], p[1], p[0], alpha]);
                }
                result = image.save(out).is_ok();
            }
            _ = DeleteDC(dc);
        }
        if !info.hbmColor.is_invalid() {
            _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
        }
        if !info.hbmMask.is_invalid() {
            _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
        }
        _ = DestroyIcon(icon);
        result
    }
}

#[cfg(not(windows))]
pub fn save_icon_png(_exe: &Path, _out: &Path) -> bool {
    false
}

#[derive(Clone, Copy)]
pub enum Hive {
    LocalMachine,
    CurrentUser,
}

#[cfg(windows)]
fn hkey(hive: Hive) -> windows::Win32::System::Registry::HKEY {
    match hive {
        Hive::LocalMachine => windows::Win32::System::Registry::HKEY_LOCAL_MACHINE,
        Hive::CurrentUser => windows::Win32::System::Registry::HKEY_CURRENT_USER,
    }
}

/// A string value from the registry.
#[cfg(windows)]
pub fn registry_string(hive: Hive, key: &str, value: &str) -> Option<String> {
    use windows::{Win32::System::Registry::{RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW}, core::PCWSTR};
    let (key, value) = (wide(key.as_ref()), wide(value.as_ref()));
    unsafe {
        let mut size = 0u32;
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
        if RegGetValueW(hkey(hive), PCWSTR(key.as_ptr()), PCWSTR(value.as_ptr()), flags, None, None, Some(&mut size)).is_err() || size == 0 {
            return None;
        }
        let mut buf = vec![0u16; size as usize / 2 + 1];
        RegGetValueW(hkey(hive), PCWSTR(key.as_ptr()), PCWSTR(value.as_ptr()), flags, None, Some(buf.as_mut_ptr() as _), Some(&mut size)).ok().ok()?;
        let text = String::from_utf16_lossy(&buf[..size as usize / 2]).trim_end_matches('\0').to_string();
        (!text.is_empty()).then_some(text)
    }
}

/// The names of a registry key's subkeys.
#[cfg(windows)]
pub fn registry_subkeys(hive: Hive, key: &str) -> Vec<String> {
    use windows::{Win32::System::Registry::{HKEY, KEY_READ, RegCloseKey, RegEnumKeyExW, RegOpenKeyExW}, core::{PCWSTR, PWSTR}};
    let key = wide(key.as_ref());
    let mut out = Vec::new();
    unsafe {
        let mut handle = HKEY::default();
        if RegOpenKeyExW(hkey(hive), PCWSTR(key.as_ptr()), None, KEY_READ, &mut handle).is_err() {
            return out;
        }
        for index in 0.. {
            let mut name = [0u16; 256];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(handle, index, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None).is_err() {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
        }
        _ = RegCloseKey(handle);
    }
    out
}

#[cfg(not(windows))]
pub fn registry_string(_hive: Hive, _key: &str, _value: &str) -> Option<String> {
    None
}

#[cfg(not(windows))]
pub fn registry_subkeys(_hive: Hive, _key: &str) -> Vec<String> {
    Vec::new()
}

/// A string value under HKEY_CURRENT_USER.
#[cfg(windows)]
pub fn user_registry_string(key: &str, value: &str) -> Option<String> {
    use windows::{Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW}, core::PCWSTR};
    let (key, value) = (wide(key.as_ref()), wide(value.as_ref()));
    unsafe {
        let mut size = 0u32;
        if RegGetValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(value.as_ptr()), RRF_RT_REG_SZ, None, None, Some(&mut size)).is_err() || size == 0 {
            return None;
        }
        let mut buf = vec![0u16; size as usize / 2 + 1];
        RegGetValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(value.as_ptr()), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as _), Some(&mut size)).ok().ok()?;
        let text = String::from_utf16_lossy(&buf[..size as usize / 2]);
        Some(text.trim_end_matches('\0').to_string())
    }
}

#[cfg(not(windows))]
pub fn user_registry_string(_key: &str, _value: &str) -> Option<String> {
    None
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn reads_a_system_program() {
        let notepad = std::path::PathBuf::from(std::env::var("SystemRoot").unwrap()).join("System32").join("notepad.exe");
        let name = version_string(&notepad, "FileDescription").or_else(|| version_string(&notepad, "ProductName"));
        println!("notepad: {name:?}");
        assert!(name.is_some());
        let out = std::env::temp_dir().join("flux-icon-test.png");
        let explorer = std::path::PathBuf::from(std::env::var("SystemRoot").unwrap()).join("explorer.exe");
        assert!(save_icon_png(&explorer, &out), "explorer's icon");
        let img = image::open(&out).unwrap();
        println!("icon {}x{}", img.width(), img.height());
        assert!(img.width() >= 32);
        assert!(user_registry_string("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Shell Folders", "Desktop").is_some());
    }
}
