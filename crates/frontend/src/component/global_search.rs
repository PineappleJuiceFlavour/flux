//! The top bar's search: finds games, Minecraft instances and places in Flux. Enter opens the
//! best match; Ctrl+K focuses it from anywhere.

use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme, Icon, Sizable, h_flex, input::{Input, InputEvent, InputState}, v_flex};

use crate::{entity::DataEntities, games, icon::PandoraIcon, root, ui::PageType};

#[derive(Clone)]
enum Target {
    Game(String),
    Instance(gpui::SharedString),
    Page(PageType),
    Settings,
}

#[derive(Clone)]
struct Hit {
    group: &'static str,
    label: SharedString,
    detail: SharedString,
    target: Target,
}

pub struct GlobalSearch {
    data: DataEntities,
    input: Entity<InputState>,
    open: bool,
    selected: usize,
    _subscription: Subscription,
}

/// Places you can jump to by name.
const PLACES: &[(&str, &str)] = &[
    ("Emulators", "emulators"),
    ("Controllers", "controllers"),
    ("Add a game", "add-game"),
    ("Import games", "import-games"),
    ("Browse mods", "mods"),
    ("Skins", "skins"),
    ("Quick Play", "quickplay"),
    ("Minecraft instances", "instances"),
    ("Settings", "settings"),
];

fn place(key: &str) -> Target {
    match key {
        "emulators" => Target::Page(PageType::Emulators),
        "controllers" => Target::Page(PageType::Controllers),
        "add-game" => Target::Page(PageType::AddGame),
        "import-games" => Target::Page(PageType::ImportGames),
        "mods" => Target::Page(PageType::Modrinth { installing_for: None }),
        "skins" => Target::Page(PageType::Skins),
        "quickplay" => Target::Page(PageType::Quickplay),
        "instances" => Target::Page(PageType::Instances),
        _ => Target::Settings,
    }
}

impl GlobalSearch {
    pub fn new(data: &DataEntities, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search games, instances, settings"));
        let _subscription = cx.subscribe_in(&input, window, |this, input, event: &InputEvent, window, cx| match event {
            InputEvent::Change => {
                this.open = !input.read(cx).value().trim().is_empty();
                this.selected = 0;
                cx.notify();
            },
            InputEvent::PressEnter { .. } => {
                if let Some(hit) = this.results(cx).get(this.selected).cloned() {
                    this.activate(hit, window, cx);
                }
            },
            _ => {},
        });
        Self { data: data.clone(), input, open: false, selected: 0, _subscription }
    }

    /// Put the cursor in the search box (Ctrl+K).
    pub fn focus(this: &Entity<Self>, window: &mut Window, cx: &mut App) {
        let input = this.read(cx).input.clone();
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn results(&self, cx: &App) -> Vec<Hit> {
        let query = self.input.read(cx).value().trim().to_string();
        if query.is_empty() {
            return Vec::new();
        }
        let matches = |name: &str| crate::pages::games_home_page::search_matches(&query, name);
        let mut hits: Vec<Hit> = games::games(cx).iter().filter(|g| matches(&g.name)).take(6).map(|g| Hit {
            group: "Games",
            label: g.name.clone().into(),
            detail: games::kind_label(g).into(),
            target: Target::Game(g.id.clone()),
        }).collect();
        let instances = self.data.instances.read(cx);
        hits.extend(instances.entries.values().map(|e| e.read(cx)).filter(|e| e.name != schema::quickplay::INSTANCE_NAME && matches(&e.name)).take(4).map(|e| Hit {
            group: "Minecraft",
            label: e.name.clone(),
            detail: "Minecraft instance".into(),
            target: Target::Instance(e.name.clone()),
        }));
        hits.extend(PLACES.iter().filter(|(name, _)| matches(name)).map(|(name, key)| Hit {
            group: "Go to",
            label: (*name).into(),
            detail: "".into(),
            target: place(key),
        }));
        hits
    }

    fn activate(&mut self, hit: Hit, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.input.update(cx, |input, cx| input.set_value("", window, cx));
        match hit.target {
            Target::Game(id) => root::switch_page(PageType::Game { id: id.into() }, &[], window, cx),
            Target::Instance(name) => root::switch_page(PageType::InstancePage { name }, &[PageType::Instances], window, cx),
            Target::Page(page) => root::switch_page(page, &[], window, cx),
            Target::Settings => crate::settings::open_settings_window(window, &self.data, cx),
        }
        cx.notify();
    }
}

impl Render for GlobalSearch {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border, popover, primary) = (theme.muted_foreground, theme.border, theme.popover, theme.primary);
        let results = if self.open { self.results(cx) } else { Vec::new() };

        let mut panel = v_flex().w(px(460.)).mt(px(40.)).p_1().rounded_lg().bg(popover).border_1().border_color(border).shadow_lg();
        let mut last_group = "";
        for (i, hit) in results.iter().enumerate() {
            if hit.group != last_group {
                last_group = hit.group;
                panel = panel.child(div().px_2().pt_2().pb_1().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(muted).child(hit.group));
            }
            let chosen = i == self.selected;
            let target = hit.clone();
            panel = panel.child(h_flex().id(("search-hit", i)).gap_2().px_2().py_1p5().rounded_md().cursor_pointer().text_sm()
                .when(chosen, |this| this.bg(primary.opacity(0.22)))
                .hover(|s| s.bg(primary.opacity(0.14)))
                .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(hit.label.clone()))
                .child(div().text_xs().text_color(muted).child(hit.detail.clone()))
                .when(chosen, |this| this.child(div().text_xs().text_color(muted).child("Enter")))
                .on_click(cx.listener(move |this, _, window, cx| this.activate(target.clone(), window, cx))));
        }
        if self.open && results.is_empty() {
            panel = panel.child(div().p_3().text_sm().text_color(muted).child("Nothing found"));
        }

        div()
            .relative()
            .w_full()
            .max_w(px(460.))
            // The top bar is a window-drag area: keep presses here for the search box.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .child(Input::new(&self.input).small().prefix(Icon::new(PandoraIcon::Search).size_4().text_color(muted)))
            .when(self.open, |this| this.child(deferred(anchored().snap_to_window().child(
                div().on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.open = false;
                    cx.notify();
                })).child(panel),
            )).with_priority(1)))
    }
}
