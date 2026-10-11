//! Flux's game library (Games mode). It starts empty: players add their own games, and Flux looks at
//! what they picked (the disc's file system, the game's metadata, the program itself) to work out
//! which console it's for, which emulator plays it, its title and its art. Flux ships no games.

use std::path::{Path, PathBuf};

use gpui::{App, SharedString};
use serde::{Deserialize, Serialize};

use crate::{disc, emulators, interface_config::InterfaceConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameKind {
    Ps1,
    Ps2,
    Ps3,
    Ps4,
    Psp,
    /// The original Xbox.
    Xbox,
    Xbox360,
    /// Recognised only: no Xbox One emulator exists.
    XboxOne,
    Switch,
    /// The Simpsons: Hit & Run Rust port (shar-rs), optionally with Skate 3 built in.
    SharPort,
    /// Any Windows program.
    Windows,
}

impl GameKind {
    pub fn label(self) -> &'static str {
        match self {
            GameKind::Ps1 => "PlayStation game",
            GameKind::Ps2 => "PlayStation 2 game",
            GameKind::Ps3 => "PlayStation 3 game",
            GameKind::Ps4 => "PlayStation 4 game",
            GameKind::Psp => "PSP game",
            GameKind::Xbox => "Xbox game",
            GameKind::Xbox360 => "Xbox 360 game",
            GameKind::XboxOne => "Xbox One game",
            GameKind::Switch => "Nintendo Switch game",
            GameKind::SharPort => "Hit & Run Rust port",
            GameKind::Windows => "Windows game",
        }
    }

    /// Which emulator plays it (`crate::emulators`), if it needs one.
    pub fn emulator(self) -> Option<&'static str> {
        match self {
            GameKind::Ps1 => Some(emulators::DUCKSTATION),
            GameKind::Ps2 => Some(emulators::PCSX2),
            GameKind::Ps3 => Some(emulators::RPCS3),
            GameKind::Ps4 => Some(emulators::SHADPS4),
            GameKind::Psp => Some(emulators::PPSSPP),
            GameKind::Xbox => Some(emulators::XEMU),
            GameKind::Xbox360 => Some(emulators::XENIA),
            GameKind::Switch => Some(emulators::SWITCH),
            GameKind::XboxOne | GameKind::SharPort | GameKind::Windows => None,
        }
    }

    pub fn default_scene(self) -> &'static str {
        match self {
            GameKind::Xbox360 | GameKind::Xbox | GameKind::XboxOne => "skatepark",
            GameKind::Ps1 | GameKind::Ps2 | GameKind::Ps3 | GameKind::Ps4 | GameKind::Psp => "neon-city",
            GameKind::Switch => "blocky-valley",
            GameKind::SharPort => "cartoon-town",
            GameKind::Windows => "galaxy",
        }
    }

    /// The system's folder in libretro's thumbnail archive (screenshots and box art).
    pub fn libretro_system(self) -> Option<&'static str> {
        match self {
            GameKind::Ps1 => Some("Sony - PlayStation"),
            GameKind::Ps2 => Some("Sony - PlayStation 2"),
            GameKind::Ps3 => Some("Sony - PlayStation 3"),
            GameKind::Ps4 => Some("Sony - PlayStation 4"),
            GameKind::Psp => Some("Sony - PlayStation Portable"),
            GameKind::Xbox => Some("Microsoft - Xbox"),
            GameKind::Xbox360 => Some("Microsoft - Xbox 360"),
            GameKind::XboxOne | GameKind::Switch | GameKind::SharPort | GameKind::Windows => None,
        }
    }

    /// The platform's name, to narrow web searches.
    pub fn platform(self) -> Option<&'static str> {
        match self {
            GameKind::Ps1 => Some("PlayStation"),
            GameKind::Ps2 => Some("PlayStation 2"),
            GameKind::Ps3 => Some("PlayStation 3"),
            GameKind::Ps4 => Some("PlayStation 4"),
            GameKind::Psp => Some("PSP"),
            GameKind::Xbox => Some("Xbox"),
            GameKind::Xbox360 => Some("Xbox 360"),
            GameKind::XboxOne => Some("Xbox One"),
            GameKind::Switch => Some("Nintendo Switch"),
            GameKind::SharPort | GameKind::Windows => None,
        }
    }

    /// Mods and Debug tabs (`crate::game_mods`): Xenia and RPCS3 games.
    pub fn has_mods(self) -> bool {
        matches!(self, GameKind::Xbox360 | GameKind::Ps3)
    }

    /// Does Flux have its own settings page for this game's emulator (the Graphics tab)?
    pub fn has_emulator_settings(self) -> bool {
        self == GameKind::Xbox360
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LibraryGame {
    pub id: String,
    pub name: String,
    pub kind: GameKind,
    /// The disc image, program, game folder or port folder.
    pub path: PathBuf,
    /// Background scene id (`crate::backgrounds::SCENES`), used when there's no game art.
    pub scene: String,
    /// Steam app id, when the game is installed through Steam (it then starts through Steam).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steam_appid: Option<u32>,
    /// The game's own art, copied into Flux's `games/<id>` folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<PathBuf>,
    /// Flux has looked for this game's art on the web (automatically, once).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub web_art_checked: bool,
    /// Where the art came from, in words ("Steam", "Wikipedia", ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub art_source: Option<String>,
    /// Xbox 360: Xenia's hash of the game's executable, remembered from its log (plugins need it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_hash: Option<String>,
    /// Folders of replacement game files (`crate::game_mods`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_mods: Vec<crate::game_mods::FileMod>,
    /// Don't back the saves up before each launch.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_auto_save_backup: bool,
    /// Details from the web (genre, developer, description, ...), looked up once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<GameInfo>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub info_checked: bool,
    /// Portrait/square box art for the Games home tiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<PathBuf>,
    /// Seconds played through Flux, and when it was last started (Unix seconds).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub play_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_played: Option<i64>,
    /// Games imported from another launcher start (and install) through it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_uri: Option<String>,
    /// Where it was imported from ("Steam", "Epic Games", "Xbox", ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Art the player chose ("tile", "background", "logo", "icon"): never replaced automatically.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locked_art: Vec<String>,
    /// Starred by the player (★ badge, Favourites filter).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub favourite: bool,
    /// When the player last opened it from the Library (puts it in the Home row's recent games).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_opened: Option<i64>,
}

/// Most recent of played and opened: the "Recently played" order.
pub fn recent_key(game: &LibraryGame) -> i64 {
    game.last_played.unwrap_or(0).max(game.last_opened.unwrap_or(0))
}

/// The player opened this game (from the Library): it moves to the front of the recent games.
pub fn mark_opened(id: &str, cx: &mut App) {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    if let Some(game) = InterfaceConfig::get_mut(cx).games.iter_mut().find(|g| g.id == id) {
        game.last_opened = Some(now);
    }
}

/// Star or unstar a game.
pub fn toggle_favourite(id: &str, cx: &mut App) {
    if let Some(game) = InterfaceConfig::get_mut(cx).games.iter_mut().find(|g| g.id == id) {
        game.favourite = !game.favourite;
    }
    cx.refresh_windows();
}

/// Is the game in the player's Custom row?
pub fn in_custom(id: &str, cx: &App) -> bool {
    InterfaceConfig::get(cx).custom_order.iter().any(|g| g == id)
}

/// Add a game to the end of the Custom row, or take it out.
pub fn toggle_custom(id: &str, cx: &mut App) {
    let order = &mut InterfaceConfig::get_mut(cx).custom_order;
    if order.iter().any(|g| g == id) {
        order.retain(|g| g != id);
    } else {
        order.push(id.to_string());
    }
    cx.refresh_windows();
}

/// Move `dragged` to where `target` is in the Custom row.
pub fn move_in_custom(dragged: &str, target: &str, cx: &mut App) {
    if dragged == target {
        return;
    }
    reorder(&mut InterfaceConfig::get_mut(cx).custom_order, dragged, target);
    cx.refresh_windows();
}

/// Put `dragged` where `target` is; the games between shift over by one.
fn reorder(order: &mut Vec<String>, dragged: &str, target: &str) {
    let (Some(from), Some(to)) = (order.iter().position(|g| g == dragged), order.iter().position(|g| g == target)) else { return };
    let id = order.remove(from);
    order.insert(to, id);
}

#[cfg(test)]
#[test]
fn custom_reorder() {
    let mut order: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
    reorder(&mut order, "a", "c");
    assert_eq!(order, ["b", "c", "a", "d"]);
    reorder(&mut order, "d", "b");
    assert_eq!(order, ["d", "b", "c", "a"]);
    reorder(&mut order, "x", "b");
    assert_eq!(order, ["d", "b", "c", "a"]);
}

/// The kinds of art a game has, for the art picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtKind {
    Tile,
    Background,
    Logo,
    Icon,
}

impl ArtKind {
    pub const ALL: [ArtKind; 4] = [ArtKind::Tile, ArtKind::Background, ArtKind::Logo, ArtKind::Icon];

    pub fn key(self) -> &'static str {
        match self {
            ArtKind::Tile => "tile",
            ArtKind::Background => "background",
            ArtKind::Logo => "logo",
            ArtKind::Icon => "icon",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ArtKind::Tile => "Tile",
            ArtKind::Background => "Background",
            ArtKind::Logo => "Logo",
            ArtKind::Icon => "Icon",
        }
    }

    /// SteamGridDB's name for it.
    pub fn sgdb(self) -> &'static str {
        match self {
            ArtKind::Tile => "grids",
            ArtKind::Background => "heroes",
            ArtKind::Logo => "logos",
            ArtKind::Icon => "icons",
        }
    }

    pub fn current(self, game: &LibraryGame) -> Option<PathBuf> {
        match self {
            ArtKind::Tile => game.cover.clone(),
            ArtKind::Background => game.background.clone(),
            ArtKind::Logo => game.logo.clone(),
            ArtKind::Icon => game.icon.clone(),
        }.filter(|p| p.is_file())
    }

    fn slot(self, game: &mut LibraryGame) -> &mut Option<PathBuf> {
        match self {
            ArtKind::Tile => &mut game.cover,
            ArtKind::Background => &mut game.background,
            ArtKind::Logo => &mut game.logo,
            ArtKind::Icon => &mut game.icon,
        }
    }
}

/// Use a picture the player chose, and keep it: automatic lookups never replace it.
pub fn set_art(id: &str, kind: ArtKind, path: PathBuf, cx: &mut App) {
    if let Some(game) = InterfaceConfig::get_mut(cx).games.iter_mut().find(|g| g.id == id) {
        *kind.slot(game) = Some(path);
        if !game.locked_art.iter().any(|k| k == kind.key()) {
            game.locked_art.push(kind.key().into());
        }
        if kind == ArtKind::Background {
            // Show it even if a Flux scene was picked before.
            game.scene = game.scene.trim_start_matches('!').to_string();
        }
    }
    cx.refresh_windows();
}

/// Let automatic lookups choose this kind of art again.
pub fn unlock_art(id: &str, kind: ArtKind, cx: &mut App) {
    if let Some(game) = InterfaceConfig::get_mut(cx).games.iter_mut().find(|g| g.id == id) {
        game.locked_art.retain(|k| k != kind.key());
    }
    cx.refresh_windows();
}

/// SteamGridDB's choices of one kind of art for a game.
pub fn sgdb_options(game: &LibraryGame, kind: ArtKind, page: u32, cx: &App) -> tokio::sync::oneshot::Receiver<Result<Vec<bridge::message::SgdbImage>, std::sync::Arc<str>>> {
    let (send, recv) = tokio::sync::oneshot::channel();
    let key = InterfaceConfig::get(cx).steamgriddb_key.clone().unwrap_or_default();
    match BACKEND.get() {
        Some(backend) => backend.send(bridge::message::MessageToBackend::SgdbOptions {
            name: game.name.as_str().into(),
            steam_appid: game.steam_appid,
            key: key.into(),
            kind: kind.sgdb(),
            page,
            result: send,
        }),
        None => _ = send.send(Err("Flux isn't ready yet".into())),
    }
    recv
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct GameInfo {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub developers: Vec<String>,
    #[serde(default)]
    pub publishers: Vec<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub links: Vec<(String, String)>,
    #[serde(default)]
    pub source: String,
}

impl LibraryGame {
    /// The picture for a tile: box art, then the program icon, then the background.
    pub fn tile_image(&self) -> Option<PathBuf> {
        [&self.cover, &self.icon, &self.background].into_iter().flatten().find(|p| p.is_file()).cloned()
    }

    pub fn installed(&self) -> bool {
        self.path.exists()
    }

    /// Started through another launcher (Steam, Epic, Xbox, ...), which installs it too.
    pub fn via_launcher(&self) -> bool {
        self.steam_appid.is_some() || self.launch_uri.is_some()
    }

    /// Where the game comes from, for the Details table.
    pub fn library_name(&self) -> String {
        if let Some(source) = &self.source {
            return source.clone();
        }
        if self.steam_appid.is_some() {
            return "Steam".into();
        }
        match self.kind.emulator().and_then(emulators::emulator) {
            Some(emu) => emu.name.into(),
            None => "Flux".into(),
        }
    }
}

impl LibraryGame {
    /// Build a library entry from a detection, copying or generating its art.
    pub fn from_detection(detection: &Detection, name: String) -> Self {
        let id = new_id(&name);
        let art_dir = crate::LAUNCHER_DIR.get().map(|d| d.join("games").join(&id));
        let mut art = crate::game_art::Art::default();
        if let Some(dir) = &art_dir {
            if detection.kind == GameKind::Windows {
                art = crate::game_art::prepare(&detection.path, detection.steam.as_ref(), dir);
            } else {
                // PS3/PS4 games carry their own background (PIC1) and icon (ICON0).
                let _ = std::fs::create_dir_all(dir);
                let copy = |src: &Option<PathBuf>, name: &str| src.as_ref().and_then(|s| {
                    let out = dir.join(name);
                    std::fs::copy(s, &out).ok().map(|_| out)
                });
                art.background = copy(&detection.art_background, "background.png");
                art.icon = copy(&detection.art_icon, "icon.png");
            }
        }
        LibraryGame {
            id,
            name,
            kind: detection.kind,
            path: detection.path.clone(),
            scene: detection.kind.default_scene().into(),
            steam_appid: detection.steam.as_ref().map(|s| s.appid),
            background: art.background,
            logo: art.logo,
            icon: art.icon,
            web_art_checked: false,
            art_source: None,
            module_hash: None,
            file_mods: Vec::new(),
            no_auto_save_backup: false,
            info: None,
            info_checked: false,
            cover: None,
            play_seconds: 0,
            last_played: None,
            launch_uri: None,
            install_uri: None,
            source: None,
            locked_art: Vec::new(),
            favourite: false,
            last_opened: None,
        }
    }

    pub fn art_dir(&self) -> Option<PathBuf> {
        crate::LAUNCHER_DIR.get().map(|d| d.join("games").join(&self.id))
    }
}

/// What Flux worked out about a file or folder the player picked.
#[derive(Debug, Clone)]
pub struct Detection {
    pub kind: GameKind,
    pub name: String,
    pub path: PathBuf,
    /// Plain-words notes shown before adding (what it needs, what Flux will do).
    pub notes: Vec<SharedString>,
    /// Set when the program is installed through Steam.
    pub steam: Option<crate::game_art::SteamInfo>,
    /// Art found in the game's own files (PS3/PS4 PIC1.PNG / ICON0.PNG).
    pub art_background: Option<PathBuf>,
    pub art_icon: Option<PathBuf>,
}

impl Detection {
    fn new(kind: GameKind, name: String, path: PathBuf) -> Self {
        let mut notes: Vec<SharedString> = Vec::new();
        match kind.emulator().and_then(emulators::emulator) {
            Some(emu) if emu.managed => {
                notes.push(format!("Plays on {} (the {} emulator), which Flux installs and keeps up to date.", emu.name, emu.consoles).into());
                notes.push(emu.needs.into());
            },
            Some(emu) => notes.push(emu.needs.into()),
            None if kind == GameKind::XboxOne => notes.push("No Xbox One emulator exists yet, so Flux can list it but can't play it.".into()),
            None => {},
        }
        if kind.has_emulator_settings() {
            notes.push("Picture quality settings are in the game's Graphics tab.".into());
        }
        Detection { kind, name, path, notes, steam: None, art_background: None, art_icon: None }
    }
}

/// Fix entries saved before Flux knew better (e.g. a port added by its shar_rs.exe instead of its
/// folder). Names, art and background choices the player made are kept.
pub fn normalize_library(cx: &mut App) {
    let config = InterfaceConfig::get_mut(cx);
    for game in &mut config.games {
        if game.kind != GameKind::SharPort
            && game.path.file_name().is_some_and(|n| n.eq_ignore_ascii_case("shar_rs.exe"))
            && let Some(folder) = game.path.parent()
        {
            game.path = folder.to_path_buf();
            game.kind = GameKind::SharPort;
            if !game.scene.starts_with('!') {
                game.scene = GameKind::SharPort.default_scene().into();
            }
        }
    }
}

static BACKEND: std::sync::OnceLock<bridge::handle::BackendHandle> = std::sync::OnceLock::new();

/// Web art lookups in progress (game id), and the last failure per game, for the game page.
static LOOKUPS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
static LOOKUP_ERRORS: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

pub fn backend() -> Option<&'static bridge::handle::BackendHandle> {
    BACKEND.get()
}

pub fn set_backend(backend: bridge::handle::BackendHandle) {
    _ = BACKEND.set(backend);
}

/// Download a small file through the backend (patch lists and the like).
pub fn fetch(url: &str) -> tokio::sync::oneshot::Receiver<Result<Vec<u8>, std::sync::Arc<str>>> {
    let (send, recv) = tokio::sync::oneshot::channel();
    match BACKEND.get() {
        Some(backend) => backend.send(bridge::message::MessageToBackend::FetchUrl { url: url.into(), result: send }),
        None => _ = send.send(Err("Flux isn't ready yet".into())),
    }
    recv
}

pub fn web_art_busy(id: &str) -> bool {
    LOOKUPS.lock().unwrap().iter().any(|i| i == id)
}

pub fn web_art_error(id: &str) -> Option<String> {
    LOOKUP_ERRORS.lock().unwrap().iter().find(|(i, _)| i == id).map(|(_, e)| e.clone())
}

/// Games whose art is only a Flux scene or a picture made from the program icon are worth a look
/// online. Steam's own banners and PS3/PS4 game pictures are already the real thing.
fn wants_web_art(game: &LibraryGame) -> bool {
    !game.web_art_checked
        && game.kind != GameKind::SharPort
        && (game.background.is_none() || (game.kind == GameKind::Windows && game.steam_appid.is_none() && game.logo.is_none()))
}

#[derive(Clone)]
enum Lookup {
    /// (game id, replace art Flux chose before)
    Art(String, bool),
    Info(String),
}

static QUEUE: std::sync::Mutex<std::collections::VecDeque<Lookup>> = std::sync::Mutex::new(std::collections::VecDeque::new());
static WORKERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Queue web lookups; three run at a time so a big import doesn't flood Steam or Wikipedia.
fn enqueue(jobs: Vec<Lookup>, cx: &mut App) {
    QUEUE.lock().unwrap().extend(jobs);
    use std::sync::atomic::Ordering;
    while WORKERS.load(Ordering::SeqCst) < 3 {
        WORKERS.fetch_add(1, Ordering::SeqCst);
        cx.spawn(async move |cx| {
            loop {
                let Some(job) = QUEUE.lock().unwrap().pop_front() else { break };
                let task = cx.update(|cx| match &job {
                    Lookup::Art(id, replace) => find_web_art(id, *replace, cx),
                    Lookup::Info(id) => find_info(id, cx),
                });
                if let Some(task) = task {
                    task.await;
                }
            }
            WORKERS.fetch_sub(1, Ordering::SeqCst);
        }).detach();
    }
}

/// Which art sources Flux knows (SteamGridDB, Steam 2x box art, libretro 1024px box art).
const ART_VERSION: u32 = 3;

/// Look up every game's art again with the best sources (after adding a SteamGridDB key, or once
/// when Flux learns better sources).
/// `replace`: the player asked, so art Flux picked before is replaced too (art they chose
/// themselves never is); otherwise only missing art is filled in.
pub fn refresh_all_art(replace: bool, cx: &mut App) {
    let ids: Vec<Lookup> = games(cx).iter().filter(|g| g.kind != GameKind::SharPort).map(|g| Lookup::Art(g.id.clone(), replace)).collect();
    enqueue(ids, cx);
}

/// Look online for the art and details of every game that has none yet (each game once).
pub fn find_missing_web_art(cx: &mut App) {
    if InterfaceConfig::get(cx).art_version < ART_VERSION {
        InterfaceConfig::get_mut(cx).art_version = ART_VERSION;
        refresh_all_art(false, cx);
    }
    let mut jobs: Vec<Lookup> = games(cx).iter().filter(|g| wants_web_art(g)).map(|g| Lookup::Art(g.id.clone(), true)).collect();
    jobs.extend(games(cx).iter().filter(|g| !g.info_checked && g.kind != GameKind::SharPort).map(|g| Lookup::Info(g.id.clone())));
    enqueue(jobs, cx);
}

/// Look up a game's details (Steam's store data, or Wikipedia + Wikidata), and a box-art cover
/// for Steam games.
pub fn find_info(id: &str, cx: &mut App) -> Option<gpui::Task<()>> {
    let (Some(game), Some(backend)) = (find(id, cx), BACKEND.get()) else { return None };
    let (send, recv) = tokio::sync::oneshot::channel();
    backend.send(bridge::message::MessageToBackend::FindGameInfo {
        name: game.name.as_str().into(),
        platform: game.kind.platform().map(Into::into),
        steam_appid: game.steam_appid,
        result: send,
    });
    let cover = (game.cover.is_none() && game.steam_appid.is_some()).then(|| {
        // The 2x (1200x1800) box art; sharp on big tiles.
        fetch(&format!("https://shared.steamstatic.com/store_item_assets/steam/apps/{}/library_600x900_2x.jpg", game.steam_appid.unwrap()))
    });
    let (id, dir) = (id.to_string(), game.art_dir());
    Some(cx.spawn(async move |cx| {
        let info = recv.await.ok().and_then(Result::ok);
        let cover_path = match (cover, dir) {
            (Some(cover), Some(dir)) => match cover.await {
                Ok(Ok(bytes)) if image::load_from_memory(&bytes).is_ok() => {
                    let path = dir.join("cover.jpg");
                    (std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&path, bytes).is_ok()).then_some(path)
                },
                _ => None,
            },
            _ => None,
        };
        _ = cx.update(|cx| {
            if let Some(game) = InterfaceConfig::get_mut(cx).games.iter_mut().find(|g| g.id == id) {
                game.info_checked = true;
                if let Some(info) = info {
                    game.info = Some(GameInfo {
                        description: info.description,
                        genres: info.genres,
                        developers: info.developers,
                        publishers: info.publishers,
                        release_date: info.release_date,
                        tags: info.tags,
                        links: info.links,
                        source: info.source.to_string(),
                    });
                }
                if cover_path.is_some() {
                    game.cover = cover_path;
                }
            }
            cx.refresh_windows();
        });
    }))
}

// ------------------------------------------------------------------------------------- sessions

struct Session {
    id: String,
    child: std::process::Child,
    last_tick: std::time::Instant,
}

static SESSIONS: std::sync::Mutex<Vec<Session>> = std::sync::Mutex::new(Vec::new());

fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn is_running(id: &str) -> bool {
    SESSIONS.lock().unwrap().iter().any(|s| s.id == id)
}

/// Start a game from anywhere (game page, Games home); its play time counts while it runs.
pub fn start_game(game: &LibraryGame, cx: &mut App) -> Result<String, String> {
    if let Some(problem) = problem(game, cx) {
        return Err(problem.to_string());
    }
    let child = launch(game, cx)?;
    let id = game.id.clone();
    if let Some(g) = InterfaceConfig::get_mut(cx).games.iter_mut().find(|g| g.id == id) {
        g.last_played = Some(now_unix());
    }
    if game.via_launcher() {
        // The launcher owns the game process; Flux just asked it to start (or install) the game.
        drop(child);
        let launcher = game.library_name();
        return Ok(if game.installed() {
            format!("Starting {} through {launcher}", game.name)
        } else {
            format!("Opening {launcher} to install {}", game.name)
        });
    }
    SESSIONS.lock().unwrap().push(Session { id, child, last_tick: std::time::Instant::now() });
    cx.refresh_windows();
    Ok(format!("{} is running", game.name))
}

pub fn stop_game(id: &str) {
    for s in SESSIONS.lock().unwrap().iter_mut().filter(|s| s.id == id) {
        _ = s.child.kill();
    }
}

/// Every few seconds: add play time to running games and notice games that have closed.
pub fn tick_sessions(cx: &mut App) {
    let mut played: Vec<(String, u64)> = Vec::new();
    let mut ended = false;
    {
        let mut sessions = SESSIONS.lock().unwrap();
        sessions.retain_mut(|s| {
            let secs = s.last_tick.elapsed().as_secs();
            if secs > 0 {
                played.push((s.id.clone(), secs));
                s.last_tick += std::time::Duration::from_secs(secs);
            }
            let alive = matches!(s.child.try_wait(), Ok(None));
            ended |= !alive;
            alive
        });
    }
    if played.is_empty() && !ended {
        return;
    }
    let now = now_unix();
    for (id, secs) in played {
        if let Some(g) = InterfaceConfig::get_mut(cx).games.iter_mut().find(|g| g.id == id) {
            g.play_seconds += secs;
            g.last_played = Some(now);
        }
    }
    if ended {
        cx.refresh_windows();
    }
}

/// "3 h 20 min", "45 min", "Not played".
pub fn play_time_text(seconds: u64) -> String {
    match seconds {
        0 => "Not played".into(),
        s if s < 60 => "Under a minute".into(),
        s if s < 3600 => format!("{} min", s / 60),
        s => format!("{} h {} min", s / 3600, (s % 3600) / 60),
    }
}

pub fn last_played_text(when: Option<i64>) -> String {
    let Some(when) = when else { return "Never".into() };
    match now_unix() - when {
        a if a < 120 => "Just now".into(),
        a if a < 3600 => format!("{} minutes ago", a / 60),
        a if a < 86_400 => format!("{} hours ago", a / 3600),
        a if a < 2 * 86_400 => "Yesterday".into(),
        a if a < 30 * 86_400 => format!("{} days ago", a / 86_400),
        _ => chrono::DateTime::from_timestamp(when, 0).map(|d| d.format("%d %b %Y").to_string()).unwrap_or_default(),
    }
}

/// Played in the last two weeks.
pub fn recently_played(game: &LibraryGame) -> bool {
    game.last_played.is_some_and(|t| now_unix() - t < 14 * 86_400)
}

/// Look up `id`'s art on the web (Steam, libretro thumbnails, Wikipedia) and use it when found.
/// `manual`: the player asked, so the found art also replaces a scene they picked.
pub fn find_web_art(id: &str, manual: bool, cx: &mut App) -> Option<gpui::Task<()>> {
    let (Some(game), Some(backend)) = (find(id, cx), BACKEND.get()) else { return None };
    let Some(dest) = game.art_dir() else { return None };
    {
        let mut lookups = LOOKUPS.lock().unwrap();
        if lookups.iter().any(|i| i == id) {
            return None;
        }
        lookups.push(id.to_string());
    }
    LOOKUP_ERRORS.lock().unwrap().retain(|(i, _)| i != id);
    let (send, recv) = tokio::sync::oneshot::channel();
    backend.send(bridge::message::MessageToBackend::FindGameArt {
        name: game.name.as_str().into(),
        system: game.kind.libretro_system().map(Into::into),
        platform: game.kind.platform().map(Into::into),
        steam_appid: game.steam_appid,
        sgdb_key: InterfaceConfig::get(cx).steamgriddb_key.clone().filter(|k| !k.trim().is_empty()).map(Into::into),
        dest,
        result: send,
    });
    let id = id.to_string();
    let task = cx.spawn(async move |cx| {
        let result = recv.await.unwrap_or_else(|_| Err("The lookup stopped".into()));
        _ = cx.update(|cx| {
            LOOKUPS.lock().unwrap().retain(|i| *i != id);
            let mut error = None;
            if let Some(game) = InterfaceConfig::get_mut(cx).games.iter_mut().find(|g| g.id == id) {
                game.web_art_checked = true;
                match result {
                    Ok(art) => {
                        // `manual`: replace what Flux chose before; otherwise only fill gaps. Art
                        // the player picked themselves is never replaced.
                        for (kind, found) in [(ArtKind::Background, art.background), (ArtKind::Logo, art.logo), (ArtKind::Tile, art.cover), (ArtKind::Icon, art.icon)] {
                            let Some(found) = found else { continue };
                            let locked = game.locked_art.iter().any(|k| k == kind.key());
                            if !locked && (manual || kind.current(game).is_none()) {
                                *kind.slot(game) = Some(found);
                            }
                        }
                        game.art_source = Some(art.source.to_string());
                        if manual {
                            game.scene = game.scene.trim_start_matches('!').to_string();
                        }
                    },
                    Err(e) => error = Some(e.to_string()),
                }
            }
            if let Some(e) = error {
                LOOKUP_ERRORS.lock().unwrap().push((id.clone(), e));
            }
            cx.refresh_windows();
        });
    });
    cx.refresh_windows();
    Some(task)
}

pub fn kind_label(game: &LibraryGame) -> &'static str {
    if game.steam_appid.is_some() { "Steam game" } else { game.kind.label() }
}

pub fn games(cx: &App) -> &[LibraryGame] {
    &InterfaceConfig::get(cx).games
}

pub fn find(id: &str, cx: &App) -> Option<LibraryGame> {
    games(cx).iter().find(|g| g.id == id).cloned()
}

pub fn new_id(name: &str) -> String {
    let slug: String = name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{}-{:x}", slug.trim_matches('-'), nanos & 0xffff_ffff)
}

/// Xenia's settings file lives next to its program (used by the Graphics tab).
pub fn xenia_exe() -> Option<PathBuf> {
    Some(emulators::managed_dir(emulators::XENIA)?.join("xenia_canary.exe"))
}

pub fn xenia_installed() -> bool {
    xenia_exe().is_some_and(|p| p.is_file())
}

// ------------------------------------------------------------------------------------------ detection

/// "Skate3 (USA).iso" -> "Skate3", "the_game-v2" -> "the game v2".
fn name_from(path: &Path) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let stem = stem.split(" (").next().unwrap_or(&stem).split(" [").next().unwrap_or(&stem).replace(['_', '.'], " ");
    let stem = stem.trim();
    if stem.is_empty() { "My game".into() } else { stem.to_string() }
}

/// The product name a Windows program carries in its version info ("The Simpsons Hit & Run").
fn product_name(exe: &Path) -> Option<String> {
    let raw = crate::win_native::version_string(exe, "ProductName").or_else(|| crate::win_native::version_string(exe, "FileDescription"))?;
    let name = raw.replace("(TM)", "").replace("(R)", "").replace(['™', '®', '©'], "");
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    // Engines often leave a generic product name behind.
    let generic = ["", "unity player", "unreal engine", "game", "launcher", "microsoft® windows® operating system"];
    (!generic.contains(&name.to_lowercase().as_str()) && name.len() <= 60).then_some(name)
}

/// The main program in a game folder: one named like the folder, else the biggest .exe at the top level.
pub fn main_exe_in(folder: &Path) -> Option<PathBuf> {
    main_exe(folder)
}

fn main_exe(folder: &Path) -> Option<PathBuf> {
    let folder_name = folder.file_name()?.to_string_lossy().to_lowercase().replace(' ', "");
    let exes: Vec<PathBuf> = std::fs::read_dir(folder).ok()?.flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")))
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy().to_lowercase();
            !(n.contains("unins") || n.contains("setup") || n.contains("crash") || n.contains("backup") || n.contains("redist"))
        })
        .collect();
    exes.iter().find(|p| p.file_stem().unwrap().to_string_lossy().to_lowercase().replace(' ', "") == folder_name).cloned()
        .or_else(|| exes.into_iter().max_by_key(|p| p.metadata().map(|m| m.len()).unwrap_or(0)))
}

/// The EBOOT.BIN to boot for a PS3 game folder (disc layout or installed/PSN layout).
fn ps3_eboot(folder: &Path) -> Option<PathBuf> {
    let disc = disc::child(folder, "PS3_GAME").and_then(|g| disc::child(&g, "USRDIR")).and_then(|u| disc::child(&u, "EBOOT.BIN"));
    disc.or_else(|| disc::child(folder, "USRDIR").and_then(|u| disc::child(&u, "EBOOT.BIN")))
}

fn ps3_folder(folder: &Path) -> Option<Detection> {
    ps3_eboot(folder)?;
    let game_dir = disc::child(folder, "PS3_GAME").unwrap_or_else(|| folder.to_path_buf());
    let title = disc::child(&game_dir, "PARAM.SFO").and_then(|sfo| disc::sfo_title(&sfo));
    let mut d = Detection::new(GameKind::Ps3, title.unwrap_or_else(|| name_from(folder)), folder.to_path_buf());
    d.art_background = disc::child(&game_dir, "PIC1.PNG").or_else(|| disc::child(&game_dir, "PIC0.PNG"));
    d.art_icon = disc::child(&game_dir, "ICON0.PNG");
    Some(d)
}

fn ps4_folder(folder: &Path) -> Option<Detection> {
    let eboot = disc::child(folder, "eboot.bin")?;
    let sce_sys = disc::child(folder, "sce_sys")?;
    let title = disc::child(&sce_sys, "param.sfo").and_then(|sfo| disc::sfo_title(&sfo));
    let mut d = Detection::new(GameKind::Ps4, title.unwrap_or_else(|| name_from(folder)), eboot);
    d.art_background = disc::child(&sce_sys, "pic1.png").or_else(|| disc::child(&sce_sys, "pic0.png"));
    d.art_icon = disc::child(&sce_sys, "icon0.png");
    Some(d)
}

/// PlayStation discs (PS1, PS2, PSP, PS3) by what's on them.
fn playstation_disc(image: &Path, path: &Path) -> Option<Detection> {
    let mut iso = disc::Iso::open(image)?;
    let names = iso.root_names();
    let has = |n: &str| names.iter().any(|x| x == n);
    if has("PS3_DISC.SFB") || has("PS3_GAME") {
        let title = iso.read_file("PS3_GAME/PARAM.SFO").and_then(|s| disc::sfo_value(&s, "TITLE"));
        return Some(Detection::new(GameKind::Ps3, title.unwrap_or_else(|| name_from(path)), path.to_path_buf()));
    }
    if has("UMD_DATA.BIN") || has("PSP_GAME") {
        let title = iso.read_file("PSP_GAME/PARAM.SFO").and_then(|s| disc::sfo_value(&s, "TITLE"));
        return Some(Detection::new(GameKind::Psp, title.unwrap_or_else(|| name_from(path)), path.to_path_buf()));
    }
    if let Some(cnf) = iso.read_file("SYSTEM.CNF") {
        let cnf = String::from_utf8_lossy(&cnf).to_uppercase();
        let kind = if cnf.contains("BOOT2") { GameKind::Ps2 } else { GameKind::Ps1 };
        return Some(Detection::new(kind, name_from(path), path.to_path_buf()));
    }
    if iso.system_id.starts_with("PLAYSTATION") {
        return Some(Detection::new(GameKind::Ps2, name_from(path), path.to_path_buf()));
    }
    None
}

pub fn detect(picked: &Path) -> Result<Detection, String> {
    if picked.is_dir() {
        return detect_folder(picked);
    }
    if !picked.is_file() {
        return Err(format!("{} doesn't exist", picked.display()));
    }
    let file_name = picked.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let ext = picked.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let parent = picked.parent().unwrap_or(Path::new("."));

    // Files that stand for their folder.
    if file_name == "shar_rs.exe" || file_name == "play.bat" {
        return detect_folder(parent);
    }
    if file_name == "eboot.bin" {
        // PS3: <game>/PS3_GAME/USRDIR/EBOOT.BIN or <game>/USRDIR/EBOOT.BIN; PS4: <game>/eboot.bin + sce_sys.
        if let Some(d) = ps4_folder(parent) {
            return Ok(d);
        }
        let usrdir_parent = parent.parent().unwrap_or(parent);
        let game = if usrdir_parent.file_name().is_some_and(|n| n.eq_ignore_ascii_case("PS3_GAME")) {
            usrdir_parent.parent().unwrap_or(usrdir_parent)
        } else {
            usrdir_parent
        };
        if let Some(d) = ps3_folder(game) {
            return Ok(d);
        }
    }
    if file_name == "default.xbe" {
        return Err("That's an extracted original Xbox game: xemu needs it as a disc image (.iso / .xiso).".into());
    }

    match ext.as_str() {
        "nsp" | "xci" | "nro" | "nca" | "xcz" | "nsz" => {
            return Ok(Detection::new(GameKind::Switch, name_from(picked), picked.to_path_buf()));
        },
        "xvc" | "xvd" | "msixvc" => {
            return Ok(Detection::new(GameKind::XboxOne, name_from(picked), picked.to_path_buf()));
        },
        "cso" | "pbp" => {
            return Ok(Detection::new(GameKind::Psp, name_from(picked), picked.to_path_buf()));
        },
        "cue" => {
            let bin = disc::cue_bin(picked).ok_or("That .cue sheet doesn't point at a .bin file next to it")?;
            return playstation_disc(&bin, picked).ok_or_else(|| "Flux couldn't read the disc that .cue points at.".to_string());
        },
        "chd" => {
            // Compressed: guess from the size (CD-sized = PS1, DVD-sized = PS2).
            let big = picked.metadata().map(|m| m.len() > 800 * 1024 * 1024).unwrap_or(false);
            let kind = if big { GameKind::Ps2 } else { GameKind::Ps1 };
            return Ok(Detection::new(kind, name_from(picked), picked.to_path_buf()));
        },
        "pkg" => {
            return Err("That's an install package (.pkg). Install it in the emulator first (RPCS3: File > Install Packages), then add the installed game's folder.".into());
        },
        _ => {},
    }

    if disc::starts_with(picked, b"XEX2") {
        return Ok(Detection::new(GameKind::Xbox360, parent_name_if_default(picked), picked.to_path_buf()));
    }
    if let Some(xbox) = disc::xbox_disc(picked) {
        let kind = if xbox == disc::XboxDisc::Original { GameKind::Xbox } else { GameKind::Xbox360 };
        return Ok(Detection::new(kind, name_from(picked), picked.to_path_buf()));
    }
    if let Some(d) = playstation_disc(picked, picked) {
        return Ok(d);
    }
    if disc::starts_with(picked, b"MZ") {
        return Ok(windows_game(picked));
    }
    Err("Flux doesn't recognise that file. Pick a game program (.exe), a disc image (.iso, .cue, .chd, .cso), a Switch game (.nsp, .xci) or a game folder.".into())
}

fn parent_name_if_default(path: &Path) -> String {
    if path.file_name().is_some_and(|n| n.eq_ignore_ascii_case("default.xex")) {
        path.parent().map(name_from).unwrap_or_else(|| "Xbox 360 game".into())
    } else {
        name_from(path)
    }
}

fn detect_folder(folder: &Path) -> Result<Detection, String> {
    if folder.join("shar_rs.exe").is_file() {
        let skate = folder.join("skate").is_dir();
        let set_up = folder.join("gamedata").join(".shar_install").is_file();
        let mut d = Detection::new(
            GameKind::SharPort,
            if skate { "Skate 3 x Simpsons".into() } else { "The Simpsons: Hit & Run".into() },
            folder.to_path_buf(),
        );
        d.notes.push(if set_up {
            "Its game data is already set up.".into()
        } else {
            "First launch runs the port's setup, which asks for your own copy of The Simpsons: Hit & Run.".into()
        });
        if skate {
            d.notes.push("Skate 3 is built in: press B in game. It asks for your own Skate 3 disc the first time.".into());
        }
        return Ok(d);
    }
    if let Some(d) = ps3_folder(folder) {
        return Ok(d);
    }
    if let Some(d) = ps4_folder(folder) {
        return Ok(d);
    }
    if let Some(xex) = disc::child(folder, "default.xex") {
        return Ok(Detection::new(GameKind::Xbox360, name_from(folder), xex));
    }
    if disc::child(folder, "default.xbe").is_some() {
        return Err("That's an extracted original Xbox game: xemu needs it as a disc image (.iso / .xiso).".into());
    }
    if disc::child(folder, "MicrosoftGame.config").is_some() && main_exe(folder).is_none() {
        return Ok(Detection::new(GameKind::XboxOne, name_from(folder), folder.to_path_buf()));
    }
    if let Some(exe) = main_exe(folder) {
        let mut d = windows_game(&exe);
        if d.steam.is_none() && product_name(&exe).is_none() {
            d.name = name_from(folder);
        }
        return Ok(d);
    }
    Err("Couldn't find a game in that folder: no program (.exe), console game files or shar_rs.exe".into())
}

fn windows_game(exe: &Path) -> Detection {
    let steam = crate::game_art::steam_info(exe);
    let notes: Vec<SharedString> = match &steam {
        Some(s) => vec![
            format!("Steam game: {} (app {}). Flux starts it through Steam, like Steam's own Play button.", s.name, s.appid).into(),
            if s.library_cache.is_some() {
                "Its background and logo come from Steam's art for the game.".into()
            } else {
                "Steam has no art cached for it yet; Flux makes a background from the game's icon.".into()
            },
        ],
        None => vec![
            "A Windows program: Flux starts it directly, from its own folder.".into(),
            "Flux makes a background for it from the game's icon.".into(),
        ],
    };
    let name = steam.as_ref().map(|s| s.name.clone()).or_else(|| product_name(exe)).unwrap_or_else(|| name_from(exe));
    Detection { kind: GameKind::Windows, name, path: exe.to_path_buf(), notes, steam, art_background: None, art_icon: None }
}

// --------------------------------------------------------------------------------------------- launch

/// Why the game can't start yet, if it can't.
pub fn problem(game: &LibraryGame, cx: &App) -> Option<SharedString> {
    let path = &game.path;
    if game.kind == GameKind::SharPort {
        return (!path.join("shar_rs.exe").is_file()).then(|| format!("No shar_rs.exe in {}", path.display()).into());
    }
    // Launcher games that aren't installed get installed through their launcher.
    if !path.exists() && !game.via_launcher() {
        return Some(format!("{} isn't there any more: point Flux at it again in Setup", path.display()).into());
    }
    if game.kind == GameKind::XboxOne {
        return Some("No Xbox One emulator exists yet, so this can't be played".into());
    }
    if let Some(id) = game.kind.emulator() {
        let emu = emulators::emulator(id)?;
        if emulators::exe(id, cx).is_none() {
            return Some(if emu.managed {
                format!("{} isn't installed yet: Setup > Install {}", emu.name, emu.name).into()
            } else {
                "Choose your Switch emulator's program in Setup".into()
            });
        }
    }
    None
}

/// What to hand the emulator: PS3 folders boot their EBOOT.BIN.
fn boot_path(game: &LibraryGame) -> PathBuf {
    if game.kind == GameKind::Ps3 && game.path.is_dir() {
        return ps3_eboot(&game.path).unwrap_or_else(|| game.path.clone());
    }
    game.path.clone()
}

/// Start the game. Returns the process so the page can show it running and stop it.
pub fn launch(game: &LibraryGame, cx: &App) -> Result<std::process::Child, String> {
    use std::process::{Command, Stdio};
    let path = &game.path;
    let mut command = match game.kind {
        GameKind::SharPort => {
            let gamedata = path.join("gamedata");
            if !gamedata.join(".shar_install").is_file() && path.join("play.bat").is_file() {
                // First run: play.bat asks for the player's own copy and sets gamedata up, in its own
                // console window so its questions are visible.
                let mut c = Command::new("cmd");
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    c.raw_arg(format!("/C start \"Flux\" \"{}\"", path.join("play.bat").display()));
                }
                c.current_dir(path);
                return c.spawn().map_err(|e| format!("Couldn't start the game's setup: {e}"));
            }
            let mut c = Command::new(path.join("shar_rs.exe"));
            c.arg(&gamedata).current_dir(path);
            c
        },
        GameKind::Windows if game.via_launcher() => {
            // Launcher games start (or install) through their launcher, as its own Play button
            // does: DRM, overlays and cloud saves keep working.
            let uri = match (&game.launch_uri, game.steam_appid, game.installed()) {
                (_, _, false) if game.install_uri.is_some() => game.install_uri.clone().unwrap(),
                (_, Some(id), false) => format!("steam://install/{id}"),
                (Some(uri), _, _) => uri.clone(),
                (None, Some(id), _) => format!("steam://rungameid/{id}"),
                (None, None, _) => return Err("No way to start this game".into()),
            };
            let mut c = Command::new("cmd");
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                // shell:AppsFolder (Xbox app games) goes through Explorer.
                if uri.starts_with("shell:") {
                    c = Command::new("explorer.exe");
                    c.arg(&uri);
                } else {
                    c.raw_arg(format!("/C start \"\" \"{uri}\""));
                }
                c.creation_flags(0x0800_0000);
            }
            c
        },
        GameKind::Windows => {
            let mut c = Command::new(path);
            if let Some(dir) = path.parent() {
                c.current_dir(dir);
            }
            c
        },
        GameKind::XboxOne => return Err("No Xbox One emulator exists yet".into()),
        kind => {
            if matches!(kind, GameKind::Xbox360 | GameKind::Ps3) && !game.no_auto_save_backup {
                // A copy of the saves before every session, in case a mod or a crash breaks them.
                if let Err(e) = crate::game_mods::backup_saves(game, "before playing", cx) {
                    log::warn!("Couldn't back up {}'s saves: {e}", game.name);
                }
            }
            let id = kind.emulator().ok_or("No emulator for this game")?;
            let exe = emulators::exe(id, cx).ok_or("The emulator isn't set up yet (Setup tab)")?;
            let mut c = Command::new(&exe);
            c.args(emulators::launch_args(id, &boot_path(game)));
            if let Some(dir) = exe.parent() {
                c.current_dir(dir);
            }
            c
        },
    };
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    command.spawn().map_err(|e| format!("Couldn't start the game: {e}"))
}

/// Open an emulator on its own (its menus: settings, BIOS, controllers, firmware).
pub fn open_emulator(id: &str, extra: &[std::ffi::OsString], cx: &App) -> Result<(), String> {
    let exe = emulators::exe(id, cx).ok_or("The emulator isn't set up yet")?;
    let mut c = std::process::Command::new(&exe);
    c.args(extra);
    if let Some(dir) = exe.parent() {
        c.current_dir(dir);
    }
    c.spawn().map(|_| ()).map_err(|e| format!("Couldn't open the emulator: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FLUX_DETECT_TEST=<path>[;<path>...] prints what Flux makes of each path.
    #[test]
    fn detect_real_paths() {
        let Ok(paths) = std::env::var("FLUX_DETECT_TEST") else { return };
        for path in paths.split(';') {
            match detect(Path::new(path)) {
                Ok(d) => println!("{path}\n  -> {:?} \"{}\" at {} | {:?}", d.kind, d.name, d.path.display(), d.notes),
                Err(e) => println!("{path}\n  -> error: {e}"),
            }
        }
    }

    #[test]
    fn names_from_files() {
        assert_eq!(name_from(Path::new("C:/g/Skate 3 (USA, Europe).iso")), "Skate 3");
        assert_eq!(name_from(Path::new("C:/g/the_game.v2.exe")), "the game v2");
        assert_eq!(name_from(Path::new("C:/g/Crash Bandicoot [SCUS-94900].cue")), "Crash Bandicoot");
    }
}

#[cfg(test)]
mod fixtures {
    //! Tiny but real disc images and game folders in each console's layout.
    use super::*;

    fn sfo(title: &str) -> Vec<u8> {
        let value = format!("{title}\0");
        let mut v = Vec::new();
        v.extend_from_slice(b"\0PSF");
        v.extend_from_slice(&0x0101u32.to_le_bytes());
        v.extend_from_slice(&36u32.to_le_bytes()); // key table
        v.extend_from_slice(&44u32.to_le_bytes()); // data table
        v.extend_from_slice(&1u32.to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(&0x0204u16.to_le_bytes());
        v.extend_from_slice(&(value.len() as u32).to_le_bytes());
        v.extend_from_slice(&128u32.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(b"TITLE\0\0\0");
        v.extend_from_slice(value.as_bytes());
        v
    }

    fn dir_record(name: &[u8], lba: u32, size: u32, dir: bool) -> Vec<u8> {
        let len = 33 + name.len() + (name.len() + 1) % 2;
        let mut r = vec![0u8; len];
        r[0] = len as u8;
        r[2..6].copy_from_slice(&lba.to_le_bytes());
        r[10..14].copy_from_slice(&size.to_le_bytes());
        r[25] = if dir { 2 } else { 0 };
        r[32] = name.len() as u8;
        r[33..33 + name.len()].copy_from_slice(name);
        r
    }

    /// An ISO 9660 image with one file in the root; `raw` writes 2352-byte mode-2 CD sectors.
    fn iso(system_id: &str, file: &str, content: &[u8], raw: bool) -> Vec<u8> {
        let mut sectors = vec![vec![0u8; 2048]; 20];
        let pvd = &mut sectors[16];
        pvd[0] = 1;
        pvd[1..6].copy_from_slice(b"CD001");
        pvd[6] = 1;
        let sys = format!("{system_id:<32}");
        pvd[8..40].copy_from_slice(sys.as_bytes());
        let root = dir_record(&[0], 18, 2048, true);
        pvd[156..156 + 34].copy_from_slice(&root[..34]);
        sectors[17][0] = 255;
        sectors[17][1..6].copy_from_slice(b"CD001");
        let mut dir = Vec::new();
        dir.extend(dir_record(&[0], 18, 2048, true));
        dir.extend(dir_record(&[1], 18, 2048, true));
        dir.extend(dir_record(format!("{file};1").as_bytes(), 19, content.len() as u32, false));
        sectors[18][..dir.len()].copy_from_slice(&dir);
        sectors[19][..content.len()].copy_from_slice(content);
        if !raw {
            return sectors.concat();
        }
        let mut out = Vec::new();
        for s in sectors {
            out.extend_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
            out.extend_from_slice(&[0, 2, 0, 2]); // address + mode 2
            out.extend_from_slice(&[0; 8]); // subheader
            out.extend_from_slice(&s);
            out.extend_from_slice(&[0; 280]); // EDC/ECC
        }
        out
    }

    #[test]
    fn detects_every_console() {
        let root = std::env::temp_dir().join(format!("flux-detect-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let write = |rel: &str, data: &[u8]| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, data).unwrap();
            p
        };

        let ps1 = write("Crash (USA).iso", &iso("PLAYSTATION", "SYSTEM.CNF", b"BOOT = cdrom:/SCUS_949.00;1\r\n", false));
        let ps2 = write("Ratchet.iso", &iso("PLAYSTATION", "SYSTEM.CNF", b"BOOT2 = cdrom0:/SCUS_971.99;1\r\n", false));
        write("Spyro (USA).bin", &iso("PLAYSTATION", "SYSTEM.CNF", b"BOOT = cdrom:/SCUS_942.28;1\r\n", true));
        let cue = write("Spyro (USA).cue", b"FILE \"Spyro (USA).bin\" BINARY\r\n  TRACK 01 MODE2/2352\r\n    INDEX 01 00:00:00\r\n");
        let psp = write("Patapon.iso", &iso("PSP GAME", "UMD_DATA.BIN", b"UCUS-98711|", false));
        write("ps3/Demon's Souls/PS3_GAME/USRDIR/EBOOT.BIN", b"SCE\0");
        write("ps3/Demon's Souls/PS3_GAME/PARAM.SFO", &sfo("Demon's Souls"));
        write("ps3/Demon's Souls/PS3_GAME/PIC1.PNG", b"png");
        write("ps4/CUSA00001/eboot.bin", b"\x7fELF");
        write("ps4/CUSA00001/sce_sys/param.sfo", &sfo("Bloodborne"));
        let nsp = write("Mario Odyssey [0100000000010000].nsp", b"PFS0");

        let check = |path: &Path, kind: GameKind, name: &str| {
            let d = detect(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!((d.kind, d.name.as_str()), (kind, name), "{}", path.display());
            d
        };
        check(&ps1, GameKind::Ps1, "Crash");
        check(&ps2, GameKind::Ps2, "Ratchet");
        let spyro = check(&cue, GameKind::Ps1, "Spyro");
        assert_eq!(spyro.path, cue, "PS1 .cue games launch from the .cue");
        check(&psp, GameKind::Psp, "Patapon");
        let ps3 = check(&root.join("ps3/Demon's Souls"), GameKind::Ps3, "Demon's Souls");
        assert!(ps3.art_background.is_some(), "PS3 PIC1.PNG becomes the background");
        check(&root.join("ps3/Demon's Souls/PS3_GAME/USRDIR/EBOOT.BIN"), GameKind::Ps3, "Demon's Souls");
        let ps4 = check(&root.join("ps4/CUSA00001"), GameKind::Ps4, "Bloodborne");
        assert!(ps4.path.ends_with("eboot.bin"));
        check(&nsp, GameKind::Switch, "Mario Odyssey");
        let game = LibraryGame { id: "x".into(), name: "x".into(), kind: GameKind::Ps3, path: root.join("ps3/Demon's Souls"), scene: "galaxy".into(), steam_appid: None, background: None, logo: None, icon: None, web_art_checked: false, art_source: None, module_hash: None, file_mods: vec![], no_auto_save_backup: false, info: None, info_checked: false, cover: None, play_seconds: 0, last_played: None, launch_uri: None, install_uri: None, source: None, locked_art: vec![], favourite: false, last_opened: None };
        assert!(boot_path(&game).ends_with("EBOOT.BIN"), "RPCS3 boots the EBOOT.BIN");
        _ = std::fs::remove_dir_all(&root);
    }
}
