//! Flux's startup splash: the bolt ignites, the wordmark slides in, a heat bar sweeps across and
//! the whole overlay fades out. Drawn over the launcher for `SPLASH_DURATION`, then never again.

use std::time::Duration;

use gpui::{prelude::*, *};
use gpui_component::{h_flex, v_flex};

use crate::component::max_fps_toggle::{EMBER, EMBER_HOT};

pub const SPLASH_DURATION: Duration = Duration::from_millis(1900);

/// Fraction of the splash (0..1) at which the fade-out starts.
const FADE_START: f32 = 0.72;

fn segment(delta: f32, start: f32, end: f32) -> f32 {
    ((delta - start) / (end - start)).clamp(0., 1.)
}

fn ease_out_cubic(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

pub fn splash() -> impl IntoElement {
    let ember: Hsla = rgb(EMBER).into();

    div()
        .id("flux-splash")
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(0x07080c))
        .child(
            v_flex()
                .items_center()
                .gap_5()
                .child(
                    h_flex()
                        .gap_4()
                        .items_center()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .size_16()
                                .rounded_2xl()
                                .shadow(vec![BoxShadow {
                                    color: ember.opacity(0.45),
                                    offset: point(px(0.), px(0.)),
                                    blur_radius: px(40.),
                                    spread_radius: px(-6.),
                                    inset: false,
                                }])
                                .child(img(ImageSource::Resource(Resource::Embedded("images/branding/logo.png".into())))
                                    .size_full().object_fit(ObjectFit::Contain))
                                .with_animation(
                                    "splash-bolt",
                                    Animation::new(SPLASH_DURATION),
                                    |this, delta| {
                                        // Pop in with a slight overshoot.
                                        let t = segment(delta, 0., 0.22);
                                        let scale = if t < 1. { 0.4 + 0.75 * ease_out_cubic(t) } else { 1.15 - 0.15 * segment(delta, 0.22, 0.32) };
                                        let size = px(72. * scale);
                                        this.size(size).opacity(t.max(0.0001))
                                    },
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(48.))
                                .font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BOLD)
                                .text_color(rgb(0xeef1f8))
                                .child("FLUX")
                                .with_animation(
                                    "splash-wordmark",
                                    Animation::new(SPLASH_DURATION),
                                    |this, delta| {
                                        let t = ease_out_cubic(segment(delta, 0.12, 0.38));
                                        this.ml(px(24. * (1. - t))).opacity(t.max(0.0001))
                                    },
                                ),
                        ),
                )
                .child(
                    // The heat bar: an ember gradient that sweeps to full width.
                    div()
                        .w(px(320.))
                        .h(px(4.))
                        .rounded_full()
                        .bg(rgb(0x151924))
                        .overflow_hidden()
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .bg(linear_gradient(90., linear_color_stop(rgb(EMBER), 0.), linear_color_stop(rgb(0x22d3ee), 1.)))
                                .with_animation(
                                    "splash-heat",
                                    Animation::new(SPLASH_DURATION),
                                    |this, delta| {
                                        let t = ease_out_cubic(segment(delta, 0.25, 0.68));
                                        this.w(px(320. * t))
                                    },
                                ),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(0x8a93a8))
                        .child("more frames, less waiting")
                        .with_animation(
                            "splash-caption",
                            Animation::new(SPLASH_DURATION),
                            |this, delta| this.opacity(segment(delta, 0.3, 0.5).max(0.0001)),
                        ),
                ),
        )
        .with_animation(
            "splash-fade",
            Animation::new(SPLASH_DURATION),
            |this, delta| this.opacity((1. - ease_out_cubic(segment(delta, FADE_START, 1.))).max(0.0001)),
        )
}
