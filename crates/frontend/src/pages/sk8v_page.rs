//! Tools › SK8V: Skate 3 in GTA V (github.com/becaring/SK8V, a fan mod by becaring). Before it's
//! installed: what SK8V needs and whether Flux found it, the stance, and Install. Afterwards: PLAY
//! (GTA V Story Mode with SK8V), updates, the controls, and Uninstall. `offer` is the popup Flux
//! shows once when it finds both games.

use bridge::{handle::BackendHandle, message::MessageToBackend, modal_action::ModalAction};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, Icon, Sizable, WindowExt, button::{Button, ButtonVariants}, h_flex, scroll::ScrollableElement, v_flex,
};

use crate::{
    entity::DataEntities,
    icon::PandoraIcon,
    interface_config::InterfaceConfig,
    pages::page::Page,
    sk8v::{self, REQUIRED_GTA, Status},
    ui::PageType,
};

const GREEN: u32 = 0x2f8f57;
const AMBER: u32 = 0xc7811f;
/// SK8V's setup needs about 7 GB free on the GTA drive while it works (about 2 GB stays).
const NEEDED_BYTES: u64 = 7_000_000_000;
static TEST_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub struct Sk8vPage {
    backend: BackendHandle,
    message: Option<(SharedString, bool)>,
    _pick: Option<Task<()>>,
    /// The install in progress (or its result), shown on the page.
    job: Option<ModalAction>,
    _job_watch: Option<Task<()>>,
}

impl Sk8vPage {
    pub fn new(data: &DataEntities, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self { backend: data.backend_handle.clone(), message: None, _pick: None, job: None, _job_watch: None }
    }

    /// Install (or update / reinstall), and follow it: the page shows its progress and result, and
    /// looks at the GTA folder again when it ends.
    fn start_install(&mut self, status: &Status, window: &mut Window, cx: &mut Context<Self>) {
        let Some(job) = install(&self.backend, status, window, cx) else { return };
        self.message = None;
        let notify = job.get_notify();
        let watched = job.clone();
        self._job_watch = Some(cx.spawn_in(window, async move |page, cx| {
            loop {
                notify.notified().await;
                let done = watched.get_finished_at().is_some() || watched.get_error_message().is_some();
                if page.update(cx, |_, cx| {
                    sk8v::refresh();
                    cx.notify();
                }).is_err() || done {
                    break;
                }
            }
        }));
        self.job = Some(job);
        cx.notify();
    }

    fn pick_gta(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false, directories: true, multiple: false,
            prompt: Some("Choose your GTA V Legacy folder (the one with GTA5.exe)".into()),
        });
        self._pick = Some(cx.spawn_in(window, async move |page, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else { return };
            let Some(dir) = paths.into_iter().next() else { return };
            _ = page.update(cx, move |page, cx| {
                if dir.join("GTA5.exe").is_file() {
                    InterfaceConfig::get_mut(cx).sk8v_gta_dir = Some(dir);
                    page.message = None;
                } else {
                    page.message = Some(("That folder has no GTA5.exe. GTA V Legacy's folder has GTA5.exe (Enhanced has GTA5_Enhanced.exe, which SK8V doesn't support).".into(), true));
                }
                sk8v::refresh();
                cx.notify();
            });
        }));
    }
}

/// Start installing (or updating) SK8V with what Flux found.
fn install(backend: &BackendHandle, status: &Status, window: &mut Window, cx: &mut App) -> Option<ModalAction> {
    let (Some((_, disc)), Some(gta)) = (status.skate_xbox.as_ref(), status.gta.as_ref()) else { return None };
    let modal_action = ModalAction::default();
    backend.send(MessageToBackend::InstallSk8v {
        skate: disc.clone().into(),
        gta: gta.dir.clone().into(),
        goofy: InterfaceConfig::get(cx).sk8v_goofy,
        modal_action: modal_action.clone(),
    });
    sk8v::refresh();
    crate::modals::generic::show_notification(window, cx, "Couldn't install SK8V".into(), modal_action.clone());
    Some(modal_action)
}

fn gb(bytes: u64) -> String {
    format!("{:.0} GB", bytes as f64 / 1e9)
}

/// One checklist line: (state, title, detail, optional button).
enum Mark {
    Ok,
    Todo,
    Warn,
}

fn row(mark: Mark, title: impl Into<SharedString>, detail: impl Into<SharedString>, button: Option<AnyElement>, cx: &App) -> Div {
    let (icon, color): (PandoraIcon, Hsla) = match mark {
        Mark::Ok => (PandoraIcon::Check, rgb(GREEN).into()),
        Mark::Todo => (PandoraIcon::Download, crate::pages::home_page::blue().into()),
        Mark::Warn => (PandoraIcon::TriangleAlert, rgb(AMBER).into()),
    };
    h_flex().gap_3().items_center().p_3().rounded_lg().bg(cx.theme().background)
        .child(div().size_8().flex_shrink_0().rounded_full().flex().items_center().justify_center().bg(color)
            .child(Icon::new(icon).size_4().text_color(white())))
        .child(v_flex().flex_1().min_w_0().gap_0p5()
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(title.into()))
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(detail.into())))
        .children(button)
}

impl Page for Sk8vPage {
    fn controls(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }

    fn scrollable(&self, _cx: &App) -> bool {
        false
    }
}

impl Render for Sk8vPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = sk8v::status(cx);
        let latest = sk8v::latest(cx);
        // FLUX_SK8V_TEST_INSTALL=1: start installing as soon as the page opens (testing).
        if std::env::var_os("FLUX_SK8V_TEST_INSTALL").is_some() && status.can_install()
            && !TEST_STARTED.swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            self.start_install(&status, window, cx);
        }
        let theme = cx.theme();
        let (muted, border, secondary) = (theme.muted_foreground, theme.border, theme.secondary);
        let card = |title: &'static str| v_flex().gap_3().p_5().rounded_xl().bg(secondary).border_1().border_color(border)
            .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BOLD).text_color(crate::pages::home_page::sky()).child(title));

        let header = v_flex().gap_1()
            .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BLACK).text_size(px(40.)).child("SK8V"))
            .child(h_flex().gap_2().items_center().text_sm().text_color(muted)
                .child("Skate 3's skating in GTA V Story Mode · a free fan mod by becaring")
                .child(Button::new("sk8v-github").small().ghost().label("GitHub").icon(PandoraIcon::ExternalLink)
                    .on_click(|_, _, cx| cx.open_url("https://github.com/becaring/SK8V"))));
        let job_line = self.job.as_ref().and_then(|job| {
            if let Some(error) = job.get_error_message() {
                Some((SharedString::from(format!("Couldn't install SK8V: {error}")), true))
            } else if job.get_finished_at().is_some() {
                Some(("SK8V is installed.".into(), false))
            } else {
                Some(("Installing SK8V… the progress is in the corner; you can keep using Flux.".into(), false))
            }
        });
        let message = self.message.clone().or(job_line).map(|(text, error)| {
            div().text_sm().text_color(if error { cx.theme().danger } else { rgb(0x5ee38a).into() }).child(text)
        });

        let mut content = v_flex().w_full().p_6().gap_4().max_w(px(1000.)).child(header).children(message);

        if status.ready() {
            // ---- Installed: PLAY, updates, controls, options.
            let gta = status.gta.clone().unwrap();
            let installed = status.installed.version.clone();
            // (text, button) when there's something newer, or Flux can't tell (installed some other way).
            let update = match (&installed, &latest) {
                (Some(have), Some(new)) if have != new => Some((format!("SK8V {new} is out."), "Update")),
                (None, Some(new)) => Some((format!("Flux didn't install this copy, so it can't tell its version. Install SK8V {new} to be sure you have the newest."), "Install latest")),
                _ => None,
            };
            let stance = status.installed.stance.clone().unwrap_or_else(|| "Regular".into());
            let summary = format!("SK8V {} · {stance} stance · GTA V Legacy {REQUIRED_GTA} · Skate 3 (Xbox 360)",
                installed.as_deref().unwrap_or("installed"));
            let play = {
                let gta = gta.clone();
                div().id("sk8v-play").w(px(280.)).h(px(58.)).rounded_lg().flex().items_center().justify_center().cursor_pointer()
                    .bg(linear_gradient(90., linear_color_stop(crate::pages::home_page::sky(), 0.), linear_color_stop(crate::pages::home_page::violet(), 1.)))
                    .hover(|s| s.opacity(0.92)).active(|s| s.opacity(0.85))
                    .font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BLACK).text_size(px(26.)).text_color(white())
                    .child("PLAY")
                    .on_click(cx.listener(move |page, _, _, cx| {
                        page.message = Some(match sk8v::launch(&gta, cx) {
                            Ok(()) => ("Starting GTA V Story Mode with SK8V. Pick the skateboard in the weapon wheel, then press Y.".into(), false),
                            Err(e) => (e.into(), true),
                        });
                        cx.notify();
                    }))
            };
            let mut hero = card("READY TO SKATE")
                .child(div().text_sm().text_color(muted).child(summary))
                .child(h_flex().gap_3().items_center().child(play))
                .child(div().text_xs().text_color(muted).child(
                    "PLAY starts GTA V Story Mode without BattlEye (Rockstar's option for single-player mods), so GTA Online isn't available \
                     that session. Start GTA V from its own tile or launcher to play Online as usual.",
                ));
            if let Some((text, label)) = update {
                let status2 = status.clone();
                hero = hero.child(h_flex().gap_3().items_center().p_3().rounded_lg().bg(crate::pages::home_page::blue().opacity(0.14))
                    .child(div().flex_1().min_w_0().text_sm().child(text))
                    .child(Button::new("sk8v-update").small().primary().label(label)
                        .on_click(cx.listener(move |page, _, window, cx| page.start_install(&status2, window, cx)))));
            }
            let controls = card("CONTROLS")
                .child(div().text_xs().text_color(muted).child("All regular skating is Skate 3's. A controller is required."))
                .child(div().grid().grid_cols(2).gap_2().children([
                    ("Ride", "Weapon wheel → Skateboard, then Y"),
                    ("Back to GTA", "Hold Y"),
                    ("SK8V menu", "Back + LB (keyboard: /)"),
                    ("Weapons", "Back to equip · LB aim · RB fire"),
                    ("Radio", "Hold LB + D-pad left/right"),
                    ("Emotes", "D-pad (set them in the SK8V menu)"),
                ].map(|(k, v)| v_flex().p_2().rounded_md().bg(cx.theme().background)
                    .child(div().text_xs().text_color(muted).child(k))
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(v)))));
            let options = {
                let goofy = InterfaceConfig::get(cx).sk8v_goofy;
                let status2 = status.clone();
                let gta_dir = gta.dir.clone();
                let gta_dir2 = gta.dir.clone();
                card("OPTIONS")
                    .child(h_flex().gap_2().flex_wrap()
                        .child(Button::new("sk8v-stance").small().label(if goofy { "Stance: Goofy" } else { "Stance: Regular" })
                            .on_click(|_, _, cx| {
                                let config = InterfaceConfig::get_mut(cx);
                                config.sk8v_goofy = !config.sk8v_goofy;
                            }))
                        .child(Button::new("sk8v-reinstall").small().label("Reinstall with this stance")
                            .on_click(cx.listener(move |page, _, window, cx| page.start_install(&status2, window, cx))))
                        .child(Button::new("sk8v-folder").small().label("GTA V folder").icon(PandoraIcon::FolderOpen)
                            .on_click(move |_, window, cx| crate::open_folder(&gta_dir, window, cx)))
                        .child(Button::new("sk8v-uninstall").small().danger().label("Uninstall")
                            .on_click(cx.listener(move |page, _, _, cx| {
                                page.message = Some(match sk8v::uninstall(&gta_dir2) {
                                    Ok(_) => ("SK8V is uninstalled (ScriptHookV stays: other mods use it)".into(), false),
                                    Err(e) => (format!("{e}. If GTA V is in Program Files, Windows may not let Flux change it.").into(), true),
                                });
                                cx.notify();
                            }))))
            };
            content = content.child(hero).child(controls).child(options);
        } else {
            // ---- Not installed yet: the checklist, stance and Install.
            let mut checks = card("WHAT SK8V NEEDS");
            checks = checks.child(match (&status.skate_xbox, &status.skate_ps3) {
                (Some((game, disc)), _) => row(Mark::Ok, "Your Skate 3 disc (Xbox 360)",
                    format!("{} · {}", game.name, crate::privacy::path(disc)), None, cx),
                (None, Some(_)) => row(Mark::Warn, "Your Skate 3 is the PlayStation 3 copy",
                    "SK8V builds its skating from the Xbox 360 version of the game, so it can't use the PS3 disc. Add your Skate 3 Xbox 360 disc image.",
                    Some(Button::new("sk8v-add-skate").small().label("Add a game")
                        .on_click(|_, window, cx| crate::root::switch_page(PageType::AddGame, &[], window, cx)).into_any_element()), cx),
                (None, None) => row(Mark::Warn, "Your Skate 3 disc (Xbox 360)",
                    "Not in your library yet. Add your own Skate 3 Xbox 360 disc image (.iso) or its unpacked folder.",
                    Some(Button::new("sk8v-add-skate").small().label("Add a game")
                        .on_click(|_, window, cx| crate::root::switch_page(PageType::AddGame, &[], window, cx)).into_any_element()), cx),
            });
            let change = Button::new("sk8v-gta").small().label(if status.gta.is_some() { "Change" } else { "Choose folder" })
                .on_click(cx.listener(|page, _, window, cx| page.pick_gta(window, cx))).into_any_element();
            checks = checks.child(match &status.gta {
                Some(gta) if gta.version.as_deref() == Some(REQUIRED_GTA) => row(Mark::Ok, format!("GTA V Legacy {REQUIRED_GTA}"),
                    format!("{} · {}", if gta.steam { "Steam" } else { "Rockstar / Epic" }, crate::privacy::path(&gta.dir)), Some(change), cx),
                Some(gta) => row(Mark::Warn, format!("GTA V Legacy {REQUIRED_GTA}"),
                    format!("This copy is {}. SK8V and ScriptHookV are made for {REQUIRED_GTA}; Flux waits for an SK8V update that supports it.",
                        gta.version.as_deref().unwrap_or("an unknown version")), Some(change), cx),
                None if status.enhanced => row(Mark::Warn, "GTA V Legacy",
                    "Flux found GTA V Enhanced. SK8V works with GTA V Legacy only (on Steam, Legacy is a separate install).", Some(change), cx),
                None => row(Mark::Warn, "GTA V Legacy", "Not found. Choose the folder with GTA5.exe.", Some(change), cx),
            });
            checks = checks.child(if status.scripthook {
                row(Mark::Ok, "ScriptHookV", "Already in your GTA V folder", None, cx)
            } else {
                row(Mark::Todo, "ScriptHookV", "Flux gets it from its publisher, dev-c.com, during install", None, cx)
            });
            checks = checks.child(if status.controller {
                row(Mark::Ok, "A controller", "Connected. Keyboard skating isn't supported.", None, cx)
            } else {
                row(Mark::Warn, "A controller", "None connected right now. Plug in an Xbox or PlayStation controller before you play.", None, cx)
            });
            let space_ok = status.free_bytes.is_none_or(|b| b >= NEEDED_BYTES);
            checks = checks.child(match status.free_bytes {
                Some(free) => row(if space_ok { Mark::Ok } else { Mark::Warn }, "Space on the GTA V drive",
                    format!("{} free · setup needs about 7 GB, about 2 GB stays", gb(free)), None, cx),
                None => row(Mark::Todo, "Space on the GTA V drive", "Setup needs about 7 GB free; about 2 GB stays", None, cx),
            });

            let goofy = InterfaceConfig::get(cx).sk8v_goofy;
            let stance = |id: &'static str, label: &'static str, on: bool, value: bool| {
                Button::new(id).label(label).when(on, |b| b.primary()).on_click(move |_, _, cx| {
                    InterfaceConfig::get_mut(cx).sk8v_goofy = value;
                })
            };
            let can = status.can_install() && space_ok;
            let status2 = status.clone();
            let busy = self.job.as_ref().is_some_and(|j| j.get_finished_at().is_none() && j.get_error_message().is_none());
            let install_card = card("INSTALL")
                .child(div().text_sm().text_color(muted).child(match &latest {
                    Some(tag) => format!("Latest release: SK8V {tag}, from becaring/SK8V on GitHub (about 217 MB)"),
                    None => "Flux downloads the latest SK8V release from becaring/SK8V on GitHub (about 217 MB)".to_string(),
                }))
                .child(v_flex().gap_2()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Your stance"))
                    .child(h_flex().gap_2()
                        .child(stance("sk8v-regular", "Regular", !goofy, false))
                        .child(stance("sk8v-goofy", "Goofy", goofy, true)))
                    .child(div().text_xs().text_color(muted).child("Which foot leads, like in Skate 3. You can change it later.")))
                .child(Button::new("sk8v-install").primary().label(if busy { "Installing…" } else { "Install SK8V" }).disabled(!can || busy)
                    .on_click(cx.listener(move |page, _, window, cx| page.start_install(&status2, window, cx))))
                .child(div().text_xs().text_color(muted).child(
                    "Flux checks every download (SK8V against its checksums, its Python against python.org and PyPI), gets ScriptHookV from \
                     dev-c.com, then runs SK8V's own setup with your Skate 3 disc and GTA V folder. It takes several minutes and picks up \
                     where it left off if stopped. If GTA V is in Program Files, Windows asks for permission. Close GTA V first.",
                ));
            content = content.child(h_flex().gap_4().items_start().flex_wrap()
                .child(checks.flex_1().min_w(px(420.)))
                .child(install_card.w(px(360.)).flex_shrink_0()));
            if status.installed.in_gta && !status.gta_version_ok() {
                content = content.child(div().text_sm().text_color(rgb(AMBER)).child(
                    "SK8V is installed, but GTA V has updated past the version it supports, so PLAY is paused until SK8V catches up.",
                ));
            }
        }
        let _ = window;
        v_flex().size_full().items_center().child(content).overflow_y_scrollbar()
    }
}

/// Once, when Flux finds both games and SK8V isn't installed: offer it.
pub fn offer(window: &mut Window, cx: &mut App) {
    let config = InterfaceConfig::get(cx);
    if config.sk8v_prompt_dismissed {
        return;
    }
    let status = sk8v::status(cx);
    if status.skate_xbox.is_none() || status.gta.is_none() || status.installed.in_gta {
        return;
    }
    InterfaceConfig::get_mut(cx).sk8v_prompt_dismissed = true;
    window.open_dialog(cx, move |dialog, _, cx| {
        let muted = cx.theme().muted_foreground;
        dialog
            .title("Skate 3 in GTA V")
            .overlay_closable(true)
            .child(v_flex().gap_3()
                .child(div().text_sm().child(
                    "You have Skate 3 (Xbox 360) and GTA V, so you can play SK8V: Skate 3's Flick-It controls, tricks and Hall of Meat in \
                     GTA V Story Mode's open world. Flux installs it and sets everything up.",
                ))
                .child(div().text_xs().text_color(muted).child(
                    "Story Mode only: Flux starts GTA V without BattlEye for it, so GTA Online isn't available that session. Needs a \
                     controller and about 7 GB free. SK8V is a free fan project by becaring, not made by Flux, EA or Rockstar.",
                ))
                .child(h_flex().gap_2().w_full()
                    .child(Button::new("sk8v-offer-yes").flex_1().primary().label("Set up SK8V").on_click(|_, window, cx| {
                        window.close_all_dialogs(cx);
                        crate::root::switch_page(PageType::Sk8v, &[], window, cx);
                    }))
                    .child(Button::new("sk8v-offer-no").flex_1().label("Not now").on_click(|_, window, cx| {
                        window.close_all_dialogs(cx);
                    }))))
    });
}

