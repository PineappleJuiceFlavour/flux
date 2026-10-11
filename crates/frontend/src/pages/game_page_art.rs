//! The ART card of a game page: pick the tile, background, logo and icon from SteamGridDB's
//! choices (or your own picture). Chosen art is locked: automatic lookups never replace it.

use bridge::message::SgdbImage;
use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme, Disableable, Sizable, button::{Button, ButtonVariants}, h_flex, v_flex};

use super::GamePage;
use crate::{
    games::{self, ArtKind, LibraryGame},
    interface_config::InterfaceConfig,
    pages::home_page::{FLUX_SKY},
};

#[derive(Default)]
pub(super) struct ArtPicker {
    kind: Option<ArtKind>,
    options: Vec<SgdbImage>,
    page: u32,
    busy: Option<&'static str>,
    message: Option<(SharedString, bool)>,
    _task: Option<Task<()>>,
}

impl GamePage {
    pub(super) fn load_art_options(&mut self, game: &LibraryGame, kind: ArtKind, page: u32, cx: &mut Context<Self>) {
        self.art.kind = Some(kind);
        self.art.page = page;
        self.art.busy = Some("Loading SteamGridDB's choices...");
        self.art.message = None;
        if page == 0 {
            self.art.options.clear();
        }
        let recv = games::sgdb_options(game, kind, page, cx);
        self.art._task = Some(cx.spawn(async move |page_entity, cx| {
            let result = recv.await.unwrap_or_else(|_| Err("Stopped".into()));
            _ = page_entity.update(cx, |this, cx| {
                this.art.busy = None;
                match result {
                    Ok(list) if list.is_empty() && this.art.options.is_empty() => this.art.message = Some(("SteamGridDB has none of these for this game yet".into(), true)),
                    Ok(list) if list.is_empty() => this.art.message = Some(("That's all of them".into(), false)),
                    Ok(list) => this.art.options.extend(list),
                    Err(e) => this.art.message = Some((e.to_string().into(), true)),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn choose_art(&mut self, game: &LibraryGame, kind: ArtKind, url: String, cx: &mut Context<Self>) {
        let Some(dir) = game.art_dir() else { return };
        let id = game.id.clone();
        self.art.busy = Some("Downloading...");
        cx.notify();
        let recv = games::fetch(&url);
        self.art._task = Some(cx.spawn(async move |page_entity, cx| {
            let bytes = recv.await.ok().and_then(Result::ok).filter(|b| image::load_from_memory(b).is_ok());
            _ = page_entity.update(cx, |this, cx| {
                this.art.busy = None;
                let Some(bytes) = bytes else {
                    this.art.message = Some(("Couldn't download that picture".into(), true));
                    cx.notify();
                    return;
                };
                let ext = url.rsplit('.').next().filter(|e| e.len() <= 4).unwrap_or("png").to_string();
                let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
                let path = dir.join(format!("chosen-{}-{stamp}.{ext}", kind.key()));
                if std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, bytes)).is_ok() {
                    games::set_art(&id, kind, path, cx);
                    this.art.message = Some((format!("{} changed and kept", kind.label()).into(), false));
                } else {
                    this.art.message = Some(("Couldn't save that picture".into(), true));
                }
                cx.notify();
            });
        }));
    }

    fn own_art(&mut self, game: &LibraryGame, kind: ArtKind, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Choose a picture".into()) });
        let (id, dir) = (game.id.clone(), game.art_dir());
        self.art._task = Some(cx.spawn_in(window, async move |page_entity, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else { return };
            let (Some(path), Some(dir)) = (paths.into_iter().next(), dir) else { return };
            _ = page_entity.update(cx, |this, cx| {
                if image::image_dimensions(&path).is_err() {
                    this.art.message = Some(("That isn't a picture Flux can read (PNG, JPG, WEBP...)".into(), true));
                } else {
                    let ext = path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_else(|| "png".into());
                    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
                    let dest = dir.join(format!("own-{}-{stamp}.{ext}", kind.key()));
                    if std::fs::create_dir_all(&dir).and_then(|_| std::fs::copy(&path, &dest)).is_ok() {
                        games::set_art(&id, kind, dest, cx);
                        this.art.message = Some((format!("{} changed and kept", kind.label()).into(), false));
                    }
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn render_art_card(&mut self, game: &LibraryGame, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, border, secondary, background, primary, danger) = (theme.muted_foreground, theme.border, theme.secondary, theme.background, theme.primary, theme.danger);
        let has_key = InterfaceConfig::get(cx).steamgriddb_key.as_ref().is_some_and(|k| !k.trim().is_empty());
        let green: Hsla = rgb(0x5ee38a).into();

        // The four kinds with what's used now; click one to see the choices.
        let kinds = h_flex().gap_3().flex_wrap().children(ArtKind::ALL.into_iter().map(|kind| {
            let current = kind.current(game);
            let locked = game.locked_art.iter().any(|k| k == kind.key());
            let active = self.art.kind == Some(kind);
            let (w, h) = match kind {
                ArtKind::Tile | ArtKind::Icon => (110., 110.),
                ArtKind::Background => (220., 110.),
                ArtKind::Logo => (180., 110.),
            };
            let game_for_click = game.clone();
            v_flex().id(SharedString::from(format!("art-kind-{}", kind.key()))).gap_1().cursor_pointer()
                .child(div().w(px(w)).h(px(h)).rounded_lg().overflow_hidden().bg(background)
                    .border_2().border_color(if active { primary } else { border })
                    .children(current.map(|p| img(ImageSource::Resource(Resource::Path(p.into()))).w(px(w)).h(px(h)).aspect_ratio(w / h)
                        .object_fit(if kind == ArtKind::Logo { ObjectFit::Contain } else { ObjectFit::Cover }))))
                .child(h_flex().gap_1p5().items_center()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(kind.label()))
                    .when(locked, |this| this.child(div().text_xs().text_color(rgb(FLUX_SKY)).child("chosen"))))
                .on_click(cx.listener(move |page, _, _, cx| {
                    if page.art.kind == Some(kind) {
                        page.art.kind = None;
                        cx.notify();
                    } else {
                        page.load_art_options(&game_for_click, kind, 0, cx);
                    }
                }))
        }));

        let mut card = v_flex().gap_3().p_5().rounded_xl().bg(secondary).border_1().border_color(border)
            .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BOLD).text_color(rgb(FLUX_SKY)).child("ART"))
            .child(div().text_sm().text_color(muted).child(
                "Click a kind to pick from SteamGridDB, or use your own picture. What you pick stays: Flux never swaps it, even after updates.",
            ))
            .child(kinds);

        if !has_key {
            card = card.child(div().text_sm().text_color(danger).child("Add your free SteamGridDB key on the Games screen (Art button) to pick from SteamGridDB."));
        }

        if let Some(kind) = self.art.kind {
            let (tw, th) = match kind {
                ArtKind::Tile | ArtKind::Icon => (150., 150.),
                ArtKind::Background => (300., 97.),
                ArtKind::Logo => (220., 110.),
            };
            let locked = game.locked_art.iter().any(|k| k == kind.key());
            let (g1, g2, g3) = (game.clone(), game.clone(), game.clone());
            let choices = self.art.options.iter().enumerate().map(|(i, option)| {
                let url = option.url.to_string();
                let game = game.clone();
                v_flex().id(("art-option", i)).gap_0p5().cursor_pointer()
                    .child(div().w(px(tw)).h(px(th)).rounded_md().overflow_hidden().bg(rgb(0x10131c)).border_1().border_color(border)
                        .hover(move |s| s.border_color(primary))
                        .child(img(SharedUri::from(option.thumb.to_string())).w(px(tw)).h(px(th)).aspect_ratio(tw / th)
                            .object_fit(if kind == ArtKind::Logo { ObjectFit::Contain } else { ObjectFit::Cover })))
                    .child(div().text_xs().text_color(muted).child(format!("{}x{}{}", option.width, option.height,
                        if option.author.is_empty() { String::new() } else { format!(" · {}", option.author) })))
                    .on_click(cx.listener(move |page, _, _, cx| page.choose_art(&game, kind, url.clone(), cx)))
            });
            card = card
                .child(h_flex().gap_2().items_center()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(format!("Choose a {}", kind.label().to_lowercase())))
                    .child(div().flex_1())
                    .child(Button::new("own-art").small().label("Use my own picture").on_click(cx.listener(move |page, _, window, cx| page.own_art(&g1, kind, window, cx))))
                    .when(locked, |this| this.child(Button::new("unlock-art").small().ghost().label("Let Flux choose again").on_click(cx.listener(move |page, _, _, cx| {
                        games::unlock_art(&g2.id, kind, cx);
                        if let Some(task) = games::find_web_art(&g2.id, true, cx) {
                            task.detach();
                        }
                        page.art.message = Some(("Flux will pick the best one again".into(), false));
                        cx.notify();
                    })))))
                .when_some(self.art.busy, |this, b| this.child(div().text_sm().text_color(rgb(FLUX_SKY)).child(b)))
                .when_some(self.art.message.clone(), |this, (m, error)| this.child(div().text_sm().text_color(if error { danger } else { green }).child(m)))
                .child(h_flex().flex_wrap().gap_3().children(choices))
                .when(!self.art.options.is_empty() && self.art.options.len() % 50 == 0, |this| this.child(h_flex().child(
                    Button::new("more-art").small().label("Show more").disabled(self.art.busy.is_some()).on_click(cx.listener(move |page, _, _, cx| {
                        let next = page.art.page + 1;
                        page.load_art_options(&g3, kind, next, cx);
                    })))));
        }
        card.into_any_element()
    }
}
