//! The "Max FPS" card in the create-instance dialog: an ember-bordered card with a breathing glow
//! while the preset is on.

use std::time::Duration;

use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme, Icon, checkbox::Checkbox, h_flex, switch::Switch, v_flex};

use crate::icon::PandoraIcon;

pub const EMBER: u32 = 0x5682ff;
pub const EMBER_HOT: u32 = 0x7aceff;

pub fn max_fps_toggle(
    enabled: bool,
    nvidium: bool,
    on_toggle: impl Fn(&bool, &mut Window, &mut App) + 'static,
    on_nvidium: impl Fn(&bool, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let ember: Hsla = rgb(EMBER).into();

    let bolt = div()
        .flex()
        .items_center()
        .justify_center()
        .size_9()
        .rounded_lg()
        .when(enabled, |this| this.bg(linear_gradient(
            135.,
            linear_color_stop(rgb(EMBER_HOT), 0.),
            linear_color_stop(rgb(EMBER), 1.),
        )))
        .when(!enabled, |this| this.bg(theme.secondary))
        .child(Icon::new(PandoraIcon::Zap).size_5().text_color(if enabled { rgb(0xffffff).into() } else { theme.muted_foreground }));

    let card = v_flex()
        .id("max-fps-card")
        .relative()
        .gap_2()
        .p_3()
        .rounded_xl()
        .border_1()
        .border_color(if enabled { ember } else { theme.border })
        // Opaque, so the halo behind only shows around the edges.
        .bg(if enabled { theme.background.blend(ember.opacity(0.07)) } else { theme.background })
        .child(
            h_flex()
                .gap_3()
                .child(bolt)
                .child(
                    v_flex()
                        .flex_1()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child("Max FPS preset"))
                        .child(div().text_xs().text_color(theme.muted_foreground)
                            .child("Fabric + Sodium, Lithium, FerriteCore, ImmediatelyFast, culling mods, tuned JVM and options")),
                )
                .child(Switch::new("max-fps-switch").checked(enabled).on_click(on_toggle)),
        )
        .when(enabled, |this| {
            this.child(
                Checkbox::new("nvidium")
                    .checked(nvidium)
                    .label("Nvidium mesh-shader renderer (NVIDIA GTX 16xx / RTX, experimental)")
                    .on_click(on_nvidium),
            )
        });

    if !enabled {
        return card.into_any_element();
    }

    // A soft ember halo behind the card that breathes while the preset is on.
    div()
        .relative()
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded_xl()
                .shadow(vec![BoxShadow {
                    color: ember.opacity(0.35),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(18.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .with_animation(
                    "max-fps-glow",
                    Animation::new(Duration::from_millis(2400)).repeat().with_easing(pulsating_between(0.25, 1.0)),
                    |this, delta| this.opacity(delta),
                ),
        )
        .child(card)
        .into_any_element()
}
