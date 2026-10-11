use std::{collections::HashSet, path::PathBuf, sync::Arc};

use bridge::{manual_download::{ManualCurseforgeDownload, ManualCurseforgeDownloadRequest}, notify_signal::KeepAliveNotifySignal};
use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme, Disableable, WindowExt, button::{Button, ButtonVariants}, dialog::Dialog, h_flex, scroll::ScrollableElement, v_flex};

use crate::{component::{path_label::PathLabel, shrinking_text::ShrinkingText}, icon::PandoraIcon};

struct ManualCurseforgeDownloadsDialog {
    files: Arc<[ManualCurseforgeDownload]>,
    download_dir_send: Option<tokio::sync::oneshot::Sender<Arc<std::path::Path>>>,
    completed: HashSet<[u8; 20]>,
    download_path_valid: bool,
    download_path: PathLabel,
    cancelled: bool,
    scroll_handle: UniformListScrollHandle,
    _frontend_alive: KeepAliveNotifySignal,
}

pub fn open(request: ManualCurseforgeDownloadRequest, window: &mut Window, cx: &mut App) {
    let mut directory = directories::UserDirs::new().and_then(|dirs| dirs.download_dir().map(PathBuf::from))
        .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.home_dir().join("Downloads")))
        .unwrap_or(PathBuf::from("."));
    if let Ok(canonicalize) = directory.canonicalize() {
        directory = canonicalize;
    }

    let download_path_valid = directory.is_dir();

    let dialog = cx.new(|_| ManualCurseforgeDownloadsDialog {
        files: request.initial_files.clone(),
        download_dir_send: Some(request.download_dir_send),
        completed: Default::default(),
        download_path_valid,
        download_path: PathLabel::new(directory, download_path_valid),
        cancelled: false,
        scroll_handle: UniformListScrollHandle::new(),
        _frontend_alive: request.frontend_alive,
    });

    let modal_open = KeepAliveNotifySignal::new();
    let modal_open_handle = modal_open.create_handle();
    window.open_dialog(cx, {
        let dialog = dialog.clone();
        move |modal, window, cx| {
            dialog.update(cx, |this, cx| {
                _ = &modal_open;
                this.render(modal, window, cx)
            })
        }
    });

    let mut finished_recv = request.finished_recv;
    let mut add_files_recv = request.add_files_recv;
    let backend_alive = request.backend_alive;
    dialog.update(cx, move |_, cx| {
        cx.spawn(async move |dialog, cx| {
            loop {
                tokio::select! {
                    recv = finished_recv.recv() => {
                        let Some(hash) = recv else {
                            break;
                        };

                        _ = dialog.update(cx, |this, cx| {
                            this.completed.insert(hash);
                            cx.notify();
                        });
                    },
                    add_files = add_files_recv.recv() => {
                        let Some(new_files) = add_files else {
                            break;
                        };

                        _ = dialog.update(cx, |this, cx| {
                            for new_file in &new_files {
                                this.completed.remove(&new_file.sha1);
                            }

                            let mut files = this.files.to_vec();
                            files.extend(new_files);
                            this.files = files.into();

                            cx.notify();
                        });
                    },
                    _ = backend_alive.await_notification() => {
                        break;
                    },
                    _ = modal_open_handle.await_notification() => {
                        break;
                    },
                }
            }
            _ = dialog.update(cx, |this, cx| {
                this.cancelled = true;
                cx.notify();
            });
        }).detach();
    });
}

impl ManualCurseforgeDownloadsDialog {
    fn start_watching(&mut self, cx: &mut Context<Self>) {
        if self.download_dir_send.is_none() {
            return;
        }

        let directory = self.download_path.path();
        self.download_path_valid = directory.is_dir();
        if !self.download_path_valid {
            return;
        }

        if let Some(send) = self.download_dir_send.take() {
            if send.send(directory).is_err() {
                self.cancelled = true;
            }
        }

        cx.notify();
    }

    fn open_all(&mut self) {
        for file in self.files.iter() {
            if self.completed.contains(&file.sha1) {
                continue;
            }
            if let Err(err) = open::that_detached(file.page_url.as_ref()) {
                log::error!("Failed to open manual CurseForge download URL: {err}");
            }
        }
    }

    fn render(&mut self, modal: Dialog, window: &mut Window, cx: &mut Context<Self>) -> Dialog {
        if self.cancelled {
            window.close_dialog(cx);
            return modal;
        }

        if self.download_dir_send.is_some() {
            let select_folder = cx.listener(|_this, _: &ClickEvent, _, cx| {
                let options = PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some(t::instance::content::manual_curseforge_downloads::choose_folder().into())
                };
                let receiver = cx.prompt_for_paths(options);
                cx.spawn(async move |this, cx| {
                    let Ok(Ok(Some(mut paths))) = receiver.await else {
                        return;
                    };
                    let Some(path) = paths.pop() else {
                        return;
                    };
                    _ = this.update(cx, |this, cx| {
                        this.download_path_valid = path.is_dir();
                        this.download_path = PathLabel::new(path, this.download_path_valid);
                        cx.notify();
                    });
                }).detach();
            });

            let start_watching = cx.listener(|this, _: &ClickEvent, _, cx| {
                this.start_watching(cx);
            });
            return modal.title(t::instance::content::manual_curseforge_downloads::title())
                .child(v_flex().gap_2().w_full().min_w_0().line_height(rems(1.2))
                    .child(div().w_full().whitespace_normal().child(t::instance::content::manual_curseforge_downloads::description()))
                    .child(self.download_path.button("choose-folder").on_click(select_folder))
                    // todo: translate
                    .child(h_flex().gap_2().w_full()
                        .child(Button::new("skip").flex_1().warning().label(t::common::skip()).on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                        }))
                        .child(Button::new("start").flex_1().success().label(t::common::cont()).disabled(!self.download_path_valid).on_click(start_watching))
                    )
                );
        }

        let max_list_height = window.viewport_size().height * 0.55;
        let list_height = px(self.files.len() as f32 * 48.0 + 4.0).min(max_list_height);
        let open_all = cx.listener(|this, _: &ClickEvent, _, _| {
            this.open_all()
        });

        let uniform_list = uniform_list("files", self.files.len(), {
            let files = self.files.clone();
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                files[range.clone()].iter().map(|file| {
                    let completed = this.completed.contains(&file.sha1);
                    let trailing = if completed {
                        div().flex_shrink_0().text_color(cx.theme().success)
                            .child(t::instance::content::manual_curseforge_downloads::downloaded()).into_any_element()
                    } else {
                        let url = file.page_url.clone();
                        let open = move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            cx.open_url(&url);
                        };
                        Button::new(SharedString::new(format!("open-{}", file.file_id))).flex_shrink_0().info()
                            .label(t::instance::content::manual_curseforge_downloads::open()).on_click(open).into_any_element()
                    };
                    h_flex().w_full().gap_2().px_2().items_center()
                        .child(v_flex().whitespace_nowrap().overflow_x_hidden().flex_1()
                            .child(SharedString::from(&file.name))
                            .child(div().text_color(cx.theme().muted_foreground).child(ShrinkingText::new(SharedString::from(&file.filename))))
                        )
                        .child(trailing)
                        .h_12()
                        .into_any_element()
                }).collect()
            })
        }).track_scroll(&self.scroll_handle).size_full();

        let elements = div().border_1().rounded(cx.theme().radius_lg).border_color(cx.theme().border)
            .w_full()
            .h(list_height)
            .child(uniform_list)
            .vertical_scrollbar(&self.scroll_handle);

        modal.title(t::instance::content::manual_curseforge_downloads::title())
            .child(v_flex().gap_2().w_full().min_w_0().line_height(rems(1.2))
                .child(div().w_full().whitespace_normal().child(t::instance::content::manual_curseforge_downloads::description()))
                .child(elements)
            ).footer(h_flex().gap_2().w_full()
                .child(Button::new("skip").flex_1().warning().label(t::common::skip()).on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                }))
                .child(Button::new("open-all").flex_1().info().icon(PandoraIcon::ExternalLink).label(t::instance::content::manual_curseforge_downloads::open_all()).on_click(open_all)))
    }
}
