#![deny(unused_must_use)]

use std::{
    borrow::Cow, path::{Path, PathBuf}, sync::{Arc, atomic::AtomicBool}
};

use bridge::{
    handle::{BackendHandle, FrontendReceiver}, quit::QuitCoordinator}
;
use gpui::*;
use gpui_component::{
    Root, StyledExt, WindowExt, notification::{Notification, NotificationType}
};
use indexmap::IndexMap;
use parking_lot::RwLock;

use crate::{
    entity::{
        DataEntities, PanicMessages, account::AccountEntries, instance::InstanceEntries, metadata::FrontendMetadata
    }, interface_config::InterfaceConfig, processor::Processor, root::{LauncherRoot, LauncherRootGlobal}
};

pub mod backgrounds;
pub mod games;
pub mod disc;
pub mod emulators;
pub mod controller;
pub mod controller_nav;
pub mod chat;
pub mod privacy;
pub mod secrets;
pub mod game_art;
pub mod game_mods;
pub mod win_native;
pub mod importers;
pub mod xenia_config;
pub mod rpcs3_config;
pub mod sk8v;
pub mod demo_tour;
pub mod component;
pub mod data_asset_loader;
pub mod entity;
pub mod game_output;
pub mod modals;
pub mod pages;
pub mod icon;
pub mod interface_config;
pub mod png_render_cache;
pub mod processor;
pub mod root;
pub mod settings;
pub mod skin_renderer;
pub mod skin_thumbnail_cache;
pub mod ui;
pub mod overlay;
pub mod media;
pub mod tray;
pub mod tray_popup;

/// A way for the UI to message itself (the tray asks for the main window this way).
pub static FRONTEND: std::sync::OnceLock<bridge::handle::FrontendHandle> = std::sync::OnceLock::new();
static QUIT: std::sync::OnceLock<QuitCoordinator> = std::sync::OnceLock::new();
/// Set while Flux is really quitting (not just closing to the tray).
static QUITTING: AtomicBool = AtomicBool::new(false);

/// The app's data, for windows opened outside the main window (tray popup, overlay).
pub struct AppData(pub DataEntities);
impl Global for AppData {}

fn run_tray_test(test: &str, cx: &mut App) {
    if let Some(at) = test.strip_prefix("popup:") {
        let mut it = at.split(',').filter_map(|v| v.trim().parse::<i32>().ok());
        if let (Some(x), Some(y)) = (it.next(), it.next()) {
            tray_popup::open(x, y, cx);
        }
    } else if let Some(rect) = test.strip_prefix("overlay-rect:") {
        let v: Vec<i32> = rect.split(',').filter_map(|v| v.trim().parse().ok()).collect();
        if v.len() == 4 {
            overlay::open_test(v[0], v[1], v[2], v[3], cx);
        }
    } else if test == "overlay-front" {
        #[cfg(windows)]
        overlay::toggle(unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }.0 as isize, cx);
    } else if let Some(hwnd) = test.strip_prefix("overlay-hwnd:").and_then(|h| h.trim().parse::<isize>().ok()) {
        log::info!("tray test: overlay over window {hwnd}");
        overlay::toggle(hwnd, cx);
    } else if let Some(title) = test.strip_prefix("overlay:") {
        #[cfg(windows)]
        {
            let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
            let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::FindWindowW(None, windows::core::PCWSTR(wide.as_ptr())) };
            if let Ok(hwnd) = hwnd {
                overlay::toggle(hwnd.0 as isize, cx);
            }
        }
    } else if test == "quit" {
        quit_flux(cx);
    }
}

/// Quit Flux for real (tray › Quit Flux, Ctrl+Q): close every window and take the tray icon away.
pub fn quit_flux(cx: &mut App) {
    QUITTING.store(true, std::sync::atomic::Ordering::SeqCst);
    tray::remove();
    for window in cx.windows() {
        _ = window.update(cx, |_, window, _| window.remove_window());
    }
    if let Some(quit) = QUIT.get() {
        quit.set_can_quit(true);
    }
}

#[derive(rust_embed::RustEmbed)]
#[folder = "../../assets"]
#[include = "icons/**/*.svg"]
#[include = "images/**/*.png"]
#[include = "fonts/**/*.ttf"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }

        Self::get(path)
            .map(|f| Some(f.data))
            .ok_or_else(|| anyhow::anyhow!("could not find asset at path \"{path}\""))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Self::iter().filter_map(|p| p.starts_with(path).then(|| p.into())).collect())
    }
}

/// Flux's data folder (emulators live under it).
pub static LAUNCHER_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

pub const FLUX_THEME: &str = include_str!("../../../assets/themes/flux.json");
pub const DEFAULT_THEME_NAME: &str = "Flux";

actions!([Quit, CloseWindow, OpenSettings, Forwards, Backwards, Confirm]);

pub fn start(
    launcher_dir: PathBuf,
    panic_message: Arc<RwLock<Option<String>>>,
    deadlock_message: Arc<RwLock<Option<String>>>,
    backend_handle: BackendHandle,
    frontend_handle: bridge::handle::FrontendHandle,
    mut recv: FrontendReceiver,
    quit_coordinator: QuitCoordinator,
) {
    let http_client = Arc::new(reqwest_client::ReqwestClient::user_agent(&*schema::USER_AGENT).unwrap());

    _ = LAUNCHER_DIR.set(launcher_dir.clone());
    _ = FRONTEND.set(frontend_handle);
    _ = QUIT.set(quit_coordinator.clone());

    gpui_platform::application().with_http_client(http_client).with_assets(Assets).run(move |cx: &mut App| {
        let _ = cx.text_system().add_fonts(vec![
            Assets.load("fonts/inter/Inter-Regular.ttf").unwrap().unwrap(),
            Assets.load("fonts/roboto-mono/RobotoMono-Regular.ttf").unwrap().unwrap(),
            Assets.load("fonts/minecraft.ttf").unwrap().unwrap(),
        ]);

        gpui_component::init(cx);
        InterfaceConfig::init(cx, launcher_dir.join("interface.json").into());
        games::normalize_library(cx);
        controller::set_mapping(InterfaceConfig::get(cx).controller_mapping.clone());
        if InterfaceConfig::get(cx).controller_emulation {
            controller::set_enabled(true);
        }

        t::set_lang(&InterfaceConfig::get(cx).language);

        gpui_component::Theme::change(gpui_component::ThemeMode::Dark, None, cx);

        let theme_folder = launcher_dir.join("themes");

        // Ship the Flux theme as a file in the themes folder: the registry rebuilds its
        // list from that folder on every reload, so a theme added only in memory would vanish.
        _ = std::fs::create_dir_all(&theme_folder);
        _ = std::fs::write(theme_folder.join("flux.json"), FLUX_THEME);

        _ = gpui_component::ThemeRegistry::watch_dir(theme_folder.clone(), cx, move |cx| {
            InterfaceConfig::apply_theme(cx, true);
        });
        InterfaceConfig::apply_theme(cx, false);

        cx.set_quit_mode(QuitMode::Explicit);

        cx.on_app_quit(|cx| {
            InterfaceConfig::force_save(cx);
            async {}
        }).detach();

        let main_window_hidden = Arc::new(AtomicBool::new(false));

        cx.on_window_closed({
            let main_window_hidden = main_window_hidden.clone();
            let quit_coordinator = quit_coordinator.clone();
            move |cx, _window| {
                if main_window_hidden.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }

                let windows = cx.windows();

                let config = InterfaceConfig::get(cx);
                // Closing the window just hides Flux to the tray; Quit Flux closes it for real.
                if config.close_to_tray && !config.quit_on_main_closed && !QUITTING.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                if config.quit_on_main_closed {
                    for window in &windows {
                        let is_main = window.read(cx, |window: Entity<Root>, cx| {
                            window.read(cx).view().clone().downcast::<LauncherRoot>().is_ok()
                        }).unwrap_or(false);
                        if is_main {
                            return;
                        }
                    }

                    for window in &windows {
                        _ = window.update(cx, |_, window, _| {
                            window.remove_window();
                        });
                    }
                }

                quit_coordinator.set_can_quit(windows.is_empty());
            }
        }).detach();

        cx.bind_keys([
            KeyBinding::new("secondary-q", Quit, None),
            KeyBinding::new("secondary-w", CloseWindow, None),
            KeyBinding::new("secondary-,", OpenSettings, None),
            KeyBinding::new("secondary-[", Backwards, None),
            KeyBinding::new("secondary-]", Forwards, None),
            KeyBinding::new("enter", Confirm, None),
        ]);

        cx.on_action(|_: &Quit, cx| quit_flux(cx));

        // The tray icon and the overlay hotkey, and what they ask for.
        {
            let config = InterfaceConfig::get(cx);
            tray::start(true, config.hotkeys());
        }
        // FLUX_TRAY_TEST=popup:<x>,<y> | overlay:<window title> | quit, 4 s after start (testing).
        let mut tray_test = std::env::var("FLUX_TRAY_TEST").ok();
        let tray_test_delay = std::env::var("FLUX_TRAY_TEST_DELAY").ok().and_then(|d| d.parse().ok()).unwrap_or(4);
        let tray_test_at = std::time::Instant::now() + std::time::Duration::from_secs(tray_test_delay);
        cx.spawn(async move |cx| loop {
            cx.background_executor().timer(std::time::Duration::from_millis(40)).await;
            if tray_test.is_some() && std::time::Instant::now() >= tray_test_at {
                let test = tray_test.take().unwrap_or_default();
                _ = cx.update(|cx| run_tray_test(&test, cx));
            }
            let events = tray::poll();
            if events.is_empty() {
                continue;
            }
            _ = cx.update(|cx| {
                for event in events {
                    match event {
                        tray::TrayEvent::Popup { x, y } => tray_popup::open(x, y, cx),
                        tray::TrayEvent::OpenFlux => overlay::open_flux(None),
                        tray::TrayEvent::Chat => overlay::open_flux(Some(overlay::Pending::Chat)),
                        tray::TrayEvent::Quit => quit_flux(cx),
                        tray::TrayEvent::Hotkey { id: tray::HOTKEY_SCREENSHOT, foreground } => overlay::quick_screenshot(foreground, cx),
                        tray::TrayEvent::Hotkey { id, .. } if (overlay::KEY_ESC..=overlay::KEY_ENTER).contains(&id) => overlay::key(id, cx),
                        tray::TrayEvent::Hotkey { foreground, .. } => overlay::toggle(foreground, cx),
                        tray::TrayEvent::Menu { x, y } => tray_popup::open_menu(x, y, cx),
                    }
                }
            });
        }).detach();

        let instances = cx.new(|_| InstanceEntries {
            entries: IndexMap::new(),
        });
        games::set_backend(backend_handle.clone());
        games::find_missing_web_art(cx);
        // Play time for games started from Flux, and noticing when they close.
        cx.spawn(async move |cx| loop {
            cx.background_executor().timer(std::time::Duration::from_secs(5)).await;
            cx.update(|cx| games::tick_sessions(cx));
        }).detach();
        let metadata = cx.new(|_| FrontendMetadata::new(backend_handle.clone()));
        let accounts = cx.new(|_| AccountEntries::default());
        let data = DataEntities {
            instances,
            metadata,
            backend_handle,
            accounts,
            theme_folder: theme_folder.into(),
            panic_messages: Arc::new(PanicMessages {
                panic_message,
                deadlock_message,
            })
        };
        cx.set_global(AppData(data.clone()));

        let mut processor = Processor::new(data.clone(), main_window_hidden, quit_coordinator);

        while let Some(message) = recv.try_recv() {
            processor.process(message, cx);
        }

        cx.spawn(async move |cx| {
            while let Some(message) = recv.recv().await {
                _ = cx.update(|cx| {
                    processor.process(message, cx);
                });
            }
        }).detach();
    });
}

pub fn open_main_window(data: &DataEntities, cx: &mut App) -> AnyWindowHandle {
    let config = InterfaceConfig::get(cx);

    let window_bounds = match config.main_window_bounds {
        interface_config::WindowBounds::Inherit => None,
        interface_config::WindowBounds::Windowed { x, y, w, h } => {
            Some(WindowBounds::Windowed(Bounds::new(Point::new(px(x), px(y)), Size::new(px(w), px(h)))))
        },
        interface_config::WindowBounds::Maximized { x, y, w, h } => {
            Some(WindowBounds::Maximized(Bounds::new(Point::new(px(x), px(y)), Size::new(px(w), px(h)))))
        },
        interface_config::WindowBounds::Fullscreen { x, y, w, h } => {
            Some(WindowBounds::Fullscreen(Bounds::new(Point::new(px(x), px(y)), Size::new(px(w), px(h)))))
        },
    };

    let use_custom_titlebar = !config.use_os_titlebar;
    crate::root::set_should_render_custom_titlebar(use_custom_titlebar);
    let handle = cx.open_window(
        WindowOptions {
            app_id: Some("FluxLauncher".into()),
            window_min_size: Some(size(px(480.0), px(270.0))),
            titlebar: Some(TitlebarOptions {
                title: Some("Flux".into()),
                appears_transparent: use_custom_titlebar,
                ..Default::default()
            }),
            app_owns_titlebar_drag: use_custom_titlebar,
            window_bounds,
            window_decorations: Some(if use_custom_titlebar { WindowDecorations::Client } else { WindowDecorations::Server }),
            ..Default::default()
        },
        |window, cx| {
            let launcher_root = cx.new(|cx| {
                cx.observe_window_bounds(window, move |_, window, cx| {
                    let origin = window.bounds().origin;
                    let size = window.viewport_size();
                    let new_bounds = (
                        origin.x.to_f64() as f32, origin.y.to_f64() as f32,
                        size.width.to_f64() as f32, size.height.to_f64() as f32
                    );

                    let old_window_bounds = InterfaceConfig::get(cx).main_window_bounds.clone();
                    let old_bounds = match old_window_bounds {
                        interface_config::WindowBounds::Inherit => new_bounds,
                        interface_config::WindowBounds::Windowed { x, y, w, h } => (x, y, w, h),
                        interface_config::WindowBounds::Maximized { x, y, w, h } => (x, y, w, h),
                        interface_config::WindowBounds::Fullscreen { x, y, w, h } => (x, y, w, h),
                    };

                    let new_window_bounds = if window.is_fullscreen() {
                        interface_config::WindowBounds::Fullscreen {
                            x: old_bounds.0,
                            y: old_bounds.1,
                            w: old_bounds.2,
                            h: old_bounds.3
                        }
                    } else if window.is_maximized() {
                        interface_config::WindowBounds::Maximized {
                            x: old_bounds.0,
                            y: old_bounds.1,
                            w: old_bounds.2,
                            h: old_bounds.3
                        }
                    } else {
                        interface_config::WindowBounds::Windowed {
                            x: new_bounds.0,
                            y: new_bounds.1,
                            w: new_bounds.2,
                            h: new_bounds.3
                        }
                    };

                    if new_window_bounds != old_window_bounds {
                        InterfaceConfig::get_mut(cx).main_window_bounds = new_window_bounds;
                    }
                }).detach();

                LauncherRoot::new(&data, window, cx)
            });

            DataEntities::init_globals(cx);
            cx.set_global(LauncherRootGlobal {
                root: launcher_root.clone(),
            });
            cx.new(|cx| Root::new(launcher_root, window, cx))
        },
    ).unwrap();

    cx.activate(true);

    handle.into()
}

pub(crate) fn is_valid_instance_name(name: &str) -> bool {
    is_single_component_path(name) &&
    sanitize_filename::is_sanitized_with_options(name, sanitize_filename::OptionsForCheck { windows: true, ..Default::default() })
}

pub(crate) fn is_single_component_path(path: &str) -> bool {
    let path = std::path::Path::new(path);
    let mut components = path.components().peekable();

    if let Some(first) = components.peek()
        && !matches!(first, std::path::Component::Normal(_))
    {
        return false;
    }

    components.count() == 1
}

pub(crate) fn get_unique_instance_name(original_name: &str, existing_names: &[&str]) -> String {
    if !existing_names.iter().any(|n| *n == original_name) {
        return original_name.to_string();
    }
    for i in 1..100 {
        let numbered = format!("{original_name} ({i})");
        if !existing_names.iter().any(|n| *n == &numbered) {
            return numbered;
        }
    }
    String::new()
}

#[inline]
pub(crate) fn labelled(label: impl Into<SharedString>, element: impl IntoElement) -> Div {
    gpui_component::v_flex().gap_0p5().child(div().text_sm().font_medium().child(label.into())).child(element)
}

pub(crate) fn open_folder(path: &Path, window: &mut Window, cx: &mut App) {
    let mut is_dir = path.is_dir();
    if !is_dir && !path.exists() {
        _ = std::fs::create_dir_all(path);
        is_dir = true;
    }
    if is_dir {
        if let Err(err) = open::that_detached(path) {
            let notification: Notification = (NotificationType::Error, SharedString::new(t::file_system::open_folder::error(err))).into();
            window.push_notification(notification.autohide(false), cx);
        }
    } else {
        let notification: Notification = (NotificationType::Error, t::file_system::open_folder::not_a_directory()).into();
        window.push_notification(notification.autohide(false), cx);
    }
}

pub fn format_downloads(downloads: u64) -> SharedString {
    if downloads >= 1_000_000_000 {
        t::instance::content::downloads::b((downloads / 10_000_000) as f64 / 100.0)
    } else if downloads >= 1_000_000 {
        t::instance::content::downloads::m((downloads / 10_000) as f64 / 100.0)
    } else if downloads >= 10_000 {
        t::instance::content::downloads::k((downloads / 10) as f64 / 100.0)
    } else {
        t::instance::content::downloads::n(downloads)
    }.into()
}
