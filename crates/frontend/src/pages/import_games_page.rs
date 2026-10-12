//! Import games: from other launchers (Steam, Epic, Xbox app, GOG, ...) or by scanning folders of
//! emulator games. The player ticks what to bring in; Flux then finds each game's art and details.

use std::{collections::BTreeSet, path::PathBuf};

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, Sizable, button::{Button, ButtonVariants}, checkbox::Checkbox, h_flex, scroll::ScrollableElement, v_flex,
};

use crate::{
    games::{self, GameKind},
    importers::{self, Found, LAUNCHERS},
    interface_config::InterfaceConfig,
    pages::{home_page::{FLUX_SKY}, page::Page},
    root,
    ui::PageType,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Launchers,
    Emulators,
}

pub struct ImportGamesPage {
    tab: Tab,
    /// Launchers found on this PC, and which ones to scan.
    detected: Vec<&'static str>,
    chosen: BTreeSet<&'static str>,
    installed_only: bool,
    folder: Option<PathBuf>,
    subfolders: bool,
    only: Option<GameKind>,
    results: Vec<Found>,
    selected: BTreeSet<String>,
    busy: Option<&'static str>,
    message: Option<(SharedString, bool)>,
    _task: Option<Task<()>>,
}

/// Is this found game already in the library?
fn in_library(found: &Found, cx: &App) -> bool {
    games::games(cx).iter().any(|g| {
        (found.steam_appid.is_some() && g.steam_appid == found.steam_appid)
            || (found.launch_uri.is_some() && g.launch_uri == found.launch_uri)
            || (!found.path.as_os_str().is_empty() && g.path == found.path)
    })
}

impl ImportGamesPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut page = Self {
            tab: Tab::Launchers,
            detected: Vec::new(),
            chosen: BTreeSet::new(),
            installed_only: true,
            folder: None,
            subfolders: true,
            only: None,
            results: Vec::new(),
            selected: BTreeSet::new(),
            busy: Some("Looking for game launchers on this PC..."),
            message: None,
            _task: None,
        };
        page._task = Some(cx.spawn(async move |page, cx| {
            let detected: Vec<&'static str> = cx.background_spawn(async move {
                LAUNCHERS.iter().filter(|l| importers::detected(l.id)).map(|l| l.id).collect()
            }).await;
            _ = page.update(cx, |page, cx| {
                page.chosen = detected.iter().copied().collect();
                page.detected = detected;
                page.busy = None;
                cx.notify();
            });
        }));
        page
    }

    fn show_results(&mut self, results: Vec<Found>, cx: &mut Context<Self>) {
        // Tick everything that isn't in the library yet.
        self.selected = results.iter().filter(|f| !in_library(f, cx)).map(|f| f.key.clone()).collect();
        self.message = Some(if results.is_empty() {
            ("No games found".into(), true)
        } else {
            (format!("Found {} games", results.len()).into(), false)
        });
        self.results = results;
        self.busy = None;
        cx.notify();
    }

    fn scan_launchers(&mut self, cx: &mut Context<Self>) {
        let chosen: Vec<&'static str> = self.chosen.iter().copied().collect();
        let installed_only = self.installed_only;
        self.busy = Some("Scanning...");
        self.results.clear();
        cx.notify();
        self._task = Some(cx.spawn(async move |page, cx| {
            let found = cx.background_spawn(async move {
                chosen.into_iter().flat_map(|id| importers::scan(id, installed_only)).collect::<Vec<_>>()
            }).await;
            _ = page.update(cx, |page, cx| page.show_results(found, cx));
        }));
    }

    fn scan_folder(&mut self, cx: &mut Context<Self>) {
        let Some(folder) = self.folder.clone() else { return };
        let (subfolders, only) = (self.subfolders, self.only);
        self.busy = Some("Scanning the folder (reading each disc)...");
        self.results.clear();
        cx.notify();
        self._task = Some(cx.spawn(async move |page, cx| {
            let found = cx.background_spawn(async move { importers::scan_folder(&folder, subfolders, only) }).await;
            _ = page.update(cx, |page, cx| page.show_results(found, cx));
        }));
    }

    fn pick_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Folder with your game backups".into()) });
        self._task = Some(cx.spawn_in(window, async move |page, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            _ = page.update(cx, |page, cx| {
                page.folder = Some(path);
                page.scan_folder(cx);
            });
        }));
    }

    fn import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked: Vec<Found> = self.results.iter().filter(|f| self.selected.contains(&f.key) && !in_library(f, cx)).cloned().collect();
        if picked.is_empty() {
            return;
        }
        let count = picked.len();
        self.busy = Some("Adding games...");
        cx.notify();
        self._task = Some(cx.spawn_in(window, async move |page, cx| {
            // Disc images copy their own art (PS3/PS4 pictures) as when added by hand.
            let new_games = cx.background_spawn(async move {
                picked.into_iter().map(|found| {
                    if found.source == "Emulator" && let Ok(d) = games::detect(&found.path) {
                        let mut game = games::LibraryGame::from_detection(&d, found.name.clone());
                        game.source = Some("Emulator".into());
                        game
                    } else {
                        found.into_game()
                    }
                }).collect::<Vec<_>>()
            }).await;
            _ = page.update_in(cx, |page, window, cx| {
                InterfaceConfig::get_mut(cx).games.extend(new_games);
                games::find_missing_web_art(cx);
                page.busy = None;
                page.results.clear();
                page.message = Some((format!("Added {count} games. Flux is finding their art and details in the background.").into(), false));
                root::switch_page(PageType::GamesHome, &[], window, cx);
            });
        }));
    }
}

impl Page for ImportGamesPage {
    fn controls(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = |id: &'static str, label: &'static str, value: Tab, cx: &mut Context<Self>| {
            let active = self.tab == value;
            v_flex().id(id).cursor_pointer().pt_1().gap_1()
                .child(div().px_1().text_sm().font_weight(FontWeight::SEMIBOLD)
                    .text_color(if active { cx.theme().foreground } else { cx.theme().muted_foreground })
                    .child(label))
                .child(div().h(px(2.)).rounded_full().when(active, |this| this.bg(cx.theme().primary)))
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .on_click(cx.listener(move |page, _, _, cx| {
                    page.tab = value;
                    page.results.clear();
                    page.message = None;
                    cx.notify();
                }))
        };
        h_flex().gap_5()
            .child(tab("tab-launchers", "Game launchers", Tab::Launchers, cx))
            .child(tab("tab-emulators", "Emulator games", Tab::Emulators, cx))
    }

    fn scrollable(&self, _cx: &App) -> bool {
        false
    }
}

impl Render for ImportGamesPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border, secondary, background, danger) = (theme.muted_foreground, theme.border, theme.secondary, theme.background, theme.danger);
        let green: Hsla = rgb(0x5ee38a).into();
        let card = |title: &'static str| v_flex().gap_3().p_5().rounded_xl().bg(secondary).border_1().border_color(border)
            .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BOLD).text_color(crate::pages::home_page::sky()).child(title));
        let busy = self.busy.is_some();

        let setup = match self.tab {
            Tab::Launchers => {
                let rows = LAUNCHERS.iter().map(|l| {
                    let found = self.detected.contains(&l.id);
                    let on = self.chosen.contains(&l.id);
                    let id = l.id;
                    h_flex().w(px(300.)).gap_2().items_center()
                        .child(Checkbox::new(SharedString::from(format!("launcher-{id}"))).checked(on && found).disabled(!found)
                            .label(l.name)
                            .on_click(cx.listener(move |page, value: &bool, _, cx| {
                                if *value { page.chosen.insert(id); } else { page.chosen.remove(id); }
                                cx.notify();
                            })))
                        .child(div().text_xs().text_color(if found { green } else { muted }).child(if found { "found" } else { "not on this PC" }))
                });
                card("IMPORT FROM GAME LAUNCHERS")
                    .child(div().text_sm().text_color(muted).child(
                        "Flux reads each launcher's own files on this PC (no sign-in). Imported games start through their launcher, \
                         so DRM, overlays and cloud saves keep working.",
                    ))
                    .child(h_flex().flex_wrap().gap_3().children(rows))
                    .child(Checkbox::new("installed-only").checked(self.installed_only)
                        .label("Installed games only")
                        .on_click(cx.listener(|page, value: &bool, _, cx| {
                            page.installed_only = *value;
                            cx.notify();
                        })))
                    .child(div().text_xs().text_color(muted).child(
                        "Untick to also list games you own but haven't installed (Steam and Epic). Playing one opens its launcher to install it.",
                    ))
                    .child(h_flex().child(Button::new("scan-launchers").primary().label("Find games").disabled(busy || self.chosen.is_empty())
                        .on_click(cx.listener(|page, _, _, cx| page.scan_launchers(cx)))))
            },
            Tab::Emulators => {
                let consoles: Vec<(Option<GameKind>, &'static str)> = vec![
                    (None, "Any console (auto)"), (Some(GameKind::Ps1), "PlayStation"), (Some(GameKind::Ps2), "PlayStation 2"),
                    (Some(GameKind::Ps3), "PlayStation 3"), (Some(GameKind::Ps4), "PlayStation 4"), (Some(GameKind::Psp), "PSP"),
                    (Some(GameKind::Xbox), "Xbox"), (Some(GameKind::Xbox360), "Xbox 360"), (Some(GameKind::Switch), "Nintendo Switch"),
                ];
                let chips = consoles.into_iter().map(|(kind, label)| {
                    let on = self.only == kind;
                    div().id(SharedString::from(format!("only-{label}"))).px_3().py_1().rounded_full().text_xs().cursor_pointer()
                        .border_1().border_color(if on { theme.primary } else { border })
                        .bg(if on { theme.primary.opacity(0.3) } else { background })
                        .child(label)
                        .on_click(cx.listener(move |page, _, _, cx| {
                            page.only = kind;
                            cx.notify();
                        }))
                });
                card("SCAN FOR EMULATOR GAMES")
                    .child(div().text_sm().text_color(muted).child(
                        "Pick the folder with your own game backups. Flux reads each disc image or game folder to tell which console it's for \
                         (.iso, .cue/.bin, .chd, .cso, .pbp, .nsp, .xci, .xex, PS3/PS4 game folders).",
                    ))
                    .child(h_flex().gap_2().items_center()
                        .child(div().text_sm().w(px(140.)).text_color(muted).child("Scan with"))
                        .child(h_flex().flex_wrap().gap_2().children(chips)))
                    .child(h_flex().gap_2().items_center()
                        .child(div().text_sm().w(px(140.)).text_color(muted).child("Folder"))
                        .child(div().flex_1().px_3().py_2().rounded_md().bg(background).text_sm()
                            .child(self.folder.as_ref().map(|f| crate::privacy::path(f)).unwrap_or_else(|| "None chosen".into())))
                        .child(Button::new("pick-folder").label("Choose folder").disabled(busy).on_click(cx.listener(|page, _, window, cx| page.pick_folder(window, cx)))))
                    .child(Checkbox::new("subfolders").checked(self.subfolders).label("Scan subfolders").on_click(cx.listener(|page, value: &bool, _, cx| {
                        page.subfolders = *value;
                        cx.notify();
                    })))
                    .child(h_flex().child(Button::new("rescan").primary().label("Scan").disabled(busy || self.folder.is_none())
                        .on_click(cx.listener(|page, _, _, cx| page.scan_folder(cx)))))
            },
        };

        // Results, in sections by launcher / console.
        let mut groups: Vec<(String, Vec<&Found>)> = Vec::new();
        for f in &self.results {
            let group = if f.source == "Emulator" { f.kind.platform().unwrap_or("Other").to_string() } else { f.source.to_string() };
            match groups.iter_mut().find(|(g, _)| *g == group) {
                Some((_, list)) => list.push(f),
                None => groups.push((group, vec![f])),
            }
        }
        let addable = self.results.iter().filter(|f| !in_library(f, cx)).count();
        let chosen = self.results.iter().filter(|f| self.selected.contains(&f.key) && !in_library(f, cx)).count();
        let results = (!self.results.is_empty()).then(|| {
            let sections = groups.into_iter().map(|(group, list)| {
                let rows = list.into_iter().map(|f| {
                    let already = in_library(f, cx);
                    let key = f.key.clone();
                    h_flex().gap_3().items_center().py_1()
                        .child(Checkbox::new(SharedString::from(format!("pick-{}", f.key))).checked(already || self.selected.contains(&f.key)).disabled(already)
                            .on_click(cx.listener(move |page, value: &bool, _, cx| {
                                if *value { page.selected.insert(key.clone()); } else { page.selected.remove(&key); }
                                cx.notify();
                            })))
                        .child(div().flex_1().min_w_0().text_sm().overflow_hidden().whitespace_nowrap().text_ellipsis().child(f.name.clone()))
                        .child(div().text_xs().text_color(if already { muted } else if f.installed { green } else { muted })
                            .child(if already { "in library" } else if f.installed { "installed" } else { "not installed" }))
                });
                v_flex().gap_0p5()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).pt_2().child(format!("{group} ({})", rows.len())))
                    .children(rows)
            });
            card("RESULTS")
                .child(h_flex().gap_2().items_center()
                    .child(Button::new("select-all").small().label("Select all").on_click(cx.listener(|page, _, _, cx| {
                        page.selected = page.results.iter().map(|f| f.key.clone()).collect();
                        cx.notify();
                    })))
                    .child(Button::new("select-none").small().label("Select none").on_click(cx.listener(|page, _, _, cx| {
                        page.selected.clear();
                        cx.notify();
                    })))
                    .child(div().flex_1())
                    .child(div().text_xs().text_color(muted).child(format!("{chosen} of {addable} new games selected")))
                    .child(Button::new("import").primary().label(format!("Import {chosen} games")).disabled(busy || chosen == 0)
                        .on_click(cx.listener(|page, _, window, cx| page.import(window, cx)))))
                .children(sections)
        });

        let _ = window;
        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .max_w(px(1000.))
            .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BLACK).text_size(px(36.)).child("Import games"))
            .child(setup)
            .when_some(self.busy, |this, b| this.child(div().text_sm().text_color(crate::pages::home_page::sky()).child(b)))
            .when_some(self.message.clone(), |this, (m, error)| this.child(div().text_sm().text_color(if error { danger } else { green }).child(m)))
            .children(results)
            .overflow_y_scrollbar()
    }
}
