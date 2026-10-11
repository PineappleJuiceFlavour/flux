use std::{collections::VecDeque, sync::Arc};

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Icon, InteractiveElementExt, WindowExt, h_flex, notification::{Notification, NotificationType}, scroll::ScrollableElement, tooltip::Tooltip, v_flex
};
use rustc_hash::FxHashMap;
use schema::pandora_update::UpdatePrompt;
use serde::{Deserialize, Serialize};

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
    fn render(self, ui: &LauncherUI, window: &mut Window, cx: &mut App) -> impl IntoElement {
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
        let page_path = PagePath::new(ui.data.clone(), config.main_page.clone(), config.page_path.clone());
        let title_bar = MainTitleBar {
            page_path,
            controls,
            update: ui.update.clone(),
            send: ui.data.backend_handle.clone(),
        };

        if scrollable {
            v_flex()
                .size_full()
                .child(title_bar)
                .child(div().flex_1().overflow_hidden().child(
                    v_flex().size_full().overflow_y_scrollbar().child(page),
                ))
        } else {
            // The page gets exactly the space under the title bar (not the full window height), so
            // pages that scroll themselves can reach their bottom in any window size.
            v_flex()
                .size_full()
                .child(title_bar)
                .child(div().flex_1().min_h_0().w_full().overflow_hidden().child(page))
        }
    }
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

        // The Games screen starts in the player's chosen layout.
        crate::pages::games_home_page::reset_layout(cx);

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
    fn sidebar_pages(cx: &App) -> Vec<PageType> {
        let config = InterfaceConfig::get(cx);
        if config.library_mode == LibraryMode::Games {
            return vec![PageType::GamesHome, PageType::AddGame, PageType::ImportGames, PageType::Emulators, PageType::Controllers];
        }
        let mut pages = vec![PageType::Home];
        if config.show_quickplay_page {
            pages.push(PageType::Quickplay);
        }
        pages.push(PageType::Instances);
        if !config.hide_skins {
            pages.push(PageType::Skins);
        }
        pages.extend([PageType::Modrinth { installing_for: None }, PageType::Curseforge { installing_for: None }, PageType::Import, PageType::Syncing]);
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

        let (page_type, show_skins, iconized_sidebar, show_sidebar_icons, show_quickplay_page) = {
            let config = InterfaceConfig::get(cx);
            (config.main_page.clone(), !config.hide_skins, config.sidebar_width == ICONIZED_SIDEBAR_WIDTH, config.show_sidebar_icons, config.show_quickplay_page)
        };

        let games_mode = InterfaceConfig::get(cx).library_mode == LibraryMode::Games;
        // Sidebar entries borrow 'static names, so the player's game names are leaked once each (a
        // handful of short strings for the app's lifetime).
        // The games themselves are on the Games home screen (row, grid or list).
        let games_entries: Vec<(bool, &'static str, PandoraIcon, PageType)> = vec![
            (true, "Home", PandoraIcon::Gamepad2, PageType::GamesHome),
            (true, "Add game", PandoraIcon::Plus, PageType::AddGame),
            (true, "Import games", PandoraIcon::Download, PageType::ImportGames),
        ];
        let games_groups: Vec<(&'static str, Vec<(bool, &'static str, PandoraIcon, PageType)>)> = vec![
            ("Library", games_entries),
            ("Tools", vec![
                (true, "Emulators", PandoraIcon::Settings, PageType::Emulators),
                (true, "Controllers", PandoraIcon::Gamepad2, PageType::Controllers),
            ]),
        ];
        let minecraft_groups: Vec<(&'static str, Vec<(bool, &'static str, PandoraIcon, PageType)>)> = vec![
            ("Minecraft", vec![
                (true, "Home", PandoraIcon::Zap, PageType::Home),
                (show_quickplay_page, t::quickplay::title(), PandoraIcon::Rocket, PageType::Quickplay),
                (true, t::instance::title(), PandoraIcon::Box, PageType::Instances),
                (show_skins, t::skins::title(), PandoraIcon::FaceSlightlySmiling, PageType::Skins),
            ]),
            (t::instance::content::title(), vec![
                (true, t::modrinth::name(), PandoraIcon::CircleCheck, PageType::Modrinth { installing_for: None }),
                (true, t::curseforge::name(), PandoraIcon::Anvil, PageType::Curseforge { installing_for: None }),
            ]),
            (t::instance::sync::files(), vec![
                (true, t::import::label(), PandoraIcon::Download, PageType::Import),
                (true, t::instance::sync::label(), PandoraIcon::RefreshCcw, PageType::Syncing),
            ]),
        ];
        let page_groups = if games_mode { &games_groups } else { &minecraft_groups };

        let sidebar_pages = if !iconized_sidebar {
            let mut groups: heapless::Vec<MenuGroup, 4> = heapless::Vec::new();

            for (group_name, group_entries) in page_groups {
                let mut group = MenuGroup::new(*group_name);
                for (show, title, icon, page) in group_entries {
                    if !*show {
                        continue;
                    }
                    let page = page.clone();
                    group = group.child(MenuGroupItem::new(*title)
                        .when(show_sidebar_icons, |this| this.icon(icon.clone()))
                        .active(page_type == page)
                        .on_click(cx.listener(move |launcher, _, window, cx| {
                            if page == PageType::GamesHome {
                                crate::pages::games_home_page::reset_layout(cx);
                                cx.refresh_windows();
                            }
                            launcher.switch_page(page.clone(), &[], window, cx);
                        })));
                }
                let _ = groups.push(group);
            }

            if !games_mode && !self.recent_instances.is_empty() {
                let mut recent_instances_group = MenuGroup::new(t::instance::recent());

                for instance in &self.recent_instances {
                    let name = instance.name.clone();
                    let active = page_type == PageType::InstancePage { name: name.clone() };
                    let item = MenuGroupItem::new(name.clone())
                        .when(show_sidebar_icons, |this| {
                            let size = (window.rem_size().as_f32() * 0.875).round() as u32;
                            let size_px = px(size as f32);
                            let icon = if let Some(icon) = instance.icon.clone() {
                                let transform = png_render_cache::ImageTransformation::Resize { width: size, height: size };
                                png_render_cache::render_with_transform(icon, transform, cx)
                                    .rounded(cx.theme().radius).size(size_px).min_w(size_px).min_h(size_px).into_any_element()
                            } else {
                                let icon_path = instance.configuration.instance_fallback_icon
                                    .map(|s| s.as_str())
                                    .unwrap_or("icons/box.svg");
                                Icon::default().path(icon_path).size(size_px).min_w(size_px).min_h(size_px).into_any_element()
                            };
                            this.icon(icon)
                        })
                        .active(active)
                        .on_click(cx.listener(move |launcher, _, window, cx| {
                            launcher.switch_page(PageType::InstancePage { name: name.clone() }, &[PageType::Instances], window, cx);
                        }));
                    recent_instances_group = recent_instances_group.child(item);
                }

                let _ = groups.push(recent_instances_group);
            }

            v_flex()
                .flex_1()
                .min_h_0()
                .px_3()
                .gap_y_3()
                .children(groups)
                .overflow_y_scrollbar()
                .into_any_element()
        } else {
            let mut groups = Vec::new();

            for (_group_name, group_entries) in page_groups.iter() {
                let mut group = v_flex()
                    .items_center().min_w_full().max_w_full().w_full()
                    .min_h_0()
                    .gap_y_0p5();
                for (show, title, icon, page) in group_entries {
                    if !*show {
                        continue;
                    }
                    let title = *title;
                    let page = page.clone();
                    group = group.child(div()
                        .id(title)
                        .p_2()
                        .rounded(cx.theme().radius)
                        .when_else(page_type == page, |this| {
                            this.bg(cx.theme().sidebar_accent)
                                .text_color(cx.theme().sidebar_accent_foreground)
                        }, |this| {
                            this.hover(|this| {
                                this.bg(cx.theme().sidebar_accent)
                                    .text_color(cx.theme().sidebar_accent_foreground)
                            })
                        })
                        .child(icon.clone())
                        .tooltip(move |window, cx| {
                            Tooltip::new(title).build(window, cx)
                        })
                        .on_click(cx.listener(move |launcher, _, window, cx| {
                            if page == PageType::GamesHome {
                                crate::pages::games_home_page::reset_layout(cx);
                                cx.refresh_windows();
                            }
                            launcher.switch_page(page.clone(), &[], window, cx);
                        })));
                }
                groups.push(group.into_any_element());
            }

            if !games_mode && !self.recent_instances.is_empty() {
                let mut group = v_flex()
                    .items_center().min_w_full().max_w_full().w_full()
                    .min_h_0()
                    .gap_y_0p5();

                for instance in &self.recent_instances {
                    let name = instance.name.clone();
                    let active = page_type == PageType::InstancePage { name: name.clone() };

                    let size = window.rem_size().as_f32().round() as u32;
                    let size_px = px(size as f32);
                    let icon = if let Some(icon) = instance.icon.clone() {
                        let transform = png_render_cache::ImageTransformation::Resize { width: size, height: size };
                        png_render_cache::render_with_transform(icon, transform, cx)
                            .rounded(cx.theme().radius).size(size_px).min_w(size_px).min_h(size_px).into_any_element()
                    } else {
                        let icon_path = instance.configuration.instance_fallback_icon
                            .map(|s| s.as_str())
                            .unwrap_or("icons/box.svg");
                        Icon::default().path(icon_path).size(size_px).min_w(size_px).min_h(size_px).into_any_element()
                    };

                    group = group.child(div()
                        .id(name.clone())
                        .p_2()
                        .rounded(cx.theme().radius)
                        .when_else(active, |this| {
                            this.bg(cx.theme().sidebar_accent)
                                .text_color(cx.theme().sidebar_accent_foreground)
                        }, |this| {
                            this.hover(|this| {
                                this.bg(cx.theme().sidebar_accent)
                                    .text_color(cx.theme().sidebar_accent_foreground)
                            })
                        })
                        .child(icon)
                        .tooltip({
                            let name = name.clone();
                            move |window, cx| {
                                Tooltip::new(name.clone()).build(window, cx)
                            }
                        })
                        .on_click(cx.listener(move |launcher, _, window, cx| {
                            launcher.switch_page(PageType::InstancePage { name: name.clone() }, &[PageType::Instances], window, cx);
                        })));
                }
                groups.push(group.into_any_element());
            }

            v_flex()
                .items_center().min_w_full().max_w_full().w_full()
                .min_h_0()
                .pt_1()
                .px_2()
                .gap_y_3()
                .children(groups)
                .overflow_y_scrollbar()
                .into_any_element()
        };


        let accounts = self.data.accounts.read(cx);
        let (account_head, account_name) = if let Some(account) = &accounts.selected_account {
            let account_name = account.username((InterfaceConfig::get(cx).hide_usernames || crate::privacy::masking()));
            let hide_skins = InterfaceConfig::get(cx).hide_skins;

            let head = if hide_skins {
                gpui::img(ImageSource::Resource(Resource::Embedded("images/hidden_head.png".into())))
            } else if let Some(head) = &account.head {
                let resize = png_render_cache::ImageTransformation::Resize { width: 32, height: 32 };
                png_render_cache::render_with_transform(head.clone(), resize, cx)
            } else {
                gpui::img(ImageSource::Resource(Resource::Embedded("images/default_head.png".into())))
            };
            (head, account_name)
        } else {
            (
                gpui::img(ImageSource::Resource(Resource::Embedded("images/default_head.png".into()))),
                t::account::none().into(),
            )
        };

        let account_button = h_flex()
            .id("account-button")
            .flex_1()
            .p_2()
            .max_w_full()
            .gap_2()
            .when(!iconized_sidebar, |this| this.justify_start())
            .when(iconized_sidebar, |this| this.justify_center())
            .text_size(rems(0.9375))
            .line_height(rems(1.0))
            .rounded(cx.theme().radius)
            .hover(|this| {
                this.bg(cx.theme().sidebar_accent)
                    .text_color(cx.theme().sidebar_accent_foreground)
            })
            .child(account_head.size_8().min_w_8().min_h_8())
            .when(!iconized_sidebar, |this| this.child(ShrinkingText::new(account_name)))
            .on_click({
                let data = self.data.clone();
                move |_, window, cx| {
                    if data.accounts.read(cx).accounts.is_empty() {
                        crate::root::start_new_account_login(&data.backend_handle, window, cx);
                        return;
                    }

                    let build = crate::modals::accounts::build_accounts_sheet(&data, window, cx);
                    window.open_sheet_at(gpui_component::Placement::Left, cx, build);
                }
            });

        let settings_button = div()
            .id("settings-button")
            .p_2()
            .rounded(cx.theme().radius)
            .hover(|this| {
                this.bg(cx.theme().sidebar_accent)
                    .text_color(cx.theme().sidebar_accent_foreground)
            })
            .child(PandoraIcon::Settings)
            .on_click({
                let data = self.data.clone();
                move |_, window, cx| {
                    crate::settings::open_settings_window(window, &data, cx);
                }
            });
        let bug_report_button = div()
            .id("bug-report-button")
            .p_2()
            .rounded(cx.theme().radius)
            .hover(|this| {
                this.bg(cx.theme().sidebar_accent)
                    .text_color(cx.theme().sidebar_accent_foreground)
            })
            .child(PandoraIcon::Bug)
            .tooltip(move |window, cx| {
                Tooltip::new(t::system::report_bug()).build(window, cx)
            })
            .on_click({
                move |_, window, cx| {
                    open_bug_report_url(window, cx);
                }
            });
        let discord_invite = option_env!("FLUX_DISCORD_INVITE").unwrap_or("");
        let discord_button = div()
            .id("discord-button")
            .p_2()
            .rounded(cx.theme().radius)
            .hover(|this| {
                this.bg(cx.theme().sidebar_accent)
                    .text_color(cx.theme().sidebar_accent_foreground)
            })
            .child(PandoraIcon::Discord)
            .tooltip(move |window, cx| {
                Tooltip::new(t::system::join_discord()).build(window, cx)
            })
            .on_click({
                move |_, _, cx| {
                    cx.open_url(discord_invite);
                }
            });

        let collapse_button = div()
            .id("sidebar-collapse")
            .p_1()
            .flex_shrink_0()
            .rounded(cx.theme().radius)
            .cursor_pointer()
            .text_color(cx.theme().muted_foreground)
            .hover(|this| this.bg(cx.theme().sidebar_accent).text_color(cx.theme().sidebar_accent_foreground))
            .child(Icon::new(if iconized_sidebar { PandoraIcon::PanelLeftOpen } else { PandoraIcon::PanelLeftClose }).size_4())
            .tooltip(move |window, cx| Tooltip::new(if iconized_sidebar { "Expand sidebar" } else { "Collapse sidebar" }).build(window, cx))
            // The header is a window-drag area: keep the press for the button.
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_click(cx.listener(move |launcher, _, _, cx| {
                launcher.toggle_sidebar(cx);
            }));
        let header_drag_state = window.use_keyed_state("sidebar-header-drag-state", cx, |_, _| TitleBarState::default());
        let header = h_flex()
            .id("sidebar-header")
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down_out(window.listener_for(&header_drag_state, |state, _, _, _| {
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
                window.listener_for(&header_drag_state, |state, _, _, _| {
                    state.should_move = true;
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                window.listener_for(&header_drag_state, |state, _, _, _| {
                    state.should_move = false;
                }),
            )
            .on_mouse_move(window.listener_for(&header_drag_state, |state, _, window, _| {
                if state.should_move {
                    state.should_move = false;
                    window.start_window_move();
                }
            }))
            .when_else(cfg!(target_os = "macos"), |this| this.pt(px(41.0)), |this| this.pt(px(14.0)))
            .when_else(iconized_sidebar, |this| this.px_1().flex_col().gap_1(), |this| this.px_3().gap_2())
            .pb_2()
            .w_full()
            .justify_center()
            .items_center()
            .text_size(rems(0.9375))
            .child(gpui::img(ImageSource::Resource(Resource::Embedded("images/branding/logo.png".into())))
                .size_7().min_w_7().min_h_7().object_fit(ObjectFit::Contain))
            .when(!iconized_sidebar, |this| this.child(div()
                .font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BOLD)
                .text_size(rems(1.25))
                .whitespace_nowrap()
                .child("FLUX")))
            .child(collapse_button);
        // Chat and Friends: round buttons above Settings.
        let unread = crate::chat::Network::ALL.iter().any(|n| !crate::chat::state(*n).unread.is_empty());
        let requests = crate::chat::Network::ALL.iter().any(|n| crate::chat::state(*n).requests.iter().any(|r| r.incoming));
        let round = |id: &'static str, icon: PandoraIcon, tip: &'static str, dot: bool, view: crate::chat::panel::ChatView, cx: &mut Context<Self>| {
            div()
                .id(id)
                .relative()
                .size(px(38.))
                .flex_shrink_0()
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .bg(cx.theme().sidebar_accent.opacity(0.6))
                .hover(|this| this.bg(cx.theme().primary).text_color(cx.theme().primary_foreground))
                .child(Icon::new(icon).size_4())
                .when(dot, |this| this.child(div().absolute().top(px(1.)).right(px(1.)).size(px(10.)).rounded_full().bg(gpui::rgb(0xff5d5d))
                    .border_2().border_color(cx.theme().sidebar)))
                .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
                .on_click(cx.listener(move |launcher, _, window, cx| launcher.open_chat(view, window, cx)))
        };
        let social = h_flex()
            .when(iconized_sidebar, |this| this.flex_col())
            .gap_2()
            .pb_2()
            .child(round("chat-button", PandoraIcon::MessageCircle, "Chat", unread, crate::chat::panel::ChatView::Chats, cx))
            .child(round("friends-button", PandoraIcon::Users, "Friends", requests, crate::chat::panel::ChatView::Friends, cx));
        let footer_buttons = h_flex()
            .when(iconized_sidebar, |this| this.flex_col())
            .child(settings_button)
            .when(option_env!("FLUX_ISSUES_URL").is_some(), |this| this.child(bug_report_button))
            .when(!discord_invite.is_empty(), |this| this.child(discord_button));
        let footer = v_flex().pb_2().px_2().items_center().min_w_full().max_w_full().w_full().child(social).child(footer_buttons);
        let mode_switch = {
            let segment = |id: &'static str, label: &'static str, icon: PandoraIcon, mode: LibraryMode, cx: &mut Context<Self>| {
                let active = InterfaceConfig::get(cx).library_mode == mode;
                h_flex()
                    .id(id)
                    .flex_1()
                    .justify_center()
                    .gap_1()
                    .py_1()
                    .rounded(cx.theme().radius)
                    .cursor_pointer()
                    .text_xs()
                    .overflow_hidden()
                    .when(active, |this| this.bg(cx.theme().primary).text_color(cx.theme().primary_foreground))
                    .when(!active, |this| this.text_color(cx.theme().muted_foreground).hover(|this| this.text_color(cx.theme().foreground)))
                    .child(Icon::new(icon).size_4())
                    .when(!iconized_sidebar, |this| this.child(label))
                    .on_click(cx.listener(move |launcher, _, window, cx| launcher.switch_mode(mode, window, cx)))
            };
            h_flex()
                .when(iconized_sidebar, |this| this.flex_col())
                .w_full()
                .p_1()
                .gap_1()
                .rounded(cx.theme().radius)
                .bg(cx.theme().sidebar_accent.opacity(0.5))
                .child(segment("mode-minecraft", "Minecraft", PandoraIcon::Pickaxe, LibraryMode::Minecraft, cx))
                .child(segment("mode-games", "Games", PandoraIcon::Gamepad2, LibraryMode::Games, cx))
        };
        let account_card = v_flex()
            .px_2()
            .pb_3()
            .mb_2()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().sidebar_border)
            .w_full()
            .child(account_button)
            .child(mode_switch);
        let sidebar = v_flex()
            .size_full()
            .min_size_full()
            .max_size_full()
            .bg(cx.theme().sidebar)
            .text_color(cx.theme().sidebar_foreground)
            .child(header)
            .child(account_card)
            .child(sidebar_pages)
            .child(footer);

        ResizePanel::new(&self.sidebar_state, sidebar, self.page.clone().render(&self, window, cx))
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
