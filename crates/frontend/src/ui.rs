use std::{collections::VecDeque, sync::Arc};

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Icon, InteractiveElementExt, WindowExt, h_flex, notification::{Notification, NotificationType}, scroll::ScrollableElement, tooltip::Tooltip, v_flex
};
use rustc_hash::FxHashMap;
use schema::pandora_update::UpdatePrompt;
use serde::{Deserialize, Serialize};

use gpui_component::{Sizable, button::{Button, ButtonVariants}, menu::{DropdownMenu, PopupMenuItem}};

use crate::component::generic_title_bar::TitleBar;
use crate::interface_config::GamesLayout;
use crate::{
    component::{generic_title_bar::TitleBarState, main_title_bar::MainTitleBar, menu::{MenuGroup, MenuGroupItem}, page_path::PagePath, resize_panel::{ResizePanel, ResizePanelState}, shrinking_text::ShrinkingText}, entity::{
        DataEntities, account::AccountExt, instance::{InstanceAddedEvent, InstanceEntries, InstanceEntry, InstanceModifiedEvent, InstanceMovedToTopEvent, InstanceRemovedEvent}
    }, icon::PandoraIcon, interface_config::{InterfaceConfig, LibraryMode}, pages::{home_page::HomePage, game_page::GamePage, add_game_page::AddGamePage, emulators_page::EmulatorsPage, controllers_page::ControllersPage, curseforge_page::CurseforgeSearchPage, import::ImportPage, instance::instance_page::InstancePage, instances_page::InstancesPage, modrinth_page::ModrinthSearchPage, modrinth_project_page::ModrinthProjectPage, page::Page, quickplay::QuickplayPage, skins_page::SkinsPage, syncing_page::SyncingPage}, png_render_cache,
};

pub struct LauncherUI {
    data: DataEntities,
    page: LauncherPage,
    pub update: Option<UpdatePrompt>,
    sidebar_state: ResizePanelState,
    recent_instances: heapless::Vec<InstanceEntry, 3>,
    page_history_backwards: VecDeque<(PageType, Arc<[PageType]>)>,
    page_history_forwards: Vec<(PageType, Arc<[PageType]>)>,
    previous_pages: FxHashMap<PageType, LauncherPage>,
    pending_page: Option<(PageType, Arc<[PageType]>)>,
    _controller: Task<()>,
    chat: Option<Entity<crate::chat::panel::ChatPanel>>,
    search: Entity<crate::component::global_search::GlobalSearch>,
    _instance_added_subscription: Subscription,
    _instance_modified_subscription: Subscription,
    _instance_removed_subscription: Subscription,
    _instance_moved_to_top_subscription: Subscription,
}

pub const ICONIZED_SIDEBAR_WIDTH: f32 = 64.0;
pub const MIN_SIDEBAR_WIDTH: f32 = 150.0;
pub const MAX_SIDEBAR_WIDTH: f32 = 225.0;

#[derive(Default, Clone, Debug, PartialEq, Eq, Deserialize, Serialize, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PageType {
    #[default]
    Home,
    Quickplay,
    Instances,
    Skins,
    Modrinth {
        installing_for: Option<SharedString>,
    },
    Curseforge {
        installing_for: Option<SharedString>,
    },
    Import,
    Syncing,
    ModrinthProject {
        project_id: SharedString,
        project_title: SharedString,
        install_for: Option<SharedString>,
    },
    InstancePage {
        name: SharedString,
    },
    /// The Games home screen (row / grid / list of the player's games).
    GamesHome,
    /// Import games from other launchers or emulator folders.
    ImportGames,
    /// A Games-mode library game (`crate::games::GAMES`).
    Game {
        id: SharedString,
    },
    /// Add a game to the Games-mode library.
    AddGame,
    /// Every emulator Flux can use (Games mode).
    Emulators,
    /// PlayStation controllers as Xbox controllers.
    Controllers,
}

impl PageType {
    pub fn title(&self, data: &DataEntities, cx: &App) -> SharedString {
        match self {
            PageType::Home => "Home".into(),
            PageType::Game { id } => crate::games::find(id, cx).map(|g| SharedString::from(g.name)).unwrap_or("Game".into()),
            PageType::AddGame => "Add a game".into(),
            PageType::GamesHome => "Games".into(),
            PageType::ImportGames => "Import games".into(),
            PageType::Emulators => "Emulators".into(),
            PageType::Controllers => "Controllers".into(),
            PageType::Quickplay => t::quickplay::title().into(),
            PageType::Instances => t::instance::title().into(),
            PageType::Skins => t::skins::title().into(),
            PageType::Modrinth { installing_for } => {
                if installing_for.is_some() {
                    t::instance::content::install::from_modrinth().into()
                } else {
                    t::modrinth::name().into()
                }
            },
            PageType::Curseforge { installing_for } => {
                if installing_for.is_some() {
                    t::instance::content::install::from_curseforge().into()
                } else {
                    t::curseforge::name().into()
                }
            },
            PageType::Import => t::import::label().into(),
            PageType::Syncing => t::instance::sync::label().into(),
            PageType::ModrinthProject { project_title, .. } => project_title.clone(),
            PageType::InstancePage { name } => {
                InstanceEntries::find_title_by_name(&data.instances, name, cx)
                    .unwrap_or_else(|| name.clone())
            },
        }
    }
}

#[derive(Clone)]
pub enum LauncherPage {
    Home(Entity<HomePage>),
    Quickplay(Entity<QuickplayPage>),
    Instances(Entity<InstancesPage>),
    Skins(Entity<SkinsPage>),
    Modrinth(Entity<ModrinthSearchPage>),
    Curseforge(Entity<CurseforgeSearchPage>),
    Import(Entity<ImportPage>),
    Syncing(Entity<SyncingPage>),
    ModrinthProject(Entity<ModrinthProjectPage>),
    InstancePage(Entity<InstancePage>),
    Game(Entity<GamePage>),
    GamesHome(Entity<crate::pages::games_home_page::GamesHomePage>),
    ImportGames(Entity<crate::pages::import_games_page::ImportGamesPage>),
    AddGame(Entity<AddGamePage>),
    Emulators(Entity<EmulatorsPage>),
    Controllers(Entity<ControllersPage>),
}

impl LauncherPage {
    fn render(self, ui: &LauncherUI, top_nav: AnyElement, window: &mut Window, cx: &mut App) -> impl IntoElement {
        fn process(entity: Entity<impl Page>, window: &mut Window, cx: &mut App) -> (bool, AnyElement, AnyElement) {
            entity.update(cx, |page, cx| {
                (page.scrollable(cx), page.controls(window, cx).into_any_element(), page.render(window, cx).into_any_element())
            })
        }

        let (scrollable, controls, page) = match self {
            LauncherPage::Home(entity) => process(entity, window, cx),
            LauncherPage::Quickplay(entity) => process(entity, window, cx),
            LauncherPage::Instances(entity) => process(entity, window, cx),
            LauncherPage::Skins(entity) => process(entity, window, cx),
            LauncherPage::Modrinth(entity) => process(entity, window, cx),
            LauncherPage::Curseforge(entity) => process(entity, window, cx),
            LauncherPage::Import(entity) => process(entity, window, cx),
            LauncherPage::Syncing(entity) => process(entity, window, cx),
            LauncherPage::ModrinthProject(entity) => process(entity, window, cx),
            LauncherPage::InstancePage(entity) => process(entity, window, cx),
            LauncherPage::Game(entity) => process(entity, window, cx),
            LauncherPage::GamesHome(entity) => process(entity, window, cx),
            LauncherPage::ImportGames(entity) => process(entity, window, cx),
            LauncherPage::AddGame(entity) => process(entity, window, cx),
            LauncherPage::Emulators(entity) => process(entity, window, cx),
            LauncherPage::Controllers(entity) => process(entity, window, cx),
        };

        let config = InterfaceConfig::get(cx);
        let main_page = config.main_page.clone();
        let breadcrumb = config.page_path.clone();
        let minecraft = is_minecraft_page(&main_page);
        // A Minecraft section opened from its tab: its buttons go beside the sub-tabs.
        let minecraft_top = minecraft && breadcrumb.is_empty();
        let mut controls = Some(controls);

        let sub_tabs = minecraft.then(|| TitleBar {
            left_content: vec![minecraft_tabs(&main_page, cx)],
            right_content: if minecraft_top { controls.take().into_iter().collect() } else { Vec::new() },
            content_only: true,
        });
        // The page's own header (where it is, and its buttons): not on Home / Library, whose
        // tabs are in the top bar.
        let page_bar = (main_page != PageType::GamesHome && !minecraft_top).then(|| TitleBar {
            left_content: vec![
                div().overflow_hidden().pr_8().child(PagePath::new(ui.data.clone(), main_page.clone(), breadcrumb.clone())).into_any_element(),
                controls.take().unwrap_or_else(|| div().into_any_element()),
            ],
            right_content: Vec::new(),
            content_only: true,
        });

        let body = if scrollable {
            div().flex_1().overflow_hidden().child(v_flex().size_full().overflow_y_scrollbar().child(page))
        } else {
            // The page gets exactly the space under the bars (not the full window height), so
            // pages that scroll themselves can reach their bottom in any window size.
            div().flex_1().min_h_0().w_full().overflow_hidden().child(page)
        };
        v_flex()
            .size_full()
            .child(top_nav)
            .children(sub_tabs)
            .children(page_bar)
            .child(body)
    }
}

/// Pages that belong to the Minecraft tab.
fn is_minecraft_page(page: &PageType) -> bool {
    matches!(page, PageType::Home | PageType::Quickplay | PageType::Instances | PageType::Skins | PageType::Modrinth { .. }
        | PageType::Curseforge { .. } | PageType::Import | PageType::Syncing | PageType::ModrinthProject { .. } | PageType::InstancePage { .. })
}

/// The Minecraft tab's sections.
fn minecraft_tabs(page: &PageType, cx: &App) -> AnyElement {
    let config = InterfaceConfig::get(cx);
    let theme = cx.theme();
    let mods = matches!(page, PageType::Modrinth { .. } | PageType::Curseforge { .. } | PageType::ModrinthProject { .. });
    let sections: Vec<(&'static str, &'static str, PageType, bool)> = [
        (true, "mc-play", "Play", PageType::Home, *page == PageType::Home),
        (true, "mc-instances", "Instances", PageType::Instances, matches!(page, PageType::Instances | PageType::InstancePage { .. })),
        (true, "mc-mods", "Browse mods", PageType::Modrinth { installing_for: None }, mods),
        (!config.hide_skins, "mc-skins", "Skins", PageType::Skins, *page == PageType::Skins),
        (config.show_quickplay_page, "mc-quickplay", "Quick Play", PageType::Quickplay, *page == PageType::Quickplay),
        (true, "mc-import", "Import", PageType::Import, *page == PageType::Import),
        (true, "mc-sync", "Sync", PageType::Syncing, *page == PageType::Syncing),
    ].into_iter().filter(|s| s.0).map(|(_, id, label, target, active)| (id, label, target, active)).collect();

    let tab = |id: &'static str, label: &'static str, target: PageType, active: bool| {
        v_flex()
            .id(id)
            .cursor_pointer()
            .gap_1()
            .pt_1()
            .child(div().px_1().text_sm().font_weight(FontWeight::SEMIBOLD)
                .text_color(if active { theme.foreground } else { theme.muted_foreground })
                .child(label))
            .child(div().h(px(2.)).rounded_full().when(active, |this| this.bg(theme.primary)))
            .on_click(move |_, window, cx| crate::root::switch_page(target.clone(), &[], window, cx))
    };
    let mut row = h_flex().gap_5().items_center();
    for (id, label, target, active) in sections {
        row = row.child(tab(id, label, target, active));
    }
    if mods && !matches!(page, PageType::ModrinthProject { .. }) {
        // Where to browse.
        let on_curseforge = matches!(page, PageType::Curseforge { .. });
        let source = |id: &'static str, label: &'static str, active: bool, target: PageType| div()
            .id(id)
            .px_2p5()
            .py_0p5()
            .rounded(theme.radius)
            .cursor_pointer()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .when(active, |this| this.bg(theme.primary).text_color(theme.primary_foreground))
            .when(!active, |this| this.text_color(theme.muted_foreground))
            .child(label)
            .on_click(move |_, window, cx| crate::root::switch_page(target.clone(), &[], window, cx));
        row = row.child(h_flex().ml_2().p_0p5().gap_0p5().rounded(theme.radius).bg(theme.secondary)
            .child(source("src-modrinth", "Modrinth", !on_curseforge, PageType::Modrinth { installing_for: None }))
            .child(source("src-curseforge", "CurseForge", on_curseforge, PageType::Curseforge { installing_for: None })));
    }
    row.into_any_element()
}

impl LauncherUI {
    pub fn new(data: &DataEntities, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let recent_instances = data
            .instances
            .read(cx)
            .entries
            .values()
            .filter_map(|entry| {
                let entry = entry.read(cx).clone();
                if entry.name == schema::quickplay::INSTANCE_NAME {
                    None
                } else {
                    Some(entry)
                }
            })
            .take(3)
            .collect();

        let _instance_added_subscription =
            cx.subscribe::<_, InstanceAddedEvent>(&data.instances, |this, _, event, cx| {
                if event.instance.name == schema::quickplay::INSTANCE_NAME {
                    return;
                }
                if this.recent_instances.is_full() {
                    this.recent_instances.pop();
                }
                let _ = this.recent_instances.insert(0, event.instance.clone());
                cx.notify();
            });
        let _instance_modified_subscription =
            cx.subscribe_in::<_, InstanceModifiedEvent>(&data.instances, window, |this, _, event, window, cx| {
                if let Some(existing) = this.recent_instances.iter_mut().find(|instance| instance.id == event.instance.id) {
                    *existing = event.instance.clone();
                    cx.notify();
                }
                if let LauncherPage::InstancePage(page) = &this.page
                    && page.read(cx).instance.read(cx).id == event.instance.id
                {
                    let page_path = InterfaceConfig::get_mut(cx).page_path.clone();
                    this.switch_page(PageType::InstancePage { name: event.instance.name.clone() }, &*page_path, window, cx);
                }
                cx.notify();
            });
        let _instance_removed_subscription =
            cx.subscribe_in::<_, InstanceRemovedEvent>(&data.instances, window, |this, _, event, window, cx| {
                this.recent_instances.retain(|entry| entry.id != event.id);

                if let LauncherPage::InstancePage(page) = &this.page
                    && page.read(cx).instance.read(cx).id == event.id
                {
                    this.switch_page(PageType::Instances, &[], window, cx);
                }
                cx.notify();
            });
        let _instance_moved_to_top_subscription =
            cx.subscribe::<_, InstanceMovedToTopEvent>(&data.instances, |this, _, event, cx| {
                if event.instance.name == schema::quickplay::INSTANCE_NAME {
                    return;
                }
                this.recent_instances.retain(|entry| entry.id != event.instance.id);
                if this.recent_instances.is_full() {
                    this.recent_instances.pop();
                }
                let _ = this.recent_instances.insert(0, event.instance.clone());
                cx.notify();
            });

        let config = InterfaceConfig::get(cx);

        let mut default_sidebar_width = config.sidebar_width;
        if default_sidebar_width <= 0.0 {
            default_sidebar_width = (MIN_SIDEBAR_WIDTH + MAX_SIDEBAR_WIDTH)/2.0;
        } else if default_sidebar_width < MIN_SIDEBAR_WIDTH {
            let min_distance = (default_sidebar_width - MIN_SIDEBAR_WIDTH).abs();
            let icon_distance = (default_sidebar_width - ICONIZED_SIDEBAR_WIDTH).abs();
            if icon_distance < min_distance {
                default_sidebar_width = ICONIZED_SIDEBAR_WIDTH;
            } else {
                default_sidebar_width = MIN_SIDEBAR_WIDTH;
            }
        }

        let sidebar_state = ResizePanelState::new(px(default_sidebar_width), px(150.0), px(225.0))
            .snap_point(px(ICONIZED_SIDEBAR_WIDTH))
            .keep_size()
            .on_resize(|width, _, cx| {
                InterfaceConfig::get_mut(cx).sidebar_width = width.as_f32();
            });

        let mut main_page = config.main_page.clone();
        let original_page_path = config.page_path.clone();

        // If quickplay is hidden, fall back to the instances page
        if !config.show_quickplay_page && main_page == PageType::Quickplay {
            main_page = PageType::Instances;
            InterfaceConfig::get_mut(cx).main_page = PageType::Instances;
        }

        // If main_page failed to deserialize, also reset the path
        if main_page == PageType::Instances {
            let config = InterfaceConfig::get_mut(cx);
            config.page_path = [].into();
        }

        // Flux always opens on Home (the game row), whatever page was open last time.
        let _ = (main_page, original_page_path);
        let main_page = PageType::GamesHome;
        let original_page_path: Arc<[PageType]> = [].into();
        {
            let config = InterfaceConfig::get_mut(cx);
            config.main_page = PageType::GamesHome;
            config.page_path = [].into();
            config.games_layout = GamesLayout::Shelf;
        }

        let mut pending_page = None;

        let page = match Self::create_page(&data, main_page.clone(), window, cx) {
            Ok(page) => page,
            Err(page_type) => {
                pending_page = Some((main_page, original_page_path));

                let config = InterfaceConfig::get_mut(cx);
                config.main_page = page_type.clone();
                config.page_path = [].into();
                Self::create_page(&data, page_type, window, cx).unwrap()
            },
        };

        crate::privacy::set(InterfaceConfig::get(cx).privacy_mode);
        crate::pages::home_page::set_accent(InterfaceConfig::get(cx).accent_color);
        // The SteamGridDB key lives in Credential Manager (an old interface.json's key moves there).
        {
            let config = InterfaceConfig::get_mut(cx);
            match config.steamgriddb_key.clone().filter(|k| !k.trim().is_empty()) {
                Some(key) => crate::secrets::write_string(crate::secrets::STEAMGRIDDB, Some(&key)),
                None => config.steamgriddb_key = crate::secrets::read_string(crate::secrets::STEAMGRIDDB),
            }
        }
        // Chat networks connected last time sign back in (for unread messages).
        crate::chat::start();

        // Controllers move through Flux's menus while its window is in front.
        // Each press is handled like a click: nothing is being updated while it runs, so it may
        // switch pages.
        let _controller = cx.spawn_in(window, async move |ui, cx| {
            let mut poller = crate::controller_nav::Poller::default();
            // FLUX_CHAT_OPEN=1: open the chat panel at startup (testing).
            let mut open_chat_once = std::env::var_os("FLUX_CHAT_OPEN").is_some();
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
                let Some(ui) = ui.upgrade() else { break };
                let result = cx.update(|window, cx| {
                    if open_chat_once {
                        open_chat_once = false;
                        ui.update(cx, |ui, cx| ui.open_chat(crate::chat::panel::ChatView::Chats, window, cx));
                    }
                    if window.is_window_active() {
                        let polled = poller.poll();
                        if !polled.presses.is_empty() || polled.scroll != 0. {
                            Self::controller_tick(&ui, polled, window, cx);
                        }
                    }
                });
                if result.is_err() {
                    break;
                }
            }
        });

        Self {
            data: data.clone(),
            page,
            update: None,
            _controller,
            chat: None,
            search: cx.new(|cx| crate::component::global_search::GlobalSearch::new(data, window, cx)),
            sidebar_state,
            recent_instances,
            page_history_backwards: VecDeque::with_capacity(32),
            page_history_forwards: Vec::new(),
            previous_pages: FxHashMap::default(),
            pending_page,
            _instance_added_subscription,
            _instance_modified_subscription,
            _instance_removed_subscription,
            _instance_moved_to_top_subscription,
        }
    }

    fn create_page(data: &DataEntities, page: PageType, window: &mut Window, cx: &mut Context<Self>) -> Result<LauncherPage, PageType> {
        match page {
            PageType::Home => {
                Ok(LauncherPage::Home(cx.new(|cx| HomePage::new(data, window, cx))))
            },
            PageType::Game { ref id } => {
                if crate::games::find(id, cx).is_none() {
                    return Err(PageType::AddGame);
                }
                let id = id.clone();
                Ok(LauncherPage::Game(cx.new(|cx| GamePage::new(&id, data, window, cx).unwrap())))
            },
            PageType::AddGame => Ok(LauncherPage::AddGame(cx.new(|cx| AddGamePage::new(window, cx)))),
            PageType::GamesHome => Ok(LauncherPage::GamesHome(cx.new(|cx| crate::pages::games_home_page::GamesHomePage::new(window, cx)))),
            PageType::ImportGames => Ok(LauncherPage::ImportGames(cx.new(|cx| crate::pages::import_games_page::ImportGamesPage::new(window, cx)))),
            PageType::Emulators => Ok(LauncherPage::Emulators(cx.new(|cx| EmulatorsPage::new(data, window, cx)))),
            PageType::Controllers => Ok(LauncherPage::Controllers(cx.new(|cx| ControllersPage::new(data, window, cx)))),
            PageType::Quickplay => {
                Ok(LauncherPage::Quickplay(cx.new(|cx| QuickplayPage::new(data, window, cx))))
            },
            PageType::Instances => {
                Ok(LauncherPage::Instances(cx.new(|cx| InstancesPage::new(data, window, cx))))
            },
            PageType::Skins => {
                Ok(LauncherPage::Skins(cx.new(|cx| SkinsPage::new(data, window, cx))))
            },
            PageType::Modrinth { installing_for } => {
                let installing_for = installing_for.as_ref().map(|name| InstanceEntries::find_id_by_name(&data.instances, name, cx));

                if let Some(None) = installing_for {
                    return Err(PageType::Modrinth { installing_for: None })
                }

                let page = cx.new(|cx| {
                    ModrinthSearchPage::new(installing_for.flatten(), data, window, cx)
                });
                Ok(LauncherPage::Modrinth(page))
            },
            PageType::Curseforge { installing_for } => {
                let installing_for = installing_for.as_ref().map(|name| InstanceEntries::find_id_by_name(&data.instances, name, cx));

                if let Some(None) = installing_for {
                    return Err(PageType::Curseforge { installing_for: None })
                }

                let page = cx.new(|cx| {
                    CurseforgeSearchPage::new(installing_for.flatten(), data, window, cx)
                });
                Ok(LauncherPage::Curseforge(page))
            },
            PageType::Import => {
                Ok(LauncherPage::Import(cx.new(|cx| ImportPage::new(data, window, cx))))
            },
            PageType::Syncing => {
                Ok(LauncherPage::Syncing(cx.new(|cx| SyncingPage::new(data, window, cx))))
            },
            PageType::ModrinthProject { project_id, install_for, project_title } => {
                let install_for_id = install_for.as_ref().map(|name| InstanceEntries::find_id_by_name(&data.instances, name, cx));

                if let Some(None) = install_for_id {
                    return Err(PageType::ModrinthProject { project_id, install_for: None, project_title })
                }

                let project_id = project_id.clone();
                let page = cx.new(|cx| {
                    ModrinthProjectPage::new(project_id, install_for_id.flatten(), data, window, cx,)
                });
                Ok(LauncherPage::ModrinthProject(page))
            },
            PageType::InstancePage { ref name } => {
                let Some(id) = InstanceEntries::find_id_by_name(&data.instances, name, cx) else {
                    return Err(PageType::Instances);
                };

                Ok(LauncherPage::InstancePage(cx.new(|cx| {
                    InstancePage::new(id, data, window, cx)
                })))
            },
        }
    }

    pub fn switch_page(&mut self, page: PageType, page_path: &[PageType], window: &mut Window, cx: &mut Context<Self>) {
        let page_path: Arc<[PageType]> = page_path.into();

        let config = InterfaceConfig::get(cx);
        if config.main_page == page {
            return;
        }

        self.page_history_forwards.clear();
        if self.page_history_backwards.len() >= 32 {
            self.page_history_backwards.pop_back();
        }
        self.page_history_backwards.push_front((config.main_page.clone(), config.page_path.clone()));

        self.switch_page_without_history(page, page_path, window, cx);
    }

    fn switch_page_without_history(&mut self, page: PageType, page_path: Arc<[PageType]>, window: &mut Window, cx: &mut Context<Self>) {
        self.pending_page = None;

        let config = InterfaceConfig::get_mut(cx);
        let previous_page_type = std::mem::replace(&mut config.main_page, page.clone());
        config.main_page = page.clone();
        config.page_path = page_path.clone();

        if let Some(previous_page) = self.previous_pages.remove(&page) {
            self.page = previous_page;
            self.previous_pages.retain(|k, _| page_path.contains(k));
            cx.notify();
            return;
        }

        match Self::create_page(&self.data, page, window, cx) {
            Ok(page) => {
                let previous_page = std::mem::replace(&mut self.page, page);
                if page_path.contains(&previous_page_type) {
                    self.previous_pages.insert(previous_page_type, previous_page);
                }
                self.previous_pages.retain(|k, _| page_path.contains(k));
            },
            Err(fallback) => {
                let config = InterfaceConfig::get_mut(cx);
                config.main_page = fallback.clone();
                config.page_path = [].into();
                self.previous_pages.clear();
                self.page = Self::create_page(&self.data, fallback, window, cx).unwrap();
            },
        }

        cx.notify();
    }

    /// The pages in the sidebar, top to bottom (LT / RT step through them).
    /// The main places, in order (LT / RT step through them).
    fn sidebar_pages(cx: &App) -> Vec<PageType> {
        let config = InterfaceConfig::get(cx);
        let mut pages = vec![PageType::GamesHome, PageType::Home, PageType::Instances, PageType::Modrinth { installing_for: None }];
        if !config.hide_skins {
            pages.push(PageType::Skins);
        }
        pages.extend([PageType::Emulators, PageType::Controllers]);
        pages
    }

    /// Switch between Minecraft and Games.
    fn switch_mode(&mut self, mode: LibraryMode, window: &mut Window, cx: &mut Context<Self>) {
        if InterfaceConfig::get(cx).library_mode == mode {
            return;
        }
        InterfaceConfig::get_mut(cx).library_mode = mode;
        let page = match mode {
            LibraryMode::Minecraft => PageType::Home,
            LibraryMode::Games => {
                crate::pages::games_home_page::reset_layout(cx);
                PageType::GamesHome
            },
        };
        self.switch_page(page, &[], window, cx);
        cx.notify();
    }

    /// One poll of the controller: the page gets first say (the Games screen, a game's page), then
    /// it works like a keyboard: the D-pad moves between buttons, A presses, B goes back.
    fn controller_tick(ui: &Entity<Self>, polled: crate::controller_nav::Polled, window: &mut Window, cx: &mut App) {
        use crate::controller_nav::Nav;
        use gpui::Keystroke;
        let key = |name: &str, window: &mut Window, cx: &mut App| {
            Keystroke::parse(name).map(|k| window.dispatch_keystroke(k, cx)).unwrap_or(false)
        };
        for nav in polled.presses {
            // The page now (a press may have switched it).
            let page = ui.read(cx).page.clone();
            let used = match &page {
                LauncherPage::GamesHome(page) => page.update(cx, |page, cx| page.controller(nav, window, cx)),
                LauncherPage::Game(page) => page.update(cx, |page, cx| page.controller(nav, cx)),
                _ => false,
            };
            if used {
                continue;
            }
            match nav {
                Nav::Up | Nav::Left => {
                    if !key(if nav == Nav::Up { "up" } else { "left" }, window, cx) {
                        window.focus_prev(cx);
                    }
                },
                Nav::Down | Nav::Right => {
                    if !key(if nav == Nav::Down { "down" } else { "right" }, window, cx) {
                        window.focus_next(cx);
                    }
                },
                Nav::Confirm => {
                    let pressed = window.focused(cx).is_some() && key("enter", window, cx);
                    if !pressed && let LauncherPage::Game(page) = &page {
                        page.update(cx, |page, cx| page.controller_confirm(cx));
                    }
                },
                Nav::Back => {
                    if !key("escape", window, cx) {
                        ui.update(cx, |ui, cx| ui.nav_backwards(window, cx));
                    }
                },
                Nav::PrevPage | Nav::NextPage => {
                    let pages = Self::sidebar_pages(cx);
                    let current = InterfaceConfig::get(cx).main_page.clone();
                    let at = pages.iter().position(|p| *p == current);
                    let next = match (at, nav == Nav::NextPage) {
                        (Some(at), true) => (at + 1) % pages.len(),
                        (Some(at), false) => (at + pages.len() - 1) % pages.len(),
                        (None, _) => 0,
                    };
                    if pages[next] == PageType::GamesHome {
                        crate::pages::games_home_page::reset_layout(cx);
                    }
                    let next = pages[next].clone();
                    ui.update(cx, |ui, cx| ui.switch_page(next, &[], window, cx));
                },
                Nav::Start => {
                    let mode = match InterfaceConfig::get(cx).library_mode {
                        LibraryMode::Minecraft => LibraryMode::Games,
                        LibraryMode::Games => LibraryMode::Minecraft,
                    };
                    ui.update(cx, |ui, cx| ui.switch_mode(mode, window, cx));
                },
                Nav::X | Nav::Y | Nav::View | Nav::PrevTab | Nav::NextTab => {},
            }
        }
        // Right stick scrolls whatever is in the middle of the page.
        if polled.scroll != 0. {
            let viewport = window.viewport_size();
            let sidebar = InterfaceConfig::get(cx).sidebar_width;
            let position = point(px(sidebar) + (viewport.width - px(sidebar)) * 0.6, viewport.height * 0.6);
            window.dispatch_event(PlatformInput::ScrollWheel(ScrollWheelEvent {
                position,
                delta: ScrollDelta::Pixels(point(px(0.), px(polled.scroll * 22.))),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            }), cx);
        }
    }

    /// Open the chat panel on its chats or friends.
    fn open_chat(&mut self, view: crate::chat::panel::ChatView, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_component::WindowExt;
        let panel = match &self.chat {
            Some(panel) => panel.clone(),
            None => {
                let panel = cx.new(|cx| crate::chat::panel::ChatPanel::new(view, window, cx));
                self.chat = Some(panel.clone());
                panel
            },
        };
        panel.update(cx, |panel, cx| {
            panel.view = view;
            cx.notify();
        });
        window.open_sheet_at(gpui_component::Placement::Right, cx, crate::chat::panel::sheet(panel));
    }

    /// Collapse the sidebar to icons, or expand it back to the width it had.
    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        let config = InterfaceConfig::get_mut(cx);
        let width = if config.sidebar_width == ICONIZED_SIDEBAR_WIDTH {
            let back = config.sidebar_expanded_width;
            if back >= MIN_SIDEBAR_WIDTH { back.min(MAX_SIDEBAR_WIDTH) } else { (MIN_SIDEBAR_WIDTH + MAX_SIDEBAR_WIDTH) / 2.0 }
        } else {
            if config.sidebar_width >= MIN_SIDEBAR_WIDTH {
                config.sidebar_expanded_width = config.sidebar_width;
            }
            ICONIZED_SIDEBAR_WIDTH
        };
        config.sidebar_width = width;
        self.sidebar_state.set_size(px(width));
        cx.refresh_windows();
    }

    pub fn nav_backwards(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((page, page_path)) = self.page_history_backwards.pop_front() else {
            return;
        };

        let config = InterfaceConfig::get(cx);
        self.page_history_forwards.push((config.main_page.clone(), config.page_path.clone()));

        self.switch_page_without_history(page, page_path, window, cx);
    }

    pub fn nav_forwards(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((page, page_path)) = self.page_history_forwards.pop() else {
            return;
        };

        let config = InterfaceConfig::get(cx);
        self.page_history_backwards.push_front((config.main_page.clone(), config.page_path.clone()));

        self.switch_page_without_history(page, page_path, window, cx);
    }
}

impl Render for LauncherUI {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(pending_page) = self.pending_page.clone() {
            if let Ok(page) = Self::create_page(&self.data, pending_page.0.clone(), window, cx) {
                self.pending_page = None;
                self.previous_pages.clear();
                self.page_history_forwards.clear();
                self.page_history_backwards.clear();

                let config = InterfaceConfig::get_mut(cx);
                config.main_page = pending_page.0.clone();
                config.page_path = pending_page.1.clone();

                self.page = page;
            }
        }

        let (page_type, games_layout) = {
            let config = InterfaceConfig::get(cx);
            (config.main_page.clone(), config.games_layout)
        };
        let theme = cx.theme().clone();

        // Main tabs. The top bar is a window-drag area, so each tab claims its mouse press.
        let tab = |id: &'static str, label: &'static str, active: bool| {
            div()
                .id(id)
                .px_3()
                .py_1p5()
                .rounded(theme.radius)
                .cursor_pointer()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .when(active, |this| this.bg(theme.primary.opacity(0.22)).text_color(theme.foreground))
                .when(!active, |this| this.text_color(theme.muted_foreground).hover(|s| s.text_color(theme.foreground)))
                .child(label)
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        };
        let home_active = page_type == PageType::GamesHome && games_layout == GamesLayout::Shelf;
        let library_active = page_type == PageType::GamesHome && games_layout != GamesLayout::Shelf;
        let nav = h_flex()
            .gap_1()
            .items_center()
            .child(gpui::img(ImageSource::Resource(Resource::Embedded("images/branding/logo.png".into())))
                .size_6().min_w_6().min_h_6().mr_3().object_fit(ObjectFit::Contain))
            .child(tab("nav-home", "Home", home_active).on_click(cx.listener(|launcher, _, window, cx| {
                InterfaceConfig::get_mut(cx).games_layout = GamesLayout::Shelf;
                launcher.switch_page(PageType::GamesHome, &[], window, cx);
                cx.refresh_windows();
            })))
            .child(tab("nav-library", "Library", library_active).on_click(cx.listener(|launcher, _, window, cx| {
                let config = InterfaceConfig::get_mut(cx);
                config.games_layout = if config.games_library_view == GamesLayout::List { GamesLayout::List } else { GamesLayout::Grid };
                launcher.switch_page(PageType::GamesHome, &[], window, cx);
                cx.refresh_windows();
            })))
            .child(tab("nav-minecraft", "Minecraft", is_minecraft_page(&page_type)).on_click(cx.listener(|launcher, _, window, cx| {
                launcher.switch_page(PageType::Home, &[], window, cx);
            })))
            .child(tab("nav-friends", "Friends", false).on_click(cx.listener(|launcher, _, window, cx| {
                launcher.open_chat(crate::chat::panel::ChatView::Friends, window, cx);
            })));

        let data = self.data.clone();
        let tools = Button::new("nav-tools")
            .small()
            .ghost()
            .label("Tools")
            .icon(PandoraIcon::ChevronDown)
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                let data = data.clone();
                menu.item(PopupMenuItem::new("Emulators").icon(PandoraIcon::Computer)
                        .on_click(|_, window, cx| crate::root::switch_page(PageType::Emulators, &[], window, cx)))
                    .item(PopupMenuItem::new("Controllers").icon(PandoraIcon::Gamepad2)
                        .on_click(|_, window, cx| crate::root::switch_page(PageType::Controllers, &[], window, cx)))
                    .item(PopupMenuItem::new("Art").icon(PandoraIcon::Image)
                        .on_click(|_, window, cx| {
                            crate::pages::games_home_page::open_art_panel(cx);
                            crate::root::switch_page(PageType::GamesHome, &[], window, cx);
                        }))
                    .item(PopupMenuItem::new("Add a game").icon(PandoraIcon::Plus)
                        .on_click(|_, window, cx| crate::root::switch_page(PageType::AddGame, &[], window, cx)))
                    .item(PopupMenuItem::separator())
                    .item(PopupMenuItem::new("Settings").icon(PandoraIcon::Settings)
                        .on_click(move |_, window, cx| crate::settings::open_settings_window(window, &data, cx)))
            });
        let import = Button::new("nav-import")
            .small()
            .ghost()
            .icon(PandoraIcon::Download)
            .label("Import games")
            .on_click(|_, window, cx| crate::root::switch_page(PageType::ImportGames, &[], window, cx));

        let unread = crate::chat::Network::ALL.iter().any(|n| !crate::chat::state(*n).unread.is_empty() || crate::chat::state(*n).requests.iter().any(|r| r.incoming));
        let chat = div()
            .id("nav-chat")
            .relative()
            .size_8()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme.radius)
            .cursor_pointer()
            .hover(|s| s.bg(theme.secondary_hover))
            .child(Icon::new(PandoraIcon::MessageCircle).size_4())
            .when(unread, |this| this.child(div().absolute().top(px(3.)).right(px(3.)).size(px(8.)).rounded_full().bg(gpui::rgb(0xff5d5d))))
            .tooltip(|window, cx| Tooltip::new("Chat").build(window, cx))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(cx.listener(|launcher, _, window, cx| launcher.open_chat(crate::chat::panel::ChatView::Chats, window, cx)));

        let accounts = self.data.accounts.read(cx);
        let (account_head, account_name) = if let Some(account) = &accounts.selected_account {
            let account_name = account.username(InterfaceConfig::get(cx).hide_usernames || crate::privacy::masking());
            let head = if InterfaceConfig::get(cx).hide_skins {
                gpui::img(ImageSource::Resource(Resource::Embedded("images/hidden_head.png".into())))
            } else if let Some(head) = &account.head {
                let resize = png_render_cache::ImageTransformation::Resize { width: 32, height: 32 };
                png_render_cache::render_with_transform(head.clone(), resize, cx)
            } else {
                gpui::img(ImageSource::Resource(Resource::Embedded("images/default_head.png".into())))
            };
            (head, account_name)
        } else {
            (gpui::img(ImageSource::Resource(Resource::Embedded("images/default_head.png".into()))), t::account::none().into())
        };
        let account = h_flex()
            .id("nav-account")
            .gap_2()
            .pl_1()
            .pr_2()
            .py_1()
            .rounded(theme.radius)
            .cursor_pointer()
            .text_sm()
            .hover(|s| s.bg(theme.secondary_hover))
            .child(account_head.size_7().min_w_7().min_h_7().rounded(theme.radius))
            .child(div().max_w(px(140.)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(account_name))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click({
                let data = self.data.clone();
                move |_, window, cx| {
                    if data.accounts.read(cx).accounts.is_empty() {
                        crate::root::start_new_account_login(&data.backend_handle, window, cx);
                        return;
                    }
                    let build = crate::modals::accounts::build_accounts_sheet(&data, window, cx);
                    window.open_sheet_at(gpui_component::Placement::Right, cx, build);
                }
            });

        let update = self.update.clone().map(|update| {
            let send = self.data.backend_handle.clone();
            Button::new("nav-update")
                .label(t::system::update::available())
                .success()
                .compact()
                .small()
                .icon(PandoraIcon::Download)
                .on_click(move |_, window, cx| crate::modals::update_prompt::open_update_prompt(update.clone(), send.clone(), window, cx))
                .into_any_element()
        });

        let top_nav = TitleBar {
            left_content: vec![
                nav.into_any_element(),
                div().flex_1().into_any_element(),
                self.search.clone().into_any_element(),
                div().flex_1().into_any_element(),
            ],
            right_content: update.into_iter()
                .chain([import.into_any_element(), tools.into_any_element(), chat.into_any_element(), account.into_any_element()])
                .collect(),
            content_only: !crate::root::should_render_custom_titlebar(),
        };

        let search = self.search.clone();
        div()
            .size_full()
            .bg(theme.background)
            // Ctrl+K: search everything.
            .on_key_down(move |event, window, cx| {
                if event.keystroke.key == "k" && (event.keystroke.modifiers.control || event.keystroke.modifiers.platform) {
                    crate::component::global_search::GlobalSearch::focus(&search, window, cx);
                    cx.stop_propagation();
                }
            })
            .child(self.page.clone().render(&self, top_nav.into_any_element(), window, cx))
    }
}

/// A name that lives for the rest of the program, interned so each distinct name is leaked once.
fn static_name(name: &str) -> &'static str {
    static NAMES: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut names = NAMES.lock().unwrap();
    if let Some(existing) = names.iter().find(|n| **n == name) {
        return existing;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    names.push(leaked);
    leaked
}

fn open_bug_report_url(window: &mut Window, cx: &mut App) {
    let mut body = String::from(r#"## Description of bug
(Write here)

## Steps to reproduce
(Write here)

## This issue is unique
- [ ] I've searched the other issues and didn't see an issue describing the same bug

## Environment
"#);

    use std::fmt::Write;
    _ = writeln!(&mut body, "Version: {:?}", option_env!("FLUX_VERSION"));
    _ = writeln!(&mut body, "Update channel: {:?}", option_env!("FLUX_UPDATE_URL"));
    _ = writeln!(&mut body, "OS: {} ({})", std::env::consts::OS, std::env::consts::ARCH);

    if cfg!(target_os = "linux") {
        if let Ok(os_release) = std::fs::read_to_string("/etc/os-release") {
            for line in os_release.lines() {
                let line = line.trim_ascii();
                if let Some(name) = line.strip_prefix("NAME=") {
                    _ = writeln!(&mut body, "OS Name: {}", name);
                } else if let Some(version) = line.strip_prefix("VERSION=") {
                    _ = writeln!(&mut body, "OS Version: {}", version);
                }
            }
        }

        _ = writeln!(&mut body, "Desktop: {:?}", std::env::var_os("XDG_CURRENT_DESKTOP"));

        if let Some(snap_name) = std::env::var_os("SNAP_NAME") {
            _ = writeln!(&mut body, "Snap: {:?}", snap_name);
        }
        if let Some(snap_name) = std::env::var_os("FLATPAK_ID") {
            _ = writeln!(&mut body, "Flatpak ID: {:?}", snap_name);
        }
        if std::env::var_os("APPIMAGE").is_some() {
            body.push_str("AppImage: true\n");
        }
    }

    let Some(github) = option_env!("GITHUB_REPOSITORY_URL") else {
        let mut notification: Notification = (NotificationType::Error, SharedString::from("Unable to report bug, GITHUB_REPOSITORY_URL was not set at compile time")).into();
        notification = notification.autohide(false);
        window.push_notification(notification, cx);
        return;
    };

    cx.open_url(&format!("{}/issues/new?body={}", github, urlencoding::encode(&body)));
}
