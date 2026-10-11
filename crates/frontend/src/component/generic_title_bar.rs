use gpui::{prelude::FluentBuilder, *};
use gpui_component::{ActiveTheme, Colorize, InteractiveElementExt, h_flex};

use crate::icon::PandoraIcon;

#[derive(IntoElement)]
pub struct TitleBar {
    pub left_content: Vec<AnyElement>,
    pub right_content: Vec<AnyElement>,
    pub content_only: bool,
}

#[derive(Default)]
pub(crate) struct TitleBarState {
    pub(crate) should_move: bool,
}

impl RenderOnce for TitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state("title-bar-state", cx, |_, _| TitleBarState::default());

        let base = h_flex()
            .id("bar")
            .w_full()
            .min_h(rems(3.5625))
            .max_h(rems(3.5625))
            .h(rems(3.5625))
            .p_4()
            .border_b_1()
            .border_color(cx.theme().border)
            .text_xl();

        if self.content_only {
            return base.child(h_flex()
                .left_2()
                .w_full()
                .child(h_flex().h_full().gap_1()
                    .flex_1().overflow_hidden()
                    .children(self.left_content))
                .child(h_flex().h_full().gap_1()
                    .flex_shrink_0()
                    .children(self.right_content)));
        }

        let window_controls = window.window_controls();

        base
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down_out(window.listener_for(&state, |state, _, _, _| {
                state.should_move = false;
            }))
            .when(cfg!(target_os = "linux"), |this| {
                this.on_double_click(|_, window, _| window.zoom_window())
            })
            .when(cfg!(target_os = "macos"), |this| {
                this.on_double_click(|_, window, _| window.titlebar_double_click())
            })
            .on_mouse_down(
                MouseButton::Left,
                window.listener_for(&state, |state, _, _, _| {
                    state.should_move = true;
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                window.listener_for(&state, |state, _, _, _| {
                    state.should_move = false;
                }),
            )
            .on_mouse_move(window.listener_for(&state, |state, _, window, _| {
                if state.should_move {
                    state.should_move = false;
                    window.start_window_move();
                }
            }))
            .child(h_flex()
                .left_2()
                .w_full()
                .on_any_mouse_down(stop_propagation_if_default_prevented)
                .child(h_flex().h_full().gap_1()
                    .flex_1().overflow_hidden()
                    .on_any_mouse_down(stop_propagation_if_default_prevented)
                    .children(self.left_content))
                .child(h_flex().h_full().gap_1()
                    .flex_shrink_0()
                    .on_any_mouse_down(stop_propagation_if_default_prevented)
                    .children(self.right_content)
                    .when(!cfg!(target_os = "macos"), |this| {
                        this
                            .when(window_controls.minimize, |this| this.child(WindowControl::Minimize))
                            .when(window_controls.maximize, |this| this.child(if window.is_maximized() {
                                WindowControl::Restore
                            } else {
                                WindowControl::Maximize
                            }))
                            .child(WindowControl::Close)
                    })))
    }
}

fn stop_propagation_if_default_prevented(_: &MouseDownEvent, window: &mut Window, cx: &mut App) {
    if window.default_prevented() {
        cx.stop_propagation();
    }
}

#[derive(IntoElement, Clone, Copy, PartialEq, Eq)]
pub enum WindowControl {
    Minimize,
    Maximize,
    Restore,
    Close,
}

impl RenderOnce for WindowControl {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let base = h_flex()
            .id(match self {
                WindowControl::Minimize => "minimize",
                WindowControl::Maximize => "maximize",
                WindowControl::Restore => "restore",
                WindowControl::Close => "close",
            })
            .occlude()
            .window_control_area(match self {
                WindowControl::Minimize => WindowControlArea::Min,
                WindowControl::Maximize | WindowControl::Restore => WindowControlArea::Max,
                WindowControl::Close => WindowControlArea::Close,
            })
            .size_8()
            .justify_center()
            .content_center()
            .rounded(cx.theme().radius)
            .hover(|this| {
                let col = if self == WindowControl::Close {
                    cx.theme().danger_hover
                } else if cx.theme().mode.is_dark() {
                    cx.theme().secondary.lighten(0.1).opacity(0.8)
                } else {
                    cx.theme().secondary.darken(0.1).opacity(0.8)
                };
                this.bg(col)
            });

        #[cfg(windows)]
        return base
            .font_family(*WINDOWS_ICON_FONT)
            .text_size(px(10.0))
            .child(match self {
                WindowControl::Minimize => "\u{e921}",
                WindowControl::Maximize => "\u{e922}",
                WindowControl::Restore => "\u{e923}",
                WindowControl::Close => "\u{e8bb}",
            });

        #[cfg(not(windows))]
        return base
            .on_click(move |_, window, _| {
                match self {
                    WindowControl::Minimize => window.minimize_window(),
                    WindowControl::Maximize | WindowControl::Restore => window.zoom_window(),
                    WindowControl::Close => window.remove_window(),
                }
            }).child(match self {
                WindowControl::Minimize => PandoraIcon::WindowMinimize,
                WindowControl::Maximize => PandoraIcon::WindowMaximize,
                WindowControl::Restore => PandoraIcon::WindowRestore,
                WindowControl::Close => PandoraIcon::WindowClose,
            });
    }
}

#[cfg(windows)]
static WINDOWS_ICON_FONT: once_cell::sync::Lazy<&'static str> = once_cell::sync::Lazy::new(|| {
    let mut version = unsafe { std::mem::zeroed() };
    let status = unsafe {
        windows::Wdk::System::SystemServices::RtlGetVersion(&mut version)
    };

    if status.is_ok() && version.dwBuildNumber >= 22000 {
        // Windows 11
        "Segoe Fluent Icons"
    } else {
        // Windows 10 and prior
        "Segoe MDL2 Assets"
    }
});
