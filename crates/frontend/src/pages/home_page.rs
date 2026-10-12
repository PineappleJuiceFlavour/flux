//! Flux's Home page: a launcher-style hero (animated scene, big pixel title, PLAY bar) with tabs for
//! the installations list and patch notes. Every other Pandora page stays reachable from the sidebar.

use std::time::Duration;

use bridge::{instance::InstanceStatus, message::MessageToBackend};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, Sizable, button::{Button, ButtonVariants}, h_flex, scroll::ScrollableElement,
    select::{Select, SelectEvent, SelectState}, v_flex,
};

use crate::{
    entity::{DataEntities, instance::InstanceEntry},
    icon::PandoraIcon,
    interface_config::InterfaceConfig,
    pages::{instances_page::{InstancesPage, VersionList}, page::Page, instance::instance_page::InstanceSubpageType},
    root,
    ui::PageType,
};

pub const PIXEL_FONT: &str = "Minecraft Default";
/// Headings outside Minecraft: a clean, heavy sans.
pub const DISPLAY_FONT: &str = "Segoe UI";
/// The Minecraft Home page always shows the Galaxy scene (pixel or smooth, per Settings).
pub const MINECRAFT_SCENE: &str = "galaxy";
pub const FLUX_SKY: u32 = 0x7aceff;
pub const FLUX_BLUE: u32 = 0x5682ff;
pub const FLUX_VIOLET: u32 = 0x7a2cff;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HomeTab {
    Play,
    Installations,
    PatchNotes,
}

pub struct HomePage {
    data: DataEntities,
    tab: HomeTab,
    instances_page: Option<Entity<InstancesPage>>,
    instance_select: Entity<SelectState<VersionList>>,
    listed_names: Vec<SharedString>,
}

impl HomePage {
    pub fn new(data: &DataEntities, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let instance_select = cx.new(|cx| SelectState::new(VersionList::default(), None, window, cx).searchable(true));
        cx.subscribe(&instance_select, |_, _, event: &SelectEvent<VersionList>, cx| {
            if let SelectEvent::Confirm(Some(name)) = event {
                InterfaceConfig::get_mut(cx).home_instance = Some(name.clone());
                cx.notify();
            }
        }).detach();
        cx.observe(&data.instances, |_, _, cx| cx.notify()).detach();

        Self { data: data.clone(), tab: HomeTab::Play, instances_page: None, instance_select, listed_names: Vec::new() }
    }

    fn selected_instance(&self, cx: &App) -> Option<InstanceEntry> {
        let entries = &self.data.instances.read(cx).entries;
        let wanted = InterfaceConfig::get(cx).home_instance.clone();
        let visible = |e: &InstanceEntry| e.name != schema::quickplay::INSTANCE_NAME;
        wanted
            .and_then(|name| entries.values().map(|e| e.read(cx)).find(|e| e.name == name && visible(e)).cloned())
            .or_else(|| entries.values().map(|e| e.read(cx)).find(|e| visible(e)).cloned())
    }

    fn sync_instance_select(&mut self, selected: Option<&SharedString>, window: &mut Window, cx: &mut Context<Self>) {
        let names: Vec<SharedString> = self.data.instances.read(cx).entries.values()
            .map(|e| e.read(cx).name.clone())
            .filter(|n| n != schema::quickplay::INSTANCE_NAME)
            .collect();
        if names != self.listed_names {
            self.listed_names = names.clone();
            self.instance_select.update(cx, |select, cx| {
                select.set_items(VersionList { versions: names.clone(), matched_versions: names }, window, cx);
            });
        }
        if let Some(selected) = selected {
            self.instance_select.update(cx, |select, cx| {
                if select.selected_value() != Some(selected) {
                    select.set_selected_value(selected, window, cx);
                }
            });
        }
    }

    fn tab_button(&self, tab: HomeTab, label: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.tab == tab;
        let accent: Hsla = cx.theme().primary;
        v_flex()
            .id(label)
            .cursor_pointer()
            .pt_1()
            .gap_1()
            .child(div().px_1().text_sm().font_weight(FontWeight::SEMIBOLD)
                .text_color(if active { cx.theme().foreground } else { cx.theme().muted_foreground })
                .child(label))
            .child(div().h(px(2.)).rounded_full().when(active, |this| this.bg(accent)))
            .hover(|this| this.text_color(cx.theme().foreground))
            // The title bar is a window-drag area: claim the press so it doesn't start a window move.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.tab = tab;
                cx.notify();
            }))
    }

    fn render_play(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected_instance(cx);
        self.sync_instance_select(selected.as_ref().map(|s| &s.name), window, cx);

        // The page fills the window minus the sidebar and the title bar.
        let viewport = window.viewport_size();
        let sidebar = 0.0f32; // no sidebar: the top bar holds navigation
        let size = size(viewport.width - px(sidebar), viewport.height - px(44.));

        let (title, subtitle) = match &selected {
            Some(instance) => {
                let config = &instance.configuration;
                (instance.name.to_uppercase(), format!("Minecraft {} · {:?}", config.minecraft_version, config.loader))
            },
            None => ("FLUX".to_string(), "Create an installation to start playing".to_string()),
        };
        let words: Vec<&str> = title.split_whitespace().collect();
        let (head, last) = match words.split_last() {
            Some((last, head)) if !head.is_empty() => (head.join(" "), last.to_string()),
            _ => (String::new(), title.clone()),
        };

        let scene = crate::backgrounds::scene(MINECRAFT_SCENE);
        let hero_title = v_flex()
            .absolute()
            .top(px(36.))
            .left(px(40.))
            .max_w(px(820.))
            .gap_3()
            .child(
                pixel_title(&head, &last, 64.)
                    .with_animation(
                        "home-title",
                        Animation::new(Duration::from_millis(700)).with_easing(ease_out_quint()),
                        |this, delta| this.opacity(delta).mt(px(18. * (1. - delta))),
                    ),
            )
            .child(div().text_sm().font_weight(FontWeight::MEDIUM).text_color(gpui::white().opacity(0.85)).child(subtitle))
            .child(div().text_xs().text_color(gpui::white().opacity(0.55)).child(scene.tagline));

        let scrim = div().absolute().inset_0().bg(linear_gradient(
            180.,
            linear_color_stop(rgb(0x05060a).opacity(0.55), 0.),
            linear_color_stop(rgb(0x05060a).opacity(0.0), 0.45),
        ));

        let bottom = self.render_play_bar(selected, cx);

        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .child(crate::backgrounds::render_scene(size, MINECRAFT_SCENE, cx))
            .child(scrim)
            .child(hero_title)
            .child(bottom)
            .into_any_element()
    }

    fn render_play_bar(&mut self, selected: Option<InstanceEntry>, cx: &mut Context<Self>) -> AnyElement {
        let status = selected.as_ref().map(|s| s.status).unwrap_or(InstanceStatus::NotRunning);
        let (label, status_text, status_color) = match (&selected, status) {
            (None, _) => ("CREATE", "No installations yet".to_string(), cx.theme().muted_foreground),
            (Some(_), InstanceStatus::NotRunning) => ("PLAY", "Ready to play".to_string(), rgb(0x5ee38a).into()),
            (Some(_), InstanceStatus::Launching) => ("LAUNCHING", "Starting the game...".to_string(), rgb(FLUX_SKY).into()),
            (Some(_), InstanceStatus::Running) => ("STOP", "Playing now".to_string(), rgb(FLUX_SKY).into()),
            (Some(_), InstanceStatus::Stopping) => ("STOPPING", "Closing the game...".to_string(), cx.theme().muted_foreground),
        };
        let status_line = match &selected {
            Some(instance) => format!("{status_text} · {}", crate::privacy::path(&instance.dot_minecraft_folder)),
            None => status_text,
        };

        let play = {
            let data = self.data.clone();
            let selected = selected.clone();
            play_button(label, move |window, cx| {
                match &selected {
                    None => {
                        crate::modals::create_instance::open_create_instance(data.metadata.clone(), data.instances.clone(),
                            data.backend_handle.clone(), window, cx);
                    },
                    Some(instance) => match instance.status {
                        InstanceStatus::NotRunning => root::start_instance(instance.id, instance.name.clone(), None, &data, window, cx),
                        InstanceStatus::Running => data.backend_handle.send(MessageToBackend::KillInstance { id: instance.id }),
                        _ => {},
                    },
                }
            })
        };

        let open_subpage = |subpage: InstanceSubpageType, name: Option<SharedString>| {
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                if let Some(name) = &name {
                    InterfaceConfig::get_mut(cx).instance_subpage = subpage;
                    root::switch_page(PageType::InstancePage { name: name.clone() }, &[PageType::Home], window, cx);
                }
            }
        };
        let name = selected.as_ref().map(|s| s.name.clone());
        let folder = selected.as_ref().map(|s| s.dot_minecraft_folder.clone());

        h_flex()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .h(px(104.))
            .px_6()
            .gap_6()
            .items_center()
            .bg(rgb(0x07080d).opacity(0.78))
            .border_t_1()
            .border_color(gpui::white().opacity(0.08))
            .child(
                div().w(px(300.)).child(
                    Select::new(&self.instance_select)
                        .placeholder("Choose an installation")
                        .search_placeholder("Search installations")
                        .disabled(self.listed_names.is_empty()),
                ),
            )
            .child(
                v_flex().flex_1().items_center().gap_1p5()
                    .child(play)
                    .child(div().text_xs().text_color(status_color).overflow_hidden().whitespace_nowrap().text_ellipsis().max_w(px(520.)).child(status_line)),
            )
            .child(
                h_flex().w(px(300.)).justify_end().gap_2()
                    .child(Button::new("home-mods").label("Mods").icon(PandoraIcon::Box).small()
                        .disabled(name.is_none()).on_click(open_subpage(InstanceSubpageType::Mods, name.clone())))
                    .child(Button::new("home-log").label("Log").small()
                        .disabled(name.is_none()).on_click(open_subpage(InstanceSubpageType::Logs, name.clone())))
                    .child(Button::new("home-folder").label("Folder").small()
                        .disabled(folder.is_none())
                        .on_click(move |_, window, cx| {
                            if let Some(folder) = &folder {
                                crate::open_folder(folder, window, cx);
                            }
                        })),
            )
            .into_any_element()
    }

    fn render_patch_notes(&self, cx: &mut Context<Self>) -> AnyElement {
        let section = |date: &'static str, items: &'static [&'static str], cx: &App| {
            v_flex()
                .gap_2()
                .p_5()
                .rounded_xl()
                .bg(cx.theme().secondary)
                .border_1()
                .border_color(cx.theme().border)
                .child(div().font_family(PIXEL_FONT).text_color(rgb(FLUX_SKY)).child(date))
                .children(items.iter().map(|item| h_flex().gap_2().items_start()
                    .child(div().mt(px(7.)).size_1p5().flex_shrink_0().rounded_full().bg(cx.theme().muted_foreground))
                    .child(div().flex_1().min_w_0().text_sm().child(*item))))
        };
        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .max_w(px(900.))
            .child(section("OCTOBER 10, 2026", &[
                "Flux is born: a fork of Pandora Launcher with every Pandora feature kept (instances, Modrinth and CurseForge, syncing, quick play, skins, imports, sandboxing)",
                "Max FPS preset: one switch builds a Fabric instance with Sodium, Lithium, FerriteCore, ImmediatelyFast, Entity Culling, More Culling, Dynamic FPS and ModernFix, picked per Minecraft version",
                "Tuned G1 JVM flags, 4 GB heap and an FPS-first options.txt (no VSync, unlimited frame rate)",
                "Optional Nvidium mesh-shader renderer for NVIDIA GTX 16xx / RTX",
                "The Flux companion mod: a repeatable benchmark with average FPS, 1% and 0.1% lows",
                "New home: animated scenes in pixel-art and smooth styles, accent colors and your own picture",
            ], cx))
            .overflow_y_scrollbar()
            .into_any_element()
    }
}

/// A big pixel-font title whose last word is in the accent sky blue, with a hard drop shadow. One
/// text run, so the title and its shadow always wrap the same way.
pub(crate) fn pixel_title(head: &str, last: &str, size: f32) -> Div {
    let full = if head.is_empty() { last.to_string() } else { format!("{head} {last}") };
    let accent = full.len() - last.len()..full.len();
    let styled = StyledText::new(full.clone()).with_highlights([(accent, HighlightStyle { color: Some(rgb(FLUX_SKY).into()), ..Default::default() })]);
    div()
        .relative()
        .font_family(PIXEL_FONT)
        .text_size(px(size))
        .line_height(px(size * 1.1))
        .child(div().absolute().top(px(5.)).left(px(5.)).w_full().text_color(rgb(0x0a0718).opacity(0.85)).child(full))
        .child(div().relative().text_color(gpui::white()).child(styled))
}

/// The big PLAY button: logo gradient, a breathing glow and a light sweep across it.
/// A game's name as a big title (when it has no logo): heavy sans, the last word in Flux blue.
pub(crate) fn display_title(head: &str, last: &str, size: f32) -> Div {
    let full = if head.is_empty() { last.to_string() } else { format!("{head} {last}") };
    let accent = full.len() - last.len()..full.len();
    let styled = StyledText::new(full).with_highlights([(accent, HighlightStyle { color: Some(rgb(FLUX_SKY).into()), ..Default::default() })]);
    div()
        .font_family(DISPLAY_FONT)
        .font_weight(FontWeight::BLACK)
        .text_size(px(size))
        .line_height(px(size * 1.08))
        .text_color(gpui::white())
        .child(styled)
}

pub(crate) fn play_button(label: &'static str, on_click: impl Fn(&mut Window, &mut App) + 'static) -> AnyElement {
    let blue: Hsla = rgb(FLUX_BLUE).into();
    div()
        .relative()
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded_lg()
                .shadow(vec![BoxShadow { color: blue.opacity(0.55), offset: point(px(0.), px(0.)), blur_radius: px(22.), spread_radius: px(0.), inset: false }])
                .with_animation(
                    "play-glow",
                    Animation::new(Duration::from_millis(2200)).repeat().with_easing(pulsating_between(0.35, 1.0)),
                    |this, delta| this.opacity(delta),
                ),
        )
        .child(
            div()
                .id("home-play")
                .relative()
                .overflow_hidden()
                .w(px(300.))
                .h(px(58.))
                .rounded_lg()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .bg(linear_gradient(100., linear_color_stop(rgb(FLUX_SKY), 0.), linear_color_stop(rgb(FLUX_VIOLET), 1.)))
                .border_b_4()
                .border_color(rgb(0x3a1d99))
                .hover(|this| this.opacity(0.92))
                .active(|this| this.mt(px(2.)).border_b_2())
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .w(px(60.))
                        .bg(linear_gradient(90., linear_color_stop(gpui::white().opacity(0.), 0.), linear_color_stop(gpui::white().opacity(0.35), 0.5)))
                        .with_animation(
                            "play-shine",
                            Animation::new(Duration::from_millis(3200)).repeat(),
                            |this, delta| {
                                let t = (delta / 0.35).min(1.);
                                this.left(px(-80. + t * 460.))
                            },
                        ),
                )
                .child(div().font_family(DISPLAY_FONT).font_weight(FontWeight::BLACK).text_size(px(26.)).text_color(gpui::white()).child(label))
                .on_click(move |_, window, cx| on_click(window, cx)),
        )
        .into_any_element()
}

impl Page for HomePage {
    fn controls(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_5()
            .child(self.tab_button(HomeTab::Play, "Play", cx))
            .child(self.tab_button(HomeTab::Installations, "Installations", cx))
            .child(self.tab_button(HomeTab::PatchNotes, "Patch Notes", cx))
    }

    fn scrollable(&self, _cx: &App) -> bool {
        false
    }
}

impl Render for HomePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.tab {
            HomeTab::Play => self.render_play(window, cx),
            HomeTab::Installations => {
                let data = self.data.clone();
                let page = self.instances_page.get_or_insert_with(|| cx.new(|cx| InstancesPage::new(&data, window, cx))).clone();
                let controls = page.update(cx, |page, cx| page.controls(window, cx).into_any_element());
                v_flex()
                    .size_full()
                    .child(h_flex().px_4().py_3().border_b_1().border_color(cx.theme().border).child(controls))
                    .child(div().flex_1().overflow_hidden().child(v_flex().size_full().overflow_y_scrollbar().child(page)))
                    .into_any_element()
            },
            HomeTab::PatchNotes => self.render_patch_notes(cx),
        };
        div().size_full().child(content)
    }
}
