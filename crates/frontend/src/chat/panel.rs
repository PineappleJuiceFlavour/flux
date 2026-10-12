//! The chat panel (a sheet on the right): Flux / PlayStation / Xbox switch, your chats and your
//! friends (add, accept, remove).

use std::time::Duration;

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, Icon, Sizable, button::{Button, ButtonVariants}, h_flex, input::{Input, InputEvent, InputState},
    scroll::ScrollableElement, sheet::Sheet, v_flex,
};

use super::{Friend, Network, NetState};
use crate::{icon::PandoraIcon, interface_config::InterfaceConfig, pages::home_page::{DISPLAY_FONT, FLUX_SKY}};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChatView {
    Chats,
    Friends,
}

pub struct ChatPanel {
    pub view: ChatView,
    /// The open conversation (friend id) per network.
    open: std::collections::HashMap<Network, String>,
    message: Entity<InputState>,
    add: Entity<InputState>,
    npsso: Entity<InputState>,
    homeserver: Entity<InputState>,
    messages_scroll: ScrollHandle,
    shown_messages: usize,
    seen_revision: u64,
    _poll: Task<()>,
}

/// The sheet holding the panel.
pub fn sheet(panel: Entity<ChatPanel>) -> impl Fn(Sheet, &mut Window, &mut App) -> Sheet + 'static {
    move |sheet, _, _| sheet.title("Chat").size(px(460.)).child(panel.clone())
}

impl ChatPanel {
    pub fn new(view: ChatView, window: &mut Window, cx: &mut Context<Self>) -> Self {
        super::start();
        let message = cx.new(|cx| InputState::new(window, cx).placeholder("Message"));
        let add = cx.new(|cx| InputState::new(window, cx).placeholder("Username"));
        let npsso = cx.new(|cx| InputState::new(window, cx).placeholder("Paste your npsso code").masked(true));
        let homeserver = cx.new(|cx| InputState::new(window, cx).default_value("matrix.org"));
        cx.subscribe_in(&message, window, |panel, input, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                panel.send_message(input.clone(), window, cx);
            }
        }).detach();
        cx.subscribe_in(&add, window, |panel, input, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                panel.add_friend(input.clone(), window, cx);
            }
        }).detach();
        // Redraw when chat changes; keep an open PlayStation / Xbox chat fresh.
        let _poll = cx.spawn(async move |panel, cx| {
            let mut tick = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_millis(400)).await;
                tick += 1;
                let alive = panel.update(cx, |panel, cx| {
                    if panel.seen_revision != super::revision() {
                        panel.seen_revision = super::revision();
                        cx.notify();
                    }
                    if tick % 12 == 0 {
                        let network = InterfaceConfig::get(cx).chat_network;
                        if network != Network::Flux && panel.view == ChatView::Chats && let Some(friend) = panel.open.get(&network) {
                            super::open_chat(network, friend.clone());
                        }
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        Self {
            view,
            open: Default::default(),
            message,
            add,
            npsso,
            homeserver,
            messages_scroll: ScrollHandle::new(),
            shown_messages: 0,
            seen_revision: 0,
            _poll,
        }
    }

    fn network(&self, cx: &App) -> Network {
        InterfaceConfig::get(cx).chat_network
    }

    fn send_message(&mut self, input: Entity<InputState>, window: &mut Window, cx: &mut Context<Self>) {
        let network = self.network(cx);
        let Some(friend) = self.open.get(&network).cloned() else { return };
        let text = input.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        super::send(network, friend, text);
        input.update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn add_friend(&mut self, input: Entity<InputState>, window: &mut Window, cx: &mut Context<Self>) {
        let name = input.read(cx).value().to_string();
        super::add_friend(self.network(cx), name);
        input.update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn open_friend(&mut self, network: Network, friend: &str, cx: &mut Context<Self>) {
        self.open.insert(network, friend.to_string());
        self.view = ChatView::Chats;
        self.shown_messages = 0;
        super::open_chat(network, friend.to_string());
        cx.notify();
    }
}

/// A friend's name as shown (a stand-in in privacy mode).
fn shown_name(friend: &Friend) -> String {
    crate::privacy::name(&friend.id, &friend.name)
}

/// A friend's status line ("Online", "Playing Skate 3"), with ids and emails hidden in privacy mode.
fn shown_status(friend: &Friend) -> String {
    if crate::privacy::masking() && friend.status.contains(&friend.id) {
        return if friend.online { "Online".into() } else { "Offline".into() };
    }
    crate::privacy::text(&friend.status)
}

fn avatar(friend: &Friend, size: f32) -> AnyElement {
    let ring: Hsla = if friend.online { rgb(0x5ee38a).into() } else { gpui::white().opacity(0.15) };
    let picture = if crate::privacy::masking() { None } else { friend.avatar.as_ref() };
    let initial = shown_name(friend).chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let inner = match picture {
        Some(url) => img(SharedUri::from(url.clone())).size(px(size)).rounded_full().into_any_element(),
        None => div().size(px(size)).rounded_full().bg(rgb(0x24305a)).flex().items_center().justify_center()
            .text_size(px(size * 0.42)).font_weight(FontWeight::BOLD)
            .child(initial)
            .into_any_element(),
    };
    div().flex_shrink_0().p(px(2.)).rounded_full().border_2().border_color(ring).child(inner).into_any_element()
}

impl Render for ChatPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let network = self.network(cx);
        let state = super::state(network);
        let theme = cx.theme();
        let (muted, border, primary, danger) = (theme.muted_foreground, theme.border, theme.primary, theme.danger);
        let height = (window.viewport_size().height - px(110.)).max(px(300.));

        // Flux / PlayStation / Xbox.
        let switch = h_flex().p_1().gap_1().rounded_lg().bg(gpui::white().opacity(0.05)).children(Network::ALL.map(|n| {
            let on = n == network;
            let unread = !super::state(n).unread.is_empty();
            h_flex().id(n.label()).flex_1().justify_center().gap_1p5().py_1p5().rounded_md().cursor_pointer().text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .when(on, |this| this.bg(primary).text_color(cx.theme().primary_foreground))
                .when(!on, |this| this.text_color(muted).hover(|s| s.bg(gpui::white().opacity(0.06))))
                .child(n.label())
                .when(unread, |this| this.child(div().size(px(7.)).rounded_full().bg(rgb(0xff5d5d))))
                .on_click(cx.listener(move |_, _, _, cx| {
                    InterfaceConfig::get_mut(cx).chat_network = n;
                    cx.notify();
                }))
        }));

        let requests_in = state.requests.iter().filter(|r| r.incoming).count();
        let tabs = h_flex().gap_4().children([(ChatView::Chats, "Chats".to_string()), (ChatView::Friends, if requests_in > 0 {
            format!("Friends ({requests_in})")
        } else {
            "Friends".to_string()
        })].map(|(view, label)| {
            let on = self.view == view;
            v_flex().id(SharedString::from(format!("chat-tab-{view:?}"))).cursor_pointer().gap_1()
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(if on { cx.theme().foreground } else { muted }).child(label))
                .child(div().h(px(2.)).rounded_full().when(on, |this| this.bg(primary)))
                .on_click(cx.listener(move |panel, _, _, cx| {
                    panel.view = view;
                    cx.notify();
                }))
        }));

        let banner = state.error.clone().map(|e| (e, true)).or(state.notice.clone().map(|n| (n, false))).map(|(mut text, error)| {
            if crate::privacy::masking() {
                for friend in &state.friends {
                    text = text.replace(&friend.name, &shown_name(friend)).replace(&friend.id, &shown_name(friend));
                }
                for request in &state.requests {
                    text = text.replace(&request.name, &crate::privacy::name(&request.id, &request.name));
                }
                text = crate::privacy::text(&text);
            }
            h_flex().gap_2().items_start().p_2().rounded_md().bg(if error { danger.opacity(0.15) } else { primary.opacity(0.15) })
                .child(div().flex_1().text_xs().child(text))
                .child(div().id("chat-banner-close").cursor_pointer().text_xs().text_color(muted).child("✕").on_click(move |_, _, _| {
                    super::update(network, |s| {
                        s.error = None;
                        s.notice = None;
                    });
                }))
        });

        let body = if !state.connected {
            self.connect_card(network, &state, cx)
        } else if self.view == ChatView::Friends {
            self.friends_view(network, &state, cx)
        } else {
            match self.open.get(&network).and_then(|id| state.friends.iter().find(|f| &f.id == id)).cloned() {
                Some(friend) => self.conversation(network, &state, &friend, window, cx),
                None => self.chat_list(network, &state, cx),
            }
        };

        let privacy = crate::privacy::on();
        let privacy_toggle = h_flex().id("chat-privacy").gap_1p5().items_center().cursor_pointer().text_xs()
            .text_color(if privacy { crate::pages::home_page::sky().into() } else { muted })
            .child(Icon::new(if privacy { PandoraIcon::EyeOff } else { PandoraIcon::Eye }).size_3p5())
            .child(if privacy { "Privacy mode on: names and messages hidden" } else { "Privacy mode" })
            .on_click(|_, _, cx| {
                let on = !crate::privacy::on();
                InterfaceConfig::get_mut(cx).privacy_mode = on;
                crate::privacy::set(on);
                cx.refresh_windows();
            });
        v_flex().h(height).gap_3()
            .child(h_flex().justify_end().child(privacy_toggle))
            .child(switch)
            .when(state.connected, |this| this.child(tabs))
            .children(banner)
            .child(div().flex_1().min_h_0().child(body))
    }
}

impl ChatPanel {
    fn connect_card(&mut self, network: Network, state: &NetState, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let sky: Hsla = crate::pages::home_page::sky().into();
        let heading = |text: &'static str| div().font_family(DISPLAY_FONT).font_weight(FontWeight::BOLD).text_lg().child(text);
        let step = |text: &'static str| div().text_sm().text_color(muted).child(text);
        let link = |id: &'static str, label: &'static str, url: &'static str| div().id(id).text_sm().text_color(sky).cursor_pointer()
            .hover(|s| s.underline()).child(label).on_click(move |_, _, cx| cx.open_url(url));
        let busy = state.connecting;
        let card = v_flex().gap_2p5().p_4().rounded_xl().bg(gpui::white().opacity(0.04)).border_1().border_color(cx.theme().border);
        match network {
            Network::Flux => card
                .child(heading("Flux chat"))
                .child(step("Chat with friends you add here. Flux chat runs on Matrix, a free and open chat network: no Flux server, and your friends can also reply from any Matrix app (like Element)."))
                .child(step("Click below: your browser opens Matrix's sign-in page, where you can sign in or create a free account. Flux never sees your password."))
                .child(h_flex().gap_2().items_center()
                    .child(div().text_xs().text_color(muted).child("Server"))
                    .child(div().w(px(180.)).child(Input::new(&self.homeserver).small())))
                .child(Button::new("connect-flux").primary().label(if busy { "Waiting for your browser..." } else { "Sign in or create an account" })
                    .disabled(busy)
                    .on_click(cx.listener(|panel, _, _, cx| {
                        let server = panel.homeserver.read(cx).value().to_string();
                        super::connect_flux(server);
                    })))
                .into_any_element(),
            Network::PlayStation => card
                .child(heading("PlayStation chat"))
                .child(div().p_2().rounded_md().bg(rgb(0x5a3b00).opacity(0.5)).text_xs().child(
                    "Heads up: Sony has no public chat API. Flux uses the same private API as the PlayStation App, so it can stop working when Sony changes it, and Sony could notice an unusual app on your account. Your token stays on this PC (encrypted).",
                ))
                .child(step("1. Sign in at playstation.com in your browser."))
                .child(link("psn-site", "Open playstation.com", "https://www.playstation.com/"))
                .child(step("2. Then open this page. It shows {\"npsso\":\"...\"}."))
                .child(link("psn-npsso", "Open the npsso page", "https://ca.account.sony.com/api/v1/ssocookie"))
                .child(step("3. Copy the long code between the quotes (or the whole line) and paste it here:"))
                .child(Input::new(&self.npsso))
                .child(Button::new("connect-psn").primary().label(if busy { "Connecting..." } else { "Connect PlayStation" }).disabled(busy)
                    .on_click(cx.listener(|panel, _, window, cx| {
                        let token = panel.npsso.read(cx).value().to_string();
                        if !token.trim().is_empty() {
                            super::connect_playstation(token);
                            panel.npsso.update(cx, |input, cx| input.set_value("", window, cx));
                        }
                    })))
                .into_any_element(),
            Network::Xbox => card
                .child(heading("Xbox chat"))
                .child(div().p_2().rounded_md().bg(rgb(0x5a3b00).opacity(0.5)).text_xs().child(
                    "Heads up: Xbox only delivers messages from its own apps, so Flux signs in to Xbox chat the way the Xbox app does (not an official API for other apps). It can stop working when Microsoft changes it, and Microsoft could notice an unusual app on your account. The sign-in stays on this PC, in Windows Credential Manager.",
                ))
                .child(step("Click below, then sign in with your Microsoft account at microsoft.com/link using the code Flux shows. Your friends, their status and your messages then work here, and messages you send arrive in the Xbox app."))
                .children(state.sign_in_code.clone().map(|(code, link)| v_flex().gap_1p5().p_3().rounded_lg().bg(gpui::white().opacity(0.06))
                    .child(div().text_xs().text_color(muted).child("Enter this code at microsoft.com/link (opened in your browser):"))
                    .child(div().font_family(DISPLAY_FONT).font_weight(FontWeight::BLACK).text_size(px(28.)).child(code))
                    .child(div().id("xbox-link").text_sm().text_color(sky).cursor_pointer().hover(|s| s.underline())
                        .child("Open microsoft.com/link again")
                        .on_click(move |_, _, cx| cx.open_url(&link)))))
                .child(Button::new("connect-xbox").primary()
                    .label(if state.sign_in_code.is_some() { "Waiting for you to sign in..." } else if busy { "Connecting..." } else { "Connect Xbox chat" })
                    .disabled(busy)
                    .on_click(|_, _, _| super::connect_xbox()))
                .into_any_element(),
        }
    }

    fn chat_list(&mut self, network: Network, state: &NetState, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        if state.friends.is_empty() {
            return v_flex().gap_3().pt_4().items_center()
                .child(div().text_sm().text_color(muted).child("No friends here yet."))
                .child(Button::new("go-friends").label("Add friends").on_click(cx.listener(|panel, _, _, cx| {
                    panel.view = ChatView::Friends;
                    cx.notify();
                })))
                .into_any_element();
        }
        let rows = v_flex().gap_1().children(state.friends.iter().map(|friend| {
            let id = friend.id.clone();
            let unread = state.unread.contains(&friend.id);
            let last = state.messages.get(&friend.id).and_then(|m| m.last()).map(|m| m.text.clone());
            h_flex().id(SharedString::from(format!("chat-{}", friend.id))).gap_3().p_2().rounded_lg().cursor_pointer()
                .hover(|s| s.bg(gpui::white().opacity(0.06)))
                .child(avatar(friend, 38.))
                .child(v_flex().flex_1().min_w_0()
                    .child(div().text_sm().font_weight(if unread { FontWeight::BOLD } else { FontWeight::MEDIUM }).child(shown_name(friend)))
                    .child(div().text_xs().text_color(muted).overflow_hidden().whitespace_nowrap().text_ellipsis()
                        .child(last.map(|t| crate::privacy::message(&t)).unwrap_or_else(|| shown_status(friend)))))
                .when(unread, |this| this.child(div().size(px(9.)).rounded_full().bg(rgb(0xff5d5d))))
                .on_click(cx.listener(move |panel, _, _, cx| panel.open_friend(network, &id, cx)))
        }));
        v_flex().size_full().child(rows).overflow_y_scrollbar().into_any_element()
    }

    fn conversation(&mut self, network: Network, state: &NetState, friend: &Friend, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let primary = cx.theme().primary;
        let messages = state.messages.get(&friend.id).cloned().unwrap_or_default();
        if messages.len() != self.shown_messages {
            self.shown_messages = messages.len();
            let handle = self.messages_scroll.clone();
            // Newest at the bottom, in view.
            window.on_next_frame(move |_, _| handle.scroll_to_bottom());
        }
        if state.unread.contains(&friend.id) {
            let id = friend.id.clone();
            super::update(network, |s| s.unread.retain(|f| f != &id));
        }
        let header = h_flex().gap_3().items_center()
            .child(div().id("chat-back").p_1().rounded_md().cursor_pointer().hover(|s| s.bg(gpui::white().opacity(0.08)))
                .child(Icon::new(PandoraIcon::ArrowLeft).size_4())
                .on_click(cx.listener(move |panel, _, _, cx| {
                    panel.open.remove(&network);
                    cx.notify();
                })))
            .child(avatar(friend, 30.))
            .child(v_flex().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(shown_name(friend)))
                .child(div().text_xs().text_color(muted).child(shown_status(friend))));
        let list = v_flex().id("chat-messages").size_full().gap_1p5().py_2().overflow_y_scroll().track_scroll(&self.messages_scroll)
            .when(messages.is_empty(), |this| this.child(div().pt_6().text_center().text_sm().text_color(muted).child("Say hi!")))
            .children(messages.iter().map(|m| {
                let bubble = v_flex().max_w(px(320.)).px_3().py_1p5().rounded_xl()
                    .bg(if m.mine { primary.opacity(0.85) } else { gpui::white().opacity(0.08) })
                    .child(div().text_sm().child(crate::privacy::message(&m.text)))
                    .child(div().text_xs().text_color(gpui::white().opacity(0.55)).child(super::time_text(m.time)));
                h_flex().w_full().when(m.mine, |this| this.justify_end()).child(bubble)
            }));
        let input = h_flex().gap_2()
            .child(div().flex_1().child(Input::new(&self.message)))
            .child(Button::new("chat-send").primary().icon(PandoraIcon::ArrowRight).on_click(cx.listener(|panel, _, window, cx| {
                let input = panel.message.clone();
                panel.send_message(input, window, cx);
            })));
        v_flex().size_full().gap_2()
            .child(header)
            .child(div().flex_1().min_h_0().relative().child(list).vertical_scrollbar(&self.messages_scroll))
            .child(input)
            .into_any_element()
    }

    fn friends_view(&mut self, network: Network, state: &NetState, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let hint = match network {
            Network::Flux => "Add a friend by username (e.g. sam or @sam:matrix.org)",
            Network::PlayStation => "Find a player by PSN online ID",
            Network::Xbox => "Add a friend by gamertag",
        };
        let section = |title: String| div().pt_2().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(muted).child(title);
        let mut list = v_flex().gap_1()
            .child(div().text_xs().text_color(muted).child(hint))
            .child(h_flex().gap_2()
                .child(div().flex_1().child(Input::new(&self.add)))
                .child(Button::new("add-friend").primary().icon(PandoraIcon::Plus).label("Add").on_click(cx.listener(|panel, _, window, cx| {
                    let input = panel.add.clone();
                    panel.add_friend(input, window, cx);
                }))));

        if !state.requests.is_empty() {
            list = list.child(section(format!("REQUESTS ({})", state.requests.len())));
            for request in &state.requests {
                let (yes, no) = (request.id.clone(), request.id.clone());
                list = list.child(h_flex().gap_2().items_center().p_2().rounded_lg().bg(gpui::white().opacity(0.04))
                    .child(v_flex().flex_1().min_w_0()
                        .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(crate::privacy::name(&request.id, &request.name)))
                        .child(div().text_xs().text_color(muted).child(if request.incoming { "Wants to be friends" } else { "Request sent" })))
                    .when(request.incoming, |this| this.child(Button::new(SharedString::from(format!("accept-{}", request.id))).small().primary().label("Accept")
                        .on_click(move |_, _, _| super::answer_request(network, yes.clone(), true))))
                    .child(Button::new(SharedString::from(format!("decline-{}", request.id))).small().ghost()
                        .label(if request.incoming { "Decline" } else { "Cancel" })
                        .on_click(move |_, _, _| super::answer_request(network, no.clone(), false))));
            }
        }

        let online = state.friends.iter().filter(|f| f.online).count();
        list = list.child(section(format!("FRIENDS ({}) · {online} ONLINE", state.friends.len())));
        for friend in &state.friends {
            let (chat_id, remove_id) = (friend.id.clone(), friend.id.clone());
            let name = shown_name(friend);
            list = list.child(h_flex().gap_3().items_center().p_2().rounded_lg().hover(|s| s.bg(gpui::white().opacity(0.04)))
                .child(avatar(friend, 34.))
                .child(v_flex().flex_1().min_w_0()
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(shown_name(friend)))
                    .child(div().text_xs().text_color(if friend.online { rgb(0x5ee38a).into() } else { muted }).overflow_hidden().whitespace_nowrap().text_ellipsis().child(shown_status(friend))))
                .child(Button::new(SharedString::from(format!("chat-with-{}", friend.id))).small().icon(PandoraIcon::MessageCircle)
                    .on_click(cx.listener(move |panel, _, _, cx| panel.open_friend(network, &chat_id, cx))))
                .child(Button::new(SharedString::from(format!("remove-{}", friend.id))).small().ghost().icon(PandoraIcon::Trash2)
                    .tooltip(format!("Remove {name}"))
                    .on_click(move |_, _, _| super::remove_friend(network, remove_id.clone()))));
        }

        list = list.child(h_flex().pt_4().gap_2().items_center()
            .child(div().flex_1().text_xs().text_color(muted).child(if state.me.is_empty() { String::new() } else { format!("Signed in as {}", crate::privacy::me(&state.me)) }))
            .when(network != Network::Flux, |this| this.child(Button::new("chat-refresh").small().ghost().icon(PandoraIcon::RefreshCcw)
                .on_click(move |_, _, _| super::refresh(network))))
            .child(Button::new("chat-sign-out").small().ghost().label("Disconnect").on_click(move |_, _, _| super::disconnect(network))));
        v_flex().size_full().child(list).overflow_y_scrollbar().into_any_element()
    }
}
