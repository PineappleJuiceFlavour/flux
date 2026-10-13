//! The Mods and Debug tabs of a game page (Xbox 360 games on Xenia, PS3 games on RPCS3).

use std::{path::PathBuf, time::Duration};

use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme, Disableable, Sizable, button::{Button, ButtonVariants}, h_flex, scroll::ScrollableElement, switch::Switch, v_flex};

use super::{GamePage, update_game};
use crate::interface_config::InterfaceConfig;
use crate::{
    game_mods::{self, FileMod},
    games::{GameKind, LibraryGame},
    pages::home_page::{FLUX_SKY},
};

fn size_text(bytes: u64) -> String {
    match bytes {
        b if b >= 1_000_000_000 => format!("{:.1} GB", b as f64 / 1e9),
        b if b >= 1_000_000 => format!("{:.1} MB", b as f64 / 1e6),
        b => format!("{} KB", b.div_ceil(1000)),
    }
}

fn ago(time: std::time::SystemTime) -> String {
    let secs = time.elapsed().map(|d| d.as_secs()).unwrap_or(0);
    match secs {
        s if s < 90 => "just now".into(),
        s if s < 5400 => format!("{} minutes ago", s / 60),
        s if s < 172_800 => format!("{} hours ago", s / 3600),
        s => format!("{} days ago", s / 86_400),
    }
}

fn open_path(path: &std::path::Path) -> Result<(), String> {
    if !path.exists() {
        _ = std::fs::create_dir_all(path);
    }
    open::that(path).map_err(|e| format!("Couldn't open {}: {e}", path.display()))
}

impl GamePage {
    fn note(&mut self, result: Result<String, String>, cx: &mut Context<Self>) {
        self.mods_message = Some(match result {
            Ok(text) => (text.into(), false),
            Err(text) => (text.into(), true),
        });
        cx.notify();
    }

    fn pick(&mut self, folder: bool, prompt: &'static str, window: &mut Window, cx: &mut Context<Self>,
        then: impl FnOnce(&mut GamePage, PathBuf, &mut Context<GamePage>) + 'static)
    {
        let receiver = cx.prompt_for_paths(PathPromptOptions { files: !folder, directories: folder, multiple: false, prompt: Some(prompt.into()) });
        self._pick = Some(cx.spawn_in(window, async move |page, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            _ = page.update(cx, move |page, cx| then(page, path, cx));
        }));
    }

    pub(super) fn fetch_patches(&mut self, game: &LibraryGame, window: &mut Window, cx: &mut Context<Self>) {
        let ids = game_mods::ids(game, cx);
        self.mods_busy = Some("Downloading the community patches...");
        cx.notify();
        let task = match game.kind {
            GameKind::Xbox360 => {
                let (Some(dir), Some(tid)) = (game_mods::xenia_dir(cx), ids.title_id) else {
                    self.mods_busy = None;
                    return self.note(Err("Install Xenia first, and make sure Flux can read the game's title ID".into()), cx);
                };
                cx.spawn_in(window, async move |page, cx| {
                    let result = game_mods::fetch_xenia_patches(dir, tid).await.map(|n| format!("Got {n} community patch file{} from xenia-canary/game-patches", if n == 1 { "" } else { "s" }));
                    _ = page.update(cx, |page, cx| {
                        page.mods_busy = None;
                        page.note(result, cx);
                    });
                })
            },
            _ => {
                let Some(base) = game_mods::rpcs3_dir(cx) else {
                    self.mods_busy = None;
                    return self.note(Err("Install RPCS3 first".into()), cx);
                };
                cx.spawn_in(window, async move |page, cx| {
                    let result = game_mods::fetch_rpcs3_patches(base).await.map(|n| format!("Got RPCS3's community patch list ({n} games)"));
                    _ = page.update(cx, |page, cx| {
                        page.mods_busy = None;
                        page.note(result, cx);
                    });
                })
            },
        };
        self._mods_task = Some(task);
    }

    fn start_unpack(&mut self, game: &LibraryGame, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dest) = game_mods::unpack_target(game) else { return };
        let id = game.id.clone();
        let iso = game.path.clone();
        game_mods::UNPACKING.lock().unwrap().push((id.clone(), 0, 1));
        std::thread::Builder::new().name("Flux unpack".into()).spawn({
            let id = id.clone();
            move || {
                let result = game_mods::unpack_disc(&id, &iso, &dest);
                game_mods::UNPACK_DONE.lock().unwrap().push((id, result));
            }
        }).ok();
        self.watch_unpack(window, cx);
    }

    /// Refresh the progress bar; when the unpack ends, point the game at the unpacked copy.
    pub(super) fn watch_unpack(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.id.clone();
        self._unpack_watch = Some(cx.spawn_in(window, async move |page, cx| loop {
            cx.background_executor().timer(Duration::from_millis(400)).await;
            let done = {
                let mut finished = game_mods::UNPACK_DONE.lock().unwrap();
                finished.iter().position(|(i, _)| *i == id).map(|at| finished.remove(at).1)
            };
            let keep = page.update(cx, |page, cx| {
                if let Some(result) = done {
                    game_mods::UNPACKING.lock().unwrap().retain(|e| e.0 != id);
                    match result {
                        Ok(xex) => {
                            update_game(&id, cx, |g| g.path = xex.clone());
                            page.note(Ok(format!("Unpacked. Flux now plays {}; your disc image is untouched.", xex.display())), cx);
                        },
                        Err(e) => page.note(Err(format!("Unpacking stopped: {e}")), cx),
                    }
                    return false;
                }
                cx.notify();
                true
            });
            if !matches!(keep, Ok(true)) {
                break;
            }
        }));
    }

    pub(super) fn render_mods(&mut self, game: &LibraryGame, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, border, secondary, background, danger) = (theme.muted_foreground, theme.border, theme.secondary, theme.background, theme.danger);
        let green: Hsla = rgb(0x5ee38a).into();
        let warn: Hsla = rgb(0xffb547).into();
        let card = |title: &'static str| v_flex().gap_3().p_5().rounded_xl().bg(secondary).border_1().border_color(border)
            .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BOLD).text_color(crate::pages::home_page::sky()).child(title));
        let xbox = game.kind == GameKind::Xbox360;
        let emulator = if xbox { "Xenia" } else { "RPCS3" };
        let installed = if xbox { game_mods::xenia_dir(cx).is_some() } else { game_mods::rpcs3_dir(cx).is_some() };
        let running = self.is_running();
        let ids = game_mods::ids(game, cx);
        // Remember the module hash: Xenia's log only has the last game played.
        if xbox && let Some(hash) = ids.module_hash.clone() && game.module_hash.as_ref() != Some(&hash) {
            update_game(&self.id, cx, |g| g.module_hash = Some(hash));
        }

        if !installed {
            return v_flex().p_6().gap_3()
                .child(div().text_sm().child(format!("Install {emulator} first (Setup tab or Games > Tools > Emulators); mods for this game appear here after that.")))
                .into_any_element();
        }

        let status = self.mods_busy.map(|b| (SharedString::from(b), false)).or(self.mods_message.clone());
        let status = status.map(|(text, error)| div().text_sm().text_color(if error { danger } else { green }).child(text));
        let ids_text = if xbox {
            format!("Title ID {} · game version {}", ids.title_id.as_deref().unwrap_or("unknown"),
                ids.module_hash.as_deref().unwrap_or("unknown until you play it once"))
        } else {
            format!("Serial {} · version {}", ids.serial.as_deref().unwrap_or("unknown"), ids.app_version.as_deref().unwrap_or("unknown"))
        };

        // ---- Patches
        let mut patches = game_mods::patches(game, cx);
        // This game's own patches first, then ones made for every game.
        patches.sort_by_key(|p| (!p.fits, p.source != "your patches", p.name.to_lowercase()));
        let game_ref = game.clone();
        let rows: Vec<AnyElement> = patches.iter().enumerate().map(|(i, p)| {
            let key = p.key.clone();
            let game = game_ref.clone();
            h_flex().gap_3().items_start().p_3().rounded_lg().bg(background)
                .child(Switch::new(("patch", i)).checked(p.enabled).disabled(!p.fits && !p.enabled)
                    .on_click(cx.listener(move |page, on: &bool, _, cx| {
                        let result = game_mods::set_patch(&game, &key, *on, cx).map(|_| String::new());
                        if let Err(e) = result {
                            page.note(Err(e), cx);
                        }
                        cx.notify();
                    })))
                .child(v_flex().flex_1().min_w_0().gap_0p5()
                    .child(h_flex().gap_2().items_center()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(p.name.clone()))
                        .when(p.source == "your patches", |this| this.child(div().text_xs().text_color(crate::pages::home_page::sky()).child("yours")))
                        .when(!p.fits, |this| this.child(div().text_xs().text_color(warn).child("other game version"))))
                    .when(!p.author.is_empty(), |this| this.child(div().text_xs().text_color(muted).child(format!("by {}", p.author))))
                    .when(!p.notes.is_empty(), |this| this.child(div().text_xs().text_color(muted).child(p.notes.chars().take(400).collect::<String>()))))
                .into_any_element()
        }).collect();
        let game_for_add = game.clone();
        let patches_card = card("PATCHES")
            .child(div().text_sm().text_color(muted).child(if xbox {
                "Community fixes and cheats for this exact game (60 FPS, no fall damage, ...). Switch one on and start the game; Xenia applies it."
            } else {
                "Community fixes for this game from RPCS3's patch list (60 FPS, no blur, aspect ratio, ...). Switch one on and start the game."
            }))
            .child(div().text_xs().text_color(muted).child(ids_text.clone()))
            .child(h_flex().gap_2().flex_wrap()
                .child(Button::new("get-patches").primary().small().label(if patches.is_empty() { "Get community patches" } else { "Update community patches" })
                    .disabled(self.mods_busy.is_some())
                    .on_click({
                        let game = game.clone();
                        cx.listener(move |page, _, window, cx| page.fetch_patches(&game, window, cx))
                    }))
                .child(Button::new("add-patch").small().label(if xbox { "Add a patch file (.patch.toml)" } else { "Add a patch file (.yml)" })
                    .on_click(cx.listener(move |page, _, window, cx| {
                        let game = game_for_add.clone();
                        page.pick(false, "Choose a patch file", window, cx, move |page, path, cx| {
                            let result = game_mods::add_patch_file(&game, &path, cx);
                            page.note(result, cx);
                        });
                    })))
                .child(Button::new("open-patches").small().ghost().label("Patches folder").on_click({
                    let dir = if xbox { game_mods::xenia_dir(cx) } else { game_mods::rpcs3_dir(cx) }.map(|d| d.join("patches"));
                    cx.listener(move |page, _, _, cx| {
                        if let Some(dir) = &dir {
                            let result = open_path(dir).map(|_| String::new());
                            if result.is_err() {
                                page.note(result, cx);
                            }
                        }
                    })
                })))
            .children(status)
            .when(rows.is_empty(), |this| this.child(div().text_sm().text_color(muted).child("No patches yet: get the community ones, or add your own file.")))
            .children(rows);

        // ---- Plugins (Xenia)
        let plugins_card = xbox.then(|| {
            let list = game_mods::plugins(game, cx);
            let game_add = game.clone();
            let rows: Vec<AnyElement> = list.iter().enumerate().map(|(i, p)| {
                let (file, file2) = (p.file.clone(), p.file.clone());
                let (g1, g2) = (game.clone(), game.clone());
                h_flex().gap_3().items_center().p_3().rounded_lg().bg(background)
                    .child(Switch::new(("plugin", i)).checked(p.enabled).on_click(cx.listener(move |page, on: &bool, _, cx| {
                        if let Err(e) = game_mods::set_plugin(&g1, &file, Some(*on), cx) {
                            page.note(Err(e), cx);
                        }
                        cx.notify();
                    })))
                    .child(v_flex().flex_1().min_w_0()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(p.name.clone()))
                        .child(div().text_xs().text_color(if p.fits { muted } else { warn }).child(if p.fits { p.file.clone() } else { format!("{}: listed for another version of the game", p.file) })))
                    .child(Button::new(("remove-plugin", i)).small().ghost().label("Remove").on_click(cx.listener(move |page, _, _, cx| {
                        let result = game_mods::set_plugin(&g2, &file2, None, cx).map(|_| format!("Removed {file2}"));
                        page.note(result, cx);
                    })))
                    .into_any_element()
            }).collect();
            card("PLUGINS")
                .child(div().text_sm().text_color(muted).child(
                    "Plugin mods are .xex files that run inside the game (trainers, mod menus, script mods). Xenia only loads a plugin for the \
                     exact game version it's listed for, so play the game once before adding one.",
                ))
                .child(h_flex().child(Button::new("add-plugin").small().label("Add a plugin (.xex)").on_click(cx.listener(move |page, _, window, cx| {
                    let game = game_add.clone();
                    page.pick(false, "Choose a plugin .xex", window, cx, move |page, path, cx| {
                        let result = game_mods::add_plugin(&game, &path, cx);
                        page.note(result, cx);
                    });
                }))))
                .when(rows.is_empty(), |this| this.child(div().text_sm().text_color(muted).child("No plugins added.")))
                .children(rows)
        });

        // ---- File mods
        let root = game_mods::mod_root(game, cx);
        let unpacking = game_mods::UNPACKING.lock().unwrap().iter().find(|e| e.0 == game.id).cloned();
        let mut files_card = card("FILE MODS").child(div().text_sm().text_color(muted).child(
            "Mods that replace the game's own files (textures, models, sounds, maps). Flux keeps a copy of every file a mod replaces, \
             so switching the mod off puts the original back.",
        ));
        if let Some((_, done, total)) = unpacking {
            let pct = if total > 0 { done as f32 / total as f32 } else { 0. };
            let id = game.id.clone();
            files_card = files_card
                .child(div().text_sm().child(format!("Unpacking the disc... {} of {}", size_text(done), size_text(total))))
                .child(div().h(px(8.)).w_full().rounded_full().bg(background)
                    .child(div().h_full().rounded_full().bg(crate::pages::home_page::sky()).w(relative(pct))))
                .child(h_flex().child(Button::new("stop-unpack").small().label("Stop").on_click(move |_, _, _| {
                    game_mods::UNPACK_STOP.lock().unwrap().push(id.clone());
                })));
        } else if xbox && root.is_none() {
            let size = game_mods::disc_size(&game.path);
            let free = game_mods::unpack_target(game).and_then(|t| t.parent().map(|p| p.to_path_buf())).and_then(|p| game_mods::free_space(&p));
            let enough = matches!((size, free), (Some(s), Some(f)) if f > s + 500_000_000) || free.is_none();
            let target = game_mods::unpack_target(game).map(|t| t.display().to_string()).unwrap_or_default();
            let game_unpack = game.clone();
            files_card = files_card
                .child(div().text_sm().child(format!(
                    "{} is a disc image, whose files can't be replaced. Unpack it once{} and Flux plays the unpacked copy instead (it plays the same); \
                     your disc image isn't changed.",
                    game.name, size.map(|s| format!(" ({})", size_text(s))).unwrap_or_default()
                )))
                .child(div().text_xs().text_color(muted).child(format!("Unpacks to {target}")))
                .when(!enough, |this| this.child(div().text_sm().text_color(danger).child(format!(
                    "Not enough free space on that drive ({} free).", free.map(size_text).unwrap_or_default()))))
                .child(h_flex().child(Button::new("unpack").primary().small().label("Unpack disc for file mods").disabled(!enough || running)
                    .on_click(cx.listener(move |page, _, window, cx| page.start_unpack(&game_unpack, window, cx)))));
        } else if root.is_none() {
            files_card = files_card.child(div().text_sm().text_color(warn).child(
                "Flux can't find this game's USRDIR folder. File mods work for PS3 games added as a folder (a disc folder or an installed game).",
            ));
        } else {
            let mods = game.file_mods.clone();
            let game_add = game.clone();
            let rows: Vec<AnyElement> = mods.iter().enumerate().map(|(i, m)| {
                let (id, id2) = (m.id.clone(), m.id.clone());
                let (g1, g2) = (game.clone(), game.clone());
                let all = mods.clone();
                h_flex().gap_3().items_center().p_3().rounded_lg().bg(background)
                    .child(Switch::new(("file-mod", i)).checked(m.enabled).disabled(running).on_click(cx.listener(move |page, on: &bool, _, cx| {
                        let on = *on;
                        match game_mods::set_file_mod(&g1, &all, &id, on, cx) {
                            Ok(installed) => {
                                let n = installed.len();
                                update_game(&page.id, cx, |g| if let Some(m) = g.file_mods.iter_mut().find(|m| m.id == id) {
                                    m.enabled = on;
                                    m.installed = installed;
                                });
                                page.note(Ok(if on { format!("Mod on: {n} files replaced") } else { "Mod off: original files back".into() }), cx);
                            },
                            Err(e) => page.note(Err(e), cx),
                        }
                    })))
                    .child(v_flex().flex_1().min_w_0()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(m.name.clone()))
                        .child(div().text_xs().text_color(muted).child(if m.enabled { format!("{} files in the game", m.installed.len()) } else { "Off".into() })))
                    .child(Button::new(("remove-mod", i)).small().ghost().label("Remove").disabled(m.enabled).on_click(cx.listener(move |page, _, _, cx| {
                        game_mods::remove_file_mod(&g2, &id2);
                        update_game(&page.id, cx, |g| g.file_mods.retain(|m| m.id != id2));
                        page.note(Ok("Mod removed".into()), cx);
                    })))
                    .into_any_element()
            }).collect();
            files_card = files_card
                .child(div().text_xs().text_color(muted).child(format!("Game files: {}", crate::privacy::path(root.as_ref().unwrap()))))
                .child(h_flex().gap_2()
                    .child(Button::new("add-file-mod").small().label("Add a mod folder").on_click(cx.listener(move |page, _, window, cx| {
                        let game = game_add.clone();
                        page.pick(true, "Choose the mod's folder (its files laid out like the game's)", window, cx, move |page, path, cx| {
                            match game_mods::add_file_mod(&game, &path) {
                                Ok(m) => {
                                    let name = m.name.clone();
                                    update_game(&page.id, cx, |g: &mut LibraryGame| g.file_mods.push(m));
                                    page.note(Ok(format!("Added {name}: switch it on to use it")), cx);
                                },
                                Err(e) => page.note(Err(e), cx),
                            }
                        });
                    })))
                    .child(Button::new("add-zip-mod").small().label("Add a mod .zip / .7z").on_click({
                        let game = game.clone();
                        cx.listener(move |page, _, window, cx| {
                            let game = game.clone();
                            page.pick(false, "Choose the mod's .zip / .7z / .rar", window, cx, move |page, path, cx| {
                                match game_mods::add_file_mod(&game, &path) {
                                    Ok(m) => {
                                        let name = m.name.clone();
                                        update_game(&page.id, cx, |g: &mut LibraryGame| g.file_mods.push(m));
                                        page.note(Ok(format!("Added {name}: switch it on to use it")), cx);
                                    },
                                    Err(e) => page.note(Err(e), cx),
                                }
                            });
                        })
                    }))
                    .when(xbox, |this| this.child(Button::new("use-disc").small().ghost().label("Play the disc image again").disabled(running).on_click({
                        let game = game.clone();
                        cx.listener(move |page, _, window, cx| {
                            let iso = game.path.parent().and_then(|dir| {
                                let stem = dir.file_name()?.to_string_lossy().trim_end_matches(" (unpacked)").to_string();
                                let iso = dir.parent()?.join(format!("{stem}.iso"));
                                iso.is_file().then_some(iso)
                            });
                            match iso {
                                Some(iso) if game.file_mods.iter().all(|m| !m.enabled) => {
                                    update_game(&page.id, cx, |g| g.path = iso);
                                    page.note(Ok("Playing the disc image (the unpacked copy is still there)".into()), cx);
                                },
                                Some(_) => page.note(Err("Switch the file mods off first".into()), cx),
                                None => page.note(Err("Couldn't find the original disc image next to the unpacked copy".into()), cx),
                            }
                            let _ = window;
                        })
                    }))))
                .when(rows.is_empty(), |this| this.child(div().text_sm().text_color(muted).child(
                    "No file mods yet. A mod folder's files go into the game's folder at the same paths (PS3 mods may include PS3_GAME/USRDIR).",
                )))
                .children(rows)
                .when(running, |this| this.child(div().text_xs().text_color(warn).child("Close the game before switching file mods.")));
        }

        // ---- Saves
        let saves = game_mods::save_summary(game, cx);
        let backups = game_mods::save_backups(game);
        let (g_backup, g_import, g_auto) = (game.clone(), game.clone(), game.id.clone());
        let saves_folder = saves.folders.first().cloned().or_else(|| if xbox { game_mods::xenia_dir(cx).map(|d| d.join("content")) } else {
            game_mods::rpcs3_dir(cx).map(|d| d.join("dev_hdd0").join("home").join("00000001").join("savedata"))
        });
        let backup_rows: Vec<AnyElement> = backups.iter().take(6).enumerate().map(|(i, (path, label))| {
            let (path, game) = (path.clone(), game.clone());
            h_flex().gap_3().items_center().px_3().py_2().rounded_lg().bg(background)
                .child(div().flex_1().text_sm().child(label.clone()))
                .child(Button::new(("restore", i)).small().label("Restore").disabled(running).on_click(cx.listener(move |page, _, _, cx| {
                    let result = game_mods::restore_saves(&game, &path, cx).map(|_| "Saves restored (your previous saves were backed up first)".to_string());
                    page.note(result, cx);
                })))
                .into_any_element()
        }).collect();
        let saves_card = card("SAVES")
            .child(div().text_sm().child(if saves.files > 0 {
                format!("{} save file{}, {}, last saved {}", saves.files, if saves.files == 1 { "" } else { "s" }, size_text(saves.bytes),
                    saves.modified.map(ago).unwrap_or_else(|| "?".into()))
            } else if xbox {
                "No saves yet. Xenia only saves when a profile is signed in: in Xenia open the Profiles menu, create a profile, sign in, then save in the game.".into()
            } else {
                "No saves yet for this game.".into()
            }))
            .child(h_flex().gap_2().flex_wrap()
                .child(Button::new("backup-saves").small().primary().label("Back up now").disabled(saves.files == 0).on_click(cx.listener(move |page, _, _, cx| {
                    let result = game_mods::backup_saves(&g_backup, "backup", cx).map(|p| match p {
                        Some(_) => "Saves backed up".to_string(),
                        None => "No saves to back up".to_string(),
                    });
                    page.note(result, cx);
                })))
                .child(Button::new("import-save").small().label("Import a save folder").disabled(running).on_click(cx.listener(move |page, _, window, cx| {
                    let game = g_import.clone();
                    page.pick(true, "Choose the save folder to import", window, cx, move |page, path, cx| {
                        let result = game_mods::import_save(&game, &path, cx);
                        page.note(result, cx);
                    });
                })))
                .when_some(saves_folder, |this, folder| this.child(Button::new("open-saves").small().ghost().label("Saves folder").on_click(cx.listener(move |page, _, _, cx| {
                    if let Err(e) = open_path(&folder) {
                        page.note(Err(e), cx);
                    }
                }))))
                .when_some(game.art_dir().map(|d| d.join("save-backups")), |this, folder| this.child(Button::new("open-backups").small().ghost().label("Backups folder").on_click(cx.listener(move |page, _, _, cx| {
                    if let Err(e) = open_path(&folder) {
                        page.note(Err(e), cx);
                    }
                })))))
            .child(h_flex().gap_3().items_center()
                .child(Switch::new("auto-backup").checked(!game.no_auto_save_backup).on_click(move |on: &bool, _, cx| {
                    let on = *on;
                    update_game(&g_auto, cx, |g| g.no_auto_save_backup = !on);
                    cx.refresh_windows();
                }))
                .child(div().text_sm().child("Back up the saves every time Flux starts the game (keeps the last 15)")))
            .when(!backup_rows.is_empty(), |this| this.child(div().text_xs().text_color(muted).child("Backups")))
            .children(backup_rows);

        // ---- Skate 3 on RPCS3: the Native Menu and skaters.
        let skate3 = !xbox && ids.serial.as_deref().is_some_and(crate::rpcs3_config::is_skate3);
        let skate3_cards = skate3.then(|| {
            let config = InterfaceConfig::get(cx);
            let menu = game_mods::native_menu_exe(cx);
            let on = config.skate3_native_menu_on;
            let menu_card = card("NATIVE MENU")
                .child(div().text_sm().text_color(muted).child(
                    "The community Skate 3 Native Menu (S3Native.exe): teleports, physics and skater mods, custom maps and challenges, \
                     changed live while you play. Flux can start it with the game; it attaches to RPCS3 once Skate 3 has loaded.",
                ))
                .child(h_flex().gap_3().items_center()
                    .child(Switch::new("skate3-native-menu").checked(on && menu.is_some()).disabled(menu.is_none()).on_click(|on: &bool, _, cx| {
                        InterfaceConfig::get_mut(cx).skate3_native_menu_on = *on;
                        cx.refresh_windows();
                    }))
                    .child(div().text_sm().child("Start the Native Menu when I play Skate 3")))
                .child(div().text_xs().text_color(muted).child(match &menu {
                    Some(exe) => format!("Menu: {}", crate::privacy::path(exe)),
                    None => "Flux didn't find S3Native.exe (it looks in Downloads and on the Desktop). Choose it to switch this on.".to_string(),
                }))
                .child(h_flex().gap_2().flex_wrap()
                    .child(Button::new("skate3-menu-choose").small().label(if menu.is_some() { "Change" } else { "Choose S3Native.exe" })
                        .on_click(cx.listener(|page, _, window, cx| {
                            page.pick(false, "Choose S3Native.exe (Skate 3 Native Menu)", window, cx, |page, path, cx| {
                                let ok = path.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().ends_with(".exe"));
                                if ok {
                                    let config = InterfaceConfig::get_mut(cx);
                                    config.skate3_native_menu = Some(path);
                                    config.skate3_native_menu_on = true;
                                }
                                page.note(if ok { Ok("Native Menu set: it starts with Skate 3".into()) } else { Err("Choose the menu's S3Native.exe".into()) }, cx);
                            });
                        })))
                    .when_some(menu.clone(), |this, exe| this.child(Button::new("skate3-menu-open").small().label("Open it now")
                        .on_click(cx.listener(move |page, _, _, cx| {
                            let result = game_mods::start_native_menu(&exe).map(|_| "Native Menu started".to_string());
                            page.note(result, cx);
                        })))))
                .child(div().text_xs().text_color(muted).child(
                    "It must run at the same level as RPCS3 (both normally, or both as administrator). Use its physics and speed mods \
                     offline or with friends who agree: in online lobbies everyone needs the same settings or you'll desync.",
                ));

            let skaters = game_mods::saved_skaters(game);
            let rows: Vec<AnyElement> = skaters.iter().enumerate().map(|(i, (path, name))| {
                let (use_path, del_path, g_use) = (path.clone(), path.clone(), game.clone());
                h_flex().gap_3().items_center().p_2().rounded_lg().bg(background)
                    .child(div().flex_1().min_w_0().text_sm().overflow_hidden().whitespace_nowrap().text_ellipsis().child(name.clone()))
                    .child(Button::new(("skater-use", i)).small().primary().label("Use").disabled(running)
                        .on_click(cx.listener(move |page, _, _, cx| {
                            let result = game_mods::use_skater(&g_use, &use_path, cx);
                            page.note(result, cx);
                        })))
                    .child(Button::new(("skater-delete", i)).small().ghost().label("Remove")
                        .on_click(cx.listener(move |page, _, _, cx| {
                            let result = game_mods::delete_saved_skater(&del_path);
                            page.note(result, cx);
                        })))
                    .into_any_element()
            }).collect();
            let (g_import, g_keep) = (game.clone(), game.clone());
            let skaters_card = card("SKATERS")
                .child(div().text_sm().text_color(muted).child(
                    "Import a skater save (a folder with a SKATER.P inside, or any folder that has the BLUS30464-ALIAS_SKATER / \
                     BLES00760-ALIAS_SKATER folder somewhere in it). Flux keeps each one here so you can switch between them, and \
                     backs up your current saves first every time.",
                ))
                .child(h_flex().gap_2().flex_wrap()
                    .child(Button::new("skater-import").small().primary().label("Import a skater").disabled(running)
                        .on_click(cx.listener(move |page, _, window, cx| {
                            let game = g_import.clone();
                            page.pick(true, "Choose the skater save folder (or the folder it came in)", window, cx, move |page, path, cx| {
                                let result = game_mods::import_skater(&game, &path, cx);
                                page.note(result, cx);
                            });
                        })))
                    .child(Button::new("skater-keep").small().label("Keep my current skater")
                        .on_click(cx.listener(move |page, _, _, cx| {
                            let result = game_mods::save_current_skater(&g_keep, cx);
                            page.note(result, cx);
                        }))))
                .when(rows.is_empty(), |this| this.child(div().text_sm().text_color(muted).child("No skaters kept yet.")))
                .children(rows)
                .when(running, |this| this.child(div().text_xs().text_color(warn).child("Close Skate 3 before switching skaters.")));
            [menu_card, skaters_card]
        });

        let _ = window;
        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .max_w(px(1000.))
            .children(skate3_cards.into_iter().flatten())
            .child(patches_card)
            .children(plugins_card)
            .child(files_card)
            .child(saves_card)
            .overflow_y_scrollbar()
            .into_any_element()
    }

    pub(super) fn render_debug(&mut self, game: &LibraryGame, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, border, secondary, background, primary, hover) = (theme.muted_foreground, theme.border, theme.secondary, theme.background, theme.primary, theme.secondary_hover);
        let card = |title: &'static str| v_flex().gap_3().p_5().rounded_xl().bg(secondary).border_1().border_color(border)
            .child(div().font_family(crate::pages::home_page::DISPLAY_FONT).font_weight(FontWeight::BOLD).text_color(crate::pages::home_page::sky()).child(title));
        let xbox = game.kind == GameKind::Xbox360;
        let ids = game_mods::ids(game, cx);
        let emu_id = game.kind.emulator().unwrap_or_default();
        let exe = crate::emulators::exe(emu_id, cx);
        let emu_dir = if xbox { game_mods::xenia_dir(cx) } else { game_mods::rpcs3_dir(cx) };
        let log = game_mods::log_path(game, cx);

        let row = |label: &'static str, value: String| h_flex().gap_3().items_start()
            .child(div().w(px(130.)).flex_shrink_0().text_sm().text_color(muted).child(label))
            .child(div().flex_1().min_w_0().text_sm().font_family("Roboto Mono").child(value));
        let info = card("GAME INFO")
            .child(row("Emulator", exe.as_ref().map(|p| crate::privacy::path(p)).unwrap_or_else(|| "not installed".into())))
            .child(row("Game", crate::privacy::path(&game.path)))
            .when(xbox, |this| this
                .child(row("Title ID", ids.title_id.clone().unwrap_or_else(|| "unknown".into())))
                .child(row("Module hash", ids.module_hash.clone().unwrap_or_else(|| "unknown (play the game once)".into()))))
            .when(!xbox, |this| this
                .child(row("Serial", ids.serial.clone().unwrap_or_else(|| "unknown".into())))
                .child(row("Version", ids.app_version.clone().unwrap_or_else(|| "unknown".into())))
                .child(row("Title", ids.title.clone().unwrap_or_else(|| "unknown".into()))))
            .child(row("Saves", game_mods::save_folders(game, cx).first().map(|p| crate::privacy::path(p)).unwrap_or_else(|| "none yet".into())))
            .child(h_flex().gap_2().flex_wrap()
                .when_some(emu_dir.clone(), |this, dir| this.child(Button::new("open-emu").small().label("Emulator folder").on_click(cx.listener(move |page, _, _, cx| {
                    if let Err(e) = open_path(&dir) {
                        page.note(Err(e), cx);
                    }
                }))))
                .when_some(log.clone(), |this, log| this.child(Button::new("open-log").small().label("Open the full log").on_click(cx.listener(move |page, _, _, cx| {
                    if let Err(e) = open::that(&log) {
                        page.note(Err(format!("Couldn't open the log: {e}")), cx);
                    }
                }))))
                .child(Button::new("copy-info").small().ghost().label("Copy info").on_click({
                    let text = format!("{}\n{}\n{:?}\nemulator: {:?}", game.name, game.path.display(), ids, exe);
                    move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
                })));

        // Xenia switches that matter when something goes wrong.
        let xenia_card = xbox.then(|| {
            let level = crate::xenia_config::get_value("log_level").and_then(|v| v.parse::<i64>().ok()).unwrap_or(2);
            let chip = |i: i64, label: &'static str| div().id(("log-level", i as usize)).px_3().py_1p5().rounded_md().text_sm().cursor_pointer()
                .border_1().border_color(if level == i { primary } else { border })
                .bg(if level == i { primary.opacity(0.25) } else { background })
                .hover(move |s| s.bg(hover))
                .child(label)
                .on_click(cx.listener(move |page, _, _, cx| {
                    let result = crate::xenia_config::set_value("Logging", "log_level", crate::xenia_config::Val::Int(i)).map(|_| format!("Log level set to {label}"));
                    page.note(result, cx);
                }));
            let toggle = |id: &'static str, label: &'static str, section: &'static str, key: &'static str| {
                let on = crate::xenia_config::get_value(key).is_some_and(|v| v == "true");
                h_flex().gap_3().items_center()
                    .child(Switch::new(id).checked(on).on_click(cx.listener(move |page, on: &bool, _, cx| {
                        if let Err(e) = crate::xenia_config::set_value(section, key, crate::xenia_config::Val::Bool(*on)) {
                            page.note(Err(e), cx);
                        }
                        cx.notify();
                    })))
                    .child(div().text_sm().child(label))
            };
            card("XENIA DEBUG")
                .child(div().text_sm().text_color(muted).child("How much Xenia writes to its log. Use Debug when chasing a crash, then set it back: big logs slow games down."))
                .child(h_flex().gap_2().child(chip(0, "Errors")).child(chip(1, "Warnings")).child(chip(2, "Info")).child(chip(3, "Debug")))
                .child(toggle("apply-patches", "Apply patches (switch off to test the game without any)", "General", "apply_patches"))
                .child(toggle("allow-plugins", "Load plugins", "General", "allow_plugins"))
        });

        let lines = log.as_ref().map(|l| game_mods::log_tail(l, 80)).unwrap_or_default();
        let log_card = card("LOG")
            .child(h_flex().gap_2().items_center()
                .child(div().flex_1().text_xs().text_color(muted).child(log.as_ref().map(|l| crate::privacy::path(l)).unwrap_or_else(|| "No log yet".into())))
                .child(Button::new("refresh-log").small().label("Refresh").on_click(cx.listener(|_, _, _, cx| cx.notify()))))
            .child(v_flex().p_3().rounded_lg().bg(background).max_h(px(420.)).overflow_y_scrollbar()
                .children(if lines.is_empty() { vec!["The log appears here after the game has run.".to_string()] } else { lines }.into_iter().map(|l| {
                    let color: Hsla = if l.starts_with("!>") || l.contains("E ") && l.contains("{E}") || l.starts_with("E ") { rgb(0xff6b6b).into() }
                        else if l.starts_with("w>") || l.starts_with("W ") { rgb(0xffb547).into() } else { muted };
                    div().text_xs().font_family("Roboto Mono").text_color(color).child(l)
                })));

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .max_w(px(1000.))
            .children(self.mods_message.clone().map(|(t, e)| div().text_sm().text_color(if e { cx.theme().danger } else { rgb(0x5ee38a).into() }).child(t)))
            .child(info)
            .children(xenia_card)
            .child(log_card)
            .overflow_y_scrollbar()
            .into_any_element()
    }
}

#[allow(dead_code)]
fn _assert_types(_: &FileMod) {}
