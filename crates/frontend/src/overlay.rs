//! The in-game overlay: press the overlay hotkey (Shift+F1 by default, Settings › Appearance) and a
//! dimmed panel covers the game's window exactly, whatever its size: your session, friends online,
//! quick actions and how hard the PC is working. Emulator controls only appear when the game in front
//! runs in an emulator. Esc or the hotkey closes it and gives the game its focus back.
//!
//! It's a normal always-on-top Flux window, nothing is injected into the game, so it shows over
//! windowed and borderless games (and every emulator), not over exclusive-fullscreen ones.

use std::sync::{Mutex, atomic::{AtomicBool, Ordering}};

use gpui::{prelude::*, *};
use gpui_component::{Icon, Root, h_flex, v_flex};

use crate::{chat, icon::PandoraIcon, interface_config::InterfaceConfig, ui::PageType};

/// Something to do in the main window once it's open (asked for from the tray or the overlay).
#[derive(Clone)]
pub enum Pending {
    Chat,
    Page(PageType),
}

pub static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static OVERLAY: Mutex<Option<AnyWindowHandle>> = Mutex::new(None);
static PERF_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Default)]
struct Perf {
    cpu: f32,
    ram_used_gb: f32,
    ram_total_gb: f32,
    gpu: Option<f32>,
}

static PERF: Mutex<Perf> = Mutex::new(Perf { cpu: 0., ram_used_gb: 0., ram_total_gb: 0., gpu: None });

/// The window the overlay sits on.
#[derive(Clone)]
pub struct GameWindow {
    pub hwnd: isize,
    pub pid: u32,
    pub title: String,
    /// The program's file name, lowercase ("rpcs3.exe").
    pub exe: String,
    /// Physical pixels.
    pub rect: (i32, i32, i32, i32),
    pub scale: f32,
    /// When the program started (Unix seconds).
    pub started: Option<i64>,
}

const EMULATORS: [&str; 16] = ["rpcs3", "xenia", "pcsx2", "duckstation", "ppsspp", "shadps4", "xemu", "ryujinx", "yuzu", "suyu",
    "sudachi", "citron", "eden", "retroarch", "dolphin", "cemu"];

impl GameWindow {
    pub fn is_emulator(&self) -> bool {
        EMULATORS.iter().any(|e| self.exe.contains(e))
    }
}

/// Open Flux's main window (and then the chat or a page in it).
pub fn open_flux(then: Option<Pending>) {
    *PENDING.lock().unwrap() = then;
    if let Some(handle) = crate::FRONTEND.get() {
        handle.send(bridge::message::MessageToFrontend::OpenOrFocusMainWindow);
    }
}

pub fn is_open() -> bool {
    OVERLAY.lock().unwrap().is_some()
}

/// The hotkey: open the overlay over `foreground`, or close it if it's already open.
pub fn toggle(foreground: isize, cx: &mut App) {
    let open = OVERLAY.lock().unwrap().take();
    if let Some(handle) = open {
        _ = handle.update(cx, |root, window, cx| {
            if let Ok(root) = root.downcast::<Root>() {
                if let Ok(view) = root.read(cx).view().clone().downcast::<OverlayView>() {
                    view.update(cx, |view, cx| view.close(window, cx));
                    return;
                }
            }
            window.remove_window();
        });
        return;
    }
    if !InterfaceConfig::get(cx).overlay_enabled {
        return;
    }
    let Some(game) = win::inspect(foreground) else {
        log::info!("Overlay: the window in front isn't a game window");
        return;
    };
    log::info!("Overlay over {} ({}), {:?} at scale {}", game.title, game.exe, game.rect, game.scale);
    open_overlay(game, cx);
}

fn open_overlay(game: GameWindow, cx: &mut App) {
    let (x, y, w, h) = game.rect;
    let s = game.scale.max(0.5);
    let bounds = Bounds { origin: point(px(x as f32 / s), px(y as f32 / s)), size: size(px(w as f32 / s), px(h as f32 / s)) };
    let display_id = cx.displays().into_iter().find(|d| d.bounds().contains(&bounds.center())).map(|d| d.id());

    start_perf();
    let handle = cx.open_window(WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        display_id,
        titlebar: None,
        focus: true,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some("FluxLauncher".into()),
        ..Default::default()
    }, move |window, cx| {
        let view = cx.new(|cx| OverlayView::new(game, window, cx));
        window.activate_window();
        cx.new(|cx| Root::new(view, window, cx))
    });
    match handle {
        Ok(handle) => *OVERLAY.lock().unwrap() = Some(handle.into()),
        Err(e) => log::warn!("Couldn't open the overlay: {e}"),
    }
}

fn start_perf() {
    if PERF_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new().name("flux-overlay-perf".into()).spawn(|| {
        let mut sampler = win::PerfSampler::new();
        while PERF_RUNNING.load(Ordering::SeqCst) {
            let perf = sampler.sample();
            *PERF.lock().unwrap() = perf;
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }
    }).ok();
}

pub struct OverlayView {
    game: GameWindow,
    focus: FocusHandle,
    /// The emulator is paused (suspended) while the overlay is open.
    paused: bool,
    note: Option<String>,
    /// Show the "turn on Game Bar recording" help.
    record_help: bool,
    flux_game: Option<(String, String, u64)>,
    shots: Vec<std::path::PathBuf>,
    _tick: Task<()>,
}

/// Screenshots folder (Pictures\Flux).
pub fn shots_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Pictures").join("Flux")
}

/// File-name-safe version of a game's title (its screenshots start with it).
pub fn safe_name(title: &str) -> String {
    title.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect::<String>().trim().to_string()
}

/// This game's newest screenshots.
fn recent_shots(title: &str) -> Vec<std::path::PathBuf> {
    let prefix = safe_name(title);
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = std::fs::read_dir(shots_dir()).into_iter().flatten().flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.into_iter().take(2).map(|f| f.1).collect()
}

fn flux_game_of(game: &GameWindow, cx: &App) -> Option<(String, String, u64)> {
    crate::games::game_for_pid(game.pid)
        .and_then(|id| InterfaceConfig::get(cx).games.iter().find(|g| g.id == id).map(|g| (g.id.clone(), g.name.clone(), g.play_seconds)))
}

fn display_title(game: &GameWindow, flux_game: Option<&(String, String, u64)>) -> String {
    if let Some((_, name, _)) = flux_game {
        return name.clone();
    }
    let t = game.title.trim();
    if t.is_empty() { game.exe.trim_end_matches(".exe").to_string() } else { t.to_string() }
}

/// The screenshot shortcut, pressed in a game: save it and show a small "Saved" note on top.
pub fn quick_screenshot(foreground: isize, cx: &mut App) {
    if is_open() || !InterfaceConfig::get(cx).overlay_enabled {
        return;
    }
    let Some(game) = win::inspect(foreground) else { return };
    let title = display_title(&game, flux_game_of(&game, cx).as_ref());
    let text = match win::screenshot(game.hwnd, &title) {
        Ok(_) => "Screenshot saved to Pictures › Flux".to_string(),
        Err(e) => format!("Couldn't take a screenshot: {e}"),
    };
    toast(&game, text, cx);
}

/// A small note in the top-right corner of the game for a few seconds (it doesn't take focus).
fn toast(game: &GameWindow, text: String, cx: &mut App) {
    let s = game.scale.max(0.5);
    let (x, y, w, _) = game.rect;
    let size = size(px(340.), px(54.));
    let origin = point(px((x + w) as f32 / s) - size.width - px(24.), px(y as f32 / s) + px(24.));
    let bounds = Bounds { origin, size };
    let display_id = cx.displays().into_iter().find(|d| d.bounds().contains(&bounds.center())).map(|d| d.id());
    let handle = cx.open_window(WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        display_id,
        titlebar: None,
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some("FluxLauncher".into()),
        ..Default::default()
    }, move |_, cx| cx.new(|_| Toast(text)));
    if let Ok(handle) = handle {
        cx.spawn(async move |cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(2600)).await;
            _ = handle.update(cx, |_, window, _| window.remove_window());
        }).detach();
    }
}

struct Toast(String);

impl Render for Toast {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        h_flex().size_full().gap_3().px_4().rounded_xl().bg(rgb(0x0d1018)).border_1().border_color(rgb(0x2a3044)).text_color(white())
            .child(Icon::new(PandoraIcon::Camera).size_5().text_color(rgb(0x7aceff)))
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(self.0.clone()))
    }
}

impl OverlayView {
    fn new(game: GameWindow, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        // Redraw every second for the clocks and the PC stats.
        let _tick = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(std::time::Duration::from_secs(1)).await;
            if this.update(cx, |_, cx| cx.notify()).is_err() {
                break;
            }
        });
        let flux_game = flux_game_of(&game, cx);
        let shots = recent_shots(&display_title(&game, flux_game.as_ref()));
        Self { game, focus, paused: false, note: None, record_help: false, flux_game, shots, _tick }
    }

    fn close(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        if self.paused {
            win::suspend(self.game.pid, false);
            self.paused = false;
        }
        OVERLAY.lock().unwrap().take();
        PERF_RUNNING.store(false, Ordering::SeqCst);
        window.remove_window();
        win::focus(self.game.hwnd);
    }

    fn title(&self) -> String {
        display_title(&self.game, self.flux_game.as_ref())
    }
}

fn panel() -> Div {
    v_flex().gap_2().p_4().rounded_xl().bg(rgba(0x0d1018ee)).border_1().border_color(white().opacity(0.07))
}

fn heading(text: &str) -> Div {
    div().text_xs().font_weight(FontWeight::BOLD).text_color(rgb(0x8d95aa)).child(text.to_uppercase())
}

fn duration_text(secs: i64) -> String {
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    if h > 0 { format!("{h} h {m} min") } else { format!("{m} min") }
}

/// One button in the bottom bar: icon, what it does, and its shortcut.
fn dock_button(id: &'static str, icon: PandoraIcon, label: &str, shortcut: Option<String>, active: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Stateful<Div>
{
    v_flex().id(id).w(px(128.)).items_center().gap_1().px_2().py_3().rounded_lg().cursor_pointer()
        .bg(if active { rgb(0x5682ff) } else { rgb(0x151a28) }).hover(|s| s.bg(rgb(0x222a40)))
        .child(Icon::new(icon).size_6().text_color(if active { rgb(0xffffff) } else { rgb(0x7aceff) }))
        .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_center().child(label.to_string()))
        .when_some(shortcut, |this, s| this.child(div().px_1p5().rounded_sm().bg(white().opacity(0.08)).text_xs()
            .text_color(rgb(0xb7bfd4)).child(s)))
        .on_click(on_click)
}

impl Render for OverlayView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent: Hsla = rgb(0x7aceff).into();
        let muted: Hsla = rgb(0x8d95aa).into();
        let viewport = window.viewport_size();
        let narrow = viewport.width < px(1000.);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let local = chrono::Local::now();
        let config = InterfaceConfig::get(cx);
        let overlay_key = crate::tray::hotkey_label(config.overlay_hotkey_mods, config.overlay_hotkey_vk);
        let shot_key = crate::tray::hotkey_label(config.screenshot_hotkey_mods, config.screenshot_hotkey_vk);
        let game_hwnd = self.game.hwnd;

        // Top: time and session on the left, the game in the middle, back to the game on the right.
        let mut left = v_flex().gap_0p5()
            .child(div().text_2xl().font_weight(FontWeight::BOLD).child(local.format("%-I:%M %p").to_string()))
            .child(div().text_sm().text_color(muted).child(local.format("%a, %B %-d, %Y").to_string()));
        if let Some(started) = self.game.started {
            left = left.child(div().text_sm().text_color(muted).child(format!("{} this session", duration_text((now - started).max(0)))));
        }
        left = left.child(div().id("ov-exit").mt_2().px_3().py_1().rounded_md().cursor_pointer().bg(white().opacity(0.08))
            .hover(|s| s.bg(white().opacity(0.14))).text_xs().font_weight(FontWeight::BOLD).child("EXIT GAME")
            .on_click(cx.listener(move |this, _, window, cx| {
                this.close(window, cx);
                win::close(game_hwnd);
            })));
        let right = h_flex().gap_3().items_start()
            .child(v_flex().items_end()
                .child(div().text_lg().font_weight(FontWeight::BOLD).child("Back to game"))
                .child(div().text_xs().text_color(muted).child(format!("({overlay_key} or Esc)"))))
            .child(div().id("ov-back").size(px(40.)).rounded_md().cursor_pointer().bg(white().opacity(0.1)).hover(|s| s.bg(white().opacity(0.18)))
                .flex().items_center().justify_center().child(Icon::new(PandoraIcon::Close).size_5())
                .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))));
        let top = h_flex().w_full().items_start().justify_between()
            .child(left.w(px(260.)).flex_shrink_0())
            .child(div().flex_1().min_w_0().text_center().px_4().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BLACK)
                .text_size(px(if narrow { 26. } else { 38. })).overflow_hidden().whitespace_nowrap().text_ellipsis().child(self.title()))
            .child(div().w(px(260.)).flex_shrink_0().flex().justify_end().child(right));

        // Left panel: this session, the PC, and this game's screenshots.
        let perf = *PERF.lock().unwrap();
        let row = |label: &str, value: String| h_flex().justify_between().text_sm()
            .child(div().text_color(muted).child(label.to_string())).child(div().font_weight(FontWeight::SEMIBOLD).child(value));
        let mut overview = panel().w(px(if narrow { 300. } else { 360. })).child(heading("Game overview"));
        if let Some(started) = self.game.started {
            overview = overview.child(row("This session", duration_text((now - started).max(0))));
        }
        if let Some((_, _, played)) = &self.flux_game {
            overview = overview.child(row("Played in Flux", crate::games::play_time_text(*played)));
        }
        overview = overview
            .child(div().mt_2().child(heading("Your PC")))
            .child(row("CPU", format!("{:.0}%", perf.cpu)))
            .child(row("GPU", perf.gpu.map(|g| format!("{g:.0}%")).unwrap_or_else(|| "–".into())))
            .child(row("Memory", format!("{:.1} / {:.0} GB", perf.ram_used_gb, perf.ram_total_gb)));
        if !self.shots.is_empty() {
            let mut thumbs = h_flex().gap_2();
            for shot in &self.shots {
                thumbs = thumbs.child(img(shot.clone()).w(px(158.)).h(px(89.)).rounded_md().object_fit(ObjectFit::Cover));
            }
            overview = overview.child(div().mt_2().child(heading("Your screenshots"))).child(thumbs);
        }

        // Right panel: friends online across Flux, PlayStation and Xbox (privacy mode hides names as usual).
        let mut friends = panel().w(px(if narrow { 280. } else { 340. })).child(heading("Friends online"));
        let mut online = 0;
        let mut latest: Option<(String, String)> = None;
        for network in chat::Network::ALL {
            let state = chat::state(network);
            if !state.connected {
                continue;
            }
            for f in state.friends.iter().filter(|f| f.online) {
                online += 1;
                if online <= 10 {
                    friends = friends.child(h_flex().gap_2().items_center()
                        .child(div().size(px(8.)).flex_shrink_0().rounded_full().bg(rgb(0x4caf72)))
                        .child(v_flex().min_w_0()
                            .child(div().text_sm().child(crate::privacy::name(&f.id, &f.name)))
                            .child(div().text_xs().text_color(muted).overflow_hidden().whitespace_nowrap().text_ellipsis()
                                .child(format!("{} · {}", network.label(), crate::privacy::text(&f.status))))));
                }
            }
            for id in &state.unread {
                if let Some(m) = state.messages.get(id).and_then(|m| m.last()) {
                    let name = state.friends.iter().find(|f| &f.id == id).map(|f| crate::privacy::name(&f.id, &f.name)).unwrap_or_default();
                    latest = Some((name, crate::privacy::message(&m.text)));
                }
            }
        }
        if online == 0 {
            friends = friends.child(div().text_sm().text_color(muted).child("No friends online right now"));
        }
        if let Some((name, text)) = latest {
            friends = friends.child(div().mt_2().pt_2().border_t_1().border_color(white().opacity(0.07)).text_sm()
                .child(div().text_color(accent).child(name)).child(div().child(text)));
        }

        // Bottom: every action as a labelled button with its shortcut.
        let mut dock = h_flex().gap_2().p_2().rounded_xl().bg(rgba(0x0d1018ee)).border_1().border_color(white().opacity(0.07))
            .flex_wrap().justify_center()
            .child(dock_button("ov-shot", PandoraIcon::Camera, "Take a screenshot", Some(shot_key), false, cx.listener(move |this, _, _, cx| {
                this.note = Some(match win::screenshot(game_hwnd, &this.title()) {
                    Ok(_) => "Screenshot saved to Pictures › Flux".to_string(),
                    Err(e) => format!("Couldn't take a screenshot: {e}"),
                });
                this.shots = recent_shots(&this.title());
                cx.notify();
            })))
            .child(dock_button("ov-record", PandoraIcon::Film, "Record last 30 seconds", Some("Win+Alt+G".into()), false,
                cx.listener(|this, _, window, cx| {
                    if !win::game_bar_history_on() {
                        this.record_help = true;
                        cx.notify();
                        return;
                    }
                    this.close(window, cx);
                    // Windows Game Bar saves the last 30 seconds of the game in front ("Record that").
                    std::thread::spawn(|| {
                        std::thread::sleep(std::time::Duration::from_millis(400));
                        win::press_record_that();
                    });
                })))
            .child(dock_button("ov-flux", PandoraIcon::House, "Open Flux", None, false, cx.listener(|this, _, window, cx| {
                this.close(window, cx);
                open_flux(None);
            })))
            .child(dock_button("ov-chat", PandoraIcon::MessageCircle, "Chat", None, false, cx.listener(|this, _, window, cx| {
                this.close(window, cx);
                open_flux(Some(Pending::Chat));
            })));
        if let Some((id, _, _)) = self.flux_game.clone() {
            dock = dock.child(dock_button("ov-page", PandoraIcon::Gamepad2, "Game page", None, false, cx.listener(move |this, _, window, cx| {
                this.close(window, cx);
                open_flux(Some(Pending::Page(PageType::Game { id: id.clone().into() })));
            })));
        }
        // Emulator controls: only for games running in an emulator.
        if self.game.is_emulator() {
            let (paused, pid) = (self.paused, self.game.pid);
            dock = dock
                .child(div().w(px(1.)).h(px(64.)).mx_1().bg(white().opacity(0.12)))
                .child(dock_button("ov-pause", if paused { PandoraIcon::Play } else { PandoraIcon::Pause },
                    if paused { "Resume game" } else { "Pause game" }, None, paused, cx.listener(move |this, _, _, cx| {
                        this.paused = !this.paused;
                        win::suspend(pid, this.paused);
                        this.note = this.paused.then(|| "Paused. It resumes when you close the overlay".to_string());
                        cx.notify();
                    })))
                .child(dock_button("ov-emus", PandoraIcon::SlidersVertical, "Emulator settings", None, false, cx.listener(|this, _, window, cx| {
                    this.close(window, cx);
                    open_flux(Some(Pending::Page(PageType::Emulators)));
                })))
                .child(dock_button("ov-stop", PandoraIcon::CircleX, "Stop emulator", None, false, cx.listener(move |this, _, window, cx| {
                    this.close(window, cx);
                    win::close(game_hwnd);
                })));
        }

        let mut bottom = v_flex().items_center().gap_2().w_full();
        if self.record_help {
            bottom = bottom.child(h_flex().gap_3().px_4().py_2().rounded_lg().bg(rgba(0x2a2210ee)).text_sm()
                .child("Recording the last 30 seconds uses Windows Game Bar: turn on \"Record what happened\" first.")
                .child(div().id("ov-gb").px_3().py_1().rounded_md().cursor_pointer().bg(rgb(0x5682ff)).font_weight(FontWeight::SEMIBOLD)
                    .child("Open capture settings").on_click(|_, _, cx| cx.open_url("ms-settings:gaming-gamedvr"))));
        }
        if let Some(note) = self.note.clone() {
            bottom = bottom.child(div().text_sm().text_color(accent).child(note));
        }
        bottom = bottom.child(dock);

        div().id("flux-overlay").track_focus(&self.focus).size_full().bg(hsla(0.62, 0.4, 0.03, 0.62)).text_color(white())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.close(window, cx);
                }
            }))
            .child(v_flex().size_full().p(px(if narrow { 20. } else { 32. })).gap_4()
                .child(top)
                .child(h_flex().id("overlay-body").flex_1().min_h_0().w_full().justify_between().items_start().gap_4().overflow_y_scroll()
                    .child(overview)
                    .child(friends))
                .child(bottom))
    }
}

#[cfg(windows)]
pub mod win {
    use super::*;
    use windows::Win32::{
        Foundation::{CloseHandle, FILETIME, HWND, LPARAM, RECT, WPARAM},
        Graphics::Gdi::*,
        Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow},
        System::{
            Performance::*,
            SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
            Threading::{GetProcessTimes, GetSystemTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SUSPEND_RESUME, QueryFullProcessImageNameW},
        },
        UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::*},
    };

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtSuspendProcess(process: windows::Win32::Foundation::HANDLE) -> i32;
        fn NtResumeProcess(process: windows::Win32::Foundation::HANDLE) -> i32;
    }

    fn filetime_unix(ft: FILETIME) -> i64 {
        let t = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
        (t / 10_000_000) as i64 - 11_644_473_600
    }

    /// The window that had focus, unless it's Flux itself, the desktop or the taskbar.
    pub fn inspect(hwnd: isize) -> Option<GameWindow> {
        unsafe {
            let h = HWND(hwnd as _);
            if hwnd == 0 || !IsWindowVisible(h).as_bool() {
                return None;
            }
            let mut class = [0u16; 64];
            let n = GetClassNameW(h, &mut class);
            let class = String::from_utf16_lossy(&class[..n.max(0) as usize]);
            if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid == std::process::id() {
                return None;
            }
            let mut title = [0u16; 512];
            let n = GetWindowTextW(h, &mut title);
            let title = String::from_utf16_lossy(&title[..n.max(0) as usize]);
            let (mut exe, mut started) = (String::new(), None);
            if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut buf = [0u16; 1024];
                let mut len = buf.len() as u32;
                if QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
                    let path = String::from_utf16_lossy(&buf[..len as usize]);
                    exe = path.rsplit('\\').next().unwrap_or_default().to_lowercase();
                }
                let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
                if GetProcessTimes(process, &mut c, &mut e, &mut k, &mut u).is_ok() {
                    started = Some(filetime_unix(c));
                }
                _ = CloseHandle(process);
            }
            let mut r = RECT::default();
            GetWindowRect(h, &mut r).ok()?;
            let dpi = GetDpiForWindow(h);
            Some(GameWindow {
                hwnd,
                pid,
                title,
                exe,
                rect: (r.left, r.top, r.right - r.left, r.bottom - r.top),
                scale: if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 },
                started,
            })
        }
    }

    pub fn focus(hwnd: isize) {
        unsafe { _ = SetForegroundWindow(HWND(hwnd as _)) };
    }

    /// Ask the window to close, as if its X was clicked.
    pub fn close(hwnd: isize) {
        unsafe { _ = PostMessageW(Some(HWND(hwnd as _)), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    }

    /// Freeze or unfreeze a whole program (used to pause emulators).
    pub fn suspend(pid: u32, pause: bool) {
        unsafe {
            if let Ok(process) = OpenProcess(PROCESS_SUSPEND_RESUME, false, pid) {
                if pause { NtSuspendProcess(process); } else { NtResumeProcess(process); }
                _ = CloseHandle(process);
            }
        }
    }

    /// The game's window as a PNG in Pictures\Flux (the overlay itself isn't in it).
    pub fn screenshot(hwnd: isize, title: &str) -> Result<std::path::PathBuf, String> {
        unsafe {
            let h = HWND(hwnd as _);
            let mut r = RECT::default();
            GetWindowRect(h, &mut r).map_err(|e| e.to_string())?;
            let (w, hgt) = (r.right - r.left, r.bottom - r.top);
            if w <= 0 || hgt <= 0 {
                return Err("the game's window has no size".into());
            }
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, w, hgt);
            let old = SelectObject(dc, bitmap.into());
            // 2 = PW_RENDERFULLCONTENT: works for DirectX/OpenGL games too.
            let ok = PrintWindow(h, dc, PRINT_WINDOW_FLAGS(2)).as_bool();
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -hgt,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut pixels = vec![0u8; (w * hgt * 4) as usize];
            let lines = GetDIBits(dc, bitmap, 0, hgt as u32, Some(pixels.as_mut_ptr() as _), &mut info, DIB_RGB_COLORS);
            SelectObject(dc, old);
            _ = DeleteObject(bitmap.into());
            _ = DeleteDC(dc);
            ReleaseDC(None, screen);
            if !ok || lines == 0 {
                return Err("Windows wouldn't capture this window".into());
            }
            for px in pixels.chunks_exact_mut(4) {
                px.swap(0, 2);
                px[3] = 255;
            }
            let image = image::RgbaImage::from_raw(w as u32, hgt as u32, pixels).ok_or("bad image")?;
            let dir = super::shots_dir();
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let name = format!("{} {}.png", super::safe_name(title), chrono::Local::now().format("%Y-%m-%d %H-%M-%S"));
            let path = dir.join(name);
            image.save(&path).map_err(|e| e.to_string())?;
            Ok(path)
        }
    }

    /// Is Game Bar's "Record what happened" (background recording) switched on?
    pub fn game_bar_history_on() -> bool {
        use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
        let mut value = 0u32;
        let mut size = 4u32;
        let result = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\GameDVR"),
                windows::core::w!("HistoricalCaptureEnabled"), RRF_RT_REG_DWORD, None, Some(&mut value as *mut u32 as _), Some(&mut size))
        };
        result.is_ok() && value != 0
    }

    /// Win+Alt+G: Game Bar's "Record that" (saves the last 30 seconds of the game in front).
    pub fn press_record_that() {
        use windows::Win32::UI::Input::KeyboardAndMouse::{KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_LWIN, VK_MENU, keybd_event};
        unsafe {
            keybd_event(VK_LWIN.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
            keybd_event(VK_MENU.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
            keybd_event(b'G', 0, KEYBD_EVENT_FLAGS(0), 0);
            keybd_event(b'G', 0, KEYEVENTF_KEYUP, 0);
            keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_KEYUP, 0);
            keybd_event(VK_LWIN.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        }
    }

    /// CPU (from system times), memory, and GPU (3D engine load, from Windows' performance counters).
    pub struct PerfSampler {
        last: Option<(u64, u64)>,
        query: Option<(PDH_HQUERY, PDH_HCOUNTER)>,
    }

    fn ft(f: FILETIME) -> u64 {
        ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
    }

    impl PerfSampler {
        pub fn new() -> Self {
            let query = unsafe {
                let mut q = PDH_HQUERY::default();
                let mut c = PDH_HCOUNTER::default();
                if PdhOpenQueryW(None, 0, &mut q) == 0
                    && PdhAddEnglishCounterW(q, windows::core::w!("\\GPU Engine(*engtype_3D)\\Utilization Percentage"), 0, &mut c) == 0
                {
                    PdhCollectQueryData(q);
                    Some((q, c))
                } else {
                    None
                }
            };
            Self { last: None, query }
        }

        pub fn sample(&mut self) -> Perf {
            let mut perf = Perf::default();
            unsafe {
                let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
                if GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).is_ok() {
                    let (i, total) = (ft(idle), ft(kernel) + ft(user));
                    if let Some((li, lt)) = self.last {
                        let dt = total.saturating_sub(lt).max(1);
                        perf.cpu = (100.0 * (1.0 - (i.saturating_sub(li)) as f64 / dt as f64)).clamp(0.0, 100.0) as f32;
                    }
                    self.last = Some((i, total));
                }
                let mut mem = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
                if GlobalMemoryStatusEx(&mut mem).is_ok() {
                    let gb = 1024.0 * 1024.0 * 1024.0;
                    perf.ram_total_gb = mem.ullTotalPhys as f32 / gb;
                    perf.ram_used_gb = (mem.ullTotalPhys - mem.ullAvailPhys) as f32 / gb;
                }
                if let Some((q, c)) = self.query {
                    if PdhCollectQueryData(q) == 0 {
                        let (mut size, mut count) = (0u32, 0u32);
                        PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, None);
                        if size > 0 {
                            let mut buf = vec![0u8; size as usize];
                            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
                            if PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) == 0 {
                                let total: f64 = std::slice::from_raw_parts(items, count as usize).iter()
                                    .map(|it| it.FmtValue.Anonymous.doubleValue).sum();
                                perf.gpu = Some(total.clamp(0.0, 100.0) as f32);
                            }
                        }
                    }
                }
            }
            perf
        }
    }
}

#[cfg(not(windows))]
pub mod win {
    use super::*;
    pub fn inspect(_hwnd: isize) -> Option<GameWindow> { None }
    pub fn focus(_hwnd: isize) {}
    pub fn close(_hwnd: isize) {}
    pub fn suspend(_pid: u32, _pause: bool) {}
    pub fn screenshot(_hwnd: isize, _title: &str) -> Result<std::path::PathBuf, String> { Err("Windows only".into()) }
    pub fn game_bar_history_on() -> bool { false }
    pub fn press_record_that() {}
    pub struct PerfSampler;
    impl PerfSampler {
        pub fn new() -> Self { Self }
        pub fn sample(&mut self) -> Perf { Perf::default() }
    }
}
