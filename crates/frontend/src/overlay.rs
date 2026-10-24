//! The in-game overlay: press the overlay shortcut (Shift+F1 by default, Settings › Appearance) in a
//! game Flux started and a see-through panel covers the game's window exactly, whatever its size:
//! the time and session, game and PC stats, your newest screenshots and clips (with a viewer that
//! copies and trims them), friends online, and a bar of actions. Emulator controls only appear for
//! games running in an emulator.
//!
//! The overlay never takes focus from the game, so a fullscreen game stays fullscreen underneath it.
//! While it's open, Esc, the arrow keys and Enter (and any controller) drive it; switching to another
//! app (Alt+Tab) closes it. Nothing is injected into the game, so true exclusive-fullscreen games
//! (rare today) can't show it.

use std::{collections::HashMap, path::PathBuf, sync::{Mutex, atomic::{AtomicBool, AtomicU32, Ordering}}};

use gpui::{prelude::*, *};
use gpui_component::{Icon, h_flex, v_flex};

use crate::{chat, icon::PandoraIcon, interface_config::InterfaceConfig, media::{self, MediaItem}, ui::PageType};

/// Something to do in the main window once it's open (asked for from the tray or the overlay).
#[derive(Clone)]
pub enum Pending {
    Chat,
    Page(PageType),
    /// A game's page on a tab ("graphics", "mods", ...).
    GameTab(String, &'static str),
    Settings,
}

pub static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
/// A game the overlay couldn't open over (exclusive fullscreen): explained next time Flux is open.
pub static FULLSCREEN_NOTICE: Mutex<Option<String>> = Mutex::new(None);

/// The note for the main window about exclusive fullscreen, once.
pub fn take_fullscreen_notice() -> Option<String> {
    FULLSCREEN_NOTICE.lock().unwrap().take().map(|game| format!(
        "The overlay can't open over {game} while it's in exclusive fullscreen (Windows would knock the game out of fullscreen).          Set the game's display mode to Borderless (or Windowed fullscreen) and the overlay works on top of it."))
}
static OVERLAY: Mutex<Option<AnyWindowHandle>> = Mutex::new(None);
static PERF_RUNNING: AtomicBool = AtomicBool::new(false);
static GAME_PID: AtomicU32 = AtomicU32::new(0);
/// Exe paths (lowercase) of games Flux started through another launcher (Steam, Epic, ...).
static LAUNCHED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Keys that drive the overlay while it's open (registered only then).
pub const KEY_ESC: i32 = 20;
pub const KEY_LEFT: i32 = 21;
pub const KEY_UP: i32 = 22;
pub const KEY_RIGHT: i32 = 23;
pub const KEY_DOWN: i32 = 24;
pub const KEY_ENTER: i32 = 25;
const KEYS: [(i32, u32); 6] = [(KEY_ESC, 0x1B), (KEY_LEFT, 0x25), (KEY_UP, 0x26), (KEY_RIGHT, 0x27), (KEY_DOWN, 0x28), (KEY_ENTER, 0x0D)];

#[derive(Clone, Copy, Default)]
pub struct Perf {
    cpu: f32,
    ram_used_gb: f32,
    ram_total_gb: f32,
    gpu: Option<f32>,
    game_gb: Option<f32>,
}

static PERF: Mutex<Perf> = Mutex::new(Perf { cpu: 0., ram_used_gb: 0., ram_total_gb: 0., gpu: None, game_gb: None });

/// The window the overlay sits on.
#[derive(Clone)]
pub struct GameWindow {
    pub hwnd: isize,
    pub pid: u32,
    pub title: String,
    /// The program's file name, lowercase ("rpcs3.exe"), and its full path (lowercase).
    pub exe: String,
    pub exe_path: String,
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

/// A game Flux started through another launcher: remember its exe so the overlay works on it.
pub fn note_launched(exe: &std::path::Path) {
    let path = exe.to_string_lossy().to_lowercase();
    let mut list = LAUNCHED.lock().unwrap();
    if !list.contains(&path) {
        list.push(path);
    }
}

/// Flux's library game for this window: started by Flux directly (or its emulator), or through a
/// launcher (matched by exe). (id, name, seconds played)
fn flux_game_of(game: &GameWindow, cx: &App) -> Option<(String, String, u64)> {
    let config = InterfaceConfig::get(cx);
    let by_pid = crate::games::game_for_pid(game.pid);
    let found = match by_pid {
        Some(id) => config.games.iter().find(|g| g.id == id),
        None if LAUNCHED.lock().unwrap().contains(&game.exe_path) => {
            config.games.iter().find(|g| g.path.to_string_lossy().to_lowercase() == game.exe_path)
        },
        None => None,
    };
    found.map(|g| (g.id.clone(), g.name.clone(), g.play_seconds))
}

/// A Minecraft instance Flux started (java/javaw while an instance is running).
fn is_flux_minecraft(game: &GameWindow, cx: &App) -> bool {
    if !matches!(game.exe.as_str(), "java.exe" | "javaw.exe") {
        return false;
    }
    let Some(data) = cx.try_global::<crate::AppData>() else { return false };
    data.0.instances.read(cx).entries.values().any(|e| e.read(cx).status == bridge::instance::InstanceStatus::Running)
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

fn with_view(cx: &mut App, f: impl FnOnce(&mut OverlayView, &mut Window, &mut Context<OverlayView>)) {
    let handle = *OVERLAY.lock().unwrap();
    if let Some(handle) = handle {
        _ = handle.update(cx, |root, window, cx| {
            if let Ok(view) = root.downcast::<OverlayView>() {
                view.update(cx, |view, cx| f(view, window, cx));
            } else {
                window.remove_window();
            }
        });
    }
}

/// The shortcut: open the overlay over the game in front, or close it if it's open.
pub fn toggle(foreground: isize, cx: &mut App) {
    if is_open() {
        with_view(cx, |view, window, cx| view.close(window, cx));
        return;
    }
    if !InterfaceConfig::get(cx).overlay_enabled {
        return;
    }
    let Some(game) = win::inspect(foreground) else { return };
    // Only over games Flux started: never over a browser, Discord, this chat, ...
    if flux_game_of(&game, cx).is_none() && !is_flux_minecraft(&game, cx) {
        log::info!("Overlay: {} wasn't started by Flux", game.exe);
        return;
    }
    // A game in exclusive fullscreen drops out of fullscreen (often minimizing or quitting) as soon
    // as any window appears over it, so don't: tell the player instead.
    if win::exclusive_fullscreen() {
        log::info!("Overlay: {} is in exclusive fullscreen", game.exe);
        win::beep();
        *PENDING.lock().unwrap() = None;
        *FULLSCREEN_NOTICE.lock().unwrap() = Some(display_title(&game, flux_game_of(&game, cx).as_ref()));
        return;
    }
    open_overlay(game, cx);
}

/// Esc / arrows / Enter while the overlay is open.
pub fn key(id: i32, cx: &mut App) {
    let nav = match id {
        KEY_ESC => crate::controller_nav::Nav::Back,
        KEY_LEFT => crate::controller_nav::Nav::Left,
        KEY_RIGHT => crate::controller_nav::Nav::Right,
        KEY_UP => crate::controller_nav::Nav::Up,
        KEY_DOWN => crate::controller_nav::Nav::Down,
        _ => crate::controller_nav::Nav::Confirm,
    };
    with_view(cx, |view, window, cx| view.nav(nav, window, cx));
}

/// Testing only: the overlay over any window, even one Flux didn't start.
pub fn open_test_window(hwnd: isize, cx: &mut App) {
    if let Some(game) = win::inspect(hwnd) {
        open_overlay(game, cx);
    }
}

/// Testing only: the overlay over a rectangle of the screen, pretending an emulator runs there.
pub fn open_test(x: i32, y: i32, w: i32, h: i32, cx: &mut App) {
    let game = GameWindow { hwnd: 0, pid: 0, title: "Test game".into(), exe: "rpcs3.exe".into(), exe_path: String::new(),
        rect: (x, y, w, h), scale: crate::tray_popup::scale_at(x, y), started: Some(0) };
    open_overlay(game, cx);
    // FLUX_OVERLAY_TEST_CLIP=<mp4>: open it in the viewer, muted.
    if let Ok(clip) = std::env::var("FLUX_OVERLAY_TEST_CLIP") {
        with_view(cx, |view, _, cx| {
            let path = PathBuf::from(clip);
            let duration = media::duration(&path).unwrap_or(0.0);
            let item = MediaItem { path: path.clone(), video: true, modified: std::time::SystemTime::now() };
            view.viewer = Some(Viewer { player: Some(media::Player::open(&path, true)), image: None, seen: 0, duration,
                start: 0.0, end: duration, busy: false, item });
            view.sel = (1, 0);
            cx.notify();
        });
    }
}

fn open_overlay(game: GameWindow, cx: &mut App) {
    let (x, y, w, h) = game.rect;
    let s = game.scale.max(0.5);
    let bounds = Bounds { origin: point(px(x as f32 / s), px(y as f32 / s)), size: size(px(w as f32 / s), px(h as f32 / s)) };
    let display_id = cx.displays().into_iter().find(|d| d.bounds().contains(&bounds.center())).map(|d| d.id());

    GAME_PID.store(game.pid, Ordering::SeqCst);
    start_perf();
    let rect = game.rect;
    let handle = cx.open_window(WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        display_id,
        titlebar: None,
        // Never take focus: the game keeps it (and stays fullscreen).
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
    }, move |window, cx| cx.new(|cx| OverlayView::new(game, window, cx)));
    match handle {
        Ok(handle) => {
            *OVERLAY.lock().unwrap() = Some(handle.into());
            win::make_no_activate(rect);
            for (id, vk) in KEYS {
                crate::tray::set_hotkey(id, Some((0, vk)));
            }
        },
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
            let perf = sampler.sample(GAME_PID.load(Ordering::SeqCst));
            *PERF.lock().unwrap() = perf;
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }
    }).ok();
}

/// File-name-safe version of a game's title (its screenshots start with it).
pub fn safe_name(title: &str) -> String {
    title.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect::<String>().trim().to_string()
}

fn display_title(game: &GameWindow, flux_game: Option<&(String, String, u64)>) -> String {
    if let Some((_, name, _)) = flux_game {
        return name.clone();
    }
    let t = game.title.trim();
    if t.is_empty() { game.exe.trim_end_matches(".exe").to_string() } else { t.to_string() }
}

/// The screenshot shortcut, pressed in a Flux game: save it and show a small "Saved" note on top.
pub fn quick_screenshot(foreground: isize, cx: &mut App) {
    if is_open() || !InterfaceConfig::get(cx).overlay_enabled {
        return;
    }
    let Some(game) = win::inspect(foreground) else { return };
    let flux_game = flux_game_of(&game, cx);
    if flux_game.is_none() && !is_flux_minecraft(&game, cx) {
        return;
    }
    // In exclusive fullscreen even the little "saved" note would knock the game out.
    let quiet = win::exclusive_fullscreen();
    let title = display_title(&game, flux_game.as_ref());
    let text = match win::screenshot(game.hwnd, &title) {
        Ok(_) => "Screenshot saved to Pictures › Flux".to_string(),
        Err(e) => format!("Couldn't take a screenshot: {e}"),
    };
    if quiet {
        win::beep();
    } else {
        toast(&game, text, cx);
    }
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

/// Everything you can select in the overlay (with the mouse, keys or a controller).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Act {
    Exit,
    Back,
    Media(usize),
    Shot,
    Record,
    Flux,
    Chat,
    GamePage,
    Pause,
    EmuSettings,
    StopEmu,
    ViewClose,
    ViewCopy,
    ViewFolder,
    ViewOpen,
    PlayPause,
    Back5,
    Fwd5,
    SetStart,
    SetEnd,
    TrimSave,
}

/// The screenshot or clip open in the viewer.
struct Viewer {
    item: MediaItem,
    /// Clips play right here in the overlay, with sound; screenshots just show.
    player: Option<media::Player>,
    /// The clip's current frame, and which one it is.
    image: Option<std::sync::Arc<RenderImage>>,
    seen: u64,
    duration: f64,
    start: f64,
    end: f64,
    busy: bool,
}

pub struct OverlayView {
    game: GameWindow,
    /// The emulator is paused (suspended) while the overlay is open.
    paused: bool,
    note: Option<String>,
    /// Show the "turn on Game Bar recording" help.
    record_help: bool,
    flux_game: Option<(String, String, u64)>,
    media: Vec<MediaItem>,
    thumbs: HashMap<PathBuf, PathBuf>,
    viewer: Option<Viewer>,
    /// Selected (row, column) for keys and controllers.
    sel: (usize, usize),
    poller: crate::controller_nav::Poller,
    _tick: Task<()>,
}

impl OverlayView {
    fn new(game: GameWindow, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        // A controller drives it too; and switching to another app (Alt+Tab) closes it.
        let _tick = cx.spawn(async move |this, cx| {
            let mut n = 0u32;
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
                n += 1;
                let alive = this.update(cx, |view, cx| {
                    let polled = view.poller.poll();
                    let repaint = n % 30 == 0 || !polled.presses.is_empty();
                    (polled.presses, repaint, view.game.hwnd)
                });
                let Ok((presses, repaint, game_hwnd)) = alive else { break };
                // Close when you switch to another app (taskbar, Alt+Tab, Win key), or the game is minimized or gone.
                let left_game = game_hwnd != 0 && win::left_game(game_hwnd);
                let handle = *OVERLAY.lock().unwrap();
                let Some(window) = handle else { break };
                _ = window.update(cx, |root, window, cx| {
                    let Ok(view) = root.downcast::<OverlayView>() else { return };
                    view.update(cx, |view, cx| {
                        if left_game {
                            view.close(window, cx);
                            return;
                        }
                        for nav in presses {
                            view.nav(nav, window, cx);
                        }
                        let new_frame = view.pump_player(window);
                        if repaint || new_frame {
                            cx.notify();
                        }
                    });
                });
            }
        });
        let flux_game = flux_game_of(&game, cx);
        let mut view = Self {
            game, paused: false, note: None, record_help: false, flux_game, media: Vec::new(), thumbs: HashMap::new(),
            viewer: None, sel: (2, 0), poller: crate::controller_nav::Poller::default(), _tick,
        };
        view.load_media(cx);
        view
    }

    /// The newest screenshots and clips, and thumbnails for the clips (made in the background).
    fn load_media(&mut self, cx: &mut Context<Self>) {
        self.media = media::recent(12);
        let videos: Vec<PathBuf> = self.media.iter().filter(|m| m.video && !self.thumbs.contains_key(&m.path)).map(|m| m.path.clone()).collect();
        if videos.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let made = cx.background_executor().spawn(async move {
                videos.into_iter().filter_map(|v| media::frame(&v, 1.0).ok().map(|t| (v, t))).collect::<Vec<_>>()
            }).await;
            _ = this.update(cx, |view, cx| {
                view.thumbs.extend(made);
                cx.notify();
            });
        }).detach();
    }

    fn close(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        if self.paused {
            win::suspend(self.game.pid, false);
            self.paused = false;
        }
        for (id, _) in KEYS {
            crate::tray::set_hotkey(id, None);
        }
        OVERLAY.lock().unwrap().take();
        PERF_RUNNING.store(false, Ordering::SeqCst);
        window.remove_window();
    }

    fn title(&self) -> String {
        display_title(&self.game, self.flux_game.as_ref())
    }

    /// The selectable things, row by row (top to bottom).
    fn rows(&self) -> Vec<Vec<Act>> {
        if let Some(v) = &self.viewer {
            return if v.item.video {
                vec![vec![Act::ViewClose], vec![Act::PlayPause, Act::Back5, Act::Fwd5, Act::ViewCopy, Act::ViewFolder],
                    vec![Act::SetStart, Act::SetEnd, Act::TrimSave]]
            } else {
                vec![vec![Act::ViewClose], vec![Act::ViewCopy, Act::ViewFolder, Act::ViewOpen]]
            };
        }
        let mut dock = vec![Act::Shot, Act::Record, Act::Flux, Act::Chat];
        if self.flux_game.is_some() {
            dock.push(Act::GamePage);
        }
        if self.game.is_emulator() {
            dock.extend([Act::Pause, Act::EmuSettings, Act::StopEmu]);
        }
        vec![vec![Act::Exit, Act::Back], (0..self.media.len()).map(Act::Media).collect(), dock]
    }

    fn selected(&self) -> Option<Act> {
        let rows = self.rows();
        rows.get(self.sel.0).and_then(|r| r.get(self.sel.1)).copied()
    }

    fn nav(&mut self, nav: crate::controller_nav::Nav, window: &mut Window, cx: &mut Context<Self>) {
        use crate::controller_nav::Nav;
        let rows = self.rows();
        let (mut r, mut c) = self.sel;
        match nav {
            Nav::Up | Nav::Down => {
                let step: isize = if nav == Nav::Up { -1 } else { 1 };
                let mut next = r as isize + step;
                // Skip empty rows (no screenshots yet).
                while next >= 0 && (next as usize) < rows.len() && rows[next as usize].is_empty() {
                    next += step;
                }
                if next >= 0 && (next as usize) < rows.len() {
                    r = next as usize;
                    c = c.min(rows[r].len().saturating_sub(1));
                }
            },
            Nav::Left => c = c.saturating_sub(1),
            Nav::Right => c = (c + 1).min(rows.get(r).map(|row| row.len().saturating_sub(1)).unwrap_or(0)),
            Nav::Confirm => {
                if let Some(act) = self.selected() {
                    self.run(act, window, cx);
                }
                return;
            },
            Nav::Back => {
                if self.viewer.is_some() {
                    self.run(Act::ViewClose, window, cx);
                } else {
                    self.close(window, cx);
                }
                return;
            },
            _ => return,
        }
        self.sel = (r, c);
        cx.notify();
    }

    fn run(&mut self, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        let game_hwnd = self.game.hwnd;
        match act {
            Act::Exit => {
                self.close(window, cx);
                win::close(game_hwnd);
            },
            Act::Back => self.close(window, cx),
            Act::Media(i) => {
                let Some(item) = self.media.get(i).cloned() else { return };
                let duration = if item.video { media::duration(&item.path).unwrap_or(0.0) } else { 0.0 };
                let player = item.video.then(|| media::Player::open(&item.path, false));
                self.viewer = Some(Viewer { player, image: None, seen: 0, duration, start: 0.0, end: duration, busy: false, item });
                self.sel = (1, 0);
            },
            Act::Shot => {
                self.note = Some(match win::screenshot(game_hwnd, &self.title()) {
                    Ok(_) => "Screenshot saved to Pictures › Flux".to_string(),
                    Err(e) => format!("Couldn't take a screenshot: {e}"),
                });
                self.load_media(cx);
            },
            Act::Record => {
                if !win::game_bar_history_on() {
                    self.record_help = true;
                    cx.notify();
                    return;
                }
                self.close(window, cx);
                // Windows Game Bar saves the last 30 seconds of the game in front ("Record that").
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    win::press_record_that();
                });
            },
            Act::Flux => {
                self.close(window, cx);
                open_flux(None);
            },
            Act::Chat => {
                self.close(window, cx);
                open_flux(Some(Pending::Chat));
            },
            Act::GamePage => {
                let Some((id, _, _)) = self.flux_game.clone() else { return };
                self.close(window, cx);
                open_flux(Some(Pending::Page(PageType::Game { id: id.into() })));
            },
            Act::Pause => {
                self.paused = !self.paused;
                win::suspend(self.game.pid, self.paused);
                self.note = self.paused.then(|| "Paused. It resumes when you close the overlay".to_string());
            },
            Act::EmuSettings => {
                self.close(window, cx);
                // This game's emulator settings (the Graphics tab), else the emulators list.
                match self.flux_game.clone() {
                    Some((id, _, _)) => open_flux(Some(Pending::GameTab(id, "graphics"))),
                    None => open_flux(Some(Pending::Page(PageType::Emulators))),
                }
            },
            Act::StopEmu => {
                self.close(window, cx);
                win::close(game_hwnd);
            },
            Act::ViewClose => {
                if let Some(image) = self.viewer.take().and_then(|v| v.image) {
                    _ = window.drop_image(image);
                }
                self.sel = (1, 0);
            },
            Act::ViewCopy => {
                let Some(v) = &self.viewer else { return };
                let path = v.item.path.clone();
                self.note = Some(if v.item.video {
                    match media::copy_file(&path) {
                        Ok(()) => "Clip copied: paste it in Discord, a chat or a folder".into(),
                        Err(e) => format!("Couldn't copy it: {e}"),
                    }
                } else {
                    match std::fs::read(&path) {
                        Ok(bytes) => {
                            let format = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) { ImageFormat::Png } else { ImageFormat::Jpeg };
                            cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(format, bytes)));
                            "Screenshot copied: paste it anywhere".into()
                        },
                        Err(e) => format!("Couldn't copy it: {e}"),
                    }
                });
            },
            Act::ViewFolder => {
                if let Some(v) = &self.viewer {
                    media::show_in_folder(&v.item.path);
                    self.close(window, cx);
                }
            },
            Act::ViewOpen => {
                if let Some(v) = &self.viewer {
                    media::open(&v.item.path);
                    self.close(window, cx);
                }
            },
            Act::PlayPause | Act::Back5 | Act::Fwd5 | Act::SetStart | Act::SetEnd => {
                let Some(v) = &mut self.viewer else { return };
                let Some(player) = &v.player else { return };
                let (time, paused, ended) = {
                    let st = player.state.lock().unwrap();
                    (st.time, st.paused, st.ended)
                };
                match act {
                    Act::PlayPause if paused || ended => player.play(),
                    Act::PlayPause => player.pause(),
                    Act::Back5 => player.seek(time - 5.0),
                    Act::Fwd5 => player.seek((time + 5.0).min(v.duration)),
                    Act::SetStart => v.start = time.min(v.end - 0.5).max(0.0),
                    _ => v.end = time.max(v.start + 0.5).min(v.duration),
                }
            },
            Act::TrimSave => {
                let Some(v) = &mut self.viewer else { return };
                if v.busy {
                    return;
                }
                v.busy = true;
                let (path, start, end) = (v.item.path.clone(), v.start, v.end);
                self.note = Some("Saving the trimmed clip…".into());
                cx.spawn(async move |this, cx| {
                    let result = cx.background_executor().spawn(async move { media::trim(&path, start, end) }).await;
                    _ = this.update(cx, |view, cx| {
                        if let Some(v) = &mut view.viewer {
                            v.busy = false;
                        }
                        view.note = Some(match result {
                            Ok(out) => format!("Saved {}", out.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
                            Err(e) => format!("Couldn't trim it: {e}"),
                        });
                        view.load_media(cx);
                        cx.notify();
                    });
                }).detach();
            },
        }
        cx.notify();
    }

    /// A new frame from the clip player: put it on screen (and free the old one).
    fn pump_player(&mut self, window: &mut Window) -> bool {
        let Some(v) = &mut self.viewer else { return false };
        let Some(state) = v.player.as_ref().map(|p| p.state.clone()) else { return false };
        let frame = {
            let st = state.lock().unwrap();
            if st.seq == v.seen { None } else { v.seen = st.seq; st.frame.clone() }
        };
        let Some((w, h, pixels)) = frame else { return false };
        // GPUI keeps images in BGRA, which is what the player hands over.
        let Some(buffer) = image::RgbaImage::from_raw(w, h, pixels) else { return false };
        let image = std::sync::Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
        if let Some(old) = v.image.replace(image) {
            _ = window.drop_image(old);
        }
        true
    }

}

fn panel() -> Div {
    v_flex().gap_2().p_4().rounded_xl().bg(rgba(0x0d1018e6)).border_1().border_color(white().opacity(0.07))
}

fn heading(text: &str) -> Div {
    div().text_xs().font_weight(FontWeight::BOLD).text_color(rgb(0x8d95aa)).child(text.to_uppercase())
}

fn duration_text(secs: i64) -> String {
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    if h > 0 { format!("{h} h {m} min") } else { format!("{m} min") }
}

fn clock(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The selection ring for keys and controllers.
fn ring(this: Stateful<Div>, on: bool) -> Stateful<Div> {
    if on { this.border_2().border_color(rgb(0x7aceff)) } else { this.border_2().border_color(transparent_black()) }
}

impl Render for OverlayView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent: Hsla = rgb(0x7aceff).into();
        let muted: Hsla = rgb(0x8d95aa).into();
        let narrow = window.viewport_size().width < px(1000.);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let local = chrono::Local::now();
        let config = InterfaceConfig::get(cx);
        let overlay_key = crate::tray::hotkey_label(config.overlay_hotkey_mods, config.overlay_hotkey_vk);
        let shot_key = crate::tray::hotkey_label(config.screenshot_hotkey_mods, config.screenshot_hotkey_vk);
        let sel = self.selected();
        let entity = cx.entity();
        let click = |act: Act| {
            let entity = entity.clone();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| entity.update(cx, |view, cx| view.run(act, window, cx))
        };

        // Top: time and session on the left, the game in the middle, back to the game on the right.
        let mut left = v_flex().gap_0p5()
            .child(div().text_2xl().font_weight(FontWeight::BOLD).child(local.format("%-I:%M %p").to_string()))
            .child(div().text_sm().text_color(muted).child(local.format("%a, %B %-d, %Y").to_string()));
        if let Some(started) = self.game.started {
            left = left.child(div().text_sm().text_color(muted).child(format!("{} this session", duration_text((now - started).max(0)))));
        }
        left = left.child(ring(div().id("ov-exit").mt_2().px_3().py_1().rounded_md().cursor_pointer().bg(white().opacity(0.08))
            .hover(|s| s.bg(white().opacity(0.14))).text_xs().font_weight(FontWeight::BOLD).child("EXIT GAME"), sel == Some(Act::Exit))
            .on_click(click(Act::Exit)));
        let right = h_flex().gap_3().items_start()
            .child(v_flex().items_end()
                .child(div().text_lg().font_weight(FontWeight::BOLD).child("Back to game"))
                .child(div().text_xs().text_color(muted).child(format!("({overlay_key} or Esc)"))))
            .child(ring(div().id("ov-back").size(px(40.)).rounded_md().cursor_pointer().bg(white().opacity(0.1))
                .hover(|s| s.bg(white().opacity(0.18))).flex().items_center().justify_center().child(Icon::new(PandoraIcon::Close).size_5()),
                sel == Some(Act::Back)).on_click(click(Act::Back)));
        let top = h_flex().w_full().items_start().justify_between()
            .child(left.w(px(240.)).flex_shrink_0())
            .child(div().flex_1().min_w_0().text_center().px_4().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BLACK)
                .text_size(px(if narrow { 26. } else { 38. })).overflow_hidden().whitespace_nowrap().text_ellipsis().child(self.title()))
            .child(div().w(px(240.)).flex_shrink_0().flex().justify_end().child(right));

        // Left panel: this session, the game and the PC.
        let perf = *PERF.lock().unwrap();
        let row = |label: &str, value: String| h_flex().justify_between().text_sm()
            .child(div().text_color(muted).child(label.to_string())).child(div().font_weight(FontWeight::SEMIBOLD).child(value));
        let mut overview = panel().w(px(if narrow { 280. } else { 330. })).child(heading("Game overview"));
        if let Some(started) = self.game.started {
            overview = overview.child(row("This session", duration_text((now - started).max(0))));
        }
        if let Some((_, _, played)) = &self.flux_game {
            overview = overview.child(row("Played in Flux", crate::games::play_time_text(*played)));
        }
        overview = overview
            .child(row("Game memory", perf.game_gb.map(|g| format!("{g:.1} GB")).unwrap_or_else(|| "–".into())))
            .child(div().mt_2().child(heading("Your PC")))
            .child(row("CPU", format!("{:.0}%", perf.cpu)))
            .child(row("GPU", perf.gpu.map(|g| format!("{g:.0}%")).unwrap_or_else(|| "–".into())))
            .child(row("Memory", format!("{:.1} / {:.0} GB", perf.ram_used_gb, perf.ram_total_gb)));

        // Right panel: friends online across Flux, PlayStation and Xbox (privacy mode hides names as usual).
        let mut friends = panel().w(px(if narrow { 260. } else { 320. })).child(heading("Friends online"));
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

        // Middle: the viewer, or your newest screenshots and clips.
        // The picture gets whatever height is left between the top, its controls and the bottom bar.
        let reserved = if self.viewer.as_ref().is_some_and(|v| v.item.video) { 520. } else { 420. };
        let media_h = px((f32::from(window.viewport_size().height) - reserved).max(120.));
        let middle: AnyElement = if let Some(v) = &self.viewer {
            let name = v.item.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let picture: AnyElement = match (&v.item.video, &v.image) {
                (false, _) => img(v.item.path.clone()).w_full().h(media_h).object_fit(ObjectFit::Contain).into_any_element(),
                (true, Some(frame)) => img(ImageSource::Render(frame.clone())).w_full().h(media_h).object_fit(ObjectFit::Contain).into_any_element(),
                (true, None) => div().h(px(240.)).flex().items_center().justify_center().text_color(muted).child("Loading…").into_any_element(),
            };
            let button = |act: Act, icon: PandoraIcon, label: &str| ring(h_flex().id(SharedString::from(format!("view-{act:?}"))).gap_2().px_3().py_2().rounded_lg()
                .cursor_pointer().bg(rgb(0x151a28)).hover(|s| s.bg(rgb(0x222a40))).text_sm().font_weight(FontWeight::SEMIBOLD)
                .child(Icon::new(icon).size_4().text_color(rgb(0x7aceff))).child(label.to_string()), sel == Some(act)).on_click(click(act));
            let mut viewer = panel().flex_1().min_w_0()
                .child(h_flex().justify_between().items_center()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().text_ellipsis().child(name))
                    .child(button(Act::ViewClose, PandoraIcon::Close, "Close")))
                .child(picture);
            if let Some(player) = &v.player {
                let (time, paused, ended, error) = {
                    let st = player.state.lock().unwrap();
                    (st.time, st.paused, st.ended, st.error.clone())
                };
                let (d, s, e) = (v.duration.max(0.1), v.start, v.end);
                let playing = !(paused || ended);
                viewer = viewer
                    // Where you are in the clip, with the part you're keeping highlighted.
                    .child(div().relative().h(px(8.)).w_full().rounded_full().bg(white().opacity(0.1))
                        .child(div().absolute().top_0().bottom_0().left(relative((s / d) as f32)).w(relative(((e - s) / d) as f32))
                            .rounded_full().bg(rgba(0x5682ff99)))
                        .child(div().absolute().top(px(-3.)).h(px(14.)).w(px(3.)).rounded_full().bg(white()).left(relative((time / d).clamp(0.0, 1.0) as f32))))
                    .child(h_flex().flex_wrap().gap_2().items_center()
                        .child(button(Act::PlayPause, if playing { PandoraIcon::Pause } else { PandoraIcon::Play }, if playing { "Pause" } else { "Play" }))
                        .child(button(Act::Back5, PandoraIcon::ChevronLeft, "5 s"))
                        .child(button(Act::Fwd5, PandoraIcon::ChevronRight, "5 s"))
                        .child(div().text_sm().text_color(muted).child(format!("{} / {}", clock(time), clock(v.duration))))
                        .child(div().flex_1())
                        .child(button(Act::ViewCopy, PandoraIcon::Copy, "Copy"))
                        .child(button(Act::ViewFolder, PandoraIcon::FolderOpen, "Show in folder")))
                    .child(h_flex().flex_wrap().gap_2().items_center().text_sm()
                        .child(heading("Trim"))
                        .child(button(Act::SetStart, PandoraIcon::ChevronLeft, "Set start here"))
                        .child(button(Act::SetEnd, PandoraIcon::ChevronRight, "Set end here"))
                        .child(div().text_color(muted).child(format!("Keeping {} to {}", clock(s), clock(e))))
                        .child(div().flex_1())
                        .child(button(Act::TrimSave, PandoraIcon::Check, if v.busy { "Saving…" } else { "Save trimmed clip" })))
                    .when_some(error, |this, e| this.child(div().text_sm().text_color(rgb(0xff7a7a)).child(e)));
            } else {
                viewer = viewer.child(h_flex().gap_2().child(button(Act::ViewCopy, PandoraIcon::Copy, "Copy"))
                    .child(button(Act::ViewFolder, PandoraIcon::FolderOpen, "Show in folder"))
                    .child(button(Act::ViewOpen, PandoraIcon::Image, "Open")));
            }
            viewer.into_any_element()
        } else {
            let mut strip = h_flex().id("ov-media").gap_2().overflow_x_scroll().w_full();
            for (i, m) in self.media.iter().enumerate() {
                let thumb = if m.video { self.thumbs.get(&m.path).cloned() } else { Some(m.path.clone()) };
                let tile = ring(div().id(("media", i)).relative().flex_shrink_0().w(px(176.)).h(px(99.)).rounded_lg().overflow_hidden()
                    .cursor_pointer().bg(rgb(0x151a28)), sel == Some(Act::Media(i)))
                    .when_some(thumb, |t, p| t.child(img(p).size_full().object_fit(ObjectFit::Cover)))
                    .when(m.video, |t| t.child(div().absolute().bottom_1().left_1().px_1p5().rounded_sm().bg(black().opacity(0.7))
                        .flex().gap_1().items_center().text_xs().child(Icon::new(PandoraIcon::Film).size_3()).child("Clip")))
                    .on_click(click(Act::Media(i)));
                strip = strip.child(tile);
            }
            let body = if self.media.is_empty() {
                div().text_sm().text_color(muted).child(format!("Your screenshots ({shot_key}) and clips show up here.")).into_any_element()
            } else {
                strip.into_any_element()
            };
            panel().flex_1().min_w_0().child(heading("Recent screenshots and clips")).child(body).into_any_element()
        };

        // Bottom: every action as a same-size button with a small icon; its shortcut shows on hover.
        let dock_button = |act: Act, icon: PandoraIcon, label: &str, shortcut: Option<String>, active: bool| {
            let group: SharedString = format!("dock-{act:?}").into();
            ring(v_flex().id(group.clone()).group(group.clone()).relative().w(px(118.)).h(px(64.)).flex_shrink_0()
                .items_center().justify_center().gap_1().px_2().rounded_lg().cursor_pointer()
                .bg(if active { rgb(0x5682ff) } else { rgb(0x151a28) }).hover(|s| s.bg(rgb(0x222a40)))
                .child(Icon::new(icon).size_4().text_color(if active { rgb(0xffffff) } else { rgb(0x7aceff) }))
                .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_center().line_height(px(14.)).child(label.to_string()))
                .when_some(shortcut, |this, s| this.child(div().absolute().top(px(-26.)).px_2().py_0p5().rounded_md().bg(rgb(0x0d1018))
                    .border_1().border_color(rgb(0x2a3044)).text_xs().whitespace_nowrap().invisible().group_hover(group, |s| s.visible()).child(s))),
                sel == Some(act)).on_click(click(act))
        };
        let mut dock = h_flex().gap_2().p_2().rounded_xl().bg(rgba(0x0d1018e6)).border_1().border_color(white().opacity(0.07)).flex_wrap().justify_center()
            .child(dock_button(Act::Shot, PandoraIcon::Camera, "Take a screenshot", Some(shot_key), false))
            .child(dock_button(Act::Record, PandoraIcon::Film, "Record last 30 s", Some("Win+Alt+G".into()), false))
            .child(dock_button(Act::Flux, PandoraIcon::House, "Open Flux", None, false))
            .child(dock_button(Act::Chat, PandoraIcon::MessageCircle, "Chat", None, false));
        if self.flux_game.is_some() {
            dock = dock.child(dock_button(Act::GamePage, PandoraIcon::Gamepad2, "Game page", None, false));
        }
        // Emulator controls: only for games running in an emulator.
        if self.game.is_emulator() {
            dock = dock
                .child(div().w(px(1.)).h(px(48.)).mx_1().bg(white().opacity(0.12)))
                .child(dock_button(Act::Pause, if self.paused { PandoraIcon::Play } else { PandoraIcon::Pause },
                    if self.paused { "Resume game" } else { "Pause game" }, None, self.paused))
                .child(dock_button(Act::EmuSettings, PandoraIcon::SlidersVertical, "Emulator settings", None, false))
                .child(dock_button(Act::StopEmu, PandoraIcon::CircleX, "Stop emulator", None, false));
        }

        let mut bottom = v_flex().items_center().gap_2().w_full();
        if self.record_help {
            bottom = bottom.child(h_flex().gap_3().px_4().py_2().rounded_lg().bg(rgba(0x2a2210ee)).text_sm()
                .child("Recording the last 30 seconds uses Windows Game Bar: turn on \"Record what happened\" first.")
                .child(div().id("ov-gb").px_3().py_1().rounded_md().cursor_pointer().bg(rgb(0x5682ff)).font_weight(FontWeight::SEMIBOLD)
                    .child("Open capture settings").on_click(|_, _, cx| cx.open_url("ms-settings:gaming-gamedvr"))));
        }
        if let Some(note) = self.note.clone() {
            bottom = bottom.child(div().px_3().py_1().rounded_md().bg(rgba(0x0d1018e6)).text_sm().text_color(accent).child(note));
        }
        bottom = bottom.child(dock);

        div().id("flux-overlay").size_full().bg(hsla(0.62, 0.4, 0.03, 0.55)).text_color(white())
            .child(v_flex().size_full().p(px(if narrow { 20. } else { 32. })).gap_4()
                .child(top)
                .child(h_flex().flex_1().min_h_0().overflow_hidden().w_full().items_start().gap_4()
                    .child(overview)
                    .child(middle)
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
            let (mut exe, mut exe_path, mut started) = (String::new(), String::new(), None);
            if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut buf = [0u16; 1024];
                let mut len = buf.len() as u32;
                if QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
                    let path = String::from_utf16_lossy(&buf[..len as usize]);
                    exe = path.rsplit('\\').next().unwrap_or_default().to_lowercase();
                    exe_path = path.to_lowercase();
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
                exe_path,
                rect: (r.left, r.top, r.right - r.left, r.bottom - r.top),
                scale: if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 },
                started,
            })
        }
    }

    /// You switched away from the game (another app in front), or it's minimized or closed.
    pub fn left_game(game: isize) -> bool {
        unsafe {
            let h = HWND(game as _);
            if !IsWindow(Some(h)).as_bool() || IsIconic(h).as_bool() {
                return true;
            }
            let front = GetForegroundWindow();
            front.0 as isize != 0 && front != h && !is_own_window(front.0 as isize)
        }
    }

    /// A Direct3D game is in exclusive fullscreen right now.
    pub fn exclusive_fullscreen() -> bool {
        use windows::Win32::UI::Shell::{QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState};
        unsafe { SHQueryUserNotificationState().is_ok_and(|s| s == QUNS_RUNNING_D3D_FULL_SCREEN) }
    }

    pub fn beep() {
        unsafe { _ = windows::Win32::System::Diagnostics::Debug::MessageBeep(MB_OK) };
    }

    pub fn foreground() -> isize {
        unsafe { GetForegroundWindow().0 as isize }
    }

    /// A window of Flux itself (the main window, a popup, ...).
    pub fn is_own_window(hwnd: isize) -> bool {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(HWND(hwnd as _), Some(&mut pid)) };
        pid == std::process::id()
    }

    /// The overlay must never take focus from the game (a fullscreen game would drop out of
    /// fullscreen): find our window covering `rect` and make it a no-activate window.
    pub fn make_no_activate(rect: (i32, i32, i32, i32)) {
        unsafe extern "system" fn each(h: HWND, lparam: LPARAM) -> windows::core::BOOL {
            unsafe {
                let target = &*(lparam.0 as *const (i32, i32, i32, i32));
                let mut pid = 0u32;
                GetWindowThreadProcessId(h, Some(&mut pid));
                let mut r = RECT::default();
                // GPUI adds invisible resize borders around the window, so allow a few pixels.
                let topmost = GetWindowLongPtrW(h, GWL_EXSTYLE) & WS_EX_TOPMOST.0 as isize != 0;
                if pid == std::process::id() && topmost && IsWindowVisible(h).as_bool() && GetWindowRect(h, &mut r).is_ok()
                    && (r.left - target.0).abs() <= 24 && (r.top - target.1).abs() <= 24
                    && ((r.right - r.left) - target.2).abs() <= 48 && ((r.bottom - r.top) - target.3).abs() <= 48
                {
                    let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
                    SetWindowLongPtrW(h, GWL_EXSTYLE, ex | WS_EX_NOACTIVATE.0 as isize);
                }
            }
            true.into()
        }
        unsafe { _ = EnumWindows(Some(each), LPARAM(&rect as *const _ as isize)) };
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
            let dir = crate::media::screenshots_dir();
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

        pub fn sample(&mut self, game_pid: u32) -> Perf {
            let mut perf = Perf::default();
            unsafe {
                // The game's own memory (working set).
                use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
                if game_pid != 0 && let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, game_pid) {
                    let mut counters = PROCESS_MEMORY_COUNTERS { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
                    if K32GetProcessMemoryInfo(process, &mut counters, counters.cb).as_bool() {
                        perf.game_gb = Some(counters.WorkingSetSize as f32 / (1024.0 * 1024.0 * 1024.0));
                    }
                    _ = CloseHandle(process);
                }
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
    pub fn foreground() -> isize { 0 }
    pub fn left_game(_game: isize) -> bool { false }
    pub fn exclusive_fullscreen() -> bool { false }
    pub fn beep() {}
    pub fn is_own_window(_hwnd: isize) -> bool { false }
    pub fn make_no_activate(_rect: (i32, i32, i32, i32)) {}
    pub fn close(_hwnd: isize) {}
    pub fn suspend(_pid: u32, _pause: bool) {}
    pub fn screenshot(_hwnd: isize, _title: &str) -> Result<std::path::PathBuf, String> { Err("Windows only".into()) }
    pub fn game_bar_history_on() -> bool { false }
    pub fn press_record_that() {}
    pub struct PerfSampler;
    impl PerfSampler {
        pub fn new() -> Self { Self }
        pub fn sample(&mut self, _game_pid: u32) -> Perf { Perf::default() }
    }
}
