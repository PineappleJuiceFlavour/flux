//! Importing games from other launchers (Steam, Epic Games, Xbox app / Game Pass, GOG, Ubisoft
//! Connect, EA app, Battle.net, Amazon Games, Rockstar) and scanning folders of emulator games.
//! Everything is read from the launchers' own files and Windows' registry on this PC: no logins.

use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
};

use crate::games::{GameKind, LibraryGame};

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// Identifies the game across scans ("steam:271590", "epic:Fortnite", a path).
    pub key: String,
    pub name: String,
    /// "Steam", "Epic Games", "Xbox", "Emulator", ...
    pub source: &'static str,
    pub kind: GameKind,
    pub installed: bool,
    /// The game folder / program / disc image (empty when not installed).
    pub path: PathBuf,
    pub steam_appid: Option<u32>,
    /// Starts the game through its launcher.
    pub launch_uri: Option<String>,
    /// Opens the launcher's install page for a game that isn't installed.
    pub install_uri: Option<String>,
}

impl Found {
    pub fn into_game(self) -> LibraryGame {
        let mut game = LibraryGame {
            id: crate::games::new_id(&self.name),
            name: self.name,
            kind: self.kind,
            path: self.path,
            scene: self.kind.default_scene().into(),
            steam_appid: self.steam_appid,
            background: None,
            logo: None,
            icon: None,
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
            source: Some(self.source.to_string()),
            locked_art: Vec::new(),
            favourite: false,
            last_opened: None,
        };
        game.launch_uri = self.launch_uri;
        game.install_uri = self.install_uri;
        game
    }
}

pub struct Launcher {
    pub id: &'static str,
    pub name: &'static str,
    /// Can list owned games that aren't installed (from local files, no login).
    pub lists_owned: bool,
}

pub const LAUNCHERS: &[Launcher] = &[
    Launcher { id: "steam", name: "Steam", lists_owned: true },
    Launcher { id: "epic", name: "Epic Games", lists_owned: true },
    Launcher { id: "xbox", name: "Xbox app / Game Pass", lists_owned: false },
    Launcher { id: "gog", name: "GOG Galaxy", lists_owned: false },
    Launcher { id: "ubisoft", name: "Ubisoft Connect", lists_owned: false },
    Launcher { id: "ea", name: "EA app", lists_owned: false },
    Launcher { id: "battlenet", name: "Battle.net", lists_owned: false },
    Launcher { id: "amazon", name: "Amazon Games", lists_owned: false },
    Launcher { id: "rockstar", name: "Rockstar Games Launcher", lists_owned: false },
];

/// Is the launcher on this PC?
pub fn detected(id: &str) -> bool {
    match id {
        "steam" => steam_root().is_some(),
        "epic" => epic_manifests_dir().is_dir(),
        "xbox" => !xbox_roots().is_empty(),
        _ => !scan(id, true).is_empty(),
    }
}

pub fn scan(id: &str, installed_only: bool) -> Vec<Found> {
    let mut found = match id {
        "steam" => steam(installed_only),
        "epic" => epic(installed_only),
        "xbox" => xbox(),
        "gog" => gog(),
        "ubisoft" => ubisoft(),
        "ea" => uninstall_entries("EA app", &["electronic arts", "ea app", "ea games"], &["ea desktop", "ea app", "origin"]),
        "battlenet" => uninstall_entries("Battle.net", &["blizzard", "battle.net"], &["battle.net", "blizzard app"]),
        "amazon" => uninstall_entries("Amazon Games", &["amazon games", "amazon.com services"], &["amazon games"]),
        "rockstar" => uninstall_entries("Rockstar", &["rockstar games"], &["rockstar games launcher", "social club"]),
        _ => Vec::new(),
    };
    found.sort_by_key(|f| (!f.installed, f.name.to_lowercase()));
    found.dedup_by(|a, b| a.key == b.key);
    found
}

// ------------------------------------------------------------------------------------------- Steam

pub fn steam_root() -> Option<PathBuf> {
    crate::win_native::user_registry_string(r"Software\Valve\Steam", "SteamPath")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| Some(PathBuf::from(r"C:\Program Files (x86)\Steam")).filter(|p| p.is_dir()))
}

/// `"key"  "value"` pairs of a text KeyValues file, for every key named `key`.
fn vdf_values(text: &str, key: &str) -> Vec<String> {
    text.lines().filter_map(|line| {
        let parts: Vec<&str> = line.split('"').collect();
        (parts.len() >= 4 && parts[1] == key).then(|| parts[3].replace("\\\\", "\\"))
    }).collect()
}

pub fn steam_library_folders(root: &Path) -> Vec<PathBuf> {
    let text = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")).unwrap_or_default();
    let mut libs: Vec<PathBuf> = vdf_values(&text, "path").into_iter().map(PathBuf::from).collect();
    if libs.is_empty() {
        libs.push(root.to_path_buf());
    }
    libs
}

/// Time played and last played per Steam app, from every Steam account on this PC
/// (`userdata/<account>/config/localconfig.vdf`): app id -> (minutes, unix time). Accounts' minutes
/// add up; the latest time wins.
pub fn steam_playtimes() -> HashMap<u32, (u64, i64)> {
    let mut out: HashMap<u32, (u64, i64)> = HashMap::new();
    let Some(root) = steam_root() else { return out };
    for user in std::fs::read_dir(root.join("userdata")).into_iter().flatten().flatten() {
        let text = std::fs::read_to_string(user.path().join("config").join("localconfig.vdf")).unwrap_or_default();
        parse_playtimes(&text, &mut out);
    }
    out
}

/// The "Playtime" (minutes) and "LastPlayed" (unix time) under each app id key of a localconfig.vdf.
pub fn parse_playtimes(text: &str, out: &mut HashMap<u32, (u64, i64)>) {
    let mut app = None;
    for line in text.lines() {
        let t = line.trim();
        // A block key: an app id starts that app's block (other keys, like "cloud", are blocks inside it).
        if t.starts_with('"') && t.ends_with('"') && t.matches('"').count() == 2 {
            if let Ok(id) = t.trim_matches('"').parse::<u32>() {
                app = Some(id);
            }
            continue;
        }
        let (Some(id), Some(rest)) = (app, t.strip_prefix('"')) else { continue };
        let mut parts = rest.split('"');
        let (Some(key), _, Some(value)) = (parts.next(), parts.next(), parts.next()) else { continue };
        match key {
            "Playtime" => if let Ok(minutes) = value.parse::<u64>() {
                out.entry(id).or_default().0 += minutes;
            },
            "LastPlayed" => if let Ok(when) = value.parse::<i64>() {
                let entry = out.entry(id).or_default();
                entry.1 = entry.1.max(when);
            },
            _ => {},
        }
    }
}

/// App names and types from Steam's own app cache (binary KeyValues, `appinfo.vdf` v28/v29).
pub fn steam_appinfo(root: &Path) -> HashMap<u32, (String, String)> {
    let mut out = HashMap::new();
    let Ok(data) = std::fs::read(root.join("appcache").join("appinfo.vdf")) else { return out };
    if data.len() < 16 {
        return out;
    }
    let u32_at = |at: usize| data.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let magic = u32_at(0).unwrap_or(0);
    let v29 = magic == 0x0756_4429;
    if !(v29 || magic == 0x0756_4428) {
        return out;
    }
    // v29 stores key names once, in a table at the end.
    let mut strings: Vec<String> = Vec::new();
    let mut pos = 8;
    if v29 {
        let table = u64::from_le_bytes(data[8..16].try_into().unwrap()) as usize;
        pos = 16;
        if let Some(count) = u32_at(table) {
            let mut at = table + 4;
            for _ in 0..count {
                let Some(end) = data[at..].iter().position(|b| *b == 0) else { break };
                strings.push(String::from_utf8_lossy(&data[at..at + end]).to_string());
                at += end + 1;
            }
        }
    }
    let cstr = |at: &mut usize| -> String {
        let end = data[*at..].iter().position(|b| *b == 0).unwrap_or(0);
        let s = String::from_utf8_lossy(&data[*at..*at + end]).to_string();
        *at += end + 1;
        s
    };
    loop {
        let Some(appid) = u32_at(pos) else { break };
        if appid == 0 {
            break;
        }
        let Some(size) = u32_at(pos + 4) else { break };
        let next = pos + 8 + size as usize;
        let mut at = pos + 8 + 4 + 4 + 8 + 20 + 4 + 20;
        // Walk the key/values, remembering the path, to pick out common/name and common/type.
        let (mut name, mut kind) = (String::new(), String::new());
        let mut path: Vec<String> = Vec::new();
        while at < next {
            let t = data[at];
            at += 1;
            if t == 8 {
                if path.pop().is_none() {
                    break;
                }
                continue;
            }
            let key = if v29 {
                let Some(i) = u32_at(at) else { break };
                at += 4;
                strings.get(i as usize).cloned().unwrap_or_default()
            } else {
                cstr(&mut at)
            };
            match t {
                0 => path.push(key),
                1 => {
                    let value = cstr(&mut at);
                    if path.last().is_some_and(|p| p == "common") {
                        match key.as_str() {
                            "name" => name = value,
                            "type" => kind = value,
                            _ => {},
                        }
                    }
                },
                2 | 3 | 4 | 6 => at += 4,
                7 | 10 => at += 8,
                _ => break,
            }
            if !name.is_empty() && !kind.is_empty() {
                break;
            }
        }
        if !name.is_empty() {
            out.insert(appid, (name, kind.to_lowercase()));
        }
        pos = next;
    }
    out
}

fn steam(installed_only: bool) -> Vec<Found> {
    let Some(root) = steam_root() else { return vec![] };
    let mut out = Vec::new();
    let mut installed_ids = BTreeSet::new();
    for lib in steam_library_folders(&root) {
        let apps = lib.join("steamapps");
        for entry in std::fs::read_dir(&apps).into_iter().flatten().flatten() {
            let file = entry.file_name().to_string_lossy().to_string();
            if !(file.starts_with("appmanifest_") && file.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
            let (Some(id), Some(name), Some(dir)) = (
                vdf_values(&text, "appid").first().and_then(|v| v.parse::<u32>().ok()),
                vdf_values(&text, "name").into_iter().next(),
                vdf_values(&text, "installdir").into_iter().next(),
            ) else { continue };
            // Steam's own tools (Proton, redistributables) aren't games.
            if id == 228980 || name.starts_with("Steamworks") || name.contains("Redistributable") {
                continue;
            }
            installed_ids.insert(id);
            out.push(Found {
                key: format!("steam:{id}"),
                name,
                source: "Steam",
                kind: GameKind::Windows,
                installed: true,
                path: apps.join("common").join(dir),
                steam_appid: Some(id),
                launch_uri: None,
                install_uri: None,
            });
        }
    }
    if installed_only {
        return out;
    }
    // Owned games: the ones Steam keeps library art for, or has play records of, typed "game"
    // in Steam's app cache.
    let info = steam_appinfo(&root);
    let mut owned: BTreeSet<u32> = BTreeSet::new();
    for entry in std::fs::read_dir(root.join("appcache").join("librarycache")).into_iter().flatten().flatten() {
        if let Ok(id) = entry.file_name().to_string_lossy().parse::<u32>() {
            owned.insert(id);
        }
    }
    for user in std::fs::read_dir(root.join("userdata")).into_iter().flatten().flatten() {
        let text = std::fs::read_to_string(user.path().join("config").join("localconfig.vdf")).unwrap_or_default();
        // App ids are the keys of the "apps" block that carry a "Playtime" or "LastPlayed".
        let mut last_key = None;
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with('"') && t.ends_with('"') && t.matches('"').count() == 2 {
                last_key = t.trim_matches('"').parse::<u32>().ok();
            } else if (t.starts_with("\"Playtime\"") || t.starts_with("\"LastPlayed\"")) && let Some(id) = last_key {
                owned.insert(id);
            }
        }
    }
    for id in owned {
        if installed_ids.contains(&id) {
            continue;
        }
        let Some((name, kind)) = info.get(&id) else { continue };
        if kind != "game" {
            continue;
        }
        out.push(Found {
            key: format!("steam:{id}"),
            name: name.clone(),
            source: "Steam",
            kind: GameKind::Windows,
            installed: false,
            path: PathBuf::new(),
            steam_appid: Some(id),
            launch_uri: None,
            install_uri: Some(format!("steam://install/{id}")),
        });
    }
    out
}

// -------------------------------------------------------------------------------------------- Epic

fn epic_manifests_dir() -> PathBuf {
    PathBuf::from(r"C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests")
}

fn epic(installed_only: bool) -> Vec<Found> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in std::fs::read_dir(epic_manifests_dir()).into_iter().flatten().flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
        let Ok(m) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        // Add-ons and content packs share the game's folder; only the application is a game.
        if m["bIsApplication"].as_bool() == Some(false) {
            continue;
        }
        let (Some(name), Some(app), Some(dir)) = (m["DisplayName"].as_str(), m["AppName"].as_str(), m["InstallLocation"].as_str()) else { continue };
        let namespace = m["CatalogNamespace"].as_str().unwrap_or_default();
        let item = m["CatalogItemId"].as_str().unwrap_or_default();
        seen.insert(app.to_string());
        out.push(Found {
            key: format!("epic:{app}"),
            name: name.to_string(),
            source: "Epic Games",
            kind: GameKind::Windows,
            installed: true,
            path: PathBuf::from(dir),
            steam_appid: None,
            launch_uri: Some(format!("com.epicgames.launcher://apps/{namespace}%3A{item}%3A{app}?action=launch&silent=true")),
            install_uri: None,
        });
    }
    if installed_only {
        return out;
    }
    // Owned games: the launcher's catalog cache (base64 JSON), when it has one.
    let cache = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default()
        .join(r"EpicGamesLauncher\Saved\Data\Catalog\catcache.bin");
    if let Ok(raw) = std::fs::read_to_string(&cache) {
        use base64::Engine;
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(raw.trim())
            && let Ok(items) = serde_json::from_slice::<Vec<serde_json::Value>>(&bytes)
        {
            for item in items {
                let is_game = item["categories"].as_array().into_iter().flatten().any(|c| c["path"].as_str() == Some("games"));
                let (Some(title), Some(app)) = (item["title"].as_str(), item["releaseInfo"][0]["appId"].as_str()) else { continue };
                if !is_game || seen.contains(app) || item["mainGameItem"].is_object() {
                    continue;
                }
                let namespace = item["namespace"].as_str().unwrap_or_default();
                let id = item["id"].as_str().unwrap_or_default();
                out.push(Found {
                    key: format!("epic:{app}"),
                    name: title.to_string(),
                    source: "Epic Games",
                    kind: GameKind::Windows,
                    installed: false,
                    path: PathBuf::new(),
                    steam_appid: None,
                    launch_uri: Some(format!("com.epicgames.launcher://apps/{namespace}%3A{id}%3A{app}?action=launch&silent=true")),
                    install_uri: Some(format!("com.epicgames.launcher://apps/{namespace}%3A{id}%3A{app}?action=install")),
                });
            }
        }
    }
    out
}

// -------------------------------------------------------------------------------------------- Xbox

fn xbox_roots() -> Vec<PathBuf> {
    ('C'..='Z').map(|d| PathBuf::from(format!("{d}:\\XboxGames"))).filter(|p| p.is_dir()).collect()
}

fn xml_attr(tag: &str, attr: &str) -> Option<String> {
    let at = tag.find(&format!(" {attr}=\""))? + attr.len() + 3;
    let end = tag[at..].find('"')? + at;
    Some(tag[at..end].replace("&amp;", "&").replace("&quot;", "\"").replace("&apos;", "'"))
}

/// The publisher id of a Windows package family name: Crockford base32 of the first 8 bytes of
/// SHA-256 over the publisher (UTF-16LE).
pub fn publisher_id(publisher: &str) -> String {
    use sha2::{Digest, Sha256};
    let utf16: Vec<u8> = publisher.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    let hash = Sha256::digest(&utf16);
    let bits = u64::from_be_bytes(hash[..8].try_into().unwrap());
    const ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";
    // 64 bits padded to 65, read 5 at a time.
    let value = (bits as u128) << 1;
    (0..13).map(|i| ALPHABET[((value >> (60 - i * 5)) & 31) as usize] as char).collect()
}

fn xbox() -> Vec<Found> {
    let mut out = Vec::new();
    for root in xbox_roots() {
        for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
            let dir = entry.path();
            let config = dir.join("Content").join("MicrosoftGame.config");
            let Ok(text) = std::fs::read_to_string(&config) else { continue };
            let Some(identity) = text.split("<Identity").nth(1).and_then(|t| t.split('>').next()) else { continue };
            let (Some(name), Some(publisher)) = (xml_attr(identity, "Name"), xml_attr(identity, "Publisher")) else { continue };
            let exe_id = text.split("<Executable").nth(1).and_then(|t| t.split('>').next()).and_then(|t| xml_attr(t, "Id")).unwrap_or_else(|| "Game".into());
            let title = text.split("<ShellVisuals").nth(1).and_then(|t| t.split('>').next()).and_then(|t| xml_attr(t, "DefaultDisplayName"))
                .filter(|t| !t.starts_with("ms-resource"))
                .unwrap_or_else(|| entry.file_name().to_string_lossy().to_string());
            let family = format!("{name}_{}", publisher_id(&publisher));
            out.push(Found {
                key: format!("xbox:{family}"),
                name: title,
                source: "Xbox",
                kind: GameKind::Windows,
                installed: true,
                path: dir.clone(),
                steam_appid: None,
                launch_uri: Some(format!("shell:AppsFolder\\{family}!{exe_id}")),
                install_uri: None,
            });
        }
    }
    out
}

// --------------------------------------------------------------------------- registry launchers

fn gog() -> Vec<Found> {
    use crate::win_native::{Hive, registry_string, registry_subkeys};
    let base = r"SOFTWARE\WOW6432Node\GOG.com\Games";
    registry_subkeys(Hive::LocalMachine, base).into_iter().filter_map(|id| {
        let key = format!("{base}\\{id}");
        let name = registry_string(Hive::LocalMachine, &key, "gameName")?;
        let path = PathBuf::from(registry_string(Hive::LocalMachine, &key, "path")?);
        let exe = registry_string(Hive::LocalMachine, &key, "exe").map(PathBuf::from).filter(|p| p.is_file());
        Some(Found {
            key: format!("gog:{id}"),
            name,
            source: "GOG",
            kind: GameKind::Windows,
            installed: path.is_dir(),
            path: exe.unwrap_or(path),
            steam_appid: None,
            launch_uri: None,
            install_uri: None,
        })
    }).collect()
}

fn ubisoft() -> Vec<Found> {
    use crate::win_native::{Hive, registry_string, registry_subkeys};
    let base = r"SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs";
    registry_subkeys(Hive::LocalMachine, base).into_iter().filter_map(|id| {
        let dir = PathBuf::from(registry_string(Hive::LocalMachine, &format!("{base}\\{id}"), "InstallDir")?.trim_end_matches(['/', '\\']).to_string());
        let name = dir.file_name()?.to_string_lossy().to_string();
        Some(Found {
            key: format!("ubisoft:{id}"),
            name,
            source: "Ubisoft Connect",
            kind: GameKind::Windows,
            installed: dir.is_dir(),
            path: dir,
            steam_appid: None,
            launch_uri: Some(format!("uplay://launch/{id}/0")),
            install_uri: None,
        })
    }).collect()
}

/// Games a launcher installed, from Windows' list of installed programs (publisher or uninstaller
/// pointing at that launcher). The launcher programs themselves are left out.
fn uninstall_entries(source: &'static str, publishers: &[&str], launcher_names: &[&str]) -> Vec<Found> {
    use crate::win_native::{Hive, registry_string, registry_subkeys};
    let mut out = Vec::new();
    for (hive, base) in [
        (Hive::LocalMachine, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        (Hive::LocalMachine, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        (Hive::CurrentUser, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    ] {
        for sub in registry_subkeys(hive, base) {
            let key = format!("{base}\\{sub}");
            let Some(name) = registry_string(hive, &key, "DisplayName") else { continue };
            let publisher = registry_string(hive, &key, "Publisher").unwrap_or_default().to_lowercase();
            let uninstall = registry_string(hive, &key, "UninstallString").unwrap_or_default().to_lowercase();
            let by_launcher = publishers.iter().any(|p| publisher.contains(p) || uninstall.contains(p));
            let lower = name.to_lowercase();
            if !by_launcher || launcher_names.iter().any(|l| lower.contains(l)) || lower.contains("redistributable") {
                continue;
            }
            let Some(dir) = registry_string(hive, &key, "InstallLocation").map(|d| PathBuf::from(d.trim_matches('"'))).filter(|p| p.is_dir()) else { continue };
            let exe = crate::games::main_exe_in(&dir);
            out.push(Found {
                key: format!("{}:{}", source.to_lowercase(), dir.display()),
                name,
                source,
                kind: GameKind::Windows,
                installed: true,
                path: exe.unwrap_or(dir),
                steam_appid: None,
                launch_uri: None,
                install_uri: None,
            });
        }
    }
    out
}

// ------------------------------------------------------------------------------ emulator folders

const GAME_EXTENSIONS: &[&str] = &["iso", "cue", "chd", "cso", "pbp", "nsp", "xci", "xex", "xiso", "gcm", "rvz", "wbfs"];

/// Every console game in a folder (and its subfolders): disc images, Switch files and PS3/PS4
/// game folders, recognised the same way as adding one by hand. `only` keeps one console.
pub fn scan_folder(folder: &Path, subfolders: bool, only: Option<GameKind>) -> Vec<Found> {
    let mut out = Vec::new();
    let mut dirs = vec![(folder.to_path_buf(), 0)];
    let mut seen = 0;
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        // A PS3/PS4 game folder is one game: don't look inside it.
        if depth > 0 && (crate::disc::child(&dir, "PS3_GAME").is_some() || crate::disc::child(&dir, "PARAM.SFO").is_some() || crate::disc::child(&dir, "sce_sys").is_some()) {
            if let Ok(d) = crate::games::detect(&dir) {
                push_detection(&mut out, d, only);
            }
            continue;
        }
        for path in entries {
            seen += 1;
            if seen > 20_000 {
                return out;
            }
            if path.is_dir() {
                if subfolders && depth < 6 {
                    dirs.push((path, depth + 1));
                }
                continue;
            }
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            // A .bin next to its .cue is the same disc.
            let bin_with_cue = ext == "bin" && path.with_extension("cue").is_file();
            if !(GAME_EXTENSIONS.contains(&ext.as_str()) || ext == "bin" && !bin_with_cue) {
                continue;
            }
            if let Ok(d) = crate::games::detect(&path) {
                push_detection(&mut out, d, only);
            }
        }
    }
    out.sort_by_key(|f| f.name.to_lowercase());
    out
}

fn push_detection(out: &mut Vec<Found>, d: crate::games::Detection, only: Option<GameKind>) {
    if d.kind == GameKind::Windows || only.is_some_and(|k| k != d.kind) {
        return;
    }
    out.push(Found {
        key: d.path.display().to_string(),
        name: d.name.clone(),
        source: "Emulator",
        kind: d.kind,
        installed: true,
        path: d.path.clone(),
        steam_appid: None,
        launch_uri: None,
        install_uri: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_publisher_id() {
        // Microsoft's own: "CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US" -> 8wekyb3d8bbwe
        assert_eq!(publisher_id("CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US"), "8wekyb3d8bbwe");
    }

    /// FLUX_IMPORT_TEST=1: what each launcher on this PC has.
    #[test]
    fn real_launchers() {
        if std::env::var_os("FLUX_IMPORT_TEST").is_none() {
            return;
        }
        for l in LAUNCHERS {
            let installed = scan(l.id, true);
            let all = if l.lists_owned { scan(l.id, false) } else { installed.clone() };
            println!("{}: {} installed, {} with owned", l.name, installed.len(), all.len());
            for f in all.iter().take(6) {
                println!("   {} | installed {} | {} | {:?}", f.name, f.installed, f.path.display(), f.launch_uri.as_ref().or(f.install_uri.as_ref()));
            }
        }
    }
}

#[cfg(test)]
mod steam_playtime_tests {
    use std::collections::HashMap;

    #[test]
    fn reads_playtime_and_last_played() {
        let vdf = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\
            \t\t\t\t\t\"4000\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LastPlayed\"\t\t\"1791009969\"\n\t\t\t\t\t\t\"cloud\"\n\t\t\t\t\t\t{\n\t\t\t\t\t\t\t\"quota_files\"\t\t\"1\"\n\t\t\t\t\t\t}\n\
            \t\t\t\t\t\t\"Playtime2wks\"\t\t\"30\"\n\t\t\t\t\t\t\"Playtime\"\t\t\"106659\"\n\t\t\t\t\t}\n\
            \t\t\t\t\t\"105600\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"Playtime\"\t\t\"15174\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n";
        let mut out = HashMap::new();
        super::parse_playtimes(vdf, &mut out);
        assert_eq!(out[&4000], (106659, 1791009969), "fields after a nested block still count");
        assert_eq!(out[&105600], (15174, 0));
        // A second account adds its minutes.
        super::parse_playtimes(vdf, &mut out);
        assert_eq!(out[&105600].0, 30348);
    }

    /// FLUX_STEAM_TIMES=1: what this PC's Steam accounts report.
    #[test]
    fn this_pc() {
        if std::env::var_os("FLUX_STEAM_TIMES").is_none() { return }
        let mut times: Vec<_> = super::steam_playtimes().into_iter().collect();
        times.sort_by_key(|(_, (m, _))| std::cmp::Reverse(*m));
        for (id, (minutes, last)) in times.iter().take(6) {
            println!("STEAM {id}: {:.0} h, last {last}", *minutes as f64 / 60.);
        }
    }
}
