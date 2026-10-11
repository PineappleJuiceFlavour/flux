use std::time::Duration;

use bridge::{handle::BackendHandle, meta::MetadataRequest};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Colorize, Icon, Sizable, button::{Button, ButtonVariants}, checkbox::Checkbox, h_flex, hover_card::HoverCard, select::{Select, SelectEvent, SelectState}, v_flex
};
use schema::{quickplay::QuickplayPreset, version_manifest::{MinecraftVersionManifest, MinecraftVersionType}};

use crate::{entity::{DataEntities, metadata::{AsMetadataResult, FrontendMetadata, FrontendMetadataResult, FrontendMetadataState}}, icon::PandoraIcon, interface_config::InterfaceConfig, pages::{instances_page::VersionList, page::Page}};

pub struct QuickplayPage {
    backend_handle: BackendHandle,
    versions: Entity<FrontendMetadataState>,
    minecraft_version_dropdown: Entity<SelectState<VersionList>>,
    latest_version: Option<SharedString>,
    loaded_versions: bool,
    error_loading_versions: Option<SharedString>,
    _versions_updated_subscription: Subscription,
}

impl QuickplayPage {
    pub fn new(data: &DataEntities, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let versions = FrontendMetadata::request(&data.metadata, MetadataRequest::MinecraftVersionManifest, cx);

        let minecraft_version_dropdown =
            cx.new(|cx| {
                SelectState::new(VersionList::default(), None, window, cx).searchable(true)
            });

        cx.subscribe(&minecraft_version_dropdown, Self::on_minecraft_version_selected).detach();

        let _versions_updated_subscription = cx.observe_in(&versions, window, move |this, _, window, cx| {
            this.reload_version_dropdown(window, cx);
        });

        let mut this = Self {
            backend_handle: data.backend_handle.clone(),
            versions,
            minecraft_version_dropdown,
            latest_version: None,
            loaded_versions: false,
            error_loading_versions: None,
            _versions_updated_subscription
        };

        this.reload_version_dropdown(window, cx);

        this
    }
}

impl Page for QuickplayPage {
    fn controls(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }

    fn scrollable(&self, _cx: &App) -> bool {
        false
    }
}

impl Render for QuickplayPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let version_dropdown = Select::new(&self.minecraft_version_dropdown)
            .title_prefix(format!("{}: ", t::instance::mc_version()))
            .search_placeholder(t::common::search());
        let show_snapshots_button = Checkbox::new("show_snapshots")
            .checked(InterfaceConfig::get(cx).show_snapshots_in_create_instance)
            .label(t::instance::show_snapshots())
            .on_click(cx.listener(move |this, show, window, cx| {
                InterfaceConfig::get_mut(cx).show_snapshots_in_create_instance = *show;
                this.reload_version_dropdown(window, cx);
            }))
            .into_any_element();

        v_flex()
            .size_full()
            .child(h_flex()
                .p_2()
                .gap_2()
                .justify_center()
                .child(Preset(QuickplayPreset::Vanilla))
                .child(Preset(QuickplayPreset::Performance))
                .child(Preset(QuickplayPreset::Expanded))
            )
            .child(h_flex().p_2().gap_2().justify_center().border_t_1().border_color(cx.theme().border)
                .child(v_flex()
                    .gap_2()
                    .items_start()
                    .justify_start()
                    .child(div().child(version_dropdown))
                    .child(show_snapshots_button))
                .child(Button::new("play").h_full().px_20().child(div().text_lg().child("Play")).success().large()
                    .on_click(cx.listener(|this, _, window, cx| {
                        let Some(minecraft_version) = this.minecraft_version_dropdown.read(cx).selected_value() else {
                            return;
                        };

                        crate::root::start_quickplay(InterfaceConfig::get(cx).quickplay_preset, minecraft_version.as_str().into(),
                            None, this.backend_handle.clone(), window, cx);
                    })
                )))
            .child(h_flex().p_2().gap_2().justify_center().border_t_1().border_color(cx.theme().border)
                .w_full()
                .text_color(cx.theme().button_danger_foreground)
                .flex_wrap()
                .child("This 'quickplay' feature is very new, so expect issues.\nThe builtin mod lists were made with modern versions of Minecraft in mind, so you might run into problems when trying older versions of the game.\nIf you do encounter problems or have a suggestion for a mod which should be included in the presets, let me know on the discord"))
    }
}

impl QuickplayPage {
    pub fn reload_version_dropdown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.update_entity(&self.minecraft_version_dropdown, |dropdown, cx| {
            let result: FrontendMetadataResult<MinecraftVersionManifest> = self.versions.read(cx).result();
            let (versions, latest) = match result {
                FrontendMetadataResult::Loading => {
                    self.loaded_versions = false;
                    self.error_loading_versions = None;
                    (Vec::new(), None)
                },
                FrontendMetadataResult::Error(error, _) => {
                    self.loaded_versions = false;
                    self.error_loading_versions = Some(error);
                    (Vec::new(), None)
                },
                FrontendMetadataResult::Loaded(manifest) => {
                    self.loaded_versions = true;
                    self.error_loading_versions = None;

                    let show_snapshots = InterfaceConfig::get(cx).show_snapshots_in_create_instance;
                    let versions: Vec<SharedString> = if show_snapshots {
                        manifest.versions.iter().map(|v| SharedString::from(v.id.as_str())).collect()
                    } else {
                        manifest
                            .versions
                            .iter()
                            .filter(|v| !matches!(v.r#type, MinecraftVersionType::Snapshot))
                            .map(|v| SharedString::from(v.id.as_str()))
                            .collect()
                    };

                    (versions, Some(SharedString::from(manifest.latest.release.as_str())))
                },
            };

            let mut to_select = None;

            if let Some(desired_version) = InterfaceConfig::get(cx).quickplay_minecraft_version.clone()
                && versions.contains(&desired_version)
            {
                to_select = Some(desired_version);
            }

            if let Some(last_selected) = dropdown.selected_value().cloned()
                && versions.contains(&last_selected)
            {
                to_select = Some(last_selected);
            }

            self.latest_version = latest.or(versions.first().cloned());

            if to_select.is_none()
                && let Some(latest) = self.latest_version.clone()
                && versions.contains(&latest)
            {
                to_select = Some(latest);
            }

            dropdown.set_items(
                VersionList {
                    versions: versions.clone(),
                    matched_versions: versions,
                },
                window,
                cx,
            );

            if let Some(to_select) = to_select {
                dropdown.set_selected_value(&to_select, window, cx);
            }

            cx.notify();
        });
    }

    pub fn on_minecraft_version_selected(
        &mut self,
        _state: Entity<SelectState<VersionList>>,
        event: &SelectEvent<VersionList>,
        cx: &mut Context<Self>,
    ) {
        let SelectEvent::Confirm(value): &SelectEvent<VersionList> = event;

        let Some(value) = value else {
            return;
        };

        if self.latest_version.as_ref() == Some(value) {
            InterfaceConfig::get_mut(cx).quickplay_minecraft_version = None;
        } else {
            InterfaceConfig::get_mut(cx).quickplay_minecraft_version = Some(value.clone());
        }
    }
}

#[derive(IntoElement)]
struct Preset(QuickplayPreset);

impl RenderOnce for Preset {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = match self.0 {
            QuickplayPreset::Vanilla => "vanilla",
            QuickplayPreset::Performance => "performance",
            QuickplayPreset::Expanded => "expanded",
        };
        let icon = match self.0 {
            QuickplayPreset::Vanilla => PandoraIcon::Box,
            QuickplayPreset::Performance => PandoraIcon::Rocket,
            QuickplayPreset::Expanded => PandoraIcon::Galaxy,
        };
        let title = match self.0 {
            QuickplayPreset::Vanilla => "Vanilla",
            QuickplayPreset::Performance => "Performance",
            QuickplayPreset::Expanded => "Expanded",
        };
        let description = match self.0 {
            QuickplayPreset::Vanilla => "The base game with no extra mods",
            QuickplayPreset::Performance => "Performance only mods with no gameplay impact",
            QuickplayPreset::Expanded => "Expanded mod list with extra functionality",
        };
        let selected = InterfaceConfig::get(cx).quickplay_preset == self.0;

        v_flex().id(id).h_32().w_64()
            .border_1()
            .rounded(cx.theme().radius)
            .when_else(selected, |this| {
                this.border_color(cx.theme().success.mix_oklab(transparent_white(), 0.4))
                    .bg(cx.theme().tokens.success.background.opacity(0.1))
                    .text_color(cx.theme().success)
            }, |this| {
                this.border_color(cx.theme().border)
                    .hover(|style| style.bg(cx.theme().input.mix_oklab(cx.theme().transparent, 0.5)))
            })
            .text_xl()
            .items_center()
            .justify_center()
            .on_click({
                let preset = self.0;
                move |_, _, cx| {
                    InterfaceConfig::get_mut(cx).quickplay_preset = preset;
                }
            })
            .child(Icon::new(icon).size_10())
            .child(title)
            .child(div().absolute().right_2().top_2()
                .child(HoverCard::new((SharedString::new_static(id), 1))
                    .text_sm()
                    .open_delay(Duration::default())
                    .close_delay(Duration::default())
                    .trigger(Icon::new(PandoraIcon::Info).size_4())
                    .child(description)))
    }
}
