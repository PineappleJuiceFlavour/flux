//! FLUX_DEMO=<script>: Flux drives itself, for recording showcase videos and screenshots without a
//! mouse. Steps are separated by `;`:
//!
//! - `wait:<ms>`: pause.
//! - `up` `down` `left` `right` `confirm` `back` `nexttab` `prevtab`: a controller press.
//! - `scroll:<amount>:<ms>`: hold the right stick (-1..1, up is positive) for that long.
//! - `page:<name>`: home, library, minecraft, instances, mods, skins, quickplay, emulators,
//!   controllers, sk8v, or game:<id>.
//! - `chat:<flux|playstation|xbox>:<chats|friends>`: open the chat panel on that network.
//!
//! Privacy mode is always on while a tour runs, so friends' names, pictures and messages never
//! appear in a recording. Only read from the environment; nothing in normal use sets it.

use std::{collections::VecDeque, time::{Duration, Instant}};

use crate::{controller_nav::Nav, interface_config::{GamesLayout, InterfaceConfig}, ui::PageType};

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Wait(u64),
    Press(Nav),
    Scroll(f32, u64),
    Page(String),
    Chat(String, String),
}

pub fn parse(script: &str) -> VecDeque<Step> {
    script.split(';').map(str::trim).filter(|s| !s.is_empty()).filter_map(|step| {
        let mut parts = step.splitn(2, ':');
        let (name, rest) = (parts.next()?, parts.next());
        Some(match (name, rest) {
            ("wait", Some(ms)) => Step::Wait(ms.parse().ok()?),
            ("scroll", Some(rest)) => {
                let (amount, ms) = rest.split_once(':')?;
                Step::Scroll(amount.parse().ok()?, ms.parse().ok()?)
            },
            ("page", Some(page)) => Step::Page(page.to_string()),
            ("chat", Some(rest)) => {
                let (network, view) = rest.split_once(':').unwrap_or((rest, "chats"));
                Step::Chat(network.to_string(), view.to_string())
            },
            ("up", None) => Step::Press(Nav::Up),
            ("down", None) => Step::Press(Nav::Down),
            ("left", None) => Step::Press(Nav::Left),
            ("right", None) => Step::Press(Nav::Right),
            ("confirm", None) => Step::Press(Nav::Confirm),
            ("back", None) => Step::Press(Nav::Back),
            ("nexttab", None) => Step::Press(Nav::NextTab),
            ("prevtab", None) => Step::Press(Nav::PrevTab),
            _ => return None,
        })
    }).collect()
}

/// A page name -> the page, and the Home/Library layout it wants.
pub fn page(name: &str) -> Option<(PageType, Option<GamesLayout>)> {
    Some(match name {
        "home" => (PageType::GamesHome, Some(GamesLayout::Shelf)),
        "library" => (PageType::GamesHome, Some(GamesLayout::Grid)),
        "minecraft" => (PageType::Home, None),
        "instances" => (PageType::Instances, None),
        "mods" => (PageType::Modrinth { installing_for: None }, None),
        "skins" => (PageType::Skins, None),
        "quickplay" => (PageType::Quickplay, None),
        "emulators" => (PageType::Emulators, None),
        "controllers" => (PageType::Controllers, None),
        "sk8v" => (PageType::Sk8v, None),
        other => (PageType::Game { id: other.strip_prefix("game:")?.to_string().into() }, None),
    })
}

/// What the tour wants this tick.
pub enum Action {
    Press(Nav),
    Scroll(f32),
    Page(PageType, Option<GamesLayout>),
    Chat(crate::chat::Network, crate::chat::panel::ChatView),
}

pub struct Tour {
    steps: VecDeque<Step>,
    next_at: Instant,
    scroll: Option<(f32, Instant)>,
}

impl Tour {
    pub fn from_env() -> Option<Tour> {
        let script = std::env::var("FLUX_DEMO").ok()?;
        // A moment for the window and the library to load first.
        Some(Tour { steps: parse(&script), next_at: Instant::now() + Duration::from_millis(2500), scroll: None })
    }

    pub fn tick(&mut self) -> Option<Action> {
        let now = Instant::now();
        if let Some((amount, until)) = self.scroll {
            if now < until {
                return Some(Action::Scroll(amount));
            }
            self.scroll = None;
        }
        if now < self.next_at {
            return None;
        }
        match self.steps.pop_front()? {
            Step::Wait(ms) => {
                self.next_at = now + Duration::from_millis(ms);
                None
            },
            Step::Press(nav) => Some(Action::Press(nav)),
            Step::Scroll(amount, ms) => {
                self.scroll = Some((amount, now + Duration::from_millis(ms)));
                Some(Action::Scroll(amount))
            },
            Step::Page(name) => page(&name).map(|(p, layout)| Action::Page(p, layout)),
            Step::Chat(network, view) => {
                let network = match network.as_str() {
                    "playstation" | "psn" => crate::chat::Network::PlayStation,
                    "xbox" => crate::chat::Network::Xbox,
                    _ => crate::chat::Network::Flux,
                };
                let view = if view == "friends" { crate::chat::panel::ChatView::Friends } else { crate::chat::panel::ChatView::Chats };
                Some(Action::Chat(network, view))
            },
        }
    }
}

/// Apply a page's layout (Home is the shelf, Library the grid), as the top bar's tabs do.
pub fn set_layout(layout: Option<GamesLayout>, cx: &mut gpui::App) {
    if let Some(layout) = layout {
        InterfaceConfig::get_mut(cx).games_layout = layout;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_script() {
        let steps = parse("page:library; wait:1500; right;right; scroll:-0.6:2000; nexttab; bogus");
        assert_eq!(steps, [Step::Page("library".into()), Step::Wait(1500), Step::Press(Nav::Right), Step::Press(Nav::Right),
            Step::Scroll(-0.6, 2000), Step::Press(Nav::NextTab)]);
        assert!(matches!(page("game:skate3"), Some((PageType::Game { .. }, None))));
    }
}
