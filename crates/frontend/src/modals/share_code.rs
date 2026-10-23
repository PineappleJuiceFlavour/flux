//! Instance share codes: make one from an instance (right-click › Share code) and show it ready to
//! copy, or import a friend's code (the box at the top of Minecraft › Instances).

use bridge::{handle::BackendHandle, instance::InstanceID, message::MessageToBackend, modal_action::ModalAction};
use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme, WindowExt, button::{Button, ButtonVariants}, h_flex, v_flex};

use crate::modals::generic;

pub fn open_share_code(id: InstanceID, name: SharedString, backend: BackendHandle, window: &mut Window, cx: &mut App) {
    let modal_action = ModalAction::default();
    let (send, recv) = tokio::sync::oneshot::channel();
    generic::show_modal(window, cx, format!("Making a share code for {name}").into(), "Couldn't make a share code".into(), modal_action.clone());
    backend.send(MessageToBackend::CreateShareCode { id, modal_action, result: send });

    window.spawn(cx, async move |cx| {
        let Ok(Ok(shared)) = recv.await else { return };
        _ = cx.update(|window, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(shared.code.to_string()));
            show_code(name.clone(), shared, window, cx);
        });
    }).detach();
}

fn show_code(name: SharedString, shared: bridge::message::ShareCodeResult, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let muted = cx.theme().muted_foreground;
        // The code has no spaces, so break it into lines to show it (spaces are ignored on import).
        let chars: Vec<char> = shared.code.chars().collect();
        let lines: Vec<String> = chars.chunks(64).map(|c| c.iter().collect()).collect();
        let code = shared.code.clone();
        let what = format!("{} file{} (mods, resource packs and shaders)", shared.included, if shared.included == 1 { "" } else { "s" });

        let mut body = v_flex().gap_3()
            .child(div().text_sm().child("Copied to your clipboard. Send it to a friend: they paste it in the box at the top of Minecraft › Instances and Flux downloads everything for them."))
            .child(div().id("share-code-text").max_h(px(180.)).overflow_y_scroll().p_3().rounded_lg().bg(cx.theme().secondary)
                .font_family("Consolas").text_xs().children(lines.into_iter().map(|l| div().child(l))))
            .child(div().text_xs().text_color(muted).child(format!("Covers {what}. Settings and worlds aren't included.")));
        if !shared.skipped.is_empty() {
            let list = shared.skipped.iter().take(8).map(|s| s.to_string()).collect::<Vec<_>>().join(", ");
            let more = shared.skipped.len().saturating_sub(8);
            body = body.child(div().text_xs().text_color(cx.theme().warning).child(format!(
                "Not included (not on Modrinth or CurseForge, so your friend needs to add these themselves): {list}{}",
                if more > 0 { format!(" and {more} more") } else { String::new() })));
        }

        dialog.title(format!("Share code for {name}")).w(px(560.)).child(body)
            .footer(h_flex().gap_2().w_full()
                .child(Button::new("share-close").flex_1().label("Close").on_click(|_, window, cx| window.close_dialog(cx)))
                .child(Button::new("share-copy").flex_1().primary().label("Copy code").on_click(move |_, window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()));
                    window.push_notification("Share code copied", cx);
                })))
    });
}

/// Import a friend's code as a new instance.
pub fn import_code(code: &str, backend: &BackendHandle, window: &mut Window, cx: &mut App) {
    let code = code.trim();
    if code.is_empty() {
        return;
    }
    let compact: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if !compact.starts_with("FLUX1-") {
        window.push_notification(gpui_component::notification::Notification::error(
            "That isn't a Flux share code. Codes start with FLUX1-: ask your friend to use right-click › Share code on their instance."), cx);
        return;
    }
    let modal_action = ModalAction::default();
    generic::show_modal(window, cx, "Importing your friend's instance".into(), "Couldn't import that code".into(), modal_action.clone());
    backend.send(MessageToBackend::CreateInstanceFromShareCode { code: compact.into(), modal_action });
}
