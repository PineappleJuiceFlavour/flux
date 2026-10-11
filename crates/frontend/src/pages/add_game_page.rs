//! Add a game to the library: pick (or drop) a file or folder, Flux says what it found and what that
//! means, the player names it, and Flux saves it with its art.

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, Sizable, button::{Button, ButtonVariants}, h_flex, input::{Input, InputState}, scroll::ScrollableElement, v_flex,
};

use crate::{
    games::{self, Detection, LibraryGame},
    icon::PandoraIcon,
    interface_config::InterfaceConfig,
    pages::{home_page::{FLUX_SKY}, page::Page},
    root,
    ui::PageType,
};

pub struct AddGamePage {
    detection: Option<Detection>,
    error: Option<SharedString>,
    busy: Option<&'static str>,
    name_input: Entity<InputState>,
    /// FLUX_ADD_GAME=<path>: add that game straight away (same code path as the buttons; for testing).
    auto_add: bool,
    _task: Option<Task<()>>,
}

impl AddGamePage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut page = Self {
            detection: None,
            error: None,
            busy: None,
            name_input: cx.new(|cx| InputState::new(window, cx).placeholder("Game name")),
            auto_add: false,
            _task: None,
        };
        if let Some(path) = std::env::var_os("FLUX_ADD_GAME") {
            // Only once per run, even if the page is opened again.
            static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !DONE.swap(true, std::sync::atomic::Ordering::SeqCst) {
                page.auto_add = true;
                page.inspect(path.into(), window, cx);
            }
        }
        page
    }

    fn pick(&mut self, folder: bool, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: !folder,
            directories: folder,
            multiple: false,
            prompt: Some(if folder { "Choose the game's folder".into() } else { "Choose the game (.exe, disc image or default.xex)".into() }),
        });
        self._task = Some(cx.spawn_in(window, async move |page, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            _ = page.update_in(cx, |page, window, cx| page.inspect(path, window, cx));
        }));
    }

    fn inspect(&mut self, path: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.busy = Some("Looking at it...");
        self.error = None;
        self.detection = None;
        cx.notify();
        self._task = Some(cx.spawn_in(window, async move |page, cx| {
            let result = cx.background_spawn(async move { games::detect(&path) }).await;
            _ = page.update_in(cx, |page, window, cx| {
                page.busy = None;
                match result {
                    Ok(detection) => {
                        page.name_input.update(cx, |input, cx| input.set_value(detection.name.clone(), window, cx));
                        page.detection = Some(detection);
                        if std::mem::take(&mut page.auto_add) {
                            page.add(window, cx);
                        }
                    },
                    Err(error) => page.error = Some(error.into()),
                }
                cx.notify();
            });
        }));
    }

    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(detection) = self.detection.clone() else { return };
        let name = self.name_input.read(cx).value().trim().to_string();
        let name = if name.is_empty() { detection.name.clone() } else { name };
        self.busy = Some("Getting the game's art...");
        cx.notify();
        self._task = Some(cx.spawn_in(window, async move |page, cx| {
            // Art extraction runs PowerShell / image work: keep it off the UI thread.
            let game = cx.background_spawn(async move { LibraryGame::from_detection(&detection, name) }).await;
            _ = page.update_in(cx, |page, window, cx| {
                let id = game.id.clone();
                InterfaceConfig::get_mut(cx).games.push(game);
                crate::games::find_missing_web_art(cx);
                page.busy = None;
                page.detection = None;
                root::switch_page(PageType::Game { id: id.into() }, &[], window, cx);
            });
        }));
    }
}

impl Page for AddGamePage {
    fn controls(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }

    fn scrollable(&self, _cx: &App) -> bool {
        false
    }
}

impl Render for AddGamePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border, secondary, primary, danger) = (theme.muted_foreground, theme.border, theme.secondary, theme.primary, theme.danger);
        let empty_library = InterfaceConfig::get(cx).games.is_empty();

        let card = |title: &'static str| v_flex().gap_3().p_5().rounded_xl().bg(secondary).border_1().border_color(border)
            .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BOLD).text_color(rgb(FLUX_SKY)).child(title));

        let header = v_flex().gap_2()
            .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BLACK).text_size(px(40.)).child(if empty_library { "Add your first game" } else { "Add a game" }))
            .child(div().text_sm().text_color(muted).child("Flux doesn't come with any games. Point it at one of yours and it works out what it is, how to start it and which settings it needs."));

        let pick = card("CHOOSE IT")
            .child(div()
                .id("drop-zone")
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .h(px(170.))
                .rounded_lg()
                .border_2()
                .border_dashed()
                .border_color(border)
                .drag_over::<ExternalPaths>(move |style, _, _, _| style.border_color(primary).bg(primary.opacity(0.08)))
                .on_drop(cx.listener(|page, paths: &ExternalPaths, window, cx| {
                    if let Some(path) = paths.paths().first() {
                        page.inspect(path.clone(), window, cx);
                    }
                }))
                .child(div().text_sm().text_color(muted).child("Drop a game, disc image or game folder here, or"))
                .child(h_flex().gap_3()
                    .child(Button::new("pick-file").primary().icon(PandoraIcon::File).label("Choose a file").on_click(cx.listener(|this, _, window, cx| this.pick(false, window, cx))))
                    .child(Button::new("pick-folder").icon(PandoraIcon::FolderOpen).label("Choose a folder").on_click(cx.listener(|this, _, window, cx| this.pick(true, window, cx))))))
            .when_some(self.busy, |this, busy| this.child(div().text_sm().text_color(rgb(FLUX_SKY)).child(busy)))
            .when_some(self.error.clone(), |this, error| this.child(div().text_sm().text_color(danger).child(error)));

        let found = self.detection.clone().map(|d| {
            card("FOUND")
                .child(h_flex().gap_3().items_center()
                    .child(div().px_2().py_1().rounded_md().bg(primary).text_color(cx.theme().primary_foreground).text_xs().font_weight(FontWeight::SEMIBOLD).child(d.kind.label()))
                    .child(div().text_xs().text_color(muted).overflow_hidden().text_ellipsis().whitespace_nowrap().child(crate::privacy::path(&d.path))))
                .children(d.notes.iter().map(|note| h_flex().gap_2().items_start()
                    .child(div().mt(px(7.)).size_1p5().flex_shrink_0().rounded_full().bg(muted))
                    .child(div().flex_1().min_w_0().text_sm().child(note.clone()))))
                .child(h_flex().gap_3().items_center().pt_2()
                    .child(div().w(px(60.)).text_sm().text_color(muted).child("Name"))
                    .child(div().flex_1().child(Input::new(&self.name_input)))
                    .child(Button::new("add-game").primary().label("Add to library").disabled(self.busy.is_some())
                        .on_click(cx.listener(|this, _, window, cx| this.add(window, cx)))))
        });

        let kinds = card("WHAT FLUX RECOGNISES")
            .children([
                ("Windows games", "An .exe or the game's folder. Steam games are spotted automatically and get their Steam art; others get a background made from their icon."),
                ("Xbox 360 games", "A disc image (.iso) or an extracted game (default.xex). Played on Xenia, with Graphics, Mods and Debug tabs."),
                ("Original Xbox games", "A disc image (.iso). Played on xemu."),
                ("PlayStation and PlayStation 2 games", "A disc image (.iso, .bin/.cue, .chd). Played on DuckStation (PS1) or PCSX2 (PS2)."),
                ("PlayStation 3 games", "A game folder (with PS3_GAME or EBOOT.BIN). Played on RPCS3, with Mods and Debug tabs."),
                ("PlayStation 4 and PSP games", "A PS4 game folder (shadPS4, early days) or a PSP .iso/.cso (PPSSPP)."),
                ("Nintendo Switch games", "An .nsp or .xci, played on the Switch emulator you choose."),
            ].map(|(what, how)| v_flex().gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(what))
                .child(div().text_xs().text_color(muted).child(how))));

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .max_w(px(900.))
            .child(header)
            .child(pick)
            .children(found)
            .child(kinds)
            .overflow_y_scrollbar()
    }
}
