//! The Games home screen: the player's library in one of three layouts (a PlayStation-style row
//! of games with the selected game filling the screen, a library grid, or a list with the game
//! beside it), with filters, and each game's details (play time, genre, developer, links,
//! description) from `GameInfo`.

use std::{cell::Cell, path::PathBuf, rc::Rc, time::Duration};

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Icon, Sizable, button::{Button, ButtonVariants}, h_flex, input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem}, scroll::ScrollableElement, v_flex,
};

use crate::{
    component::{reorderable::SimpleDragPreview, resize_panel::{ResizePanel, ResizePanelState}},
    controller_nav::{self, Nav},
    games::{self, GameKind, LibraryGame},
    icon::PandoraIcon,
    interface_config::{GamesLayout, GamesSort, InterfaceConfig},
    pages::{home_page::{FLUX_SKY, play_button}, page::Page},
    root,
    ui::PageType,
};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Tri {
    #[default]
    Any,
    Yes,
    No,
}

#[derive(Clone, Default, PartialEq)]
struct Filter {
    genre: Option<String>,
    platform: Option<String>,
    installed: Tri,
    played: Tri,
    recent: Tri,
    favourite: Tri,
}

impl Filter {
    fn active(&self) -> usize {
        self.genre.is_some() as usize + self.platform.is_some() as usize
            + (self.installed != Tri::Any) as usize + (self.played != Tri::Any) as usize + (self.recent != Tri::Any) as usize
            + (self.favourite != Tri::Any) as usize
    }
}

/// The tiles after the games in the Home row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ShelfExtra {
    /// Opens the Library with every game.
    AllGames,
    /// Opens the "add games to your Custom row" panel.
    AddGames,
}

/// Records where an element was drawn (to keep the selection in view).
fn probe(cell: &Rc<Cell<Option<Bounds<Pixels>>>>) -> impl IntoElement {
    let cell = cell.clone();
    canvas(move |bounds, _, _| cell.set(Some(bounds)), |_, _, _, _| {}).absolute().inset_0()
}

/// A game being dragged to a new place in the Custom row.
#[derive(Clone)]
struct DraggedGame {
    id: String,
    name: SharedString,
}

const STAR: u32 = 0xffcc33;
pub const MIN_LIST_WIDTH: f32 = 200.;

/// Letters and digits only, lowercase: "Skate 3", "skate-3" and "Skate3" all read "skate3".
fn squash(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Does the search match this game? Every word (squashed) must appear in the name, so word order,
/// spaces and punctuation don't matter ("skate 3", "3 skate", "hit&run").
pub fn search_matches(query: &str, name: &str) -> bool {
    let name = squash(name);
    let whole = squash(query);
    whole.is_empty() || name.contains(&whole) || query.split_whitespace().map(squash).filter(|w| !w.is_empty()).all(|w| name.contains(&w))
}

/// The section a game is listed under ("PC (Windows)", "Sony PlayStation 2").
fn section_of(game: &LibraryGame) -> &'static str {
    match game.kind {
        GameKind::Windows | GameKind::SharPort => "PC (Windows)",
        GameKind::Ps1 => "Sony PlayStation",
        GameKind::Ps2 => "Sony PlayStation 2",
        GameKind::Ps3 => "Sony PlayStation 3",
        GameKind::Ps4 => "Sony PlayStation 4",
        GameKind::Psp => "Sony PSP",
        GameKind::Xbox => "Microsoft Xbox",
        GameKind::Xbox360 => "Microsoft Xbox 360",
        GameKind::XboxOne => "Microsoft Xbox One",
        GameKind::Switch => "Nintendo Switch",
    }
}

/// Games grouped by section, sections in name order, games keeping their sort order.
fn sections(list: &[LibraryGame]) -> Vec<(&'static str, Vec<LibraryGame>)> {
    let mut out: Vec<(&'static str, Vec<LibraryGame>)> = Vec::new();
    for game in list {
        let section = section_of(game);
        match out.iter_mut().find(|(s, _)| *s == section) {
            Some((_, games)) => games.push(game.clone()),
            None => out.push((section, vec![game.clone()])),
        }
    }
    out.sort_by_key(|(s, _)| *s);
    out
}

fn platform_of(game: &LibraryGame) -> String {
    match game.kind.platform() {
        Some(p) => p.to_string(),
        None if game.steam_appid.is_some() => "PC (Steam)".into(),
        None => "PC".into(),
    }
}

/// Filter games and put them in the chosen order. Custom shows only the games the player added
/// to their Custom row, in their order.
fn arrange(all: &[LibraryGame], query: &str, f: &Filter, sort: GamesSort, custom: &[String]) -> Vec<LibraryGame> {
    let tri = |want: Tri, is: bool| match want {
        Tri::Any => true,
        Tri::Yes => is,
        Tri::No => !is,
    };
    let mut list: Vec<LibraryGame> = all.iter().filter(|g| {
        search_matches(query, &g.name)
            && f.genre.as_ref().is_none_or(|genre| g.info.as_ref().is_some_and(|i| i.genres.contains(genre)))
            && f.platform.as_ref().is_none_or(|p| &platform_of(g) == p)
            && tri(f.installed, g.installed())
            && tri(f.played, g.play_seconds > 0 || g.last_played.is_some())
            && tri(f.recent, games::recently_played(g))
            && tri(f.favourite, g.favourite)
            && (sort != GamesSort::Custom || custom.contains(&g.id))
    }).cloned().collect();
    match sort {
        GamesSort::Recent => list.sort_by_key(|g| std::cmp::Reverse(games::recent_key(g))),
        GamesSort::Name => list.sort_by_key(|g| g.name.to_lowercase()),
        GamesSort::PlayTime => list.sort_by_key(|g| std::cmp::Reverse(g.play_seconds)),
        GamesSort::Custom => list.sort_by_key(|g| custom.iter().position(|c| c == &g.id).unwrap_or(usize::MAX)),
    }
    list
}

/// A small ★ in a tile's top-right corner.
fn star_badge(size: f32) -> AnyElement {
    div().absolute().top(px(5.)).right(px(5.)).size(px(size)).rounded_full().flex().items_center().justify_center()
        .bg(rgb(0x05060b).opacity(0.78))
        .child(Icon::new(PandoraIcon::Star).size(px(size * 0.62)).text_color(rgb(STAR)))
        .into_any_element()
}

/// The right-click menu of a game.
fn game_menu(menu: PopupMenu, id: &str, cx: &App) -> PopupMenu {
    let Some(game) = games::find(id, cx) else { return menu };
    let in_custom = games::in_custom(id, cx);
    let (fav_id, custom_id, open_id) = (id.to_string(), id.to_string(), id.to_string());
    menu
        .item(PopupMenuItem::new(if game.favourite { "Remove from favourites" } else { "Add to favourites" })
            .icon(PandoraIcon::Star)
            .on_click(move |_, _, cx| games::toggle_favourite(&fav_id, cx)))
        .item(PopupMenuItem::new(if in_custom { "Remove from Custom row" } else { "Add to Custom row" })
            .icon(if in_custom { PandoraIcon::Minus } else { PandoraIcon::Plus })
            .on_click(move |_, _, cx| games::toggle_custom(&custom_id, cx)))
        .item(PopupMenuItem::separator())
        .item(PopupMenuItem::new("Options").icon(PandoraIcon::Settings)
            .on_click(move |_, window, cx| root::switch_page(PageType::Game { id: open_id.clone().into() }, &[], window, cx)))
}

/// The ★ Favourite button (Home screen, List and game page).
pub fn favourite_button(id: impl Into<ElementId>, game: &LibraryGame) -> Button {
    let game_id = game.id.clone();
    Button::new(id)
        .icon(Icon::new(PandoraIcon::Star).when(game.favourite, |i| i.text_color(rgb(STAR))))
        .label(if game.favourite { "Favourite" } else { "Add to favourites" })
        .on_click(move |_, _, cx| games::toggle_favourite(&game_id, cx))
}

pub struct GamesHomePage {
    selected: Option<String>,
    filter: Filter,
    filters_open: bool,
    art_open: bool,
    /// The "add games to your Custom row" panel.
    adding_custom: bool,
    sgdb_key: Entity<InputState>,
    search: Entity<InputState>,
    message: Option<(SharedString, bool)>,
    focus: FocusHandle,
    tiles: ScrollHandle,
    /// The game last opened from the Library (kept in the Home row).
    opened: Option<String>,
    /// An extra tile (All games / Add games) selected in the Home row instead of a game.
    extra: Option<ShelfExtra>,
    grid_scroll: ScrollHandle,
    list_scroll: ScrollHandle,
    /// Where the selected card / row was drawn, and frames left before scrolling it into view.
    selected_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    reveal_frames: u8,
    list_split: ResizePanelState,
    /// Sections the player folded away in the list and library layouts.
    collapsed: std::collections::BTreeSet<&'static str>,
    focused_once: bool,
    _refresh: Task<()>,
}

impl GamesHomePage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search games"));
        let key = InterfaceConfig::get(cx).steamgriddb_key.clone().unwrap_or_default();
        let sgdb_key = cx.new(|cx| InputState::new(window, cx).placeholder("SteamGridDB API key").masked(true).default_value(key));
        cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                cx.notify();
            }
        }).detach();
        // Running/stopped state and play time change while the page is open.
        let _refresh = cx.spawn(async move |page, cx| loop {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            if page.update(cx, |_, cx| cx.notify()).is_err() {
                break;
            }
        });
        let list_width = InterfaceConfig::get(cx).games_list_width.max(MIN_LIST_WIDTH);
        let list_split = ResizePanelState::new(px(list_width), px(MIN_LIST_WIDTH), px(900.)).keep_size()
            .on_resize(|width, _, cx| InterfaceConfig::get_mut(cx).games_list_width = width.as_f32());
        let mut page = Self {
            selected: None,
            filter: Filter::default(),
            filters_open: false,
            art_open: false,
            adding_custom: false,
            sgdb_key,
            search,
            message: None,
            focus: cx.focus_handle(),
            tiles: ScrollHandle::new(),
            opened: None,
            extra: None,
            grid_scroll: ScrollHandle::new(),
            list_scroll: ScrollHandle::new(),
            selected_bounds: Rc::new(Cell::new(None)),
            reveal_frames: 0,
            list_split,
            collapsed: Default::default(),
            focused_once: false,
            _refresh,
        };
        page.selected = page.visible(cx).first().map(|g| g.id.clone());
        page
    }

    /// The games shown, filtered and sorted. The Home row shows only the first few
    /// (`games_row_length`); the library and list show them all.
    fn visible(&self, cx: &App) -> Vec<LibraryGame> {
        let config = InterfaceConfig::get(cx);
        let query = self.search.read(cx).value().to_string();
        let mut list = arrange(&config.games, &query, &self.filter, config.games_sort, &config.custom_order);
        if config.games_layout == GamesLayout::Shelf {
            let length = config.games_row_length.max(1) as usize;
            list.truncate(length);
            // A game opened from the Library is always in the row (first), whatever the sort.
            if let Some(id) = &self.opened && !list.iter().any(|g| &g.id == id) && let Some(game) = config.games.iter().find(|g| &g.id == id) {
                list.insert(0, game.clone());
                list.truncate(length);
            }
        }
        list
    }

    /// Open a game from the Library or List in the Home view: it joins the Home row's recent games.
    fn open_in_home(&mut self, id: &str, cx: &mut Context<Self>) {
        games::mark_opened(id, cx);
        self.opened = Some(id.to_string());
        InterfaceConfig::get_mut(cx).games_layout = GamesLayout::Shelf;
        self.select(id, cx);
        if let Some(at) = self.visible(cx).iter().position(|g| g.id == id) {
            self.tiles.scroll_to_item(at);
        }
        cx.refresh_windows();
    }

    /// Every game matching search and filters (the Home row may show fewer).
    fn matching_count(&self, cx: &App) -> usize {
        let config = InterfaceConfig::get(cx);
        let query = self.search.read(cx).value().to_string();
        arrange(&config.games, &query, &self.filter, config.games_sort, &config.custom_order).len()
    }

    fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selected = Some(id.to_string());
        self.extra = None;
        self.message = None;
        self.reveal_frames = 2;
        cx.notify();
    }

    /// The extra tiles at the end of the Home row.
    fn extras(&self, cx: &App) -> Vec<ShelfExtra> {
        let mut out = Vec::new();
        if self.matching_count(cx) > self.visible(cx).len() {
            out.push(ShelfExtra::AllGames);
        }
        if InterfaceConfig::get(cx).games_sort == GamesSort::Custom {
            out.push(ShelfExtra::AddGames);
        }
        out
    }

    /// The games in the order they're drawn: the row, or the library / list by platform section
    /// (folded sections skipped).
    fn sections_shown(&self, cx: &App) -> Vec<Vec<String>> {
        let list = self.visible(cx);
        if InterfaceConfig::get(cx).games_layout == GamesLayout::Shelf {
            return vec![list.into_iter().map(|g| g.id).collect()];
        }
        sections(&list).into_iter().filter(|(s, _)| !self.collapsed.contains(s))
            .map(|(_, games)| games.into_iter().map(|g| g.id).collect()).collect()
    }

    /// Move the selection by `delta` games (in the Home row, on to the extra tiles after them).
    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let order: Vec<String> = self.sections_shown(cx).concat();
        let shelf = InterfaceConfig::get(cx).games_layout == GamesLayout::Shelf;
        let extras = if shelf { self.extras(cx) } else { Vec::new() };
        let len = order.len() + extras.len();
        if len == 0 {
            return;
        }
        let at = match self.extra {
            Some(extra) if shelf => order.len() + extras.iter().position(|e| *e == extra).unwrap_or(0),
            _ => self.selected.as_ref().and_then(|id| order.iter().position(|g| g == id)).unwrap_or(0),
        } as isize;
        let next = (at + delta).clamp(0, len as isize - 1) as usize;
        self.tiles.scroll_to_item(next);
        if next < order.len() {
            let id = order[next].clone();
            self.select(&id, cx);
        } else {
            self.extra = Some(extras[next - order.len()]);
            self.message = None;
            cx.notify();
        }
    }

    /// Up / down in the Library grid: the card above or below (across platform sections).
    fn step_rows(&mut self, down: bool, window: &Window, cx: &mut Context<Self>) {
        let sidebar = 0.0f32; // no sidebar: the top bar holds navigation
        let width = (window.viewport_size().width.as_f32() - sidebar - 48. - 12.).max(216.);
        let cols = ((width + 16.) / 216.).floor().max(1.) as usize;
        let sections = self.sections_shown(cx);
        let Some((s, i)) = sections.iter().enumerate().find_map(|(s, games)| {
            games.iter().position(|g| Some(g) == self.selected.as_ref()).map(|i| (s, i))
        }) else {
            return self.step(0, cx);
        };
        let (len, col) = (sections[s].len(), i % cols);
        let target = if down {
            if i + cols < len {
                Some((s, i + cols))
            } else if i / cols < (len - 1) / cols {
                Some((s, len - 1))
            } else {
                sections.get(s + 1).map(|next| (s + 1, col.min(next.len() - 1)))
            }
        } else if i >= cols {
            Some((s, i - cols))
        } else if s > 0 {
            let prev = &sections[s - 1];
            let last_row = (prev.len() - 1) / cols * cols;
            Some((s - 1, (last_row + col).min(prev.len() - 1)))
        } else {
            None
        };
        if let Some((s, i)) = target {
            let id = sections[s][i].clone();
            self.select(&id, cx);
        }
    }

    /// Enter / A: play the selected game, or open the selected extra tile. In the Library it
    /// opens the game in the Home view.
    fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let layout = InterfaceConfig::get(cx).games_layout;
        if layout == GamesLayout::Shelf && let Some(extra) = self.extra {
            match extra {
                ShelfExtra::AllGames => {
                    self.extra = None;
                    InterfaceConfig::get_mut(cx).games_layout = GamesLayout::Grid;
                    self.reveal_frames = 2;
                    cx.refresh_windows();
                },
                ShelfExtra::AddGames => {
                    self.adding_custom = true;
                    cx.notify();
                },
            }
            return;
        }
        let Some(game) = self.selected.as_ref().and_then(|id| games::find(id, cx)) else { return };
        if layout == GamesLayout::Grid {
            self.open_in_home(&game.id, cx);
        } else if games::problem(&game, cx).is_some() && !games::is_running(&game.id) {
            root::switch_page(PageType::Game { id: game.id.clone().into() }, &[], window, cx);
        } else {
            self.play(&game, cx);
        }
    }

    /// A key or controller press on the Games screen. False: not used here.
    pub fn controller(&mut self, nav: Nav, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let layout = InterfaceConfig::get(cx).games_layout;
        let selected = self.selected.clone().filter(|_| self.extra.is_none() || layout != GamesLayout::Shelf);
        match nav {
            Nav::Left => self.step(-1, cx),
            Nav::Right => self.step(1, cx),
            Nav::Up if layout == GamesLayout::Grid => self.step_rows(false, window, cx),
            Nav::Down if layout == GamesLayout::Grid => self.step_rows(true, window, cx),
            Nav::Up => self.step(-1, cx),
            Nav::Down => self.step(1, cx),
            Nav::Confirm => self.activate(window, cx),
            Nav::Back => {
                if !(self.filters_open || self.art_open || self.adding_custom) {
                    return false;
                }
                self.filters_open = false;
                self.art_open = false;
                self.adding_custom = false;
                cx.notify();
            },
            Nav::X => match selected {
                Some(id) => root::switch_page(PageType::Game { id: id.into() }, &[], window, cx),
                None => return false,
            },
            Nav::Y => match selected {
                Some(id) => games::toggle_favourite(&id, cx),
                None => return false,
            },
            Nav::View => match selected {
                Some(id) => {
                    games::toggle_custom(&id, cx);
                    let now_in = games::in_custom(&id, cx);
                    self.message = Some((if now_in { "Added to your Custom row" } else { "Taken out of your Custom row" }.into(), false));
                },
                None => return false,
            },
            Nav::PrevTab | Nav::NextTab => {
                let order = [GamesLayout::Shelf, GamesLayout::Grid, GamesLayout::List];
                let at = order.iter().position(|l| *l == layout).unwrap_or(0);
                let next = if nav == Nav::NextTab { (at + 1) % 3 } else { (at + 2) % 3 };
                InterfaceConfig::get_mut(cx).games_layout = order[next];
                self.extra = None;
                self.reveal_frames = 2;
                cx.refresh_windows();
            },
            Nav::PrevPage | Nav::NextPage | Nav::Start => return false,
        }
        true
    }

    /// Scroll the Library or List so the selected game is on screen.
    fn reveal_selected(&mut self, layout: GamesLayout) {
        let handle = match layout {
            GamesLayout::Grid => &self.grid_scroll,
            GamesLayout::List => &self.list_scroll,
            GamesLayout::Shelf => return,
        };
        let Some(item) = self.selected_bounds.get() else { return };
        let view = handle.bounds();
        if view.size.height <= px(0.) {
            return;
        }
        let margin = px(24.);
        let mut offset = handle.offset();
        if item.bottom() + margin > view.bottom() {
            offset.y -= item.bottom() + margin - view.bottom();
        } else if item.top() - margin < view.top() {
            offset.y += view.top() - (item.top() - margin);
        } else {
            return;
        }
        offset.y = offset.y.min(px(0.));
        handle.set_offset(offset);
    }

    fn play(&mut self, game: &LibraryGame, cx: &mut Context<Self>) {
        if games::is_running(&game.id) {
            games::stop_game(&game.id);
            self.message = Some(("Stopped".into(), false));
        } else {
            self.message = Some(match games::start_game(game, cx) {
                Ok(m) => (m.into(), false),
                Err(e) => (e.into(), true),
            });
        }
        cx.notify();
    }

    /// In Custom order a game can be dragged onto another to take its place.
    fn draggable<E: StatefulInteractiveElement + Styled + 'static>(&self, element: E, game: &LibraryGame, custom: bool, cx: &mut Context<Self>) -> E {
        if !custom {
            return element;
        }
        let target = game.id.clone();
        element
            .on_drag(DraggedGame { id: game.id.clone(), name: game.name.clone().into() }, |dragged, _, _, cx| {
                cx.new(|_| SimpleDragPreview { name: dragged.name.clone() })
            })
            .drag_over::<DraggedGame>(|style, _, _, _| style.opacity(0.45))
            .on_drop(cx.listener(move |page, dragged: &DraggedGame, _, cx| {
                games::move_in_custom(&dragged.id, &target, cx);
                page.select(&dragged.id, cx);
            }))
    }
}

/// What each controller button does here, while a controller is plugged in.
fn controller_hints(layout: GamesLayout) -> AnyElement {
    let hint = |button: &'static str, label: &'static str| h_flex().gap_1p5().items_center()
        .child(div().px_1p5().rounded_md().bg(gpui::white().opacity(0.16)).text_xs().font_weight(FontWeight::BOLD).child(button))
        .child(div().text_xs().child(label));
    let confirm = if layout == GamesLayout::Grid { "Open" } else { "Play / open" };
    h_flex().absolute().bottom_3().right_4().gap_4().px_3().py_1p5().rounded_lg().bg(rgb(0x05060b).opacity(0.82))
        .child(hint("A", confirm))
        .child(hint("B", "Back"))
        .child(hint("X", "Options"))
        .child(hint("Y", "Favourite"))
        .child(hint("View", "Custom row"))
        .child(hint("LB/RB", "Home · Library · List"))
        .child(hint("LT/RT", "Menu"))
        .child(hint("R-stick", "Scroll"))
        .into_any_element()
}

static OPEN_ART: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Tools › Art: open the game-art panel (SteamGridDB key, refresh all art) on the Games screen.
pub fn open_art_panel(cx: &mut App) {
    OPEN_ART.store(true, std::sync::atomic::Ordering::Relaxed);
    cx.refresh_windows();
}

/// Open the Games screen in the player's chosen default layout.
pub fn reset_layout(cx: &mut App) {
    let config = InterfaceConfig::get_mut(cx);
    config.games_layout = config.games_default_layout;
}

impl Page for GamesHomePage {
    // Home / Library are in the top bar; Library's grid / list switch is in its toolbar.
    fn controls(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }

    fn scrollable(&self, _cx: &App) -> bool {
        false
    }
}

// ------------------------------------------------------------------------------- shared pieces

fn image_of(path: PathBuf) -> Img {
    img(ImageSource::Resource(Resource::Path(path.into())))
}

/// A game's tile picture, or its initials on a colour when it has no art yet.
/// Square art at an exact size: portrait box art is cropped to the square (an image left to size
/// itself keeps its own shape and spills out of the tile).
fn tile_art(game: &LibraryGame, size: f32) -> AnyElement {
    match game.tile_image() {
        // aspect_square: otherwise the image takes its own (e.g. portrait) shape whatever size it's given.
        Some(p) => image_of(p).w(px(size)).h(px(size)).aspect_square().flex_shrink_0().rounded_xl().object_fit(ObjectFit::Cover).into_any_element(),
        None => {
            let initials: String = game.name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
            div().w(px(size)).h(px(size)).rounded_xl().flex().items_center().justify_center().bg(rgb(0x1d2540))
                .font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BLACK).text_size(px(28.)).text_color(crate::pages::home_page::sky()).child(initials.to_uppercase())
                .into_any_element()
        },
    }
}

/// The selected game's picture behind everything, fading in when the selection changes.
fn backdrop(game: Option<&LibraryGame>, size: Size<Pixels>, cx: &mut App) -> AnyElement {
    let Some(game) = game else {
        return crate::backgrounds::render_scene(size, "galaxy", cx);
    };
    let art = game.background.as_ref().filter(|p| p.is_file() && !game.scene.starts_with('!')).cloned();
    let layer = match art {
        Some(p) => div().absolute().inset_0().child(image_of(p).size_full().object_fit(ObjectFit::Cover)).into_any_element(),
        None => crate::backgrounds::render_scene(size, game.scene.trim_start_matches('!'), cx),
    };
    div().absolute().inset_0()
        .child(div().absolute().inset_0().child(layer).with_animation(
            SharedString::from(format!("backdrop-{}", game.id)),
            Animation::new(Duration::from_millis(550)).with_easing(ease_out_quint()),
            |this, t| this.opacity(t),
        ))
        // Darken the left and bottom where the text sits.
        .child(div().absolute().inset_0().bg(linear_gradient(90., linear_color_stop(rgb(0x05060b).opacity(0.92), 0.), linear_color_stop(rgb(0x05060b).opacity(0.15), 0.75))))
        .child(div().absolute().inset_0().bg(linear_gradient(0., linear_color_stop(rgb(0x05060b).opacity(0.95), 0.), linear_color_stop(rgb(0x05060b).opacity(0.0), 0.6))))
        .into_any_element()
}

/// The big title: the game's logo when it has one, else its name in the pixel font.
/// The year in a release date, however it's written ("2010-05-11", "27 Sep, 2022").
fn year_of(date: &str) -> Option<String> {
    let digits: Vec<char> = date.chars().collect();
    digits.windows(4).find(|w| w.iter().all(|c| c.is_ascii_digit()) && (w[0] == '1' || w[0] == '2')).map(|w| w.iter().collect())
}

/// A logo at the exact size it's shown: fitted into max_w x max_h keeping its shape, so the text and
/// buttons under it are placed below the real picture (a box alone doesn't stop an image growing).
pub fn logo_image(path: &std::path::Path, max_w: f32, max_h: f32) -> AnyElement {
    static SIZES: std::sync::Mutex<Option<std::collections::HashMap<PathBuf, (u32, u32)>>> = std::sync::Mutex::new(None);
    let (w, h) = {
        let mut cache = SIZES.lock().unwrap();
        let map = cache.get_or_insert_with(Default::default);
        *map.entry(path.to_path_buf()).or_insert_with(|| image::image_dimensions(path).unwrap_or((4, 1)))
    };
    let scale = (max_w / w.max(1) as f32).min(max_h / h.max(1) as f32);
    let (dw, dh) = ((w as f32 * scale).floor(), (h as f32 * scale).floor());
    div().flex_shrink_0().w(px(dw)).h(px(dh)).overflow_hidden()
        .child(image_of(path.to_path_buf()).w(px(dw)).h(px(dh)).object_fit(ObjectFit::Contain))
        .into_any_element()
}

fn headline(game: &LibraryGame, size: f32) -> AnyElement {
    match game.logo.as_ref().filter(|p| p.is_file()) {
        Some(logo) => logo_image(logo, size * 7., size * 2.6),
        None => {
            let words: Vec<&str> = game.name.split_whitespace().collect();
            let (head, last) = match words.split_last() {
                Some((last, head)) if !head.is_empty() => (head.join(" ").to_uppercase(), last.to_uppercase()),
                _ => (String::new(), game.name.to_uppercase()),
            };
            crate::pages::home_page::display_title(&head, &last, size).into_any_element()
        },
    }
}

/// The Details table (left) and the Description (beside it), as in the game's library entry.
pub fn details_panel(game: &LibraryGame, cx: &App) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let info = game.info.clone().unwrap_or_default();
    let join = |v: &[String]| if v.is_empty() { "—".to_string() } else { v.join(", ") };
    let row = |label: &'static str, value: AnyElement| h_flex().gap_3().items_start()
        .child(div().w(px(110.)).flex_shrink_0().text_sm().text_color(muted).child(label))
        .child(div().flex_1().min_w_0().text_sm().child(value));
    let text = |s: String| div().child(s).into_any_element();
    let sky: Hsla = crate::pages::home_page::sky().into();

    let tags = h_flex().flex_wrap().gap_1p5().children(info.tags.iter().take(10).map(|t| {
        div().px_2().py_0p5().rounded_md().bg(gpui::white().opacity(0.08)).text_xs().child(t.clone())
    })).into_any_element();
    let links = v_flex().gap_0p5().children(info.links.iter().enumerate().map(|(i, (label, url))| {
        let url = url.clone();
        div().id(("game-link", i)).text_color(sky).cursor_pointer().hover(|s| s.underline()).child(label.clone())
            .on_click(move |_, _, cx| cx.open_url(&url))
    })).into_any_element();

    let details = v_flex().w(px(380.)).flex_shrink_0().gap_2()
        .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(muted).child("DETAILS"))
        .child(row("Time played", text(games::play_time_text(game.play_seconds))))
        .child(row("Last played", text(games::last_played_text(game.last_played))))
        .child(row("Library", text(game.library_name().to_string())))
        .child(row("Platform", text(platform_of(game))))
        .child(row("Genre", text(join(&info.genres))))
        .child(row("Developer", text(join(&info.developers))))
        .child(row("Publisher", text(join(&info.publishers))))
        .child(row("Release date", text(info.release_date.clone().unwrap_or_else(|| "—".into()))))
        .when(!info.tags.is_empty(), |this| this.child(row("Tags", tags)))
        .when(!info.links.is_empty(), |this| this.child(row("Links", links)));

    let description = v_flex().flex_1().min_w_0().gap_2()
        .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(muted).child("DESCRIPTION"))
        .child(div().text_sm().line_height(rems(1.45)).child(if info.description.is_empty() {
            if game.info_checked { "No description found for this game.".to_string() } else { "Looking up this game's details...".to_string() }
        } else {
            info.description.clone()
        }))
        .when(!info.source.is_empty(), |this| this.child(div().text_xs().text_color(muted).child(format!("From {}", info.source))));

    h_flex().gap_8().items_start().child(details).child(description).into_any_element()
}

// ------------------------------------------------------------------------------------- render

impl Render for GamesHomePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focused_once {
            self.focused_once = true;
            window.focus(&self.focus, cx);
        }
        if OPEN_ART.swap(false, std::sync::atomic::Ordering::Relaxed) {
            self.art_open = true;
        }
        let layout = InterfaceConfig::get(cx).games_layout;
        // Bring the selected card into view once it has been drawn where it now is.
        if self.reveal_frames > 0 {
            self.reveal_frames -= 1;
            if self.reveal_frames == 0 {
                self.reveal_selected(layout);
            }
            window.request_animation_frame();
        }
        let list = self.visible(cx);
        let total = games::games(cx).len();
        if self.selected.as_ref().is_none_or(|id| !list.iter().any(|g| &g.id == id)) {
            self.selected = list.first().map(|g| g.id.clone());
        }
        let selected = self.selected.as_ref().and_then(|id| list.iter().find(|g| &g.id == id)).cloned();

        let viewport = window.viewport_size();
        let sidebar = 0.0f32; // no sidebar: the top bar holds navigation
        let size = size(viewport.width - px(sidebar), viewport.height - px(44.));

        if total == 0 {
            return v_flex().size_full().items_center().justify_center().gap_4()
                .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BLACK).text_size(px(36.)).child("No games yet"))
                .child(div().text_sm().text_color(cx.theme().muted_foreground).child("Add your own games: Windows games, Steam games, or your own console game backups."))
                .child(Button::new("add-first").primary().label("Add a game").on_click(|_, window, cx| root::switch_page(PageType::AddGame, &[], window, cx)))
                .into_any_element();
        }

        let toolbar = self.toolbar(&list, total, cx);
        let filters = self.filters_open.then(|| self.filter_panel(cx));
        let art = self.art_open.then(|| self.art_panel(cx));
        let custom_panel = (self.adding_custom && InterfaceConfig::get(cx).games_sort == GamesSort::Custom).then(|| self.custom_panel(cx));

        let body = match layout {
            GamesLayout::Shelf => self.render_shelf(&list, selected.as_ref(), size.height - px(if self.filters_open { 300. } else { 60. }), cx),
            GamesLayout::Grid => self.render_grid(&list, selected.as_ref(), cx),
            GamesLayout::List => self.render_list(&list, selected.as_ref(), size.height - px(60.), cx),
        };

        let page = cx.entity();
        div()
            .id("games-home")
            .track_focus(&self.focus)
            .on_key_down(move |event, window, cx| {
                let nav = match event.keystroke.key.as_str() {
                    "left" => Nav::Left,
                    "right" => Nav::Right,
                    "up" => Nav::Up,
                    "down" => Nav::Down,
                    "enter" => Nav::Confirm,
                    "escape" => Nav::Back,
                    _ => return,
                };
                if page.update(cx, |page, cx| page.controller(nav, window, cx)) {
                    cx.stop_propagation();
                }
            })
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(rgb(0x05060b))
            .when(layout != GamesLayout::List, |this| this.child(backdrop(selected.as_ref(), size, cx)))
            .child(v_flex().absolute().inset_0()
                .child(toolbar)
                .children(filters)
                .children(art)
                .children(custom_panel)
                .child(div().flex_1().min_h_0().child(body)))
            .when(controller_nav::connected(), |this| this.child(controller_hints(layout)))
            .into_any_element()
    }
}

impl GamesHomePage {
    fn toolbar(&mut self, list: &[LibraryGame], total: usize, cx: &mut Context<Self>) -> AnyElement {
        let active = self.filter.active();
        let config = InterfaceConfig::get(cx);
        let custom = config.games_sort == GamesSort::Custom;
        let matching = self.matching_count(cx);
        let count = if custom {
            format!("{matching} in your Custom row")
        } else if list.len() < matching {
            format!("{} of {matching} games in the row", list.len())
        } else if matching == total {
            format!("{total} games")
        } else {
            format!("{matching} of {total} games")
        };
        let layout = config.games_layout;
        let view = |id: &'static str, label: &'static str, value: GamesLayout, cx: &mut Context<Self>| {
            let on = layout == value;
            div().id(id).px_3().py_1().rounded_md().cursor_pointer().text_sm().font_weight(FontWeight::SEMIBOLD)
                .when(on, |this| this.bg(cx.theme().primary).text_color(cx.theme().primary_foreground))
                .when(!on, |this| this.text_color(cx.theme().muted_foreground))
                .child(label)
                .on_click(move |_, _, cx| {
                    let config = InterfaceConfig::get_mut(cx);
                    config.games_layout = value;
                    config.games_library_view = value;
                    cx.refresh_windows();
                })
        };
        h_flex().px_6().pt_4().pb_2().gap_3().items_center()
            .when(layout != GamesLayout::Shelf, |this| this.child(div().font_weight(FontWeight::BOLD).text_xl().mr_1().child("Library")))
            .child(Button::new("filters").label(if active > 0 { format!("Filters ({active})") } else { "Filters".into() })
                .when(self.filters_open || active > 0, |b| b.primary())
                .on_click(cx.listener(|page, _, _, cx| {
                    page.filters_open = !page.filters_open;
                    cx.notify();
                })))
            .when(custom, |this| this.child(Button::new("add-custom").label("+ Add to Custom row").when(self.adding_custom, |b| b.primary())
                .on_click(cx.listener(|page, _, _, cx| {
                    page.adding_custom = !page.adding_custom;
                    cx.notify();
                }))))
            .child(div().text_xs().text_color(cx.theme().muted_foreground).whitespace_nowrap().child(count))
            .child(div().flex_1())
            .when(layout != GamesLayout::Shelf, |this| this.child(h_flex().p_0p5().gap_0p5().rounded_lg().bg(cx.theme().secondary)
                .child(view("view-grid", "Grid", GamesLayout::Grid, cx))
                .child(view("view-list", "List", GamesLayout::List, cx))))
            .when(self.art_open, |this| this.child(Button::new("art").primary().label("Close art").on_click(cx.listener(|page, _, _, cx| {
                page.art_open = false;
                cx.notify();
            }))))
            .child(Button::new("add-game").ghost().label("+ Add game").on_click(|_, window, cx| root::switch_page(PageType::AddGame, &[], window, cx)))
            .into_any_element()
    }

    fn filter_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, border, primary, background) = (theme.muted_foreground, theme.border, theme.primary, theme.background);
        let all = games::games(cx);
        let mut genres: Vec<String> = all.iter().filter_map(|g| g.info.as_ref()).flat_map(|i| i.genres.clone()).collect();
        genres.sort();
        genres.dedup();
        let mut platforms: Vec<String> = all.iter().map(platform_of).collect();
        platforms.sort();
        platforms.dedup();
        let (sort, row_length) = (InterfaceConfig::get(cx).games_sort, InterfaceConfig::get(cx).games_row_length);

        let chip = move |id: SharedString, label: String, on: bool| div().id(id).px_3().py_1().rounded_full().text_xs().cursor_pointer()
            .border_1().border_color(if on { primary } else { border })
            .bg(if on { primary.opacity(0.3) } else { background.opacity(0.6) })
            .child(label);
        let group = |title: &'static str| h_flex().gap_2().items_center().flex_wrap()
            .child(div().w(px(110.)).text_xs().font_weight(FontWeight::SEMIBOLD).text_color(muted).child(title));

        let genre_row = group("Genre").children(std::iter::once(None).chain(genres.into_iter().map(Some)).enumerate().map(|(i, g)| {
            let on = self.filter.genre == g;
            chip(format!("genre-{i}").into(), g.clone().unwrap_or_else(|| "All".into()), on)
                .on_click(cx.listener(move |page, _, _, cx| {
                    page.filter.genre = g.clone();
                    cx.notify();
                }))
        }));
        let platform_row = group("Platform").children(std::iter::once(None).chain(platforms.into_iter().map(Some)).enumerate().map(|(i, p)| {
            let on = self.filter.platform == p;
            chip(format!("platform-{i}").into(), p.clone().unwrap_or_else(|| "All".into()), on)
                .on_click(cx.listener(move |page, _, _, cx| {
                    page.filter.platform = p.clone();
                    cx.notify();
                }))
        }));
        let tri_row = |title: &'static str, key: &'static str, current: Tri, labels: [&'static str; 3], set: fn(&mut Filter, Tri), cx: &mut Context<Self>| {
            group(title).children([Tri::Any, Tri::Yes, Tri::No].into_iter().zip(labels).map(|(value, label)| {
                chip(format!("{key}-{label}").into(), label.to_string(), current == value)
                    .on_click(cx.listener(move |page, _, _, cx| {
                        set(&mut page.filter, value);
                        cx.notify();
                    }))
            }))
        };
        let favourites = tri_row("Favourites", "fav", self.filter.favourite, ["All", "Favourites", "Not favourites"], |f, v| f.favourite = v, cx);
        let installed = tri_row("Installed", "inst", self.filter.installed, ["All", "Installed", "Not installed"], |f, v| f.installed = v, cx);
        let played = tri_row("Played", "played", self.filter.played, ["All", "Played", "Never played"], |f, v| f.played = v, cx);
        let recent = tri_row("Recently played", "recent", self.filter.recent, ["All", "Last 2 weeks", "Not recently"], |f, v| f.recent = v, cx);
        let sort_row = group("Sort by").children([
            (GamesSort::Recent, "Recently played"), (GamesSort::Name, "Name"), (GamesSort::PlayTime, "Time played"), (GamesSort::Custom, "Custom"),
        ].into_iter().map(|(value, label)| {
            chip(format!("sort-{label}").into(), label.to_string(), sort == value)
                .on_click(cx.listener(move |page, _, _, cx| {
                    InterfaceConfig::get_mut(cx).games_sort = value;
                    page.adding_custom = value == GamesSort::Custom && InterfaceConfig::get(cx).custom_order.is_empty();
                    cx.notify();
                }))
        })).when(sort == GamesSort::Custom, |this| this.child(div().text_xs().text_color(muted)
            .child("Only the games you added. Drag them to reorder; right-click a game to take it out.")));
        let row_row = group("Home row").children([6u32, 8, 12, 16, 24, 32, 48].into_iter().map(|n| {
            chip(format!("row-{n}").into(), format!("{n} games"), row_length == n)
                .on_click(cx.listener(move |_, _, _, cx| {
                    InterfaceConfig::get_mut(cx).games_row_length = n;
                    cx.notify();
                }))
        }));

        v_flex().mx_6().mb_2().p_4().gap_2p5().rounded_xl().bg(rgb(0x0b0e18).opacity(0.92)).border_1().border_color(border)
            .child(sort_row)
            .child(favourites)
            .child(genre_row)
            .child(platform_row)
            .child(installed)
            .child(played)
            .child(recent)
            .child(row_row)
            .child(h_flex().child(Button::new("clear-filters").small().ghost().label("Clear filters").on_click(cx.listener(|page, _, _, cx| {
                page.filter = Filter::default();
                cx.notify();
            }))))
            .into_any_element()
    }

    /// Every game not yet in the Custom row, to click into it.
    fn custom_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let custom = InterfaceConfig::get(cx).custom_order.clone();
        let mut rest: Vec<LibraryGame> = games::games(cx).iter().filter(|g| !custom.contains(&g.id)).cloned().collect();
        rest.sort_by_key(|g| g.name.to_lowercase());
        let tiles = h_flex().flex_wrap().gap_2().children(rest.iter().map(|game| {
            let id = game.id.clone();
            h_flex().id(SharedString::from(format!("add-custom-{}", game.id))).gap_2().items_center().pr_3().rounded_lg().cursor_pointer()
                .bg(gpui::white().opacity(0.05)).hover(|s| s.bg(gpui::white().opacity(0.12)))
                .child(div().size(px(40.)).rounded_lg().overflow_hidden().child(tile_art(game, 40.)))
                .child(div().text_sm().child(game.name.clone()))
                .child(Icon::new(PandoraIcon::Plus).size_4().text_color(muted))
                .on_click(cx.listener(move |page, _, _, cx| {
                    games::toggle_custom(&id, cx);
                    page.select(&id, cx);
                }))
        }));
        v_flex().mx_6().mb_2().p_4().gap_2().rounded_xl().bg(rgb(0x0b0e18).opacity(0.94)).border_1().border_color(cx.theme().border)
            .child(h_flex().items_center()
                .child(div().flex_1().text_sm().font_weight(FontWeight::SEMIBOLD).child("Add games to your Custom row"))
                .child(Button::new("custom-done").small().primary().label("Done").on_click(cx.listener(|page, _, _, cx| {
                    page.adding_custom = false;
                    cx.notify();
                }))))
            .child(div().text_xs().text_color(muted).child(if rest.is_empty() { "Every game is in your Custom row." } else { "Click a game to add it to the end of the row." }))
            .when(!rest.is_empty(), |this| this.child(div().h(px(220.)).child(v_flex().size_full().child(tiles).overflow_y_scrollbar())))
            .into_any_element()
    }

    fn section_header(&self, section: &'static str, count: usize, cx: &mut Context<Self>) -> AnyElement {
        let open = !self.collapsed.contains(section);
        h_flex().id(SharedString::from(format!("section-{section}"))).gap_2().items_center().px_2().py_1p5().cursor_pointer().rounded_md()
            .hover(|s| s.bg(gpui::white().opacity(0.05)))
            .child(div().w(px(14.)).text_color(crate::pages::home_page::sky()).child(if open { "v" } else { ">" }))
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().text_ellipsis().child(format!("{section} ({count})")))
            .on_click(cx.listener(move |page, _, _, cx| {
                if !page.collapsed.remove(section) {
                    page.collapsed.insert(section);
                }
                cx.notify();
            }))
            .into_any_element()
    }

    fn art_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let sky: Hsla = crate::pages::home_page::sky().into();
        let has_key = InterfaceConfig::get(cx).steamgriddb_key.as_ref().is_some_and(|k| !k.trim().is_empty());
        v_flex().mx_6().mb_2().p_4().gap_2().rounded_xl().bg(rgb(0x0b0e18).opacity(0.92)).border_1().border_color(cx.theme().border)
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Game art"))
            .child(div().text_sm().text_color(muted).child(
                "Flux picks the sharpest art it can find: Steam's own banners, logos and 2x box art for Steam games, and libretro's \
                 1024-pixel box art and screenshots for console games. For the best fit everywhere (wide backgrounds, square tiles \
                 and logos for nearly every game, console ones included), add a free SteamGridDB API key.",
            ))
            .child(h_flex().gap_2().items_center()
                .child(div().w(px(340.)).child(Input::new(&self.sgdb_key).small()))
                .child(Button::new("save-key").small().primary().label(if has_key { "Save and refresh art" } else { "Save key" }).on_click(cx.listener(|page, _, _, cx| {
                    let key = page.sgdb_key.read(cx).value().trim().to_string();
                    crate::secrets::write_string(crate::secrets::STEAMGRIDDB, Some(&key));
                    InterfaceConfig::get_mut(cx).steamgriddb_key = (!key.is_empty()).then_some(key);
                    games::refresh_all_art(true, cx);
                    page.message = Some(("Looking up art for every game...".into(), false));
                    page.art_open = false;
                    cx.notify();
                })))
                .child(Button::new("refresh-art").small().label("Refresh all art").on_click(cx.listener(|page, _, _, cx| {
                    games::refresh_all_art(true, cx);
                    page.message = Some(("Looking up art for every game...".into(), false));
                    cx.notify();
                }))))
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).pt_1().child("Get your free key (about a minute):"))
            .children([
                "1. Open steamgriddb.com and click Log in (top right). It signs in with your Steam account.",
                "2. Click your profile picture (top right), then Preferences.",
                "3. Open the API tab and click Generate API Key.",
                "4. Copy the key, paste it above and click Save.",
            ].map(|step| div().text_sm().text_color(muted).child(step)))
            .child(h_flex().gap_4()
                .child(div().id("sgdb-site").text_xs().text_color(sky).cursor_pointer().hover(|s| s.underline())
                    .child("Open steamgriddb.com")
                    .on_click(|_, _, cx| cx.open_url("https://www.steamgriddb.com/")))
                .child(div().id("sgdb-link").text_xs().text_color(sky).cursor_pointer().hover(|s| s.underline())
                    .child("Open the API key page (after logging in)")
                    .on_click(|_, _, cx| cx.open_url("https://www.steamgriddb.com/profile/preferences/api"))))
            .child(div().text_xs().text_color(muted).child(
                "Your key stays on this PC, in Windows Credential Manager. Everyone uses their own key, so it's never shared.",
            ))
            .into_any_element()
    }

    /// What to show when no game matches.
    fn empty_message(&self, cx: &App) -> &'static str {
        if InterfaceConfig::get(cx).games_sort == GamesSort::Custom && self.filter.active() == 0 {
            "Your Custom row is empty. Click \"+ Add to Custom row\" above, or right-click any game in another sort and pick Add to Custom row."
        } else {
            "No games match these filters."
        }
    }

    /// PlayStation-style: a row of tiles, the selected one large with its name beside it, and
    /// the selected game's title, PLAY and details below.
    fn render_shelf(&mut self, list: &[LibraryGame], selected: Option<&LibraryGame>, height: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let sky: Hsla = crate::pages::home_page::sky().into();
        let custom = InterfaceConfig::get(cx).games_sort == GamesSort::Custom;
        let more = self.matching_count(cx).saturating_sub(list.len());
        let mut tiles = h_flex().id("shelf-tiles").px_6().py_3().gap_3().items_start().overflow_x_scroll().track_scroll(&self.tiles);
        let extra = self.extra;
        let ring = vec![
            BoxShadow { color: gpui::white().into(), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(3.), inset: false },
            BoxShadow { color: sky.opacity(0.5), offset: point(px(0.), px(0.)), blur_radius: px(28.), spread_radius: px(4.), inset: false },
        ];
        for game in list {
            let is_selected = extra.is_none() && selected.is_some_and(|s| s.id == game.id);
            let size = if is_selected { 230. } else { 160. };
            let id = game.id.clone();
            let menu_id = game.id.clone();
            let running = games::is_running(&game.id);
            let tile = div()
                .id(SharedString::from(format!("tile-{}", game.id)))
                .size(px(size))
                .flex_shrink_0()
                .rounded_xl()
                .overflow_hidden()
                .cursor_pointer()
                // A ring drawn as a shadow follows the rounded corners exactly (a border's
                // corners don't match the picture's).
                .when(is_selected, |this| this.shadow(vec![
                    BoxShadow { color: gpui::white().into(), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(3.), inset: false },
                    BoxShadow { color: sky.opacity(0.5), offset: point(px(0.), px(0.)), blur_radius: px(28.), spread_radius: px(4.), inset: false },
                ]))
                .when(!is_selected, |this| this.opacity(0.85).hover(|s| s.opacity(1.0)))
                .child(tile_art(game, size))
                .when(game.favourite, |this| this.child(star_badge(if is_selected { 30. } else { 24. })))
                .on_click(cx.listener(move |page, event: &ClickEvent, _, cx| {
                    if event.click_count() >= 2 {
                        if let Some(game) = games::find(&id, cx) {
                            page.play(&game, cx);
                        }
                    } else {
                        page.select(&id, cx);
                    }
                }));
            let tile = self.draggable(tile, game, custom, cx)
                .with_animation(
                    SharedString::from(format!("tile-anim-{}-{}", game.id, is_selected)),
                    Animation::new(Duration::from_millis(260)).with_easing(ease_out_quint()),
                    move |this, t| {
                        let from = if is_selected { 160. } else { 230. };
                        this.size(px(from + (size - from) * t))
                    },
                );
            tiles = tiles.child(h_flex().id(SharedString::from(format!("tile-box-{}", game.id))).gap_3().items_start().flex_shrink_0()
                .child(tile)
                .when(is_selected && running, |this| this.child(div().pt_2().text_xs().text_color(rgb(0x5ee38a)).child("Running")))
                .context_menu(move |menu, _, cx| game_menu(menu, &menu_id, cx)));
        }
        // The rest of the library is one click away.
        if more > 0 {
            let on = extra == Some(ShelfExtra::AllGames);
            tiles = tiles.child(v_flex().id("shelf-more").size(px(160.)).flex_shrink_0().rounded_xl().cursor_pointer().items_center().justify_center().gap_1()
                .bg(gpui::white().opacity(if on { 0.16 } else { 0.06 })).hover(|s| s.bg(gpui::white().opacity(0.12)))
                .when(on, |this| this.shadow(ring.clone()))
                .child(Icon::new(PandoraIcon::Grid2x2).size_8())
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("All games"))
                .child(div().text_xs().text_color(gpui::white().opacity(0.6)).child(format!("{more} more")))
                .on_click(cx.listener(|page, _, window, cx| {
                    page.extra = Some(ShelfExtra::AllGames);
                    page.activate(window, cx);
                })));
        }
        if custom {
            let on = extra == Some(ShelfExtra::AddGames);
            tiles = tiles.child(v_flex().id("shelf-add-custom").size(px(160.)).flex_shrink_0().rounded_xl().cursor_pointer().items_center().justify_center().gap_1()
                .border_2().border_dashed().border_color(gpui::white().opacity(0.25)).hover(|s| s.bg(gpui::white().opacity(0.08)))
                .when(on, |this| this.shadow(ring.clone()).bg(gpui::white().opacity(0.12)))
                .child(Icon::new(PandoraIcon::Plus).size_8())
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Add games"))
                .on_click(cx.listener(|page, _, _, cx| {
                    page.adding_custom = true;
                    cx.notify();
                })));
        }

        let Some(game) = selected.cloned() else {
            return v_flex().size_full().child(tiles).child(div().px_8().py_4().text_sm().child(self.empty_message(cx))).into_any_element();
        };

        let running = games::is_running(&game.id);
        let problem = games::problem(&game, cx);
        let label = if running { "STOP" } else if problem.is_some() { "SET UP" } else { "PLAY" };
        let meta: Vec<String> = [Some(platform_of(&game)), game.info.as_ref().and_then(|i| i.genres.first().cloned()),
            game.info.as_ref().and_then(|i| i.release_date.as_deref().and_then(year_of))].into_iter().flatten().collect();
        let entity = cx.entity();
        let play_game = game.clone();
        let play = play_button(label, move |window, cx| {
            // Not playable yet: its page's Setup tab says what's missing.
            if games::problem(&play_game, cx).is_some() && !games::is_running(&play_game.id) {
                root::switch_page(PageType::Game { id: play_game.id.clone().into() }, &[], window, cx);
            } else {
                entity.update(cx, |page, cx| page.play(&play_game, cx));
            }
        });
        let open_id = game.id.clone();
        let status = self.message.clone().or_else(|| problem.clone().map(|p| (p, true)));

        let hero = v_flex().gap_3()
            .child(div().child(headline(&game, 56.)).with_animation(
                SharedString::from(format!("hero-title-{}", game.id)),
                Animation::new(Duration::from_millis(600)).with_easing(ease_out_quint()),
                |this, t| this.opacity(t).ml(px(-30. * (1. - t))),
            ))
            // The game's name under its logo / title, above the platform line.
            .child(div().text_2xl().font_weight(FontWeight::SEMIBOLD).child(game.name.clone()))
            .child(div().text_sm().text_color(gpui::white().opacity(0.8)).child(meta.join("  ·  ")))
            .child(h_flex().gap_3().items_center()
                .child(div().w(px(300.)).flex_shrink_0().child(play))
                .child(Button::new("game-options").label("Options").on_click(move |_, window, cx| {
                    root::switch_page(PageType::Game { id: open_id.clone().into() }, &[], window, cx)
                }))
                .child(favourite_button("game-favourite", &game)))
            .when_some(status, |this, (text, error)| this.child(div().text_xs().text_color(if error { cx.theme().danger } else { sky }).child(text)));

        let info = game.info.clone().unwrap_or_default();
        let fact = |label: &'static str, value: String| v_flex().gap_0p5().min_w(px(140.))
            .child(div().text_xs().text_color(gpui::white().opacity(0.55)).child(label))
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(value));
        let first = |v: &[String]| v.first().cloned().unwrap_or_else(|| "—".into());
        let summary = v_flex().gap_4().max_w(px(1100.))
            .when(!info.description.is_empty(), |this| this.child(div().text_base().line_height(rems(1.5)).text_color(gpui::white().opacity(0.88))
                .line_clamp(3).child(info.description.clone())))
            .child(h_flex().flex_wrap().gap_8()
                .child(fact("Time played", games::play_time_text(game.play_seconds)))
                .child(fact("Last played", games::last_played_text(game.last_played)))
                .child(fact("Genre", first(&info.genres)))
                .child(fact("Developer", first(&info.developers)))
                .child(fact("Released", info.release_date.clone().unwrap_or_else(|| "—".into())))
                .child(fact("Library", game.library_name())))
            .with_animation(
                SharedString::from(format!("hero-summary-{}", game.id)),
                Animation::new(Duration::from_millis(500)).with_easing(ease_out_quint()),
                |this, t| this.opacity(t),
            );

        let details = div().mt_2().p_5().rounded_xl().bg(rgb(0x070910).opacity(0.78)).border_1().border_color(gpui::white().opacity(0.06))
            .child(details_panel(&game, cx))
            .with_animation(
                SharedString::from(format!("hero-details-{}", game.id)),
                Animation::new(Duration::from_millis(500)).with_easing(ease_out_quint()),
                |this, t| this.opacity(t).mt(px(16. + 24. * (1. - t))),
            );

        // The row, then the selected game's title, PLAY, a short description and quick facts fill
        // the screen; the full Details and Description start below and scroll into view.
        let first_screen = (height - px(40.)).max(px(420.));
        v_flex().size_full()
            .child(v_flex().size_full()
                .child(v_flex().min_h(first_screen).flex_shrink_0()
                    .child(tiles)
                    .child(v_flex().px_8().pt_6().gap_6().child(hero).child(summary)))
                .child(div().px_8().pb_8().child(details))
                .overflow_y_scrollbar())
            .into_any_element()
    }

    /// Library grid: square cards in rows, in platform sections.
    fn render_grid(&mut self, list: &[LibraryGame], selected: Option<&LibraryGame>, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let custom = InterfaceConfig::get(cx).games_sort == GamesSort::Custom;
        let mut body = v_flex().size_full().px_6().pb_6().gap_3();
        if list.is_empty() {
            body = body.child(div().text_sm().child(self.empty_message(cx)));
        }
        for (section, games_in) in sections(list) {
            body = body.child(self.section_header(section, games_in.len(), cx));
            if self.collapsed.contains(section) {
                continue;
            }
            let mut cards = h_flex().flex_wrap().gap_4().pb_2();
            for game in &games_in {
                let id = game.id.clone();
                let menu_id = game.id.clone();
                let is_selected = selected.is_some_and(|s| s.id == game.id);
                let card = v_flex().id(SharedString::from(format!("card-{}", game.id))).w(px(200.)).gap_1p5().cursor_pointer()
                    .child(div().relative().size(px(200.)).rounded_xl().overflow_hidden()
                        .hover(|s| s.shadow(vec![BoxShadow { color: gpui::white().into(), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(3.), inset: false }]))
                        .when(!game.installed() && !is_selected, |this| this.opacity(0.55))
                        .when(is_selected, |this| this.shadow(vec![
                            BoxShadow { color: gpui::white().into(), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(3.), inset: false },
                            BoxShadow { color: crate::pages::home_page::sky().opacity(0.5).into(), offset: point(px(0.), px(0.)), blur_radius: px(22.), spread_radius: px(3.), inset: false },
                        ]))
                        .child(tile_art(game, 200.))
                        .when(game.favourite, |this| this.child(star_badge(26.)))
                        .when(is_selected, |this| this.child(probe(&self.selected_bounds))))
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().text_ellipsis().child(game.name.clone()))
                    .child(div().text_xs().text_color(muted).child(if game.installed() {
                        format!("{} · {}", game.library_name(), games::play_time_text(game.play_seconds))
                    } else {
                        format!("{} · not installed", game.library_name())
                    }))
                    .on_click(cx.listener(move |page, _, _, cx| page.open_in_home(&id, cx)));
                cards = cards.child(self.draggable(card, game, custom, cx).context_menu(move |menu, _, cx| game_menu(menu, &menu_id, cx)));
            }
            body = body.child(cards);
        }
        div().size_full().relative()
            .child(body.id("grid-scroll").pt_1().overflow_y_scroll().track_scroll(&self.grid_scroll))
            .vertical_scrollbar(&self.grid_scroll)
            .into_any_element()
    }

    /// A list of games on the left (resizable, with or without pictures) and the selected game
    /// beside it, laid out like its game page: logo, name, platform, PLAY, then the full details.
    fn render_list(&mut self, list: &[LibraryGame], selected: Option<&LibraryGame>, area_height: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, hover, primary) = (theme.muted_foreground, theme.secondary_hover, theme.primary);
        let config = InterfaceConfig::get(cx);
        let (icons, custom) = (config.games_list_icons, config.games_sort == GamesSort::Custom);

        let view_toggle = h_flex().mx_2().mb_1().p_0p5().gap_0p5().rounded_md().bg(gpui::white().opacity(0.05))
            .children([(true, "Pictures"), (false, "Names only")].map(|(value, label)| {
                div().id(label).flex_1().py_0p5().rounded_sm().text_xs().text_center().cursor_pointer()
                    .when(icons == value, |this| this.bg(primary.opacity(0.45)))
                    .when(icons != value, |this| this.text_color(muted).hover(|s| s.bg(gpui::white().opacity(0.06))))
                    .child(label)
                    .on_click(move |_, _, cx| {
                        InterfaceConfig::get_mut(cx).games_list_icons = value;
                        cx.refresh_windows();
                    })
            }));
        let mut rows = v_flex().size_full().py_2().gap_0p5().child(view_toggle);
        if list.is_empty() {
            rows = rows.child(div().px_4().py_2().text_sm().text_color(muted).child(self.empty_message(cx)));
        }
        for (section, games_in) in sections(list) {
            rows = rows.child(div().mx_2().child(self.section_header(section, games_in.len(), cx)));
            if self.collapsed.contains(section) {
                continue;
            }
            for game in &games_in {
                let is_selected = selected.is_some_and(|s| s.id == game.id);
                let id = game.id.clone();
                let menu_id = game.id.clone();
                let row = h_flex().id(SharedString::from(format!("row-{}", game.id))).mx_2().px_2().gap_3().items_center().rounded_md().cursor_pointer()
                    .when(icons, |this| this.py_1p5())
                    .when(!icons, |this| this.py_1())
                    .when(is_selected, |this| this.bg(primary.opacity(0.35)))
                    .hover(move |s| s.bg(hover))
                    .when(icons, |this| this.child(div().relative().size(px(52.)).flex_shrink_0().rounded_lg().overflow_hidden().child(tile_art(game, 52.))))
                    .child(v_flex().flex_1().min_w_0()
                        .child(div().text_sm().overflow_hidden().whitespace_nowrap().text_ellipsis().child(game.name.clone()))
                        .when(icons, |this| this.child(div().text_xs().text_color(muted).overflow_hidden().whitespace_nowrap().text_ellipsis().child(platform_of(game)))))
                    .when(game.favourite, |this| this.child(Icon::new(PandoraIcon::Star).size_3p5().flex_shrink_0().text_color(rgb(STAR))))
                    .when(is_selected, |this| this.relative().child(probe(&self.selected_bounds)))
                    .on_click(cx.listener(move |page, _, _, cx| page.select(&id, cx)));
                rows = rows.child(self.draggable(row, game, custom, cx).context_menu(move |menu, _, cx| game_menu(menu, &menu_id, cx)));
            }
        }
        let rows = div().size_full().relative()
            .child(rows.id("list-scroll").overflow_y_scroll().track_scroll(&self.list_scroll))
            .vertical_scrollbar(&self.list_scroll);

        let right = match selected.cloned() {
            None => div().size_full().into_any_element(),
            Some(game) => self.list_detail(game, area_height, cx),
        };
        ResizePanel::new(&self.list_split, rows, right).into_any_element()
    }

    fn list_detail(&mut self, game: LibraryGame, area_height: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let viewport = size(px(1200.), px(800.));
        let running = games::is_running(&game.id);
        let problem = games::problem(&game, cx);
        let label = if running { "STOP" } else if problem.is_some() { "SET UP" } else { "PLAY" };
        let entity = cx.entity();
        let play_game = game.clone();
        let play = play_button(label, move |window, cx| {
            if games::problem(&play_game, cx).is_some() && !games::is_running(&play_game.id) {
                root::switch_page(PageType::Game { id: play_game.id.clone().into() }, &[], window, cx);
            } else {
                entity.update(cx, |page, cx| page.play(&play_game, cx));
            }
        });
        let open_id = game.id.clone();
        let sky: Hsla = crate::pages::home_page::sky().into();
        let status = self.message.clone().or_else(|| problem.map(|p| (p, true)));

        // Logo, name, platform and PLAY fill most of the first screen (PLAY near its bottom);
        // the details and description follow and the whole side scrolls.
        let content = v_flex().w_full()
            .child(v_flex().min_h((area_height - px(150.)).max(px(300.))).flex_shrink_0().px_8().pt_8().pb_6().gap_3()
                .child(div().flex_1())
                .child(div().child(headline(&game, 48.)).with_animation(
                    SharedString::from(format!("list-title-{}", game.id)),
                    Animation::new(Duration::from_millis(500)).with_easing(ease_out_quint()),
                    |this, t| this.opacity(t),
                ))
                .child(div().text_2xl().font_weight(FontWeight::SEMIBOLD).child(game.name.clone()))
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).text_color(gpui::white().opacity(0.85)).child(games::kind_label(&game)))
                .child(h_flex().gap_3().items_center().flex_wrap()
                    .child(div().w(px(300.)).flex_shrink_0().child(play))
                    .child(Button::new("list-options").label("Options").on_click(move |_, window, cx| {
                        root::switch_page(PageType::Game { id: open_id.clone().into() }, &[], window, cx)
                    }))
                    .child(favourite_button("list-favourite", &game)))
                .when_some(status, |this, (text, error)| this.child(div().text_xs().text_color(if error { cx.theme().danger } else { sky }).child(text))))
            .child(div().mx_8().mb_8().p_5().rounded_xl().bg(rgb(0x070910).opacity(0.82)).border_1().border_color(gpui::white().opacity(0.06))
                .child(details_panel(&game, cx)));

        div().size_full().relative().overflow_hidden()
            .child(backdrop(Some(&game), viewport, cx))
            .child(div().absolute().inset_0().child(v_flex().size_full().child(content).overflow_y_scrollbar()))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{Filter, Tri, arrange, search_matches};
    use crate::{games::{GameKind, LibraryGame}, interface_config::GamesSort};

    #[test]
    fn search_ignores_spaces_and_order() {
        assert!(search_matches("Skate 3", "Skate3"));
        assert!(search_matches("skate3", "Skate 3"));
        assert!(search_matches("hit & run", "Simpsons Hit and Run"));
        assert!(search_matches("zelda kingdom", "The Legend of Zelda Tears of the Kingdom"));
        assert!(search_matches("gta", "GTA V") && !search_matches("gta", "Grounded"));
        assert!(search_matches("", "Anything"));
    }

    fn game(id: &str, last_played: Option<i64>, favourite: bool) -> LibraryGame {
        let mut g: LibraryGame = serde_json::from_value(serde_json::json!({
            "id": id, "name": id, "kind": "windows", "path": "C:/x.exe", "scene": "galaxy",
        })).unwrap();
        g.last_played = last_played;
        g.favourite = favourite;
        let _ = GameKind::Windows;
        g
    }

    #[test]
    fn custom_shows_only_added_games_in_order() {
        let all = vec![game("a", Some(3), false), game("b", Some(9), true), game("c", None, false)];
        let ids = |list: Vec<LibraryGame>| list.into_iter().map(|g| g.id).collect::<Vec<_>>();
        let none = Filter::default();
        assert_eq!(ids(arrange(&all, "", &none, GamesSort::Recent, &[])), ["b", "a", "c"]);
        assert_eq!(ids(arrange(&all, "", &none, GamesSort::Name, &[])), ["a", "b", "c"]);
        let custom = vec!["c".to_string(), "a".to_string()];
        assert_eq!(ids(arrange(&all, "", &none, GamesSort::Custom, &custom)), ["c", "a"]);
        // Other sorts ignore the custom order.
        assert_eq!(ids(arrange(&all, "", &none, GamesSort::Recent, &custom)), ["b", "a", "c"]);
        let favs = Filter { favourite: Tri::Yes, ..Default::default() };
        assert_eq!(ids(arrange(&all, "", &favs, GamesSort::Recent, &[])), ["b"]);
        // Opening "c" from the Library puts it first in Recently played, without changing "Last played".
        let mut opened = all.clone();
        opened[2].last_opened = Some(20);
        assert_eq!(ids(arrange(&opened, "", &none, GamesSort::Recent, &[])), ["c", "b", "a"]);
        assert_eq!(opened[2].last_played, None);
        let not_favs = Filter { favourite: Tri::No, ..Default::default() };
        assert_eq!(ids(arrange(&all, "", &not_favs, GamesSort::Name, &[])), ["a", "c"]);
    }
}
