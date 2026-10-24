//! The Flux icon by the clock, and the overlay's global hotkey. Both live on one small thread with
//! its own hidden window and message loop; what happens there reaches the UI as [`TrayEvent`]s.
//!
//! - Left-click: the tray popup (recent games, Open Flux, Chat, Quit).
//! - Right-click: a native menu (Open Flux, Chat, Quit Flux).
//! - The overlay hotkey (customisable, Shift+F1 by default) works while any game has focus.

use std::sync::{Mutex, OnceLock, mpsc};

#[derive(Debug, Clone, Copy)]
pub enum TrayEvent {
    /// Left-click: open the popup next to the cursor (physical pixels).
    Popup { x: i32, y: i32 },
    OpenFlux,
    Chat,
    Quit,
    /// A global shortcut ([`HOTKEY_OVERLAY`], [`HOTKEY_SCREENSHOT`]), with the window that had focus.
    Hotkey { id: i32, foreground: isize },
}

/// The global shortcuts.
pub const HOTKEY_OVERLAY: i32 = 1;
pub const HOTKEY_SCREENSHOT: i32 = 2;

/// Modifier bits as RegisterHotKey wants them.
pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;

static EVENTS: OnceLock<Mutex<mpsc::Receiver<TrayEvent>>> = OnceLock::new();
static WINDOW: Mutex<isize> = Mutex::new(0);

/// Events since the last call (polled by the UI).
pub fn poll() -> Vec<TrayEvent> {
    EVENTS.get().map(|r| r.lock().unwrap().try_iter().collect()).unwrap_or_default()
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::{
        Win32::{
            Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
            System::LibraryLoader::GetModuleHandleW,
            UI::{
                Input::KeyboardAndMouse::{HOT_KEY_MODIFIERS, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey},
                Shell::{ExtractIconExW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW},
                WindowsAndMessaging::*,
            },
        },
        core::{PCWSTR, w},
    };

    const WM_TRAY: u32 = WM_APP + 1;
    const WM_SET_HOTKEY: u32 = WM_APP + 2;
    const WM_REMOVE: u32 = WM_APP + 3;
    const CMD_OPEN: usize = 1;
    const CMD_CHAT: usize = 2;
    const CMD_QUIT: usize = 3;

    static SENDER: OnceLock<Mutex<mpsc::Sender<TrayEvent>>> = OnceLock::new();
    static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

    fn send(event: TrayEvent) {
        if let Some(s) = SENDER.get() {
            _ = s.lock().unwrap().send(event);
        }
    }

    fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            ..Default::default()
        };
        // Flux's own icon, from the exe.
        if let Ok(exe) = std::env::current_exe() {
            let path: Vec<u16> = exe.as_os_str().encode_wide_null();
            let mut small = HICON::default();
            unsafe { ExtractIconExW(PCWSTR(path.as_ptr()), 0, None, Some(&mut small), 1) };
            data.hIcon = small;
        }
        if data.hIcon.is_invalid() {
            data.hIcon = unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() };
        }
        for (i, c) in "Flux".encode_utf16().enumerate() {
            data.szTip[i] = c;
        }
        data
    }

    trait WideNull {
        fn encode_wide_null(&self) -> Vec<u16>;
    }
    impl WideNull for std::ffi::OsStr {
        fn encode_wide_null(&self) -> Vec<u16> {
            use std::os::windows::ffi::OsStrExt;
            self.encode_wide().chain(std::iter::once(0)).collect()
        }
    }

    fn show_menu(hwnd: HWND) {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else { return };
            _ = AppendMenuW(menu, MF_STRING, CMD_OPEN, w!("Open Flux"));
            _ = AppendMenuW(menu, MF_STRING, CMD_CHAT, w!("Chat"));
            _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, w!("Quit Flux"));
            let mut pt = POINT::default();
            _ = GetCursorPos(&mut pt);
            // Needed so the menu closes when you click elsewhere.
            _ = SetForegroundWindow(hwnd);
            let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, None, hwnd, None);
            _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            _ = DestroyMenu(menu);
            match cmd.0 as usize {
                CMD_OPEN => send(TrayEvent::OpenFlux),
                CMD_CHAT => send(TrayEvent::Chat),
                CMD_QUIT => send(TrayEvent::Quit),
                _ => {},
            }
        }
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
            if Some(&msg) == TASKBAR_CREATED.get() {
                // Explorer restarted: put the icon back.
                _ = Shell_NotifyIconW(NIM_ADD, &icon_data(hwnd));
                return LRESULT(0);
            }
            match msg {
                WM_TRAY => {
                    match (lparam.0 as u32) & 0xffff {
                        WM_LBUTTONUP => {
                            let mut pt = POINT::default();
                            _ = GetCursorPos(&mut pt);
                            send(TrayEvent::Popup { x: pt.x, y: pt.y });
                        },
                        WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                        _ => {},
                    }
                    LRESULT(0)
                },
                WM_HOTKEY => {
                    send(TrayEvent::Hotkey { id: wparam.0 as i32, foreground: GetForegroundWindow().0 as isize });
                    LRESULT(0)
                },
                WM_SET_HOTKEY => {
                    // wParam: id << 16 | modifiers; lParam: virtual key (0 switches it off).
                    let id = (wparam.0 >> 16) as i32;
                    _ = UnregisterHotKey(Some(hwnd), id);
                    if lparam.0 != 0 {
                        let mods = HOT_KEY_MODIFIERS((wparam.0 & 0xffff) as u32) | MOD_NOREPEAT;
                        if RegisterHotKey(Some(hwnd), id, mods, lparam.0 as u32).is_err() {
                            log::warn!("Couldn't register shortcut {id} (another app uses it)");
                        }
                    }
                    LRESULT(0)
                },
                WM_REMOVE => {
                    _ = Shell_NotifyIconW(NIM_DELETE, &icon_data(hwnd));
                    _ = UnregisterHotKey(Some(hwnd), HOTKEY_OVERLAY);
                    _ = UnregisterHotKey(Some(hwnd), HOTKEY_SCREENSHOT);
                    PostQuitMessage(0);
                    LRESULT(0)
                },
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
    }

    pub fn start(show_icon: bool, hotkeys: Vec<(i32, (u32, u32))>) {
        if EVENTS.get().is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        _ = SENDER.set(Mutex::new(tx));
        _ = EVENTS.set(Mutex::new(rx));
        std::thread::Builder::new().name("flux-tray".into()).spawn(move || unsafe {
            let Ok(module) = GetModuleHandleW(None) else { return };
            let class = w!("FluxTrayWindow");
            let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: module.into(), lpszClassName: class, ..Default::default() };
            RegisterClassW(&wc);
            _ = TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));
            // A hidden (never shown) top-level window: the tray menu needs one that can take focus.
            let Ok(hwnd) = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!("Flux tray"), WS_POPUP, 0, 0, 0, 0, None, None, Some(module.into()), None) else { return };
            *WINDOW.lock().unwrap() = hwnd.0 as isize;
            if show_icon {
                _ = Shell_NotifyIconW(NIM_ADD, &icon_data(hwnd));
            }
            for (id, (mods, vk)) in hotkeys {
                _ = SendMessageW(hwnd, WM_SET_HOTKEY, Some(WPARAM(((id as usize) << 16) | mods as usize)), Some(LPARAM(vk as isize)));
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }).ok();
    }

    fn hwnd() -> Option<HWND> {
        let h = *WINDOW.lock().unwrap();
        (h != 0).then_some(HWND(h as _))
    }

    /// Change (or with `None`, switch off) a global shortcut.
    pub fn set_hotkey(id: i32, hotkey: Option<(u32, u32)>) {
        if let Some(hwnd) = hwnd() {
            let (mods, vk) = hotkey.unwrap_or((0, 0));
            unsafe { _ = PostMessageW(Some(hwnd), WM_SET_HOTKEY, WPARAM(((id as usize) << 16) | mods as usize), LPARAM(vk as isize)) };
        }
    }

    /// Take the icon away (Flux is quitting).
    pub fn remove() {
        if let Some(hwnd) = hwnd() {
            unsafe { _ = SendMessageW(hwnd, WM_REMOVE, None, None) };
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn start(_show_icon: bool, _hotkeys: Vec<(i32, (u32, u32))>) {}
    pub fn set_hotkey(_id: i32, _hotkey: Option<(u32, u32)>) {}
    pub fn remove() {}
}

pub use imp::{remove, set_hotkey, start};

/// Virtual-key code for a GPUI key name ("f1", "a", "tab", ...), for customising the hotkey.
pub fn vk_for_key(key: &str) -> Option<u32> {
    let k = key.to_ascii_lowercase();
    if k.len() == 1 {
        let c = k.as_bytes()[0];
        return match c {
            b'a'..=b'z' => Some(c.to_ascii_uppercase() as u32),
            b'0'..=b'9' => Some(c as u32),
            b'`' => Some(0xC0),
            b'-' => Some(0xBD),
            b'=' => Some(0xBB),
            b'[' => Some(0xDB),
            b']' => Some(0xDD),
            b'\\' => Some(0xDC),
            b';' => Some(0xBA),
            b'\'' => Some(0xDE),
            b',' => Some(0xBC),
            b'.' => Some(0xBE),
            b'/' => Some(0xBF),
            _ => None,
        };
    }
    if let Some(n) = k.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
        return (1..=24).contains(&n).then(|| 0x6F + n);
    }
    Some(match k.as_str() {
        "tab" => 0x09,
        "space" => 0x20,
        "enter" => 0x0D,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "insert" => 0x2D,
        "delete" => 0x2E,
        "pause" => 0x13,
        _ => return None,
    })
}

/// "Shift+F1", "Ctrl+Alt+O".
pub fn hotkey_label(mods: u32, vk: u32) -> String {
    let mut parts = Vec::new();
    if mods & MOD_CONTROL != 0 { parts.push("Ctrl".to_string()); }
    if mods & MOD_ALT != 0 { parts.push("Alt".to_string()); }
    if mods & MOD_SHIFT != 0 { parts.push("Shift".to_string()); }
    if mods & MOD_WIN != 0 { parts.push("Win".to_string()); }
    let key = match vk {
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x41..=0x5A | 0x30..=0x39 => (vk as u8 as char).to_string(),
        0x09 => "Tab".into(),
        0x20 => "Space".into(),
        0x0D => "Enter".into(),
        0x24 => "Home".into(),
        0x23 => "End".into(),
        0x21 => "Page Up".into(),
        0x22 => "Page Down".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        0x13 => "Pause".into(),
        0xC0 => "`".into(),
        _ => format!("Key {vk}"),
    };
    parts.push(key);
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert_eq!(vk_for_key("f1"), Some(0x70));
        assert_eq!(vk_for_key("o"), Some(0x4F));
        assert_eq!(hotkey_label(MOD_SHIFT, 0x70), "Shift+F1");
        assert_eq!(hotkey_label(MOD_CONTROL | MOD_ALT, 0x4F), "Ctrl+Alt+O");
    }
}
