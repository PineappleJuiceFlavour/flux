//! Chat with friends from Flux, on three networks:
//! - Flux chat: Matrix (a free, open chat network). Friends are direct chats; adding a friend
//!   invites them, and they accept from Flux or any Matrix app (Element).
//! - PlayStation: Sony's PlayStation App API (unofficial), signed in with the player's NPSSO token.
//! - Xbox: Xbox Live, through the Microsoft account the player signed into Flux with.
//!
//! Network work runs on a small runtime of its own; the chat panel reads a snapshot of the
//! state (`state`) and redraws when `revision` changes.

pub mod matrix;
pub mod panel;
pub mod psn;
mod store;
pub mod xbox;

use std::{
    collections::HashMap,
    future::Future,
    sync::{LazyLock, Mutex, OnceLock, atomic::{AtomicU64, Ordering}},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    #[default]
    Flux,
    PlayStation,
    Xbox,
}

impl Network {
    pub const ALL: [Network; 3] = [Network::Flux, Network::PlayStation, Network::Xbox];

    pub fn label(self) -> &'static str {
        match self {
            Network::Flux => "Flux",
            Network::PlayStation => "PlayStation",
            Network::Xbox => "Xbox",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Friend {
    /// Matrix user id, PSN account id or Xbox user id.
    pub id: String,
    pub name: String,
    /// "Online", "Playing Skate 3", "Request sent", ...
    pub status: String,
    pub online: bool,
    pub avatar: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Request {
    /// What answering it needs (Matrix room id, PSN account id, Xbox user id).
    pub id: String,
    pub name: String,
    /// Someone asked you (true) or you asked them (false).
    pub incoming: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub mine: bool,
    pub sender: String,
    pub text: String,
    /// Milliseconds since 1970.
    pub time: i64,
}

#[derive(Debug, Clone, Default)]
pub struct NetState {
    pub connected: bool,
    pub connecting: bool,
    /// Your name on this network.
    pub me: String,
    pub friends: Vec<Friend>,
    pub requests: Vec<Request>,
    /// Friend id -> messages, oldest first.
    pub messages: HashMap<String, Vec<Message>>,
    pub error: Option<String>,
    pub notice: Option<String>,
    /// Friends with messages you haven't opened.
    pub unread: Vec<String>,
}

static STATE: LazyLock<Mutex<HashMap<Network, NetState>>> = LazyLock::new(Default::default);
static REVISION: AtomicU64 = AtomicU64::new(0);

/// A copy of one network's state.
pub fn state(network: Network) -> NetState {
    STATE.lock().unwrap().get(&network).cloned().unwrap_or_default()
}

/// Changes whenever any chat state changes.
pub fn revision() -> u64 {
    REVISION.load(Ordering::Relaxed)
}

pub(crate) fn update(network: Network, f: impl FnOnce(&mut NetState)) {
    f(STATE.lock().unwrap().entry(network).or_default());
    REVISION.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn fail(network: Network, error: impl ToString) {
    let error = error.to_string();
    log::warn!("Chat ({}): {error}", network.label());
    update(network, |s| {
        s.connecting = false;
        s.error = Some(error);
    });
}

pub(crate) fn notice(network: Network, text: impl ToString) {
    update(network, |s| {
        s.error = None;
        s.notice = Some(text.to_string());
    });
}

/// Keep a conversation in time order, without repeats, at most 300 long.
pub(crate) fn set_messages(network: Network, friend: &str, mut list: Vec<Message>, mark_unread: bool) {
    list.sort_by_key(|m| m.time);
    list.dedup();
    if list.len() > 300 {
        list.drain(..list.len() - 300);
    }
    update(network, |s| {
        let old = s.messages.get(friend).map(|m| m.len()).unwrap_or(0);
        if mark_unread && list.len() > old && list.last().is_some_and(|m| !m.mine) && !s.unread.iter().any(|f| f == friend) {
            s.unread.push(friend.to_string());
        }
        s.messages.insert(friend.to_string(), list);
    });
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("Flux chat").enable_all().build()
            .expect("chat runtime")
    })
}

pub(crate) fn spawn(future: impl Future<Output = ()> + Send + 'static) {
    runtime().spawn(future);
}

/// One HTTP client for chat (no automatic redirects: PlayStation's sign-in reads them).
pub(crate) fn http() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .use_rustls_tls()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(10))
            .user_agent(concat!("Flux/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("chat http client")
    }).clone()
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Sign back in to every network that was connected last time.
pub fn start() {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let saved = store::load();
    if let Some(session) = saved.matrix {
        spawn(matrix::resume(session));
    }
    if let Some(refresh) = saved.psn_refresh {
        spawn(psn::resume(refresh));
    }
    if saved.xbox {
        spawn(xbox::connect());
    }
}

// ------------------------------------------------------------------- actions for the chat panel

pub fn connect_flux(homeserver: String) {
    spawn(matrix::login(homeserver));
}

pub fn connect_playstation(npsso: String) {
    spawn(psn::connect(npsso));
}

pub fn connect_xbox() {
    spawn(xbox::connect());
}

pub fn disconnect(network: Network) {
    match network {
        Network::Flux => spawn(matrix::logout()),
        Network::PlayStation => psn::disconnect(),
        Network::Xbox => xbox::disconnect(),
    }
}

pub fn refresh(network: Network) {
    match network {
        Network::Flux => {},
        Network::PlayStation => spawn(psn::refresh_friends()),
        Network::Xbox => spawn(xbox::refresh_friends()),
    }
}

/// Load a conversation (and mark it read).
pub fn open_chat(network: Network, friend: String) {
    update(network, |s| s.unread.retain(|f| f != &friend));
    match network {
        Network::Flux => {},
        Network::PlayStation => spawn(psn::load_messages(friend)),
        Network::Xbox => spawn(xbox::load_messages(friend)),
    }
}

pub fn send(network: Network, friend: String, text: String) {
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    match network {
        Network::Flux => spawn(matrix::send(friend, text)),
        Network::PlayStation => spawn(psn::send(friend, text)),
        Network::Xbox => spawn(xbox::send(friend, text)),
    }
}

pub fn add_friend(network: Network, name: String) {
    let name = name.trim().to_string();
    if name.is_empty() {
        return;
    }
    match network {
        Network::Flux => spawn(matrix::add_friend(name)),
        Network::PlayStation => spawn(psn::add_friend(name)),
        Network::Xbox => spawn(xbox::add_friend(name)),
    }
}

pub fn answer_request(network: Network, id: String, accept: bool) {
    match network {
        Network::Flux => spawn(matrix::answer(id, accept)),
        Network::PlayStation => spawn(psn::answer(id, accept)),
        Network::Xbox => spawn(xbox::answer(id, accept)),
    }
}

pub fn remove_friend(network: Network, id: String) {
    match network {
        Network::Flux => spawn(matrix::remove_friend(id)),
        Network::PlayStation => spawn(psn::remove_friend(id)),
        Network::Xbox => spawn(xbox::remove_friend(id)),
    }
}

/// Short "2:41 pm" / "Mon" / "12 Oct" for a message time.
pub fn time_text(ms: i64) -> String {
    use chrono::{Local, TimeZone};
    let Some(time) = Local.timestamp_millis_opt(ms).single() else { return String::new() };
    let now = Local::now();
    let age = now.signed_duration_since(time);
    if time.date_naive() == now.date_naive() {
        time.format("%-I:%M %p").to_string().to_lowercase()
    } else if age.num_days() < 7 {
        time.format("%a %-I:%M %p").to_string()
    } else {
        time.format("%-d %b").to_string()
    }
}
