//! Minecraft › Instances: the instance you last played as a big card with PLAY, then every instance
//! as a card with its own play button, and shortcuts to the other Minecraft sections.

use std::collections::HashSet;

use bridge::{instance::{ContentFolder, InstanceID, InstanceStatus}, message::MessageToBackend};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Icon, IndexPath, Sizable, button::{Button, ButtonVariants}, h_flex, menu::{ContextMenuExt, DropdownMenu, PopupMenuItem},
    select::{SelectDelegate, SelectItem}, v_flex,
};
use schema::loader::Loader;

use crate::{
    entity::{DataEntities, account::AccountExt, instance::{InstanceAddedEvent, InstanceEntry, InstanceModifiedEvent, InstanceMovedToTopEvent, InstanceRemovedEvent}},
    icon::PandoraIcon, interface_config::InterfaceConfig, pages::page::Page, png_render_cache, root, ui::PageType,
};

const GREEN: u32 = 0x4caf72;

pub struct InstancesPage {
    data: DataEntities,
    /// Show only instances using this loader.
    filter: Option<Loader>,
    /// Instances whose mod list we've asked for (for the "42 mods" counts).
    watched: HashSet<InstanceID>,
    _subscriptions: Vec<Subscription>,
}

impl InstancesPage {
    pub fn new(data: &DataEntities, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let instances = data.instances.clone();
        let _subscriptions = vec![
            cx.subscribe(&instances, |_, _, _: &InstanceAddedEvent, cx| cx.notify()),
            cx.subscribe(&instances, |_, _, _: &InstanceRemovedEvent, cx| cx.notify()),
            cx.subscribe(&instances, |_, _, _: &InstanceModifiedEvent, cx| cx.notify()),
            cx.subscribe(&instances, |_, _, _: &InstanceMovedToTopEvent, cx| cx.notify()),
        ];
        Self { data: data.clone(), filter: None, watched: HashSet::new(), _subscriptions }
    }

    fn entries(&self, cx: &App) -> Vec<InstanceEntry> {
        self.data.instances.read(cx).entries.values()
            .map(|e| e.read(cx).clone())
            .filter(|e| e.name != schema::quickplay::INSTANCE_NAME)
            .collect()
    }

    /// Load each instance's mod list once, and redraw when it arrives.
    fn watch_mods(&mut self, entries: &[InstanceEntry], cx: &mut Context<Self>) {
        for entry in entries {
            if self.watched.insert(entry.id) {
                entry.content_states.observe(ContentFolder::Mods);
                let sub = cx.observe(&entry.content[ContentFolder::Mods], |_, _, cx| cx.notify());
                self._subscriptions.push(sub);
            }
        }
    }

    /// The big card's instance: the one picked with Switch instance (shared with the Play tab), else
    /// the one played most recently.
    fn hero(entries: &[InstanceEntry], cx: &App) -> Option<InstanceEntry> {
        let picked = InterfaceConfig::get(cx).home_instance.clone();
        picked.and_then(|name| entries.iter().find(|e| e.name == name).cloned())
            .or_else(|| last_played(entries).cloned())
            .or_else(|| entries.first().cloned())
    }

    fn render_hero(&self, hero: Option<&InstanceEntry>, entries: &[InstanceEntry], cx: &mut Context<Self>) -> AnyElement {
        let violet: Hsla = crate::pages::home_page::violet().into();
        let card = div().relative().w_full().h(px(188.)).flex_shrink_0().rounded_xl().overflow_hidden().bg(rgb(0x141729))
            .child(div().absolute().inset_0().bg(linear_gradient(90., linear_color_stop(violet.opacity(0.0), 0.1), linear_color_stop(violet.opacity(0.5), 0.55))))
            .child(div().absolute().inset_0().bg(linear_gradient(90., linear_color_stop(rgb(GREEN).opacity(0.0), 0.45), linear_color_stop(rgb(GREEN).opacity(0.55), 1.))));

        let Some(hero) = hero else {
            let data = self.data.clone();
            return card.child(v_flex().absolute().left_8().bottom_8().gap_2()
                .child(div().text_xs().font_weight(FontWeight::BOLD).text_color(white().opacity(0.75)).child("NO INSTANCES YET"))
                .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BLACK).text_size(px(34.)).child("Make your first instance"))
                .child(Button::new("hero-create").primary().label("New instance").on_click(move |_, window, cx| {
                    crate::modals::create_instance::open_create_instance_with(data.metadata.clone(), data.instances.clone(), data.backend_handle.clone(), false, window, cx);
                })))
                .into_any_element();
        };

        let is_last = last_played(entries).is_some_and(|e| e.id == hero.id);
        let when = crate::games::last_played_text(hero.playtime.last_played_unix_ms.map(|ms| ms / 1000));
        let label = match (hero.playtime.last_played_unix_ms, is_last) {
            (None, _) => "NOT PLAYED YET".to_string(),
            (Some(_), true) => format!("LAST PLAYED · {}", when.to_uppercase()),
            (Some(_), false) => format!("SELECTED · PLAYED {}", when.to_uppercase()),
        };
        let account = self.data.accounts.read(cx).selected_account.as_ref()
            .map(|a| a.username(InterfaceConfig::get(cx).hide_usernames || crate::privacy::masking()).to_string());
        let mut line = details(hero, cx);
        if let Some(account) = account {
            line.push_str(&format!(" · playing as {account}"));
        }

        let (play_label, play_bg): (&str, Hsla) = match hero.status {
            InstanceStatus::NotRunning => ("PLAY", rgb(GREEN).into()),
            InstanceStatus::Launching => ("STARTING", cx.theme().warning),
            InstanceStatus::Running => ("STOP", cx.theme().danger),
            InstanceStatus::Stopping => ("STOPPING", cx.theme().muted),
        };
        let play = {
            let (data, id, name, status) = (self.data.clone(), hero.id, hero.name.clone(), hero.status);
            div().id("hero-play").w(px(124.)).h(px(44.)).rounded_lg().flex().items_center().justify_center().cursor_pointer()
                .bg(play_bg).hover(|s| s.opacity(0.9)).active(|s| s.opacity(0.8))
                .text_sm().font_weight(FontWeight::BOLD).text_color(white()).child(play_label)
                .on_click(move |_, window, cx| play_or_stop(id, name.clone(), status, &data, window, cx))
        };
        let names: Vec<SharedString> = entries.iter().map(|e| e.name.clone()).collect();
        let current = hero.name.clone();
        let switch = Button::new("hero-switch").label("Switch instance").icon(PandoraIcon::ChevronDown).small()
            .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, _| {
                for name in &names {
                    let pick = name.clone();
                    menu = menu.item(PopupMenuItem::new(name.clone()).checked(*name == current).on_click(move |_, _, cx| {
                        InterfaceConfig::get_mut(cx).home_instance = Some(pick.clone());
                        cx.refresh_windows();
                    }));
                }
                menu
            });

        card
            .child(v_flex().absolute().left_8().bottom_7().right(px(320.)).gap_1()
                .child(div().text_xs().font_weight(FontWeight::BOLD).text_color(white().opacity(0.8)).child(label))
                .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BLACK).text_size(px(38.))
                    .line_height(px(44.)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(hero.name.clone()))
                .child(div().text_sm().text_color(white().opacity(0.8)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(line)))
            .child(h_flex().absolute().right_6().bottom_7().gap_2().items_center().child(play).child(switch))
            .into_any_element()
    }

    fn render_card(&self, entry: &InstanceEntry, highlight: bool, cx: &mut Context<Self>) -> AnyElement {
        let (danger, muted) = (cx.theme().danger, cx.theme().muted_foreground);
        let icon: AnyElement = if let Some(icon) = entry.icon.clone() {
            let transform = png_render_cache::ImageTransformation::Resize { width: 96, height: 96 };
            png_render_cache::render_with_transform(icon, transform, cx).size_12().min_w_12().rounded_lg().into_any_element()
        } else {
            div().size_12().min_w_12().rounded_lg().bg(placeholder_color(&entry.name)).into_any_element()
        };

        let running = entry.status != InstanceStatus::NotRunning;
        let play = {
            let (data, id, name, status) = (self.data.clone(), entry.id, entry.name.clone(), entry.status);
            div().id(("card-play", entry.id.index)).size_8().flex_shrink_0().rounded_full().flex().items_center().justify_center().cursor_pointer()
                .bg(if running { danger } else if highlight { rgb(GREEN).into() } else { white().opacity(0.08) })
                .hover(|s| s.opacity(0.85))
                .child(Icon::new(if running { PandoraIcon::Close } else { PandoraIcon::Play }).size_3p5().text_color(white()))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    play_or_stop(id, name.clone(), status, &data, window, cx);
                })
        };

        let open_name = entry.name.clone();
        let menu_entry = entry.clone();
        let data = self.data.clone();
        h_flex().id(("instance-card", entry.id.index)).p_3().gap_3().items_center().rounded_xl().cursor_pointer()
            .bg(rgb(0x141826)).border_1().border_color(white().opacity(0.05))
            .hover(|s| s.bg(rgb(0x1a1f31)).border_color(white().opacity(0.1)))
            .child(icon)
            .child(v_flex().flex_1().min_w_0().gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().text_ellipsis().child(entry.name.clone()))
                .child(div().text_xs().text_color(muted).overflow_hidden().whitespace_nowrap().text_ellipsis().child(details(entry, cx))))
            .child(play)
            .on_click(move |_, window, cx| open_instance(open_name.clone(), window, cx))
            .context_menu(move |menu, _, _| {
                let open = menu_entry.name.clone();
                let (id, name) = (menu_entry.id, menu_entry.name.clone());
                let (instances, backend) = (data.instances.clone(), data.backend_handle.clone());
                menu.item(PopupMenuItem::new("Open").on_click(move |_, window, cx| open_instance(open.clone(), window, cx)))
                    .item(PopupMenuItem::new("Move to group").on_click(move |_, window, cx| {
                        crate::modals::move_instance_to_group::open_move_instance_to_group_modal(id, name.clone(), instances.clone(), backend.clone(), window, cx);
                    }))
            })
            .into_any_element()
    }
}

/// The instance played most recently, if any has been played.
fn last_played(entries: &[InstanceEntry]) -> Option<&InstanceEntry> {
    entries.iter().filter(|e| e.playtime.last_played_unix_ms.is_some()).max_by_key(|e| e.playtime.last_played_unix_ms)
}

/// "Fabric 26.3 · 42 mods", "Vanilla 26.3".
fn details(entry: &InstanceEntry, cx: &App) -> String {
    let config = &entry.configuration;
    let mut text = format!("{} {}", config.loader.pretty_name(), config.minecraft_version.as_str());
    if config.loader != Loader::Vanilla {
        if let Some(mods) = entry.content[ContentFolder::Mods].read(cx).as_ref() {
            text.push_str(&format!(" · {} mod{}", mods.len(), if mods.len() == 1 { "" } else { "s" }));
        }
    }
    text
}

/// A muted colour square for instances without an icon, the same for the same name.
fn placeholder_color(name: &str) -> Hsla {
    let hash = name.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
    hsla((hash % 360) as f32 / 360., 0.32, 0.3, 1.)
}

fn open_instance(name: SharedString, window: &mut Window, cx: &mut App) {
    root::switch_page(PageType::InstancePage { name }, &[PageType::Instances], window, cx);
}

fn play_or_stop(id: InstanceID, name: SharedString, status: InstanceStatus, data: &DataEntities, window: &mut Window, cx: &mut App) {
    match status {
        InstanceStatus::NotRunning => root::start_instance(id, name, None, data, window, cx),
        InstanceStatus::Running => data.backend_handle.send(MessageToBackend::KillInstance { id }),
        _ => {},
    }
}

impl Page for InstancesPage {
    fn controls(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }

    fn scrollable(&self, _cx: &App) -> bool {
        true
    }
}

impl Render for InstancesPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = self.entries(cx);
        self.watch_mods(&entries, cx);
        let hero = Self::hero(&entries, cx);
        let shown: Vec<&InstanceEntry> = entries.iter().filter(|e| self.filter.is_none_or(|l| e.configuration.loader == l)).collect();
        let muted = cx.theme().muted_foreground;

        // Header: title, loader filter, and the two ways to make an instance.
        let entity = cx.entity();
        let current = self.filter;
        let filters = Button::new("instance-filters").small().label(match current {
            None => "Filters".to_string(),
            Some(loader) => format!("Filters: {}", loader.pretty_name()),
        }).dropdown_menu_with_anchor(Anchor::TopLeft, move |menu, _, _| {
            let choice = |label: &'static str, loader: Option<Loader>| {
                let entity = entity.clone();
                PopupMenuItem::new(label).checked(current == loader).on_click(move |_, _, cx| {
                    entity.update(cx, |page, cx| {
                        page.filter = loader;
                        cx.notify();
                    });
                })
            };
            menu.item(choice("All loaders", None))
                .item(PopupMenuItem::separator())
                .item(choice("Vanilla", Some(Loader::Vanilla)))
                .item(choice("Fabric", Some(Loader::Fabric)))
                .item(choice("Forge", Some(Loader::Forge)))
                .item(choice("NeoForge", Some(Loader::NeoForge)))
        });
        let create = |id: &'static str, label: &'static str, max_fps: bool, data: DataEntities| {
            Button::new(id).small().label(label).on_click(move |_, window, cx| {
                crate::modals::create_instance::open_create_instance_with(data.metadata.clone(), data.instances.clone(),
                    data.backend_handle.clone(), max_fps, window, cx);
            })
        };
        let header = h_flex().gap_3().items_center()
            .child(div().text_lg().font_weight(FontWeight::BOLD).child("Your instances"))
            .child(filters)
            .child(div().flex_1())
            .child(create("new-instance", "+ New instance", false, self.data.clone()).primary())
            .child(create("max-fps-instance", "Max FPS setup", true, self.data.clone()));

        // Instance cards, then a card for making a new one.
        let width = window.viewport_size().width;
        let cols = ((f32::from(width) - 48.) / 280.).floor().clamp(1., 4.) as u16;
        let mut grid = div().grid().grid_cols(cols).gap_3().w_full();
        for entry in &shown {
            let highlight = hero.as_ref().is_some_and(|h| h.id == entry.id);
            grid = grid.child(self.render_card(entry, highlight, cx));
        }
        let data = self.data.clone();
        grid = grid.child(h_flex().id("new-instance-card").p_3().gap_3().items_center().rounded_xl().cursor_pointer().min_h(px(74.))
            .border_1().border_dashed().border_color(white().opacity(0.22)).hover(|s| s.bg(white().opacity(0.04)))
            .child(div().size_12().flex().items_center().justify_center().child(Icon::new(PandoraIcon::Plus).size_5().text_color(muted)))
            .child(v_flex().gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("New instance"))
                .child(div().text_xs().text_color(muted).child("Any version, any loader")))
            .on_click(move |_, window, cx| {
                crate::modals::create_instance::open_create_instance_with(data.metadata.clone(), data.instances.clone(),
                    data.backend_handle.clone(), false, window, cx);
            }));

        // Shortcuts to the other Minecraft sections.
        let config = InterfaceConfig::get(cx);
        let shortcuts: Vec<(&'static str, &'static str, &'static str, PageType)> = [
            (true, "go-mods", "Browse mods", "Modrinth and CurseForge, installed straight into an instance.", PageType::Modrinth { installing_for: None }),
            (!config.hide_skins, "go-skins", "Skins", "Change your skin and cape, or preview anyone's.", PageType::Skins),
            (config.show_quickplay_page, "go-quickplay", "Quick Play", "Jump straight into a server or world.", PageType::Quickplay),
        ].into_iter().filter(|s| s.0).map(|(_, id, title, text, page)| (id, title, text, page)).collect();
        let mut links = div().grid().grid_cols(shortcuts.len().max(1) as u16).gap_3().w_full();
        for (id, title, text, page) in shortcuts {
            links = links.child(v_flex().id(id).p_3().gap_0p5().rounded_xl().cursor_pointer().bg(rgb(0x141826))
                .hover(|s| s.bg(rgb(0x1a1f31)))
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(div().text_xs().text_color(muted).child(text))
                .on_click(move |_, window, cx| root::switch_page(page.clone(), &[], window, cx)));
        }

        v_flex().w_full().p_4().gap_4()
            .child(self.render_hero(hero.as_ref(), &entries, cx))
            .child(header)
            .child(grid)
            .when(shown.is_empty() && !entries.is_empty(), |this| this.child(div().text_sm().text_color(muted).child("No instances use this loader.")))
            .child(links)
    }
}

#[derive(Default)]
pub struct VersionList {
    pub versions: Vec<SharedString>,
    pub matched_versions: Vec<SharedString>,
}

impl SelectDelegate for VersionList {
    type Item = SharedString;

    fn items_count(&self, _section: usize) -> usize {
        self.matched_versions.len()
    }

    fn item(&self, ix: IndexPath) -> Option<&Self::Item> {
        self.matched_versions.get(ix.row)
    }

    fn position<V>(&self, value: &V) -> Option<IndexPath>
    where
        Self::Item: gpui_component::select::SelectItem<Value = V>,
        V: PartialEq,
    {
        for (ix, item) in self.matched_versions.iter().enumerate() {
            if item.value() == value {
                return Some(IndexPath::default().row(ix));
            }
        }

        None
    }

    fn perform_search(&mut self, query: &str, _window: &mut Window, _: &mut App) -> Task<()> {
        let lower_query = query.to_lowercase();

        self.matched_versions = self
            .versions
            .iter()
            .filter(|item| item.to_lowercase().starts_with(&lower_query))
            .cloned()
            .collect();

        Task::ready(())
    }
}
