//! Controllers: PlayStation controllers as Xbox controllers (Flux's own mapper on the ViGEmBus
//! driver), with the driver's status, connected controllers and their battery.

use std::time::Duration;

use bridge::handle::BackendHandle;
use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme, Disableable, Sizable, button::{Button, ButtonVariants}, h_flex, scroll::ScrollableElement, switch::Switch, v_flex};

use crate::{
    controller::{self, mapping::{Mapping, PRESETS, PsButton, XboxControl}},
    entity::DataEntities,
    interface_config::InterfaceConfig,
    pages::{home_page::{FLUX_SKY}, page::Page},
};

pub struct ControllersPage {
    backend: BackendHandle,
    /// The PlayStation button whose Xbox control is being picked.
    selected: Option<PsButton>,
    _refresh: Task<()>,
}

/// Change the mapping, save it and hand it to the running controllers.
fn edit_mapping(cx: &mut App, f: impl FnOnce(&mut Mapping)) {
    let config = InterfaceConfig::get_mut(cx);
    f(&mut config.controller_mapping);
    controller::set_mapping(config.controller_mapping.clone());
    cx.refresh_windows();
}

impl ControllersPage {
    pub fn new(data: &DataEntities, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Live view: controllers come and go, batteries drain, buttons light up while held.
        let _refresh = cx.spawn(async move |page, cx| {
            let mut last = controller::status();
            let mut ticks = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_millis(60)).await;
                ticks += 1;
                let now = controller::status();
                if now != last || ticks % 16 == 0 {
                    last = now;
                    if page.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            }
        });
        Self { backend: data.backend_handle.clone(), selected: None, _refresh }
    }
}

impl Page for ControllersPage {
    fn controls(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }

    fn scrollable(&self, _cx: &App) -> bool {
        false
    }
}

impl Render for ControllersPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = controller::status();
        let enabled = InterfaceConfig::get(cx).controller_emulation;
        let theme = cx.theme();
        let (muted, border, secondary, danger) = (theme.muted_foreground, theme.border, theme.secondary, theme.danger);
        let green: Hsla = rgb(0x5ee38a).into();

        let card = |title: &'static str| v_flex().gap_3().p_5().rounded_xl().bg(secondary).border_1().border_color(border)
            .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BOLD).text_color(rgb(FLUX_SKY)).child(title));

        let driver: AnyElement = if status.driver_installed {
            div().text_sm().text_color(green).child("ViGEmBus driver installed").into_any_element()
        } else {
            v_flex().gap_2()
                .child(div().text_sm().text_color(muted).child(
                    "Needs the ViGEmBus driver (free, BSD license; the same one DS4Windows uses) to create virtual Xbox controllers.                      Its author has retired it, but its final release still works on Windows 10 and 11: download the installer (.exe)                      from its official page and run it. Windows asks for admin permission because it's a driver.",
                ))
                .child(div().child(Button::new("install-vigem").primary().label("Open the ViGEmBus download page").on_click(|_, _, cx| {
                    cx.open_url("https://github.com/nefarius/ViGEmBus/releases/latest");
                })))
                .into_any_element()
        };

        let main = card("PLAYSTATION AS XBOX")
            .child(h_flex().gap_4().items_center()
                .child(v_flex().flex_1().gap_0p5()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Make PlayStation controllers act like Xbox controllers"))
                    .child(div().text_xs().text_color(muted).child(
                        "PS5 (DualSense, DualSense Edge) and PS4 controllers, over USB or Bluetooth. Games see an Xbox 360 controller; \
                         their rumble comes back to your controller.",
                    )))
                .child(Switch::new("controller-emulation").checked(enabled).on_click(|on, _, cx| {
                    InterfaceConfig::get_mut(cx).controller_emulation = *on;
                    controller::set_enabled(*on);
                    cx.refresh_windows();
                })))
            .child(driver)
            .when_some(status.error.clone(), |this, e| this.child(div().text_sm().text_color(danger).child(e)));

        let pads: Vec<AnyElement> = status.pads.iter().map(|pad| {
            let battery = match (pad.battery, pad.charging) {
                (Some(b), true) => format!("{b}% · charging"),
                (Some(b), false) => format!("{b}%"),
                (None, _) => "battery unknown".into(),
            };
            h_flex().gap_4().items_center().p_3().rounded_lg().bg(cx.theme().background)
                .child(v_flex().flex_1().gap_0p5()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(pad.name))
                    .child(div().text_xs().text_color(muted).child(format!("{} · {battery}", if pad.bluetooth { "Bluetooth" } else { "USB" }))))
                .child(div().text_sm().text_color(if pad.xbox_slot.is_some() { green } else { muted }).child(match pad.xbox_slot {
                    Some(slot) => format!("Xbox controller #{slot}"),
                    None => "Waiting for the driver".into(),
                }))
                .into_any_element()
        }).collect();
        let connected = card("CONNECTED")
            .when(pads.is_empty(), |this| this.child(div().text_sm().text_color(muted).child(if enabled {
                "No PlayStation controller found yet. Plug it in with a USB cable, or pair it in Windows' Bluetooth settings."
            } else {
                "Turn the switch on to start using your controllers."
            })))
            .children(pads);

        let tips = card("GOOD TO KNOW")
            .children([
                "Some games also read PlayStation controllers directly and may react twice; turn off their PlayStation controller support (and Steam Input for that game) if that happens.",
                "Emulators with native PlayStation controller support (PCSX2, RPCS3, DuckStation, Xenia's PlayStation mode) don't need this, but work fine with it too.",
                "The lightbar shows Flux blue while your controller is acting as an Xbox controller.",
            ].map(|t| h_flex().gap_2().items_start()
                .child(div().mt(px(7.)).size_1p5().flex_shrink_0().rounded_full().bg(muted))
                .child(div().flex_1().min_w_0().text_sm().child(t))));

        let mapping = self.render_mapping(&status, cx);

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .max_w(px(1000.))
            .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BLACK).text_size(px(40.)).child("Controllers"))
            .child(main)
            .child(connected)
            .child(mapping)
            .child(tips)
            .overflow_y_scrollbar()
    }
}

impl ControllersPage {
    fn render_mapping(&mut self, status: &controller::Status, cx: &mut Context<Self>) -> AnyElement {
        let config = InterfaceConfig::get(cx).controller_mapping.clone();
        let theme = cx.theme();
        let (muted, border, secondary, background, primary, hover, foreground) =
            (theme.muted_foreground, theme.border, theme.secondary, theme.background, theme.primary, theme.secondary_hover, theme.foreground);
        let held = status.pads.iter().fold(0u32, |all, p| all | p.held);
        let sky: Hsla = rgb(FLUX_SKY).into();

        let chip = move |id: SharedString, label: SharedString, on: bool| div().id(id)
            .px_3().py_1p5().rounded_md().text_sm().cursor_pointer()
            .border_1().border_color(if on { primary } else { border })
            .bg(if on { primary.opacity(0.25) } else { background })
            .hover(move |s| s.bg(hover))
            .child(label);

        // Ready-made layouts.
        let presets = h_flex().flex_wrap().gap_2().items_center()
            .children(PRESETS.iter().enumerate().map(|(i, preset)| {
                chip(format!("preset-{i}").into(), preset.name.into(), false)
                    .on_click(move |_, _, cx| edit_mapping(cx, |m| (preset.apply)(m)))
            }))
            .child(div().flex_1())
            .child(Button::new("mapping-reset").small().label("Reset all").disabled(config.is_default())
                .on_click(|_, _, cx| edit_mapping(cx, |m| *m = Mapping::default())));

        // One row per PlayStation button: what it presses. Click a row to pick something else.
        let mut rows: Vec<AnyElement> = Vec::new();
        for button in PsButton::ALL {
            let target = config.target(button);
            let changed = target != button.default_target();
            let pressed = held & button.bit() != 0;
            let selected = self.selected == Some(button);
            let row = h_flex().id(SharedString::from(format!("map-{button:?}"))).gap_3().items_center().px_3().py_2().rounded_lg().cursor_pointer()
                .bg(if pressed { sky.opacity(0.22) } else if selected { hover } else { background })
                .border_1().border_color(if pressed { sky } else if selected { primary } else { border })
                .hover(move |s| s.bg(hover))
                .child(div().w(px(190.)).flex_shrink_0().text_sm().font_weight(FontWeight::SEMIBOLD).child(button.name()))
                .child(div().text_color(muted).child("→"))
                .child(div().flex_1().min_w_0().text_sm().text_color(if changed { sky } else { foreground }).child(target.name()))
                .when(changed, |this| this.child(div().text_xs().text_color(muted).child(format!("default: {}", button.default_target().name()))))
                .on_click(cx.listener(move |page, _, _, cx| {
                    page.selected = if page.selected == Some(button) { None } else { Some(button) };
                    cx.notify();
                }));
            rows.push(row.into_any_element());
            if selected {
                let picker = h_flex().flex_wrap().gap_2().p_3().rounded_lg().bg(secondary).border_1().border_color(border)
                    .children(XboxControl::ALL.into_iter().map(|control| {
                        chip(format!("pick-{button:?}-{control:?}").into(), control.name().into(), control == target)
                            .on_click(cx.listener(move |page, _, _, cx| {
                                edit_mapping(cx, |m| m.set(button, control));
                                page.selected = None;
                                cx.notify();
                            }))
                    }));
                rows.push(picker.into_any_element());
            }
        }

        // Sticks and triggers.
        let toggle = |id: &'static str, label: &'static str, on: bool, f: fn(&mut Mapping, bool)| h_flex().gap_3().items_center()
            .child(Switch::new(id).checked(on).on_click(move |on, _, cx| {
                let on = *on;
                edit_mapping(cx, |m| f(m, on))
            }))
            .child(div().text_sm().child(label));
        let stepper = |id: &'static str, label: &'static str, value: u8, max: u8, get: fn(&mut Mapping) -> &mut u8| h_flex().gap_2().items_center()
            .child(div().w(px(190.)).flex_shrink_0().text_sm().child(label))
            .child(Button::new(SharedString::from(format!("{id}-down"))).small().label("-").disabled(value == 0)
                .on_click(move |_, _, cx| edit_mapping(cx, |m| {
                    let v = get(m);
                    *v = v.saturating_sub(5);
                })))
            .child(div().w(px(48.)).text_center().text_sm().child(format!("{value}%")))
            .child(Button::new(SharedString::from(format!("{id}-up"))).small().label("+").disabled(value >= max)
                .on_click(move |_, _, cx| edit_mapping(cx, |m| {
                    let v = get(m);
                    *v = (*v + 5).min(max);
                })));
        let sticks = v_flex().gap_3().pt_2()
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Sticks and triggers"))
            .child(toggle("swap-sticks", "Swap left and right sticks", config.swap_sticks, |m, on| m.swap_sticks = on))
            .child(toggle("invert-left", "Invert left stick up/down", config.invert_left_y, |m, on| m.invert_left_y = on))
            .child(toggle("invert-right", "Invert right stick up/down (camera)", config.invert_right_y, |m, on| m.invert_right_y = on))
            .child(stepper("dz-left", "Left stick dead zone", config.left_deadzone, 40, |m| &mut m.left_deadzone))
            .child(stepper("dz-right", "Right stick dead zone", config.right_deadzone, 40, |m| &mut m.right_deadzone))
            .child(stepper("dz-trigger", "L2 / R2 dead zone", config.trigger_deadzone, 60, |m| &mut m.trigger_deadzone))
            .child(div().text_xs().text_color(muted).child("Raise a stick's dead zone if your character or camera drifts on its own."));

        v_flex().gap_3().p_5().rounded_xl().bg(secondary).border_1().border_color(border)
            .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BOLD).text_color(sky).child("BUTTON MAPPING"))
            .child(div().text_sm().text_color(muted).child(
                "What each PlayStation button does on the Xbox controller games see. Click a button to change it; \
                 buttons you press on your controller light up here. Changes apply straight away.",
            ))
            .child(presets)
            .child(v_flex().gap_1p5().children(rows))
            .child(sticks)
            .into_any_element()
    }
}
