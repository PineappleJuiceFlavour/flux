//! Games mode's Emulators page: every emulator Flux can use, whether it's ready, what it needs, and
//! buttons to install / update it, use the player's own copy, or open it. `emulator_card` is also
//! shown on a game's Setup tab for that game's emulator.

use std::{ffi::OsString, path::PathBuf};

use bridge::{handle::BackendHandle, message::MessageToBackend, modal_action::ModalAction};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Sizable, button::{Button, ButtonVariants}, h_flex, scroll::ScrollableElement, v_flex,
};

use crate::{
    emulators::{self, EMULATORS},
    entity::DataEntities,
    games,
    interface_config::InterfaceConfig,
    pages::{home_page::{FLUX_SKY}, page::Page},
};

/// Ask for a file, then do something with it (runs detached; `prompt` is shown in the dialog).
fn pick_file(prompt: &'static str, cx: &mut App, then: impl FnOnce(PathBuf, &mut App) + 'static) {
    let receiver = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some(prompt.into()) });
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(paths))) = receiver.await else { return };
        let Some(path) = paths.into_iter().next() else { return };
        _ = cx.update(|cx| {
            then(path, cx);
            cx.refresh_windows();
        });
    }).detach();
}

fn notify(text: impl Into<SharedString>, error: bool, window: &mut Window, cx: &mut App) {
    use gpui_component::{WindowExt, notification::{Notification, NotificationType}};
    let kind = if error { NotificationType::Error } else { NotificationType::Success };
    window.push_notification(Notification::new().with_type(kind).title(text.into()), cx);
}

pub fn emulator_card(id: &'static str, backend: BackendHandle, cx: &App) -> AnyElement {
    let Some(emu) = emulators::emulator(id) else { return gpui::Empty.into_any_element() };
    let theme = cx.theme();
    let (muted, border, secondary) = (theme.muted_foreground, theme.border, theme.secondary);
    let exe = emulators::exe(id, cx);
    let custom = emulators::is_custom(id, cx);
    let ready = exe.is_some();

    let status: (SharedString, Hsla) = match (&exe, custom) {
        (Some(path), true) => (format!("Using your copy: {}", crate::privacy::path(path)).into(), rgb(0x5ee38a).into()),
        (Some(_), false) => ("Installed by Flux and ready".into(), rgb(0x5ee38a).into()),
        (None, _) if emu.managed => ("Not installed yet".into(), muted),
        (None, _) => ("Not set up: choose your emulator's program".into(), muted),
    };

    let mut buttons = h_flex().gap_2().flex_wrap();
    if emu.managed && !custom {
        let backend = backend.clone();
        buttons = buttons.child(Button::new(("install-emu", id_hash(id))).primary().small()
            .label(if ready { "Update" } else { "Install" })
            .on_click(move |_, window, cx| {
                let modal_action = ModalAction::default();
                backend.send(MessageToBackend::InstallEmulator { id: id.into(), modal_action: modal_action.clone() });
                crate::modals::generic::show_notification(window, cx, format!("Couldn't install {}", emu.name).into(), modal_action);
            }));
    }
    buttons = buttons.child(Button::new(("own-emu", id_hash(id))).small()
        .label(if emu.managed { "Use my own copy" } else { "Choose program" })
        .when(!emu.managed && !ready, |b| b.primary())
        .on_click(move |_, _, cx| pick_file("Choose the emulator's program (.exe)", cx, move |path, cx| {
            InterfaceConfig::get_mut(cx).emulator_paths.insert(id.to_string(), path);
        })));
    if custom && emu.managed {
        buttons = buttons.child(Button::new(("flux-emu", id_hash(id))).small().label("Use Flux's copy")
            .on_click(move |_, _, cx| {
                InterfaceConfig::get_mut(cx).emulator_paths.remove(id);
                cx.refresh_windows();
            }));
    }
    if ready {
        buttons = buttons.child(Button::new(("open-emu", id_hash(id))).small().label(format!("Open {}", emu.name))
            .on_click(move |_, window, cx| {
                if let Err(e) = games::open_emulator(id, &[], cx) {
                    notify(e, true, window, cx);
                }
            }));
        if let Some(folder) = exe.as_ref().and_then(|p| p.parent()).map(PathBuf::from) {
            buttons = buttons.child(Button::new(("folder-emu", id_hash(id))).small().label("Folder")
                .on_click(move |_, window, cx| crate::open_folder(&folder, window, cx)));
        }
        if id == emulators::RPCS3 {
            buttons = buttons.child(Button::new("rpcs3-firmware").small().label("Install PS3 system software...")
                .on_click(|_, _, cx| pick_file("Choose PS3UPDAT.PUP (PS3 system software from PlayStation's website)", cx, |pup, cx| {
                    _ = games::open_emulator(emulators::RPCS3, &[OsString::from("--installfw"), pup.into_os_string()], cx);
                })));
            buttons = buttons.child(Button::new("rpcs3-pkg").small().label("Install a .pkg...")
                .on_click(|_, _, cx| pick_file("Choose a PS3 game package (.pkg)", cx, |pkg, cx| {
                    _ = games::open_emulator(emulators::RPCS3, &[OsString::from("--installpkg"), pkg.into_os_string()], cx);
                })));
        }
    }

    v_flex()
        .gap_2()
        .p_4()
        .rounded_xl()
        .bg(secondary)
        .border_1()
        .border_color(border)
        .child(h_flex().gap_3().items_center()
            .child(div().font_weight(FontWeight::SEMIBOLD).child(emu.name))
            .child(div().px_2().py_0p5().rounded_md().bg(cx.theme().background).text_xs().text_color(muted).child(emu.consoles))
            .when(!emu.license.is_empty(), |this| this.child(div().text_xs().text_color(muted).child(emu.license))))
        .child(div().text_sm().text_color(status.1).child(status.0))
        .child(div().text_xs().text_color(muted).child(emu.needs))
        .child(buttons)
        .into_any_element()
}

fn id_hash(id: &str) -> usize {
    id.bytes().fold(7usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize))
}

pub struct EmulatorsPage {
    backend: BackendHandle,
}

impl EmulatorsPage {
    pub fn new(data: &DataEntities, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self { backend: data.backend_handle.clone() }
    }
}

impl Page for EmulatorsPage {
    fn controls(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }

    fn scrollable(&self, _cx: &App) -> bool {
        false
    }
}

impl Render for EmulatorsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let cards: Vec<AnyElement> = EMULATORS.iter().map(|e| emulator_card(e.id, self.backend.clone(), cx)).collect();
        // Centred in the window, at most 1000 px wide.
        v_flex().size_full().items_center().child(v_flex()
            .w_full()
            .p_6()
            .gap_4()
            .max_w(px(1000.))
            .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BLACK).text_size(px(40.)).child("Emulators"))
            .child(div().text_sm().text_color(muted).child(
                "Flux installs each emulator from its project's official releases the first time a game needs it, or you can install them all here. \
                 BIOS files, firmware and keys aren't included: they come from your own consoles.",
            ))
            .child(div().text_sm().text_color(crate::pages::home_page::sky()).child("Xbox One: no emulator exists yet, so Xbox One games can be listed but not played."))
            .children(cards)
            ).overflow_y_scrollbar()
    }
}
