//! Left-click on the Flux tray icon: your three most recent games and instances with Play, how many
//! friends are online, and Open Flux / Chat / Quit. Closes when you click anywhere else.

use std::sync::Mutex;

use gpui::{prelude::*, *};
use gpui_component::{Root, h_flex, v_flex};

use crate::{chat, entity::DataEntities, interface_config::InterfaceConfig, overlay::{Pending, open_flux}};

static POPUP: Mutex<Option<AnyWindowHandle>> = Mutex::new(None);

const W: f32 = 340.;

#[derive(Clone)]
enum Recent {
    Game { id: String, name: String, detail: String },
    Instance { id: bridge::instance::InstanceID, name: SharedString, detail: String },
}

fn recents(data: &DataEntities, cx: &App) -> Vec<Recent> {
    let mut list: Vec<(i64, Recent)> = Vec::new();
    for g in &InterfaceConfig::get(cx).games {
        let key = crate::games::recent_key(g);
        if key > 0 {
            let detail = g.source.clone().unwrap_or_else(|| g.kind.label().to_string());
            list.push((key, Recent::Game { id: g.id.clone(), name: g.name.clone(), detail }));
        }
    }
    for entry in data.instances.read(cx).entries.values() {
        let e = entry.read(cx);
        if e.name == schema::quickplay::INSTANCE_NAME {
            continue;
        }
        if let Some(ms) = e.playtime.last_played_unix_ms {
            let c = &e.configuration;
            let detail = format!("Minecraft · {} {}", c.loader.pretty_name(), c.minecraft_version.as_str());
            list.push(((ms / 1000) as i64, Recent::Instance { id: e.id, name: e.name.clone(), detail }));
        }
    }
    list.sort_by(|a, b| b.0.cmp(&a.0));
    list.into_iter().take(3).map(|(_, r)| r).collect()
}

pub fn close(cx: &mut App) {
    if let Some(handle) = POPUP.lock().unwrap().take() {
        _ = handle.update(cx, |_, window, _| window.remove_window());
    }
}

/// Open the popup above the tray icon; `x`/`y` is the cursor in physical pixels.
pub fn open(x: i32, y: i32, cx: &mut App) {
    if POPUP.lock().unwrap().is_some() {
        close(cx);
        return;
    }
    let Some(data) = cx.try_global::<crate::AppData>().map(|d| d.0.clone()) else { return };
    let items = recents(&data, cx);
    let h = 176. + items.len().max(1) as f32 * 54.;
    // Find the display under the cursor, and place the popup above the taskbar next to it.
    let displays = cx.displays();
    let display = displays.iter().find(|d| {
        let b = d.bounds();
        let s = crate::tray_popup::scale_at(x, y);
        b.contains(&point(px(x as f32 / s), px(y as f32 / s)))
    }).or(displays.first()).cloned();
    let Some(display) = display else { return };
    let s = scale_at(x, y);
    let b = display.bounds();
    let cursor = point(px(x as f32 / s), px(y as f32 / s));
    let left = (cursor.x - px(W - 24.)).max(b.origin.x + px(8.)).min(b.origin.x + b.size.width - px(W + 8.));
    // Just above the taskbar (the work area's bottom), whichever edge the taskbar is on.
    let work = display.visible_bounds();
    let bottom = (cursor.y - px(12.)).min(work.origin.y + work.size.height - px(10.));
    let top = (bottom - px(h)).max(b.origin.y + px(8.));
    let bounds = Bounds { origin: point(left, top), size: size(px(W), px(h)) };

    let handle = cx.open_window(WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        display_id: Some(display.id()),
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
        let view = cx.new(|cx| TrayPopup::new(data, items, window, cx));
        window.activate_window();
        cx.new(|cx| Root::new(view, window, cx))
    });
    if let Ok(handle) = handle {
        *POPUP.lock().unwrap() = Some(handle.into());
    }
}

#[cfg(windows)]
pub fn scale_at(x: i32, y: i32) -> f32 {
    use windows::Win32::{Foundation::POINT, Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromPoint}, UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI}};
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96u32, 96u32);
        _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        dx as f32 / 96.0
    }
}

#[cfg(not(windows))]
pub fn scale_at(_x: i32, _y: i32) -> f32 {
    1.0
}

struct TrayPopup {
    data: DataEntities,
    items: Vec<Recent>,
    _activation: Subscription,
}

impl TrayPopup {
    fn new(data: DataEntities, items: Vec<Recent>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Click anywhere else and it goes away.
        let _activation = cx.observe_window_activation(window, |_, window, _| {
            if !window.is_window_active() {
                POPUP.lock().unwrap().take();
                window.remove_window();
            }
        });
        Self { data, items, _activation }
    }
}

fn dismiss(window: &mut Window) {
    POPUP.lock().unwrap().take();
    window.remove_window();
}

impl Render for TrayPopup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted: Hsla = rgb(0x8d95aa).into();
        let online: usize = chat::Network::ALL.iter().map(|n| chat::state(*n)).filter(|s| s.connected)
            .map(|s| s.friends.iter().filter(|f| f.online).count()).sum();

        let mut list = v_flex().gap_1p5();
        if self.items.is_empty() {
            list = list.child(div().text_sm().text_color(muted).py_3().child("Games you play show up here."));
        }
        for (i, item) in self.items.iter().enumerate() {
            let (name, detail) = match item {
                Recent::Game { name, detail, .. } => (name.clone(), detail.clone()),
                Recent::Instance { name, detail, .. } => (name.to_string(), detail.clone()),
            };
            let letter = name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_default();
            let hue = name.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32) % 360;
            let item = item.clone();
            let data = self.data.clone();
            list = list.child(h_flex().gap_2().p_1p5().rounded_lg().bg(rgb(0x151a28)).items_center()
                .child(div().size(px(36.)).flex_shrink_0().rounded_md().bg(hsla(hue as f32 / 360., 0.35, 0.32, 1.)).flex().items_center()
                    .justify_center().font_weight(FontWeight::BLACK).child(letter))
                .child(v_flex().flex_1().min_w_0()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().text_ellipsis().child(name))
                    .child(div().text_xs().text_color(muted).overflow_hidden().whitespace_nowrap().text_ellipsis().child(detail)))
                .child(div().id(("tray-play", i)).px_3().py_1().rounded_md().bg(rgb(0x4caf72)).cursor_pointer().hover(|s| s.opacity(0.9))
                    .text_xs().font_weight(FontWeight::BOLD).child("Play")
                    .on_click(move |_, window, cx| {
                        match &item {
                            Recent::Game { id, .. } => {
                                let game = InterfaceConfig::get(cx).games.iter().find(|g| &g.id == id).cloned();
                                if let Some(game) = game && crate::games::start_game(&game, cx).is_err() {
                                    // Something needs fixing first: show the game's page.
                                    open_flux(Some(Pending::Page(crate::ui::PageType::Game { id: id.clone().into() })));
                                }
                            },
                            Recent::Instance { id, name, .. } => crate::root::start_instance(*id, name.clone(), None, &data, window, cx),
                        }
                        dismiss(window);
                    })));
        }

        let button = |id: &'static str, label: &'static str, bg: u32, fg: u32| div().id(id).h(px(34.)).flex_shrink_0().px_3().flex().items_center().justify_center().rounded_md().cursor_pointer()
            .bg(rgb(bg)).text_color(rgb(fg)).text_sm().font_weight(FontWeight::SEMIBOLD).hover(|s| s.opacity(0.9)).child(label);

        v_flex().size_full().p_3().gap_2().rounded_xl().bg(rgb(0x0d1018)).border_1().border_color(rgb(0x232838)).text_color(white())
            .child(h_flex().gap_2().items_center()
                .child(div().size(px(22.)).rounded_md().bg(rgb(0x5682ff)).flex().items_center().justify_center()
                    .text_xs().font_weight(FontWeight::BLACK).child("F"))
                .child(div().flex_1().text_sm().font_weight(FontWeight::BOLD).child("Flux"))
                .child(div().text_xs().text_color(if online > 0 { rgb(0x4caf72) } else { rgb(0x8d95aa) })
                    .child(match online { 0 => "No friends online".to_string(), 1 => "1 friend online".into(), n => format!("{n} friends online") })))
            .child(div().text_xs().text_color(muted).child("Jump back in"))
            .child(list)
            .child(div().flex_1())
            .child(h_flex().gap_1p5().flex_shrink_0()
                .child(button("tray-open", "Open Flux", 0x5682ff, 0xffffff).flex_1().on_click(|_, window, _| {
                    dismiss(window);
                    open_flux(None);
                }))
                .child(button("tray-chat", "Chat", 0x151a28, 0xeef1f8).on_click(|_, window, _| {
                    dismiss(window);
                    open_flux(Some(Pending::Chat));
                }))
                .child(button("tray-quit", "Quit", 0x2a1520, 0xff7a7a).on_click(|_, window, cx| {
                    dismiss(window);
                    crate::quit_flux(cx);
                })))
    }
}
