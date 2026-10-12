use std::rc::Rc;

use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme, Selectable, ThemeRegistry, button::{Button, ButtonGroup, ButtonVariants}, h_flex, select::{Select, SelectEvent}, v_flex};

use crate::{interface_config::BackgroundStyle, component::named_dropdown::{DropdownName, NamedDropdownItem, SearchableNamedDropdown}, entity::DataEntities, icon::PandoraIcon, interface_config::InterfaceConfig, settings::{SettingGroup, SettingItem, SettingItemWidget, SettingPage}};

pub(super) fn create_page(data: &DataEntities, window: &mut Window, cx: &mut App) -> SettingPage {
    SettingPage {
        title: t::settings::appearance,
        groups: vec![
            SettingGroup {
                title: Some(|| "Flux"),
                items: vec![
                    SettingItem {
                        title: || "Accent color",
                        description: || "Buttons, highlights and the active tab",
                        widget: SettingItemWidget::Any(Rc::new(|_, cx| accent_swatches(cx))),
                        ..Default::default()
                    },
                    SettingItem {
                        title: || "Art style",
                        description: || "Pixel art or smooth versions of the scenes (Minecraft's Galaxy and the game backgrounds)",
                        widget: SettingItemWidget::Any(Rc::new(|_, cx| {
                            let style = InterfaceConfig::get(cx).background_style;
                            ButtonGroup::new("flux-art-style")
                                .outline()
                                .child(Button::new("style-pixel").label("Pixel").selected(style == BackgroundStyle::Pixel))
                                .child(Button::new("style-smooth").label("Smooth").selected(style == BackgroundStyle::Smooth))
                                .on_click(|selected: &Vec<usize>, _, cx| {
                                    InterfaceConfig::get_mut(cx).background_style = match selected.first() {
                                        Some(1) => BackgroundStyle::Smooth,
                                        _ => BackgroundStyle::Pixel,
                                    };
                                    cx.refresh_windows();
                                })
                                .into_any_element()
                        })),
                        ..Default::default()
                    },
                    SettingItem {
                        title: || "Animations",
                        description: || "Twinkling stars, drifting clouds, the skater and the car",
                        widget: SettingItemWidget::Switch(|cfg| cfg.animations, |cfg, val| cfg.animations = val),
                        ..Default::default()
                    },
                ].into(),
                searched_items: None
            },
            SettingGroup {
                title: Some(t::settings::appearance::theme),
                items: vec![
                    SettingItem {
                        title: t::settings::appearance::theme::base_theme,
                        description: t::settings::appearance::theme::base_theme_desc,
                        widget: create_theme_dropdown(window, cx),
                        ..Default::default()
                    },
                    SettingItem {
                        title: t::settings::appearance::theme::open_theme_folder,
                        description: t::settings::appearance::theme::open_theme_folder_desc,
                        widget: SettingItemWidget::Any(Rc::new({
                            let theme_folder = data.theme_folder.clone();
                            move |_, _| {
                                Button::new("open-theme-folder").info().icon(PandoraIcon::FolderOpen).label(t::settings::appearance::theme::open_theme_folder()).on_click({
                                    let theme_folder = theme_folder.clone();
                                    move |_, window, cx| {
                                        crate::open_folder(&theme_folder, window, cx);
                                    }
                                }).into_any_element()
                            }
                        })),
                        ..Default::default()
                    },
                    SettingItem {
                        title: t::settings::appearance::theme::font,
                        description: t::settings::appearance::theme::font_desc,
                        widget: create_font_dropdown(window, cx),
                        ..Default::default()
                    },
                    SettingItem {
                        title: t::settings::appearance::theme::font_size,
                        description: t::settings::appearance::theme::font_size_desc,
                        widget: SettingItemWidget::Integer {
                            get: |cx| cx.theme().font_size.as_f32().round() as i32,
                            set: |val, cx| {
                                InterfaceConfig::get_mut(cx).set_font_size(val);
                                InterfaceConfig::apply_theme(cx, true);
                            },
                            min: Some(8),
                            max: Some(32),
                        },
                        ..Default::default()
                    },
                ].into(),
                searched_items: None
            },
            SettingGroup {
                title: Some(t::settings::appearance::window),
                items: vec![
                    SettingItem {
                        title: t::settings::appearance::window::use_os_titlebar,
                        description: t::settings::appearance::window::use_os_titlebar_desc,
                        widget: SettingItemWidget::Switch(|cfg| cfg.use_os_titlebar, |cfg, val| cfg.use_os_titlebar = val),
                        ..Default::default()
                    },
                    SettingItem {
                        title: t::settings::appearance::window::hide_main_window,
                        description: t::settings::appearance::window::hide_main_window_desc,
                        widget: SettingItemWidget::Switch(|cfg| cfg.hide_main_window_on_launch, |cfg, val| cfg.hide_main_window_on_launch = val),
                        ..Default::default()
                    },
                    SettingItem {
                        title: t::settings::appearance::window::quit_when_main_window_closed,
                        description: t::settings::appearance::window::quit_when_main_window_closed_desc,
                        widget: SettingItemWidget::Switch(|cfg| cfg.quit_on_main_closed, |cfg, val| cfg.quit_on_main_closed = val),
                        ..Default::default()
                    },
                ].into(),
                searched_items: None
            },
            SettingGroup {
                title: Some(t::settings::appearance::sidebar),
                items: vec![
                    SettingItem {
                        title: t::settings::appearance::sidebar::show_sidebar_icons,
                        description: t::settings::appearance::sidebar::show_sidebar_icons_desc,
                        widget: SettingItemWidget::Switch(|cfg| cfg.show_sidebar_icons, |cfg, val| cfg.show_sidebar_icons = val),
                        ..Default::default()
                    },
                    SettingItem {
                        title: t::settings::appearance::sidebar::show_quickplay_page,
                        description: t::settings::appearance::sidebar::show_quickplay_page_desc,
                        widget: SettingItemWidget::Switch(|cfg| cfg.show_quickplay_page, |cfg, val| cfg.show_quickplay_page = val),
                        ..Default::default()
                    },
                ].into(),
                searched_items: None
            },
        ].into(),
        searched_groups: None
    }
}

fn create_theme_dropdown(window: &mut Window, cx: &mut App) -> SettingItemWidget {
    let themes = ThemeRegistry::global(cx).sorted_themes()
        .iter()
        .map(|theme| NamedDropdownItem {
            name: DropdownName::Literal(theme.name.clone()),
            item: theme.name.clone()
        })
        .collect();

    let initial = cx.theme().theme_name().clone();
    let dropdown = SearchableNamedDropdown::create_and_select(themes, initial, window, cx);

    cx.subscribe(&dropdown, |_, event: &SelectEvent<_>, cx| {
        let SelectEvent::Confirm(Some(theme_name)) = event else {
            return
        };
        InterfaceConfig::get_mut(cx).set_active_theme(theme_name.clone());
        InterfaceConfig::apply_theme(cx, true);
    }).detach();

    SettingItemWidget::Any(Rc::new(move |_, _| {
        Select::new(&dropdown).menu_width(px(200.0)).into_any_element()
    }))
}

fn create_font_dropdown(window: &mut Window, cx: &mut App) -> SettingItemWidget {
    let fonts = cx.text_system().all_font_names()
        .iter().filter(|name| !name.starts_with('.')).map(|font_name| {
            let font_name = SharedString::from(font_name);
            NamedDropdownItem { name: font_name.clone().into(), item: font_name }
        }).collect();

    let initial = cx.theme().font_family.clone();
    let dropdown = SearchableNamedDropdown::create_and_select(fonts, initial, window, cx);

    cx.subscribe(&dropdown, |_, event: &SelectEvent<_>, cx| {
        let SelectEvent::Confirm(Some(font_family)) = event else {
            return
        };

        InterfaceConfig::get_mut(cx).set_font_family(font_family.clone());
        InterfaceConfig::apply_theme(cx, true);
    }).detach();

    SettingItemWidget::Any(Rc::new(move |_, _| {
        Select::new(&dropdown).menu_width(px(200.0)).into_any_element()
    }))
}

const ACCENTS: &[(u32, &str)] = &[
    (0x5682ff, "Flux"),
    (0x3fa34d, "Green"),
    (0x2d9cdb, "Blue"),
    (0xe0a526, "Gold"),
    (0xd64545, "Red"),
    (0x9b51e0, "Purple"),
    (0xff7a3d, "Orange"),
    (0xff5fa2, "Pink"),
];

fn accent_swatches(cx: &mut Context<super::SettingsRoot>) -> AnyElement {
    let current = InterfaceConfig::get(cx).accent_color.unwrap_or(ACCENTS[0].0);
    h_flex()
        .gap_2()
        .children(ACCENTS.iter().map(|(color, name)| {
            let color = *color;
            let selected = color == current;
            div()
                .id(*name)
                .size_8()
                .rounded_md()
                .cursor_pointer()
                .bg(rgb(color))
                .border_2()
                .border_color(if selected { gpui::white() } else { gpui::transparent_black() })
                .child(div().absolute().bottom_0().left_0().right_0().h(px(5.)).rounded_b_md().bg(gpui::black().opacity(0.25)))
                .relative()
                .hover(|this| this.opacity(0.85))
                .on_click(move |_, _, cx| {
                    InterfaceConfig::get_mut(cx).accent_color = if color == ACCENTS[0].0 { None } else { Some(color) };
                    crate::pages::home_page::set_accent(InterfaceConfig::get(cx).accent_color);
                    InterfaceConfig::apply_theme(cx, true);
                })
        }))
        .into_any_element()
}

