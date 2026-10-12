//! Minecraft › Quick Play: start Minecraft with no instance to set up (pick a style and a version),
//! jump back into the worlds and servers you played recently, and see friends who are in Minecraft.

use std::{collections::{HashMap, HashSet}, ffi::OsString};

use bridge::{
    handle::BackendHandle, instance::{InstanceID, InstanceServerSummary, InstanceWorldSummary}, message::{MessageToBackend, QuickPlayLaunch},
    meta::MetadataRequest, serial::AtomicOptionSerial,
};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Sizable, h_flex, select::{Select, SelectEvent, SelectState}, v_flex
};
use schema::{quickplay::QuickplayPreset, version_manifest::{MinecraftVersionManifest, MinecraftVersionType}};

use crate::{entity::{DataEntities, instance::InstanceEntry, metadata::{AsMetadataResult, FrontendMetadata, FrontendMetadataResult, FrontendMetadataState}}, interface_config::InterfaceConfig, pages::{instances_page::VersionList, page::Page}, png_render_cache, root};

pub struct QuickplayPage {
    data: DataEntities,
    backend_handle: BackendHandle,
    versions: Entity<FrontendMetadataState>,
    minecraft_version_dropdown: Entity<SelectState<VersionList>>,
    latest_version: Option<SharedString>,
    loaded_versions: bool,
    error_loading_versions: Option<SharedString>,
    /// Instances whose worlds and servers we've asked for, and the request serials.
    watched: HashSet<InstanceID>,
    serials: HashMap<InstanceID, (AtomicOptionSerial, AtomicOptionSerial)>,
    _subscriptions: Vec<Subscription>,
    _versions_updated_subscription: Subscription,
}

/// A world or server for "Jump back in".
enum Recent {
    World(InstanceEntry, InstanceWorldSummary),
    Server(InstanceEntry, InstanceServerSummary),
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
            data: data.clone(),
            watched: HashSet::new(),
            serials: HashMap::new(),
            _subscriptions: vec![cx.observe(&data.instances, |_, _, cx| cx.notify())],
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
        true
    }
}

const PANEL: u32 = 0x141826;
const ONLINE: u32 = 0x5ee38a;

impl Render for QuickplayPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let recent = self.recent(cx);
        let muted = cx.theme().muted_foreground;

        // Play Minecraft now: a style, a version, PLAY.
        let preset = InterfaceConfig::get(cx).quickplay_preset;
        let mut styles = h_flex().p_0p5().gap_0p5().rounded_lg().bg(white().opacity(0.05));
        for (id, label, value) in [("qp-vanilla", "Vanilla", QuickplayPreset::Vanilla), ("qp-maxfps", "Max FPS", QuickplayPreset::Performance),
            ("qp-expanded", "Expanded", QuickplayPreset::Expanded)]
        {
            let on = preset == value;
            styles = styles.child(div().id(id).px_3().py_1p5().rounded_md().cursor_pointer().text_xs().font_weight(FontWeight::SEMIBOLD)
                .when(on, |this| this.bg(crate::pages::home_page::blue()).text_color(white()))
                .when(!on, |this| this.text_color(muted).hover(|s| s.text_color(white())))
                .child(label)
                .on_click(move |_, _, cx| {
                    InterfaceConfig::get_mut(cx).quickplay_preset = value;
                    cx.refresh_windows();
                }));
        }
        let play = gradient_button("qp-play", "PLAY", px(116.), px(46.)).text_base()
            .on_click(cx.listener(|this, _, window, cx| {
                let Some(minecraft_version) = this.minecraft_version_dropdown.read(cx).selected_value() else {
                    return;
                };
                root::start_quickplay(InterfaceConfig::get(cx).quickplay_preset, minecraft_version.as_str().into(),
                    None, this.backend_handle.clone(), window, cx);
            }));
        let style_note = match preset {
            QuickplayPreset::Vanilla => "The game as Mojang made it.",
            QuickplayPreset::Performance => "Performance mods only: more FPS, nothing else changes.",
            QuickplayPreset::Expanded => "Performance mods plus handy extras.",
        };
        let launcher = h_flex().p_5().gap_4().items_center().rounded_xl().bg(rgb(PANEL))
            .child(v_flex().flex_1().min_w_0().gap_1()
                .child(div().text_lg().font_weight(FontWeight::BOLD).child("Play Minecraft now"))
                .child(div().text_xs().text_color(muted).child(format!("No instance to set up: pick a version and a style, and Flux handles the rest. {style_note}"))))
            .child(styles)
            .child(div().w(px(180.)).child(Select::new(&self.minecraft_version_dropdown).small()
                .title_prefix("Version ").search_placeholder(t::common::search())))
            .child(play);

        // Jump back in: recent worlds and servers from every instance.
        let mut list = v_flex().gap_2();
        if recent.is_empty() {
            list = list.child(div().p_5().rounded_xl().bg(rgb(PANEL)).text_sm().text_color(muted)
                .child("Worlds and servers you play show up here, ready to jump back into."));
        }
        let hide_addresses = InterfaceConfig::get(cx).hide_server_addresses || crate::privacy::masking();
        for (ix, item) in recent.iter().enumerate() {
            list = list.child(self.render_recent(ix, item, hide_addresses, cx));
        }
        let main = v_flex().flex_1().min_w_0().gap_4()
            .child(launcher)
            .child(h_flex().gap_3().items_baseline()
                .child(div().text_lg().font_weight(FontWeight::BOLD).child("Jump back in"))
                .child(div().text_xs().text_color(muted).child("servers and worlds you played recently")))
            .child(list);

        h_flex().w_full().p_4().gap_4().items_start()
            .child(main)
            .child(self.render_friends(cx))
    }
}

impl QuickplayPage {
    /// Ask for every instance's worlds and servers once; redraw when they arrive.
    fn watch(&mut self, entries: &[InstanceEntry], cx: &mut Context<Self>) {
        for entry in entries {
            let serials = self.serials.entry(entry.id).or_default();
            entry.worlds_state.set_observed();
            if entry.worlds_state.should_load() {
                self.backend_handle.send_with_serial(MessageToBackend::RequestLoadWorlds { id: entry.id }, &serials.0);
            }
            entry.servers_state.set_observed();
            if entry.servers_state.should_load() {
                self.backend_handle.send_with_serial(MessageToBackend::RequestLoadServers { id: entry.id }, &serials.1);
            }
            if self.watched.insert(entry.id) {
                self._subscriptions.push(cx.observe(&entry.worlds, |_, _, cx| cx.notify()));
                self._subscriptions.push(cx.observe(&entry.servers, |_, _, cx| cx.notify()));
            }
        }
    }

    /// Worlds by when they were last played, and each instance's servers placed by when that
    /// instance was last played (servers.dat has no dates). The eight most recent.
    fn recent(&mut self, cx: &mut Context<Self>) -> Vec<Recent> {
        let entries: Vec<InstanceEntry> = self.data.instances.read(cx).entries.values().map(|e| e.read(cx).clone())
            .filter(|e| e.name != schema::quickplay::INSTANCE_NAME).collect();
        self.watch(&entries, cx);
        let mut items: Vec<(i64, Recent)> = Vec::new();
        let mut seen_servers = HashSet::new();
        for entry in &entries {
            if let Some(worlds) = entry.worlds.read(cx).as_ref() {
                for world in worlds.iter() {
                    items.push((world.last_played, Recent::World(entry.clone(), world.clone())));
                }
            }
            let played = entry.playtime.last_played_unix_ms.unwrap_or(0);
            if let Some(servers) = entry.servers.read(cx).as_ref() {
                for (i, server) in servers.iter().enumerate() {
                    if seen_servers.insert(server.ip.to_ascii_lowercase()) {
                        items.push((played - i as i64, Recent::Server(entry.clone(), server.clone())));
                    }
                }
            }
        }
        items.sort_by_key(|(when, _)| -when);
        items.into_iter().take(8).map(|(_, item)| item).collect()
    }

    fn render_recent(&self, ix: usize, item: &Recent, hide_addresses: bool, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let (icon, title, line, right, button): (Option<schema::unique_bytes::UniqueBytes>, String, String, AnyElement, AnyElement) = match item {
            Recent::World(instance, world) => {
                let when = crate::games::last_played_text((world.last_played > 0).then_some(world.last_played / 1000));
                let (id, name, data) = (instance.id, instance.name.clone(), self.data.clone());
                let target = world.level_path.file_name().map(|f| f.to_owned()).unwrap_or_default();
                let mut line = format!("Singleplayer · {} instance", instance.name);
                if !world.subtitle.is_empty() {
                    line.push_str(&format!(" · {}", world.subtitle));
                }
                (world.png_icon.clone(), world.title.to_string(), line,
                    div().text_xs().text_color(muted).child(format!("Played {}", when.to_lowercase())).into_any_element(),
                    div().id(("recent-play", ix)).px_4().h(px(32.)).flex().items_center().rounded_lg().cursor_pointer()
                        .bg(white().opacity(0.07)).hover(|s| s.bg(white().opacity(0.13))).text_sm().font_weight(FontWeight::SEMIBOLD).child("Play")
                        .on_click(move |_, window, cx| root::start_instance(id, name.clone(), Some(QuickPlayLaunch::Singleplayer(target.clone())), &data, window, cx))
                        .into_any_element())
            },
            Recent::Server(instance, server) => {
                let (id, name, data) = (instance.id, instance.name.clone(), self.data.clone());
                let target = OsString::from(server.ip.to_string());
                let line = if hide_addresses {
                    format!("Server · {} instance", instance.name)
                } else {
                    format!("Server · {} · {} instance", server.ip, instance.name)
                };
                let status: AnyElement = match (&server.status, server.ping) {
                    (Some(status), ping) => {
                        let mut text = status.players.as_ref().map(|p| format!("● {} online", thousands(p.online as i64))).unwrap_or_else(|| "● Online".into());
                        if let Some(ping) = ping {
                            text.push_str(&format!(" · {} ms", ping.as_millis()));
                        }
                        div().text_xs().text_color(rgb(ONLINE)).child(text).into_any_element()
                    },
                    (None, _) if server.pinging => div().text_xs().text_color(muted).child("Checking…").into_any_element(),
                    (None, _) => div().text_xs().text_color(muted).child("Offline").into_any_element(),
                };
                (server.png_icon.clone(), server.name.to_string(), line, status,
                    gradient_button(("recent-join", ix), "Join", px(64.), px(32.)).text_sm()
                        .on_click(move |_, window, cx| root::start_instance(id, name.clone(), Some(QuickPlayLaunch::Multiplayer(target.clone())), &data, window, cx))
                        .into_any_element())
            },
        };
        let icon: AnyElement = match icon {
            Some(png) => png_render_cache::render(png, cx).size_12().min_w_12().rounded_lg().into_any_element(),
            None => div().size_12().min_w_12().rounded_lg().bg(placeholder(&title)).into_any_element(),
        };
        h_flex().p_3().gap_4().items_center().rounded_xl().bg(rgb(PANEL))
            .child(icon)
            .child(v_flex().flex_1().min_w_0().gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().text_ellipsis().child(title))
                .child(div().text_xs().text_color(muted).overflow_hidden().whitespace_nowrap().text_ellipsis().child(line)))
            .child(right)
            .child(button)
            .into_any_element()
    }

    /// Friends whose status says they're in Minecraft, from every chat network.
    fn render_friends(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let mut friends = Vec::new();
        for network in crate::chat::Network::ALL {
            for friend in crate::chat::state(network).friends {
                if friend.online && friend.status.to_lowercase().contains("minecraft") {
                    friends.push(friend);
                }
            }
        }
        let mut panel = v_flex().w(px(260.)).flex_shrink_0().p_4().gap_3().rounded_xl().bg(rgb(PANEL))
            .child(div().text_sm().font_weight(FontWeight::BOLD).child("Friends in Minecraft"));
        if friends.is_empty() {
            panel = panel.child(div().text_xs().text_color(muted)
                .child("When friends from your PlayStation, Xbox or Flux chat are playing Minecraft, they show up here."));
        }
        for friend in friends.iter().take(8) {
            let name = crate::privacy::name(&friend.id, &friend.name);
            let initial = name.chars().next().unwrap_or('?').to_uppercase().to_string();
            panel = panel.child(h_flex().gap_3().items_center()
                .child(div().size_8().flex_shrink_0().rounded_full().bg(placeholder(&friend.id)).flex().items_center().justify_center()
                    .text_xs().font_weight(FontWeight::BOLD).child(initial))
                .child(v_flex().min_w_0()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().text_ellipsis().child(name))
                    .child(div().text_xs().text_color(rgb(ONLINE)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(friend.status.clone()))));
        }
        panel.into_any_element()
    }
}

/// Flux's gradient button (sky to violet), sized for a row.
fn gradient_button(id: impl Into<ElementId>, label: &'static str, w: Pixels, h: Pixels) -> Stateful<Div> {
    div().id(id).w(w).h(h).flex_shrink_0().rounded_lg().flex().items_center().justify_center().cursor_pointer()
        .bg(linear_gradient(100., linear_color_stop(crate::pages::home_page::sky(), 0.), linear_color_stop(crate::pages::home_page::violet(), 1.)))
        .hover(|s| s.opacity(0.9)).active(|s| s.opacity(0.8))
        .font_weight(FontWeight::BOLD).text_color(white()).child(label)
}

/// 31204 -> "31,204".
fn thousands(n: i64) -> String {
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// A muted colour for things without an icon, the same for the same name.
fn placeholder(name: &str) -> Hsla {
    let hash = name.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
    hsla((hash % 360) as f32 / 360., 0.32, 0.32, 1.)
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
