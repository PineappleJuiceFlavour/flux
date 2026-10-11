use gpui::{
    App, Hsla, IntoElement, ParentElement, RenderOnce, Styled, Window, div, prelude::FluentBuilder, px, relative,
};
use gpui_component::ActiveTheme;

#[derive(Default)]
pub enum ProgressBarColor {
    #[default]
    Normal,
    Error,
    Success,
}

#[derive(IntoElement)]
pub struct ProgressBar {
    pub amount: f32,
    pub color_scale: f32,
    pub color: ProgressBarColor,
}

impl Default for ProgressBar {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgressBar {
    pub fn new() -> Self {
        Self {
            amount: 0.0,
            color_scale: 1.0,
            color: ProgressBarColor::Normal,
        }
    }
}

fn lerp(from: Hsla, to: Hsla, amount: f32) -> Hsla {
    if amount <= 0.0 {
        from
    } else if amount >= 1.0 {
        to
    } else {
        let mut hue_delta = to.h - from.h;
        if hue_delta < -0.5 {
            hue_delta += 1.0;
        } else if hue_delta > 0.5 {
            hue_delta -= 1.0;
        }
        let mut hue = from.h + hue_delta * amount;
        if hue < 0.0 {
            hue += 1.0;
        } else if hue > 1.0 {
            hue -= 1.0;
        }
        Hsla {
            h: hue,
            s: from.s + (to.s - from.s) * amount,
            l: from.l + (to.l - from.l) * amount,
            a: from.a + (to.a - from.a) * amount,
        }
    }
}

impl RenderOnce for ProgressBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        // Match the theme radius, if theme radius is zero use it.
        let radius = px(4.0).min(cx.theme().radius);
        let relative_w = relative(self.amount);

        let progress_bar_color = cx.theme().progress_bar;
        let color = match self.color {
            ProgressBarColor::Normal => progress_bar_color,
            ProgressBarColor::Error => lerp(progress_bar_color, cx.theme().red, self.color_scale),
            ProgressBarColor::Success => lerp(progress_bar_color, cx.theme().green, self.color_scale),
        };

        div().w_full().relative().h(px(8.0)).rounded(radius).bg(color.opacity(0.2)).child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .h_full()
                .w(relative_w)
                .bg(color)
                .map(|this| match self.amount {
                    v if v >= 1.0 => this.rounded(radius),
                    _ => this.rounded_l(radius),
                }),
        )
    }
}
