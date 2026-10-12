//! A library game's page (Games mode): the Home-style hero with PLAY, a Setup tab (name, location,
//! background, remove; the built-in emulator for Xbox 360 games) and, for emulated games, Graphics.

use std::{path::PathBuf, time::Duration};

use bridge::{message::MessageToBackend, modal_action::ModalAction};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, Sizable, button::{Button, ButtonVariants}, h_flex, input::{Input, InputEvent, InputState}, scroll::ScrollableElement, v_flex,
};

use crate::{
    entity::DataEntities,
    games::{self, GameKind, LibraryGame},
    interface_config::InterfaceConfig,
    pages::{home_page::{FLUX_SKY, play_button}, page::Page},
    root,
    ui::PageType,
};

#[path = "game_page_mods.rs"]
mod mods;
#[path = "game_page_art.rs"]
mod art;

#[derive(Clone, Copy, PartialEq, Eq)]
enum GameTab {
    Play,
    Setup,
    Graphics,
    Mods,
    Debug,
}

pub struct GamePage {
    data: DataEntities,
    id: String,
    tab: GameTab,
    message: Option<(SharedString, bool)>, // (text, is_error)
    name_input: Entity<InputState>,
    _poll: Option<Task<()>>,
    _pick: Option<Task<()>>,
    /// Mods/Debug tabs: last result, what's in progress, and the tasks doing it.
    mods_message: Option<(SharedString, bool)>,
    mods_busy: Option<&'static str>,
    _mods_task: Option<Task<()>>,
    _unpack_watch: Option<Task<()>>,
    art: art::ArtPicker,
}

fn update_game(id: &str, cx: &mut App, f: impl FnOnce(&mut LibraryGame)) {
    if let Some(game) = InterfaceConfig::get_mut(cx).games.iter_mut().find(|g| g.id == id) {
        f(game);
    }
}

impl GamePage {
    /// Controller: LB / RB switch tabs, Y stars the game. False: not used here.
    pub fn controller(&mut self, nav: crate::controller_nav::Nav, cx: &mut Context<Self>) -> bool {
        use crate::controller_nav::Nav;
        match nav {
            Nav::PrevTab | Nav::NextTab => {
                let (emulated, moddable) = match self.game(cx) {
                    Some(g) => (g.kind.has_emulator_settings(), g.kind.has_mods()),
                    None => return false,
                };
                let mut tabs = vec![GameTab::Play, GameTab::Setup];
                if emulated {
                    tabs.push(GameTab::Graphics);
                }
                if moddable {
                    tabs.extend([GameTab::Mods, GameTab::Debug]);
                }
                let at = tabs.iter().position(|t| *t == self.tab).unwrap_or(0);
                let next = if nav == Nav::NextTab { (at + 1) % tabs.len() } else { (at + tabs.len() - 1) % tabs.len() };
                self.tab = tabs[next];
                cx.notify();
                true
            },
            Nav::Y => {
                games::toggle_favourite(&self.id, cx);
                true
            },
            _ => false,
        }
    }

    /// Controller A with nothing else to press: PLAY on the Play tab.
    pub fn controller_confirm(&mut self, cx: &mut Context<Self>) -> bool {
        if self.tab == GameTab::Play {
            self.play(cx);
            true
        } else {
            false
        }
    }

    pub fn new(id: &str, data: &DataEntities, window: &mut Window, cx: &mut Context<Self>) -> Option<Self> {
        let game = games::find(id, cx)?;
        // FLUX_GAME_TAB=setup|graphics opens on that tab (for screenshots and testing).
        let tab = match std::env::var("FLUX_GAME_TAB").as_deref() {
            Ok("setup") => GameTab::Setup,
            Ok("graphics") if game.kind.has_emulator_settings() => GameTab::Graphics,
            Ok("mods") if game.kind.has_mods() => GameTab::Mods,
            Ok("debug") if game.kind.has_mods() => GameTab::Debug,
            _ => GameTab::Play,
        };
        let name_input = cx.new(|cx| InputState::new(window, cx).default_value(game.name.clone()));
        let game_id = id.to_string();
        cx.subscribe(&name_input, move |_, input, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                let name = input.read(cx).value().trim().to_string();
                if !name.is_empty() {
                    update_game(&game_id, cx, |g| g.name = name);
                    cx.notify();
                }
            }
        }).detach();
        let mut page = Self {
            data: data.clone(), id: id.to_string(), tab, message: None, name_input, _poll: None, _pick: None,
            mods_message: None, mods_busy: None, _mods_task: None, _unpack_watch: None, art: Default::default(),
        };
        if crate::game_mods::UNPACKING.lock().unwrap().iter().any(|e| e.0 == id) {
            page.watch_unpack(window, cx);
        }
        // FLUX_ART_PICK=tile|background|logo|icon opens the art picker (screenshots and testing).
        if let Ok(kind) = std::env::var("FLUX_ART_PICK") && let Some(kind) = games::ArtKind::ALL.into_iter().find(|k| k.key() == kind) {
            page.load_art_options(&game, kind, 0, cx);
        }
        Some(page)
    }

    fn game(&self, cx: &App) -> Option<LibraryGame> {
        games::find(&self.id, cx)
    }

    fn is_running(&mut self) -> bool {
        games::is_running(&self.id)
    }

    fn play(&mut self, cx: &mut Context<Self>) {
        let Some(game) = self.game(cx) else { return };
        if self.is_running() {
            games::stop_game(&self.id);
            self.message = Some(("Stopped".into(), false));
            cx.notify();
            return;
        }
        match games::start_game(&game, cx) {
            Ok(message) => {
                self.message = Some((message.into(), false));
                // Notice when the game exits so PLAY comes back.
                let id = self.id.clone();
                self._poll = Some(cx.spawn(async move |page, cx| loop {
                    cx.background_executor().timer(Duration::from_millis(1000)).await;
                    let still = games::is_running(&id);
                    if page.update(cx, |page, cx| {
                        if !still {
                            page.message = None;
                        }
                        cx.notify();
                    }).is_err() || !still {
                        break;
                    }
                }));
            },
            Err(error) => {
                if games::problem(&game, cx).is_some() {
                    self.tab = GameTab::Setup;
                }
                self.message = Some((error.into(), true));
            },
        }
        cx.notify();
    }

    /// Point the game at a new file or folder; Flux works out what it is again.
    fn relocate(&mut self, folder: bool, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: !folder,
            directories: folder,
            multiple: false,
            prompt: Some("Where is this game now?".into()),
        });
        let id = self.id.clone();
        self._pick = Some(cx.spawn_in(window, async move |page, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let detection = cx.background_spawn(async move { games::detect(&path) }).await;
            _ = page.update(cx, |page, cx| {
                match detection {
                    Ok(d) => {
                        update_game(&id, cx, |g| {
                            g.path = d.path.clone();
                            g.kind = d.kind;
                            g.steam_appid = d.steam.as_ref().map(|s| s.appid);
                        });
                        page.message = Some((format!("Found it: {}", d.kind.label()).into(), false));
                    },
                    Err(e) => page.message = Some((e.into(), true)),
                }
                cx.notify();
            });
        }));
    }

    fn remove(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(game) = self.game(cx) {
            if let Some(dir) = game.art_dir() {
                _ = std::fs::remove_dir_all(dir);
            }
        }
        let config = InterfaceConfig::get_mut(cx);
        config.games.retain(|g| g.id != self.id);
        let next = config.games.first().map(|g| PageType::Game { id: g.id.clone().into() }).unwrap_or(PageType::AddGame);
        root::switch_page(next, &[], window, cx);
    }

    fn tab_button(&self, tab: GameTab, label: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.tab == tab;
        v_flex()
            .id(label)
            .cursor_pointer()
            .pt_1()
            .gap_1()
            .child(div().px_1().text_sm().font_weight(FontWeight::SEMIBOLD)
                .text_color(if active { cx.theme().foreground } else { cx.theme().muted_foreground })
                .child(label))
            .child(div().h(px(2.)).rounded_full().when(active, |this| this.bg(cx.theme().primary)))
            // The title bar is a window-drag area: claim the press so it doesn't start a window move.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.tab = tab;
                cx.notify();
            }))
    }

    fn render_play(&mut self, game: &LibraryGame, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let viewport = window.viewport_size();
        let sidebar = 0.0f32; // no sidebar: the top bar holds navigation
        let size = size(viewport.width - px(sidebar), viewport.height - px(44.));

        let running = self.is_running();
        let problem = games::problem(game, cx);
        let label = if running { "STOP" } else if problem.is_some() { "SET UP" } else { "PLAY" };
        let (status, status_color): (SharedString, Hsla) = match (&self.message, &problem) {
            (Some((text, true)), _) => (text.clone(), cx.theme().danger),
            (Some((text, false)), _) => (text.clone(), crate::pages::home_page::sky().into()),
            (None, Some(problem)) => (problem.clone(), cx.theme().muted_foreground),
            (None, None) => (format!("Ready to play · {}", crate::privacy::path(&game.path)).into(), rgb(0x5ee38a).into()),
        };

        let use_art = game.background.as_ref().is_some_and(|p| p.is_file()) && !game.scene.starts_with('!');
        let background = if use_art {
            div().absolute().inset_0()
                .child(img(ImageSource::Resource(Resource::Path(game.background.clone().unwrap().into()))).size_full().object_fit(ObjectFit::Cover))
                .into_any_element()
        } else {
            crate::backgrounds::render_scene(size, game.scene.trim_start_matches('!'), cx)
        };

        let headline: AnyElement = match &game.logo {
            // A fixed box: the logo scales down into it, keeping its shape.
            Some(logo) if logo.is_file() => crate::pages::games_home_page::logo_image(logo, 460., 170.),
            _ => {
                let words: Vec<&str> = game.name.split_whitespace().collect();
                let (head, last) = match words.split_last() {
                    Some((last, head)) if !head.is_empty() => (head.join(" ").to_uppercase(), last.to_uppercase()),
                    _ => (String::new(), game.name.to_uppercase()),
                };
                crate::pages::home_page::display_title(&head, &last, 60.).into_any_element()
            },
        };

        let title = v_flex()
            .max_w(px(860.))
            .gap_3()
            .child(div().child(headline).with_animation(
                SharedString::from(format!("game-title-{}", game.id)),
                Animation::new(Duration::from_millis(700)).with_easing(ease_out_quint()),
                |this, delta| this.opacity(delta).mt(px(18. * (1. - delta))),
            ))
            .child(div().text_2xl().font_weight(FontWeight::SEMIBOLD).child(game.name.clone()))
            .child(div().text_sm().font_weight(FontWeight::MEDIUM).text_color(gpui::white().opacity(0.85)).child(games::kind_label(game)));

        let entity = cx.entity();
        let play = play_button(label, move |_, cx| entity.update(cx, |page, cx| page.play(cx)));
        let folder = if game.path.is_dir() { Some(game.path.clone()) } else { game.path.parent().map(PathBuf::from) };

        let icon: Option<AnyElement> = game.icon.as_ref().filter(|p| p.is_file()).map(|p| {
            img(ImageSource::Resource(Resource::Path(p.clone().into()))).size_10().rounded_md().into_any_element()
        });
        let bar = h_flex()
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
            .child(h_flex().w(px(300.)).gap_3()
                .children(icon)
                .child(v_flex().flex_1().min_w_0().gap_0p5()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().text_ellipsis().child(game.name.clone()))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).overflow_hidden().whitespace_nowrap().text_ellipsis()
                        .child(crate::privacy::path(&game.path)))))
            .child(v_flex().flex_1().items_center().gap_1p5()
                .child(play)
                .child(div().text_xs().text_color(status_color).overflow_hidden().whitespace_nowrap().text_ellipsis().max_w(px(560.)).child(status)))
            .child(h_flex().w(px(380.)).flex_shrink_0().justify_end().gap_2()
                .child(crate::pages::games_home_page::favourite_button("game-page-favourite", game).small())
                .child(Button::new("game-setup").label("Setup").small().on_click(cx.listener(|this, _, _, cx| {
                    this.tab = GameTab::Setup;
                    cx.notify();
                })))
                .when_some(folder, |this, folder| this.child(Button::new("game-folder").label("Folder").small()
                    .on_click(move |_, window, cx| crate::open_folder(&folder, window, cx)))));

        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .child(background)
            .child(div().absolute().inset_0().bg(linear_gradient(
                180.,
                linear_color_stop(rgb(0x05060a).opacity(0.55), 0.),
                linear_color_stop(rgb(0x05060a).opacity(0.0), 0.45),
            )))
            // The title fills the first screen; the details and description start near its bottom
            // edge and scroll up into view, above the PLAY bar.
            .child(div().absolute().top_0().left_0().right_0().bottom(px(104.)).overflow_hidden()
                .child(v_flex().size_full()
                    // The title, then the game's details and description right under it.
                    .child(v_flex().px(px(40.)).pt(px(36.)).pb(px(24.)).flex_shrink_0().child(title))
                    .child(div().mx(px(40.)).mb(px(24.)).max_w(px(1500.)).p_5().rounded_xl().bg(rgb(0x070910).opacity(0.82)).border_1().border_color(gpui::white().opacity(0.06))
                        .child(crate::pages::games_home_page::details_panel(game, cx)))
                    .overflow_y_scrollbar()))
            .child(bar)
            .into_any_element()
    }

    fn render_setup(&mut self, game: &LibraryGame, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, border, secondary, secondary_hover, primary, danger) =
            (theme.muted_foreground, theme.border, theme.secondary, theme.secondary_hover, theme.primary, theme.danger);
        let problem = games::problem(game, cx);

        let card = |title: &'static str| v_flex().gap_3().p_5().rounded_xl().bg(secondary).border_1().border_color(border)
            .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BOLD).text_color(crate::pages::home_page::sky()).child(title));

        let about = card("THIS GAME")
            .child(h_flex().gap_3().items_center()
                .child(div().w(px(90.)).text_sm().text_color(muted).child("Name"))
                .child(div().flex_1().child(Input::new(&self.name_input))))
            .child(h_flex().gap_3().items_center()
                .child(div().w(px(90.)).text_sm().text_color(muted).child("Type"))
                .child(div().text_sm().child(games::kind_label(game)))
                .when_some(game.steam_appid, |this, appid| this.child(div().text_xs().text_color(muted).child(format!("Steam app {appid}")))))
            .child(h_flex().gap_3().items_center()
                .child(div().w(px(90.)).text_sm().text_color(muted).child("Location"))
                .child(div().flex_1().px_3().py_2().rounded_md().bg(cx.theme().background).text_sm().child(crate::privacy::path(&game.path)))
                .child(Button::new("relocate-file").label("Choose file").small().on_click(cx.listener(|this, _, window, cx| this.relocate(false, window, cx))))
                .child(Button::new("relocate-folder").label("Choose folder").small().on_click(cx.listener(|this, _, window, cx| this.relocate(true, window, cx)))))
            .when_some(self.message.clone(), |this, (text, error)| this.child(div().text_sm().text_color(if error { danger } else { rgb(0x5ee38a).into() }).child(text)))
            .when_some(problem.clone(), |this, problem| this.child(div().text_sm().text_color(danger).child(problem)))
            .when(problem.is_none(), |this| this.child(div().text_sm().text_color(rgb(0x5ee38a)).child("Ready to play")));

        // Background: the game's own art (when it has some) or any Flux scene. A leading '!' on the
        // scene means "use this scene even though the game has its own art".
        let has_art = game.background.as_ref().is_some_and(|p| p.is_file());
        let using_art = has_art && !game.scene.starts_with('!');
        let style = InterfaceConfig::get(cx).background_style;
        let mut choices: Vec<AnyElement> = Vec::new();
        if has_art {
            let id = self.id.clone();
            let art = game.background.clone().unwrap();
            let thumb = img(ImageSource::Resource(Resource::Path(art.into()))).size_full().object_fit(ObjectFit::Cover).into_any_element();
            choices.push(scene_card("game-art", "Game art", thumb, using_art, primary, secondary_hover)
                .on_click(move |_, _, cx| update_game(&id, cx, |g| g.scene = g.scene.trim_start_matches('!').to_string()))
                .into_any_element());
        }
        for scene in crate::backgrounds::SCENES {
            let selected = !using_art && game.scene.trim_start_matches('!') == scene.id;
            let id = self.id.clone();
            let thumb = img(ImageSource::Resource(Resource::Embedded(crate::backgrounds::thumbnail_path(scene.id, style)))).size_full().object_fit(ObjectFit::Cover).into_any_element();
            choices.push(scene_card(scene.id, scene.name, thumb, selected, primary, secondary_hover)
                .on_click(move |_, _, cx| update_game(&id, cx, |g| {
                    g.scene = if g.background.is_some() { format!("!{}", scene.id) } else { scene.id.to_string() };
                }))
                .into_any_element());
        }
        let busy = games::web_art_busy(&self.id);
        let art_status: SharedString = if busy {
            "Looking online (Steam, libretro thumbnails, Wikipedia)...".into()
        } else if let Some(error) = games::web_art_error(&self.id) {
            error.into()
        } else if let Some(source) = &game.art_source {
            format!("Art from {source}").into()
        } else {
            "Flux looks for the game's art online by its name: Steam for PC games, libretro's screenshot archive and Wikipedia for console games.".into()
        };
        let lookup_id = self.id.clone();
        let backgrounds = card("BACKGROUND")
            .child(h_flex().gap_3().items_center()
                .child(div().flex_1().min_w_0().text_sm().text_color(muted).child(art_status))
                .child(Button::new("find-art").small().label(if busy { "Looking..." } else { "Find art online" }).disabled(busy)
                    .on_click(move |_, _, cx| {
                        if let Some(task) = games::find_web_art(&lookup_id, true, cx) {
                            task.detach();
                        }
                    })))
            .child(h_flex().flex_wrap().gap_3().children(choices));

        let emulator = game.kind.emulator().map(|id| crate::pages::emulators_page::emulator_card(id, self.data.backend_handle.clone(), cx));
        let art_card = self.render_art_card(game, cx);

        let remove = card("REMOVE")
            .child(div().text_sm().text_color(muted).child("Takes the game out of Flux. Your game files aren't touched."))
            .child(div().child(Button::new("remove-game").danger().label("Remove from library").on_click(cx.listener(|this, _, window, cx| this.remove(window, cx)))));

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .max_w(px(1000.))
            .child(about)
            .children(emulator)
            .child(art_card)
            .child(backgrounds)
            .child(remove)
            .overflow_y_scrollbar()
            .into_any_element()
    }

    fn render_graphics(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use crate::xenia_config::{PRESETS, SETTINGS};
        let theme = cx.theme();
        let (muted, border, secondary, secondary_hover, primary) =
            (theme.muted_foreground, theme.border, theme.secondary, theme.secondary_hover, theme.primary);

        if !games::xenia_installed() {
            return v_flex().p_6().gap_3()
                .child(div().text_sm().child("Install the emulator first (Setup > Install emulator); its settings appear here after that."))
                .into_any_element();
        }

        let running = self.is_running();
        let card = |title: &'static str| v_flex().gap_3().p_5().rounded_xl().bg(secondary).border_1().border_color(border)
            .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BOLD).text_color(crate::pages::home_page::sky()).child(title));

        let presets = card("QUALITY PRESETS")
            .child(div().text_sm().text_color(muted).child("One click sets resolution, anti-aliasing, upscaling and texture filtering together."))
            .child(h_flex().gap_3().children(PRESETS.iter().enumerate().map(|(i, (name, description, _))| {
                let name = *name;
                v_flex()
                    .id(("xenia-preset", i))
                    .flex_1()
                    .gap_1()
                    .p_3()
                    .rounded_lg()
                    .cursor_pointer()
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(border)
                    .hover(|this| this.bg(secondary_hover).border_color(primary))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(name))
                    .child(div().text_xs().text_color(muted).child(*description))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.message = Some(match crate::xenia_config::apply_preset(name) {
                            Ok(()) => (format!("{name} preset saved").into(), false),
                            Err(e) => (e.into(), true),
                        });
                        cx.notify();
                    }))
            })));

        let rows = SETTINGS.iter().enumerate().map(|(i, setting)| {
            let current = crate::xenia_config::current(setting);
            // A segmented control: the current choice filled with the accent color.
            let group = h_flex()
                .p_0p5()
                .gap_0p5()
                .rounded_lg()
                .bg(cx.theme().background)
                .border_1()
                .border_color(border)
                .children(setting.options.iter().enumerate().map(|(j, (label, _))| {
                    let selected = current == Some(*label);
                    div()
                        .id(("xenia-option", i * 16 + j))
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .text_sm()
                        .cursor_pointer()
                        .when(selected, |this| this.bg(primary).text_color(cx.theme().primary_foreground).font_weight(FontWeight::SEMIBOLD))
                        .when(!selected, |this| this.text_color(muted).hover(|this| this.bg(secondary_hover).text_color(cx.theme().foreground)))
                        .child(*label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let setting = &SETTINGS[i];
                            this.message = Some(match crate::xenia_config::set(setting, setting.options[j].1) {
                                Ok(()) => (format!("{}: {} saved", setting.label, setting.options[j].0).into(), false),
                                Err(e) => (e.into(), true),
                            });
                            cx.notify();
                        }))
                }));
            h_flex().gap_4().py_2().border_b_1().border_color(border.opacity(0.5))
                .child(v_flex().flex_1().gap_0p5()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(setting.label))
                    .child(div().text_xs().text_color(muted).child(setting.help)))
                .child(group)
        });

        let config = crate::xenia_config::config_path();
        let settings = card("EMULATOR SETTINGS")
            .child(div().text_sm().text_color(muted).child(if running {
                "Skate 3 is running: changes apply the next time it starts (press STOP, then PLAY)."
            } else {
                "Changes are saved straight away and apply the next time you press PLAY."
            }))
            .when_some(self.message.clone(), |this, (text, error)| {
                this.child(div().text_sm().text_color(if error { cx.theme().danger } else { rgb(0x5ee38a).into() }).child(text))
            })
            .children(rows)
            .when_some(config, |this, config| this.child(h_flex().gap_2().pt_2()
                .child(Button::new("open-xenia-config").label("Open full config file").small()
                    .on_click(move |_, _, cx| cx.open_with_system(&config)))));

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .max_w(px(1000.))
            .child(presets)
            .child(settings)
            .overflow_y_scrollbar()
            .into_any_element()
    }
}

fn scene_card(id: &'static str, label: &'static str, thumb: AnyElement, selected: bool, accent: Hsla, hover: Hsla) -> Stateful<Div> {
    v_flex()
        .id(id)
        .w(px(150.))
        .rounded_lg()
        .overflow_hidden()
        .cursor_pointer()
        .border_2()
        .border_color(if selected { accent } else { gpui::transparent_black() })
        .bg(gpui::black().opacity(0.35))
        .child(div().h(px(84.)).w_full().overflow_hidden().child(thumb))
        .child(div().px_2().py_1().text_xs().font_weight(FontWeight::SEMIBOLD).child(label))
        .hover(move |this| this.bg(hover))
}

impl Page for GamePage {
    fn controls(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let emulated = self.game(cx).is_some_and(|g| g.kind.has_emulator_settings());
        let moddable = self.game(cx).is_some_and(|g| g.kind.has_mods());
        h_flex()
            .gap_5()
            .child(self.tab_button(GameTab::Play, "Play", cx))
            .child(self.tab_button(GameTab::Setup, "Setup", cx))
            .when(emulated, |this| this.child(self.tab_button(GameTab::Graphics, "Graphics", cx)))
            .when(moddable, |this| this.child(self.tab_button(GameTab::Mods, "Mods", cx)).child(self.tab_button(GameTab::Debug, "Debug", cx)))
    }

    fn scrollable(&self, _cx: &App) -> bool {
        false
    }
}

impl Render for GamePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(game) = self.game(cx) else {
            return div().p_6().child("This game isn't in the library any more.").into_any_element();
        };
        let content = match self.tab {
            GameTab::Play => self.render_play(&game, window, cx),
            GameTab::Setup => self.render_setup(&game, cx),
            GameTab::Graphics => self.render_graphics(cx),
            GameTab::Mods => self.render_mods(&game, window, cx),
            GameTab::Debug => self.render_debug(&game, cx),
        };
        div().size_full().child(content).into_any_element()
    }
}
