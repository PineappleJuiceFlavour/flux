//! Mods, saves and debug info for emulated games (the Mods and Debug tabs of a game page).
//!
//! - Patches: Xenia Canary's `patches/<TitleID> - <name>.patch.toml` (community list:
//!   github.com/xenia-canary/game-patches) and RPCS3's `patches/patch.yml` (community list from
//!   rpcs3.net, the file RPCS3's own "Download latest patches" fetches), switched on and off the way
//!   each emulator stores it (`is_enabled` in the patch file; RPCS3's `config/patch_config.yml`).
//! - Xenia plugins: `plugins/<TitleID>/plugins.toml` + the plugin `.xex`, tied to the game's module
//!   hash (Xenia only loads a plugin for the exact executable it was listed for).
//! - File mods: a folder of replacement game files copied over an unpacked game, with the originals
//!   backed up so switching the mod off puts them back.
//! - Saves: Xenia's `content/<profile>/<TitleID>` and RPCS3's `dev_hdd0/home/*/savedata/<serial>*`,
//!   backed up into Flux's folder (and automatically before each launch).

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use gpui::App;
use serde::{Deserialize, Serialize};

use crate::{
    disc, emulators,
    games::{GameKind, LibraryGame},
};

// ------------------------------------------------------------------------------------ identities

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GameIds {
    /// Xbox 360 title ID, 8 hex digits.
    pub title_id: Option<String>,
    /// Xbox 360: hash of the game's executable as Xenia computes it (patches and plugins need it).
    pub module_hash: Option<String>,
    /// PS3 serial (BLUS30464) and the game's version, counting an installed update (01.05).
    pub serial: Option<String>,
    pub app_version: Option<String>,
    pub title: Option<String>,
}

/// Per game path: the ids read from the disc, and the full ids with when they were worked out.
static IDS: Mutex<Option<HashMap<PathBuf, (GameIds, Option<(GameIds, std::time::Instant)>)>>> = Mutex::new(None);

pub fn xenia_dir(cx: &App) -> Option<PathBuf> {
    Some(emulators::exe(emulators::XENIA, cx)?.parent()?.to_path_buf())
}

/// RPCS3's data folder: next to rpcs3.exe, or its `portable` folder when there is one.
pub fn rpcs3_dir(cx: &App) -> Option<PathBuf> {
    let dir = emulators::exe(emulators::RPCS3, cx)?.parent()?.to_path_buf();
    let portable = dir.join("portable");
    Some(if portable.is_dir() { portable } else { dir })
}

/// The ids of a game (cached: reading a disc for them takes a moment).
pub fn ids(game: &LibraryGame, cx: &App) -> GameIds {
    let key = game.path.clone();
    // A separate statement, so the lock is released before the refresh below takes it again.
    let cached = IDS.lock().unwrap().get_or_insert_with(HashMap::new).get(&key).cloned();
    if let Some((base, full)) = cached {
        // Logs and updates change while Flux runs: look again every few seconds, not every frame.
        if let Some((full, at)) = full && at.elapsed() < std::time::Duration::from_secs(4) {
            return full;
        }
        let full = with_hash(base.clone(), game, cx);
        IDS.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, (base, Some((full.clone(), std::time::Instant::now()))));
        return full;
    }
    let mut ids = GameIds::default();
    match game.kind {
        GameKind::Xbox360 => ids.title_id = disc::xbox360_title_id(&game.path),
        GameKind::Ps3 => {
            // An unpacked game folder, or a disc image (.iso) with PS3_GAME/PARAM.SFO inside.
            let sfo = ps3_sfo(&game.path).and_then(|p| std::fs::read(p).ok())
                .or_else(|| game.path.is_file().then(|| disc::Iso::open(&game.path)?.read_file("PS3_GAME/PARAM.SFO")).flatten());
            if let Some(sfo) = sfo {
                ids.serial = disc::sfo_value(&sfo, "TITLE_ID");
                ids.app_version = disc::sfo_value(&sfo, "APP_VER").or_else(|| disc::sfo_value(&sfo, "VERSION"));
                ids.title = disc::sfo_value(&sfo, "TITLE");
            }
        },
        _ => {},
    }
    let full = with_hash(ids.clone(), game, cx);
    IDS.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, (ids, Some((full.clone(), std::time::Instant::now()))));
    full
}

pub fn forget_ids(game: &LibraryGame) {
    if let Some(map) = IDS.lock().unwrap().as_mut() {
        map.remove(&game.path);
    }
}

/// Fill in what changes while Flux runs: the module hash (from Xenia's log or patch files) and
/// the PS3 version (an update installed in RPCS3).
fn with_hash(mut ids: GameIds, game: &LibraryGame, cx: &App) -> GameIds {
    if let (Some(tid), Some(dir)) = (&ids.title_id, xenia_dir(cx)) {
        ids.module_hash = hash_from_log(&dir.join("xenia.log"), tid)
            .or_else(|| game.module_hash.clone())
            .or_else(|| xenia_patch_files(&dir, tid).into_iter().find_map(|f| patch_file_hashes(&f).into_iter().next()));
    }
    if let (Some(serial), Some(base)) = (&ids.serial, rpcs3_dir(cx)) {
        let update = base.join("dev_hdd0").join("game").join(serial).join("PARAM.SFO");
        if let Some(ver) = std::fs::read(update).ok().and_then(|sfo| disc::sfo_value(&sfo, "APP_VER")) {
            if ids.app_version.as_deref().is_none_or(|v| ver.as_str() > v) {
                ids.app_version = Some(ver);
            }
        }
    }
    ids
}

/// The hash of the game's own executable (default.xex) in a Xenia log. Each module Xenia loads is
/// logged as "Module <path>:", "Module Hash: ...", then its header ("Title ID: ..."); a game's
/// extra modules (e.g. Skate 3's EAWebkit.xex) carry the same title ID, so only default.xex counts.
pub fn hash_from_log(log: &Path, title_id: &str) -> Option<String> {
    let bytes = std::fs::read(log).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let (mut module, mut hash) = (String::new(), None::<String>);
    let mut fallback = None;
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.split("Module Hash:").nth(1) {
            hash = Some(rest.trim().to_uppercase());
        } else if let Some(rest) = t.split("Title ID:").nth(1) {
            if rest.trim().eq_ignore_ascii_case(title_id) && let Some(h) = &hash {
                if module.to_lowercase().ends_with("default.xex") {
                    return Some(h.clone());
                }
                fallback.get_or_insert_with(|| h.clone());
            }
        } else if let Some(pos) = t.find("Module ") && t.ends_with(':') && !t.contains("Module Flags") {
            module = t[pos + 7..t.len() - 1].to_string();
            hash = None;
        }
    }
    fallback
}

fn ps3_sfo(path: &Path) -> Option<PathBuf> {
    let folder = if path.is_dir() { path.to_path_buf() } else { path.parent()?.to_path_buf() };
    // The game folder, its PS3_GAME, or (from an EBOOT.BIN in USRDIR) two levels up.
    for dir in folder.ancestors().take(4) {
        for candidate in [dir.join("PS3_GAME").join("PARAM.SFO"), dir.join("PARAM.SFO")] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------------------- patches

#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    /// Identifies the patch for toggling (file + position, or RPCS3's config path).
    pub key: String,
    pub name: String,
    pub author: String,
    pub notes: String,
    pub enabled: bool,
    /// Made for this exact version of the game.
    pub fits: bool,
    /// Which file it's in ("community" or the player's own file name).
    pub source: String,
}

fn unquote(value: &str) -> String {
    let v = value.trim();
    if let Some(rest) = v.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => if let Some(n) = chars.next() { out.push(match n { 'n' => '\n', other => other }) },
                '"' => break,
                c => out.push(c),
            }
        }
        return out;
    }
    if let Some(rest) = v.strip_prefix('\'') {
        return rest.split('\'').next().unwrap_or("").to_string();
    }
    v.split(" #").next().unwrap_or("").trim().to_string()
}

// --- Xenia

pub fn xenia_patch_files(dir: &Path, title_id: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir.join("patches")).into_iter().flatten().flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_uppercase()).unwrap_or_default();
            name.starts_with(&title_id.to_uppercase()) && name.ends_with(".PATCH.TOML")
        })
        .collect();
    files.sort();
    files
}

/// `hash = "..."` or `hash = ["...", "..."]` at the top of a patch file.
pub fn patch_file_hashes(file: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let Some(line) = text.lines().take_while(|l| !l.trim_start().starts_with("[[")).find(|l| l.trim_start().starts_with("hash")) else { return vec![] };
    let value = line.split_once('=').map(|x| x.1).unwrap_or("");
    let value = value.split('#').next().unwrap_or("");
    value.split('"').skip(1).step_by(2).map(|h| h.trim().to_uppercase()).filter(|h| !h.is_empty()).collect()
}

/// The `[[patch]]` blocks of a Xenia patch file: (name, author, desc, enabled).
pub fn parse_xenia_patches(text: &str) -> Vec<(String, String, String, bool)> {
    let mut out: Vec<(String, String, String, bool)> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t == "[[patch]]" {
            out.push(Default::default());
            continue;
        }
        if t.starts_with("[[") || t.starts_with('#') {
            continue;
        }
        let (Some(last), Some((key, value))) = (out.last_mut(), t.split_once('=')) else { continue };
        match key.trim() {
            "name" if last.0.is_empty() => last.0 = unquote(value),
            "author" if last.1.is_empty() => last.1 = unquote(value),
            "desc" if last.2.is_empty() => last.2 = unquote(value),
            "is_enabled" => last.3 = value.split('#').next().unwrap_or("").trim() == "true",
            _ => {},
        }
    }
    out
}

pub fn xenia_patches(dir: &Path, ids: &GameIds) -> Vec<Patch> {
    let Some(tid) = &ids.title_id else { return vec![] };
    let mut out = Vec::new();
    for file in xenia_patch_files(dir, tid) {
        let hashes = patch_file_hashes(&file);
        let fits = match &ids.module_hash {
            Some(h) => hashes.is_empty() || hashes.iter().any(|x| x.eq_ignore_ascii_case(h)),
            None => true,
        };
        let file_name = file.file_name().unwrap().to_string_lossy().to_string();
        let source = if file_name.contains("(Flux mods)") { "your patches".to_string() } else { "community".to_string() };
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        for (i, (name, author, notes, enabled)) in parse_xenia_patches(&text).into_iter().enumerate() {
            out.push(Patch { key: format!("{file_name}#{i}"), name, author, notes, enabled, fits, source: source.clone() });
        }
    }
    out
}

/// Turn the `index`th `[[patch]]` of a patch file on or off, editing just its `is_enabled` line.
pub fn set_xenia_patch_text(text: &str, index: usize, on: bool) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut block = None;
    let mut seen = 0usize;
    let mut done = false;
    let mut header_line = None;
    for i in 0..lines.len() {
        let t = lines[i].trim().to_string();
        if t == "[[patch]]" {
            block = Some(seen);
            if seen == index {
                header_line = Some(i);
            }
            seen += 1;
            continue;
        }
        if t.starts_with("[[patch.") {
            // Sub-tables (the patch's writes) end the block's own keys.
            if block == Some(index) && !done {
                lines.insert(i, format!("    is_enabled = {on}"));
                done = true;
            }
            block = None;
            continue;
        }
        if block == Some(index) && !done && t.split('=').next().is_some_and(|k| k.trim() == "is_enabled") {
            let indent: String = lines[i].chars().take_while(|c| c.is_whitespace()).collect();
            let comment = lines[i].find('#').map(|c| format!(" {}", &lines[i][c..])).unwrap_or_default();
            lines[i] = format!("{indent}is_enabled = {on}{comment}");
            done = true;
        }
    }
    if !done && let Some(h) = header_line {
        lines.insert(h + 1, format!("    is_enabled = {on}"));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

pub fn set_xenia_patch(dir: &Path, key: &str, on: bool) -> Result<(), String> {
    let (file, index) = key.rsplit_once('#').ok_or("Unknown patch")?;
    let index: usize = index.parse().map_err(|_| "Unknown patch")?;
    let path = dir.join("patches").join(file);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Couldn't read {file}: {e}"))?;
    std::fs::write(&path, set_xenia_patch_text(&text, index, on)).map_err(|e| format!("Couldn't save {file}: {e}"))
}

/// Download the community patch files for this title into Xenia's patches folder, keeping which
/// patches were on.
pub async fn fetch_xenia_patches(dir: PathBuf, title_id: String) -> Result<usize, String> {
    let listing = crate::games::fetch("https://api.github.com/repos/xenia-canary/game-patches/contents/patches").await
        .map_err(|_| "Download stopped".to_string())?.map_err(|e| e.to_string())?;
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&listing).map_err(|_| "Unexpected answer from GitHub".to_string())?;
    let mut count = 0;
    std::fs::create_dir_all(dir.join("patches")).map_err(|e| e.to_string())?;
    for entry in entries {
        let (Some(name), Some(url)) = (entry["name"].as_str(), entry["download_url"].as_str()) else { continue };
        if !name.to_uppercase().starts_with(&title_id.to_uppercase()) {
            continue;
        }
        let bytes = crate::games::fetch(url).await.map_err(|_| "Download stopped".to_string())?.map_err(|e| e.to_string())?;
        let mut text = String::from_utf8_lossy(&bytes).to_string();
        let path = dir.join("patches").join(name);
        // Keep the player's choices across refreshes.
        let was_on: Vec<String> = std::fs::read_to_string(&path).map(|old| parse_xenia_patches(&old).into_iter().filter(|p| p.3).map(|p| p.0).collect()).unwrap_or_default();
        for (i, patch) in parse_xenia_patches(&text.clone()).iter().enumerate() {
            if was_on.contains(&patch.0) {
                text = set_xenia_patch_text(&text, i, true);
            }
        }
        std::fs::write(&path, text).map_err(|e| format!("Couldn't save {name}: {e}"))?;
        count += 1;
    }
    if count == 0 {
        return Err(format!("The community has no patches for title {title_id} yet"));
    }
    Ok(count)
}

// --- RPCS3

#[derive(Debug, Clone, PartialEq)]
pub struct Rpcs3Patch {
    pub hash: String,
    pub name: String,
    pub title: String,
    pub serial: String,
    pub versions: Vec<String>,
    pub author: String,
    pub notes: String,
}

/// Indent, key and value of a YAML block-mapping line ("  \"Key: x\": value").
fn yaml_line(line: &str) -> Option<(usize, String, String)> {
    let indent = line.len() - line.trim_start().len();
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') || t.starts_with("- ") || t == "-" {
        return None;
    }
    let (key, rest) = if let Some(q) = t.strip_prefix('"') {
        let mut end = None;
        let bytes = q.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 1,
                b'"' => {
                    end = Some(i);
                    break;
                },
                _ => {},
            }
            i += 1;
        }
        let end = end?;
        (unquote(&t[..end + 2]), q[end + 1..].trim_start().strip_prefix(':')?.to_string())
    } else if let Some(q) = t.strip_prefix('\'') {
        let end = q.find('\'')?;
        (q[..end].to_string(), q[end + 1..].trim_start().strip_prefix(':')?.to_string())
    } else {
        let colon = t.find(": ").or_else(|| t.ends_with(':').then(|| t.len() - 1))?;
        (t[..colon].trim().to_string(), t[colon + 1..].to_string())
    };
    let value = rest.trim();
    let value = if value.starts_with('"') || value.starts_with('\'') { value.to_string() } else { value.split(" #").next().unwrap_or("").trim().to_string() };
    Some((indent, key, value))
}

/// (title, serial, versions) from a `Games:` block's lines.
fn games_from(lines: &[&str]) -> Vec<(String, String, Vec<String>)> {
    let mut out = Vec::new();
    let mut title = String::new();
    for line in lines {
        let Some((_, key, value)) = yaml_line(line) else { continue };
        if value.is_empty() {
            title = key;
        } else if let Some(list) = value.strip_prefix('[') {
            let versions = list.trim_end_matches(']').split(',').map(|v| unquote(v)).filter(|v| !v.is_empty()).collect();
            out.push((title.clone(), key, versions));
        }
    }
    out
}

/// Every patch in an RPCS3 patch file made for `serial`. Patches listed for "All" games are left
/// out: they're SPU fixes named after the games they're for (Prince of Persia, WWE...), so for any
/// other game they'd only fill the list with other games' patches.
pub fn parse_rpcs3_patches(text: &str, serial: &str) -> Vec<Rpcs3Patch> {
    let lines: Vec<&str> = text.lines().collect();
    // Anchors: "  name: &name" followed by its deeper-indented lines.
    let mut anchors: HashMap<String, Vec<&str>> = HashMap::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some(pos) = lines[i].find(": &") {
            let name = lines[i][pos + 3..].split_whitespace().next().unwrap_or("").to_string();
            let indent = lines[i].len() - lines[i].trim_start().len();
            let inline = lines[i][pos + 3 + name.len()..].trim();
            let mut body: Vec<&str> = Vec::new();
            if !inline.is_empty() {
                body.push(inline);
            }
            let mut j = i + 1;
            while j < lines.len() && (lines[j].trim().is_empty() || lines[j].len() - lines[j].trim_start().len() > indent) {
                body.push(lines[j]);
                j += 1;
            }
            anchors.insert(name, body);
        }
        i += 1;
    }

    let mut out = Vec::new();
    let mut hash = String::new();
    let mut i = 0;
    while i < lines.len() {
        let Some((indent, key, _)) = yaml_line(lines[i]) else {
            i += 1;
            continue;
        };
        if indent == 0 {
            hash = if key.starts_with("PPU-") || key.starts_with("SPU-") { key } else { String::new() };
            i += 1;
            continue;
        }
        if hash.is_empty() || indent != 2 {
            i += 1;
            continue;
        }
        // A patch: its fields are the deeper lines up to the next indent-2 (or 0) line.
        let name = key;
        let mut j = i + 1;
        while j < lines.len() && (lines[j].trim().is_empty() || lines[j].len() - lines[j].trim_start().len() > 2) {
            j += 1;
        }
        let body = &lines[i + 1..j];
        let (mut games, mut author, mut notes) = (Vec::new(), String::new(), String::new());
        let mut k = 0;
        while k < body.len() {
            if let Some((4, field, value)) = yaml_line(body[k]) {
                let mut end = k + 1;
                while end < body.len() && (body[end].trim().is_empty() || body[end].len() - body[end].trim_start().len() > 4) {
                    end += 1;
                }
                let resolve = |value: &str, block: &[&str]| -> Vec<String> {
                    match value.strip_prefix('*') {
                        Some(alias) => anchors.get(alias.trim()).map(|b| b.iter().map(|s| s.to_string()).collect()).unwrap_or_default(),
                        None if value.is_empty() => block.iter().map(|s| s.to_string()).collect(),
                        None => vec![value.to_string()],
                    }
                };
                match field.as_str() {
                    "Games" => {
                        let block = resolve(&value, &body[k + 1..end]);
                        games = games_from(&block.iter().map(String::as_str).collect::<Vec<_>>());
                    },
                    "Author" => author = unquote(&value),
                    "Notes" => notes = resolve(&value, &body[k + 1..end]).iter().map(|l| unquote(l)).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" "),
                    _ => {},
                }
                k = end;
            } else {
                k += 1;
            }
        }
        for (title, game_serial, versions) in games {
            if game_serial == serial {
                out.push(Rpcs3Patch { hash: hash.clone(), name: name.clone(), title, serial: game_serial, versions, author: author.clone(), notes: notes.clone() });
            }
        }
        i = j;
    }
    out
}

/// A tiny YAML block-mapping tree, enough for RPCS3's patch_config.yml.
#[derive(Debug, Clone, PartialEq)]
pub enum Yaml {
    Map(Vec<(String, Yaml)>),
    Val(String),
}

impl Yaml {
    pub fn parse(text: &str) -> Yaml {
        let items: Vec<(usize, String, String)> = text.lines().filter(|l| l.trim() != "---").filter_map(yaml_line).collect();
        fn build(items: &[(usize, String, String)], at: &mut usize, indent: usize) -> Yaml {
            let mut map = Vec::new();
            while *at < items.len() && items[*at].0 == indent {
                let (_, key, value) = items[*at].clone();
                *at += 1;
                if value.is_empty() && *at < items.len() && items[*at].0 > indent {
                    let child_indent = items[*at].0;
                    map.push((key, build(items, at, child_indent)));
                } else {
                    map.push((key, Yaml::Val(value)));
                }
            }
            Yaml::Map(map)
        }
        let mut at = 0;
        let first = items.first().map(|i| i.0).unwrap_or(0);
        build(&items, &mut at, first)
    }

    fn quote(key: &str) -> String {
        let plain = !key.is_empty()
            && key.chars().all(|c| c.is_ascii_alphanumeric() || " _.-()".contains(c))
            && !key.starts_with(' ') && !key.ends_with(' ');
        if plain { key.to_string() } else { format!("\"{}\"", key.replace('\\', "\\\\").replace('"', "\\\"")) }
    }

    pub fn write(&self) -> String {
        fn go(node: &Yaml, indent: usize, out: &mut String) {
            if let Yaml::Map(items) = node {
                for (key, value) in items {
                    out.push_str(&" ".repeat(indent));
                    out.push_str(&Yaml::quote(key));
                    match value {
                        Yaml::Val(v) => {
                            out.push_str(": ");
                            out.push_str(v);
                            out.push('\n');
                        },
                        Yaml::Map(_) => {
                            out.push_str(":\n");
                            go(value, indent + 2, out);
                        },
                    }
                }
            }
        }
        let mut out = String::new();
        go(self, 0, &mut out);
        out
    }

    pub fn get(&self, path: &[&str]) -> Option<&Yaml> {
        let mut node = self;
        for key in path {
            let Yaml::Map(items) = node else { return None };
            node = &items.iter().find(|(k, _)| k == key)?.1;
        }
        Some(node)
    }

    /// Set `path` to a value, creating maps on the way; None removes it (and maps left empty).
    pub fn set(&mut self, path: &[&str], value: Option<&str>) {
        let Yaml::Map(items) = self else { return };
        let Some((first, rest)) = path.split_first() else { return };
        if rest.is_empty() {
            items.retain(|(k, _)| k != first);
            if let Some(v) = value {
                items.push((first.to_string(), Yaml::Val(v.to_string())));
            }
            return;
        }
        if !items.iter().any(|(k, v)| k == first && matches!(v, Yaml::Map(_))) {
            if value.is_none() {
                return;
            }
            items.retain(|(k, _)| k != first);
            items.push((first.to_string(), Yaml::Map(Vec::new())));
        }
        let child = &mut items.iter_mut().find(|(k, _)| k == first).unwrap().1;
        child.set(rest, value);
        if matches!(child, Yaml::Map(m) if m.is_empty()) {
            items.retain(|(k, _)| k != first);
        }
    }
}

fn rpcs3_patch_texts(base: &Path, serial: &str) -> Vec<(String, String)> {
    let dir = base.join("patches");
    let mut out = Vec::new();
    for (file, source) in [("patch.yml".to_string(), "community"), ("imported_patch.yml".to_string(), "your patches"), (format!("{serial}_patch.yml"), "your patches")] {
        if let Ok(text) = std::fs::read_to_string(dir.join(&file)) {
            out.push((text, source.to_string()));
        }
    }
    out
}

fn rpcs3_config_path(base: &Path) -> PathBuf {
    base.join("config").join("patch_config.yml")
}

/// The version key RPCS3 stores a patch under for this game version (None: doesn't fit).
fn rpcs3_version_key(patch: &Rpcs3Patch, version: Option<&str>) -> Option<String> {
    if let Some(v) = version && patch.versions.iter().any(|x| x == v) {
        return Some(v.to_string());
    }
    patch.versions.iter().any(|x| x == "All").then(|| "All".to_string())
}

pub fn rpcs3_patches(base: &Path, ids: &GameIds) -> Vec<Patch> {
    let Some(serial) = &ids.serial else { return vec![] };
    let config = Yaml::parse(&std::fs::read_to_string(rpcs3_config_path(base)).unwrap_or_default());
    let mut out = Vec::new();
    for (text, source) in rpcs3_patch_texts(base, serial) {
        for p in parse_rpcs3_patches(&text, serial) {
            let version = rpcs3_version_key(&p, ids.app_version.as_deref());
            let fits = version.is_some();
            let version = version.unwrap_or_else(|| p.versions.first().cloned().unwrap_or_else(|| "All".into()));
            let enabled = config.get(&[&p.hash, &p.name, &p.title, &p.serial, &version, "Enabled"]).is_some_and(|v| v == &Yaml::Val("true".into()));
            let mut notes = p.notes.clone();
            if !fits {
                notes = format!("Made for version {} (you have {}). {notes}", p.versions.join(", "), ids.app_version.as_deref().unwrap_or("?"));
            }
            out.push(Patch {
                key: [p.hash.as_str(), &p.name, &p.title, &p.serial, &version].join("\u{1}"),
                name: p.name,
                author: p.author,
                notes,
                enabled,
                fits,
                source: source.clone(),
            });
        }
    }
    out
}

pub fn set_rpcs3_patch(base: &Path, key: &str, on: bool) -> Result<(), String> {
    let path = rpcs3_config_path(base);
    let mut config = Yaml::parse(&std::fs::read_to_string(&path).unwrap_or_default());
    let mut parts: Vec<&str> = key.split('\u{1}').collect();
    parts.push("Enabled");
    config.set(&parts, on.then_some("true"));
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, config.write()).map_err(|e| format!("Couldn't save RPCS3's patch settings: {e}"))
}

/// RPCS3's own "Download latest patches": the community patch.yml from rpcs3.net.
pub async fn fetch_rpcs3_patches(base: PathBuf) -> Result<usize, String> {
    let bytes = crate::games::fetch("https://rpcs3.net/compatibility?patch&api=v1&v=1.2").await
        .map_err(|_| "Download stopped".to_string())?.map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| "Unexpected answer from rpcs3.net".to_string())?;
    let text = json["patch"].as_str().ok_or("rpcs3.net sent no patches")?;
    std::fs::create_dir_all(base.join("patches")).map_err(|e| e.to_string())?;
    std::fs::write(base.join("patches").join("patch.yml"), text).map_err(|e| format!("Couldn't save the patches: {e}"))?;
    Ok(text.lines().filter(|l| l.starts_with("PPU-") || l.starts_with("SPU-")).count())
}

/// Add the player's own patch file (Xenia .patch.toml, RPCS3 .yml) to the emulator.
pub fn add_patch_file(game: &LibraryGame, file: &Path, cx: &App) -> Result<String, String> {
    let ids = ids(game, cx);
    let name = file.file_name().ok_or("No file")?.to_string_lossy().to_string();
    match game.kind {
        GameKind::Xbox360 => {
            let dir = xenia_dir(cx).ok_or("Install Xenia first")?;
            let tid = ids.title_id.ok_or("Flux couldn't read this game's title ID")?;
            let text = std::fs::read_to_string(file).map_err(|e| format!("Couldn't read {name}: {e}"))?;
            if !text.contains("[[patch]]") {
                return Err(format!("{name} isn't a Xenia patch file (.patch.toml)"));
            }
            // Xenia only reads files named after the title ID.
            let stem = name.trim_end_matches(".toml").trim_end_matches(".patch");
            let out = dir.join("patches").join(format!("{tid} - {stem} (Flux mods).patch.toml"));
            std::fs::create_dir_all(out.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&out, text).map_err(|e| e.to_string())?;
            Ok(format!("Added {name}"))
        },
        GameKind::Ps3 => {
            let base = rpcs3_dir(cx).ok_or("Install RPCS3 first")?;
            let serial = ids.serial.ok_or("Flux couldn't read this game's serial")?;
            let text = std::fs::read_to_string(file).map_err(|e| format!("Couldn't read {name}: {e}"))?;
            if parse_rpcs3_patches(&text, &serial).is_empty() {
                return Err(format!("{name} has no RPCS3 patches for {serial}"));
            }
            // RPCS3 reads <serial>_patch.yml next to its community file.
            let out = base.join("patches").join(format!("{serial}_patch.yml"));
            std::fs::create_dir_all(out.parent().unwrap()).map_err(|e| e.to_string())?;
            let mut combined = std::fs::read_to_string(&out).unwrap_or_default();
            if combined.is_empty() {
                combined = text;
            } else {
                combined.push('\n');
                combined.push_str(text.lines().filter(|l| !l.starts_with("Version:")).collect::<Vec<_>>().join("\n").as_str());
            }
            std::fs::write(&out, combined).map_err(|e| e.to_string())?;
            Ok(format!("Added {name}"))
        },
        _ => Err("Patches work for Xbox 360 and PS3 games".into()),
    }
}

pub fn patches(game: &LibraryGame, cx: &App) -> Vec<Patch> {
    let ids = ids(game, cx);
    match game.kind {
        GameKind::Xbox360 => xenia_dir(cx).map(|d| xenia_patches(&d, &ids)).unwrap_or_default(),
        GameKind::Ps3 => rpcs3_dir(cx).map(|d| rpcs3_patches(&d, &ids)).unwrap_or_default(),
        _ => vec![],
    }
}

pub fn set_patch(game: &LibraryGame, key: &str, on: bool, cx: &App) -> Result<(), String> {
    match game.kind {
        GameKind::Xbox360 => set_xenia_patch(&xenia_dir(cx).ok_or("Install Xenia first")?, key, on),
        GameKind::Ps3 => set_rpcs3_patch(&rpcs3_dir(cx).ok_or("Install RPCS3 first")?, key, on),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------------- Xenia plugins

#[derive(Debug, Clone, PartialEq)]
pub struct Plugin {
    pub name: String,
    pub file: String,
    pub enabled: bool,
    /// Listed for this game's executable (plugins for another version don't load).
    pub fits: bool,
}

fn plugins_toml(dir: &Path, title_id: &str) -> PathBuf {
    dir.join("plugins").join(title_id.to_uppercase()).join("plugins.toml")
}

pub fn plugins(game: &LibraryGame, cx: &App) -> Vec<Plugin> {
    let ids = ids(game, cx);
    let (Some(dir), Some(tid)) = (xenia_dir(cx), ids.title_id) else { return vec![] };
    let text = std::fs::read_to_string(plugins_toml(&dir, &tid)).unwrap_or_default();
    let mut out: Vec<(Plugin, Vec<String>)> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t == "[[plugin]]" {
            out.push((Plugin { name: String::new(), file: String::new(), enabled: false, fits: true }, vec![]));
            continue;
        }
        let (Some(last), Some((key, value))) = (out.last_mut(), t.split_once('=')) else { continue };
        match key.trim() {
            "name" => last.0.name = unquote(value),
            "file" => last.0.file = unquote(value),
            "is_enabled" => last.0.enabled = value.split('#').next().unwrap_or("").trim() == "true",
            "hash" => last.1 = value.split('"').skip(1).step_by(2).map(|h| h.to_uppercase()).collect(),
            _ => {},
        }
    }
    out.into_iter().map(|(mut p, hashes)| {
        p.fits = ids.module_hash.as_ref().is_none_or(|h| hashes.iter().any(|x| x == h));
        p
    }).collect()
}

fn write_plugins(dir: &Path, tid: &str, title: &str, hash: &str, list: &[Plugin]) -> Result<(), String> {
    let mut text = format!("title_name = \"{}\"\ntitle_id = \"{tid}\"\n", title.replace('"', "'"));
    for p in list {
        text.push_str(&format!(
            "\n[[plugin]]\n    name = \"{}\"\n    file = \"{}\"\n    hash = \"{hash}\"\n    is_enabled = {}\n",
            p.name.replace('"', "'"), p.file.replace('"', "'"), p.enabled
        ));
    }
    let path = plugins_toml(dir, tid);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("Couldn't save the plugin list: {e}"))?;
    // Xenia ignores plugins unless this is on.
    if list.iter().any(|p| p.enabled) {
        crate::xenia_config::set_value("General", "allow_plugins", crate::xenia_config::Val::Bool(true))?;
    }
    Ok(())
}

fn plugin_context(game: &LibraryGame, cx: &App) -> Result<(PathBuf, String, String), String> {
    let ids = ids(game, cx);
    let dir = xenia_dir(cx).ok_or("Install Xenia first")?;
    let tid = ids.title_id.ok_or("Flux couldn't read this game's title ID")?;
    let hash = ids.module_hash.ok_or("Play the game once first: Xenia's log tells Flux which version of the game you have, and plugins are tied to it")?;
    Ok((dir, tid, hash))
}

pub fn add_plugin(game: &LibraryGame, file: &Path, cx: &App) -> Result<String, String> {
    let (dir, tid, hash) = plugin_context(game, cx)?;
    let name = file.file_name().ok_or("No file")?.to_string_lossy().to_string();
    if !disc::starts_with(file, b"XEX2") {
        return Err(format!("{name} isn't an Xbox 360 plugin (.xex)"));
    }
    let dest = dir.join("plugins").join(&tid).join(&name);
    std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::copy(file, &dest).map_err(|e| format!("Couldn't copy {name}: {e}"))?;
    let mut list = plugins(game, cx);
    list.retain(|p| p.file != name);
    list.push(Plugin { name: name.trim_end_matches(".xex").to_string(), file: name.clone(), enabled: true, fits: true });
    write_plugins(&dir, &tid, &game.name, &hash, &list)?;
    Ok(format!("Added {name}"))
}

pub fn set_plugin(game: &LibraryGame, file: &str, on: Option<bool>, cx: &App) -> Result<(), String> {
    let (dir, tid, hash) = plugin_context(game, cx)?;
    let mut list = plugins(game, cx);
    match on {
        Some(on) => list.iter_mut().filter(|p| p.file == file).for_each(|p| p.enabled = on),
        None => {
            list.retain(|p| p.file != file);
            _ = std::fs::remove_file(dir.join("plugins").join(&tid).join(file));
        },
    }
    write_plugins(&dir, &tid, &game.name, &hash, &list)
}

// ------------------------------------------------------------------------------------- file mods

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileMod {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// Game files the mod replaced or added (relative to the game's mod root), while enabled.
    #[serde(default)]
    pub installed: Vec<String>,
}

/// Where a game's files can be replaced: an unpacked Xbox 360 game's folder, or a PS3 game's USRDIR.
pub fn mod_root(game: &LibraryGame, cx: &App) -> Option<PathBuf> {
    // Skate 3's maps and DLC-slot mods go where its update and DLC live (dev_hdd0/game/<serial>),
    // which the game reads ahead of the disc.
    if game.kind == GameKind::Ps3
        && let (Some(serial), Some(base)) = (ids(game, cx).serial, rpcs3_dir(cx))
        && crate::rpcs3_config::is_skate3(&serial)
    {
        let hdd = base.join("dev_hdd0").join("game").join(&serial).join("USRDIR");
        if hdd.is_dir() {
            return Some(hdd);
        }
    }
    match game.kind {
        GameKind::Xbox360 => {
            let folder = if game.path.is_dir() { game.path.clone() } else { game.path.parent()?.to_path_buf() };
            disc::child(&folder, "default.xex").map(|_| folder)
        },
        GameKind::Ps3 => {
            let sfo = ps3_sfo(&game.path)?;
            let dir = sfo.parent()?;
            disc::child(dir, "USRDIR").filter(|p| p.is_dir())
        },
        _ => None,
    }
}

fn mods_store(game: &LibraryGame) -> Option<PathBuf> {
    Some(game.art_dir()?.join("mods"))
}

fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, base, out);
        } else if let Ok(rel) = path.strip_prefix(base) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// A mod folder's files relative to the game's mod root: mods packed with their PS3_GAME/USRDIR
/// (or USRDIR) folders are lined up with the game's own.
fn mod_files(store: &Path) -> (PathBuf, Vec<String>) {
    let mut root = store.to_path_buf();
    for inner in ["PS3_GAME/USRDIR", "USRDIR"] {
        if let Some(found) = inner.split('/').try_fold(store.to_path_buf(), |dir, part| disc::child(&dir, part)) && found.is_dir() {
            root = found;
            break;
        }
    }
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    files.sort();
    (root, files)
}

/// Copy a mod folder into Flux (switched off until the player turns it on).
pub fn add_file_mod(game: &LibraryGame, folder: &Path) -> Result<FileMod, String> {
    let store = mods_store(game).ok_or("Flux's data folder isn't set up")?;
    let archive = folder.is_file();
    let name = if archive { folder.file_stem() } else { folder.file_name() }.map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Mod".into());
    let id = crate::games::new_id(&name);
    let dest = store.join(&id);
    if archive {
        extract_archive(folder, &dest)?;
        // A zip holding one folder: use what's inside it.
        let entries: Vec<PathBuf> = std::fs::read_dir(&dest).into_iter().flatten().flatten().map(|e| e.path()).collect();
        if let [only] = entries.as_slice() && only.is_dir() && !["PS3_GAME", "USRDIR"].iter().any(|n| only.file_name().is_some_and(|f| f.eq_ignore_ascii_case(n))) {
            let tmp = store.join(format!("{id}.tmp"));
            std::fs::rename(only, &tmp).map_err(|e| e.to_string())?;
            _ = std::fs::remove_dir_all(&dest);
            std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
        }
    } else {
        copy_dir(folder, &dest)?;
    }
    let (_, files) = mod_files(&dest);
    if files.is_empty() {
        _ = std::fs::remove_dir_all(&dest);
        return Err(format!("{name} has no files"));
    }
    Ok(FileMod { id, name, enabled: false, installed: vec![] })
}

/// Unpack a .zip / .7z / .rar / .tar mod with Windows' own bsdtar.
fn extract_archive(archive: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let tar = std::env::var_os("SystemRoot").map(|r| PathBuf::from(r).join("System32").join("tar.exe")).filter(|p| p.is_file()).unwrap_or_else(|| PathBuf::from("tar"));
    let mut command = std::process::Command::new(tar);
    command.arg("-xf").arg(archive).arg("-C").arg(dest);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let out = command.output().map_err(|e| format!("Couldn't unpack the mod: {e}"))?;
    if !out.status.success() {
        _ = std::fs::remove_dir_all(dest);
        return Err(format!("Couldn't unpack {}: {}", archive.display(), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("Couldn't create {}: {e}", to.display()))?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("Couldn't read {}: {e}", from.display()))?.flatten() {
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else {
            std::fs::copy(&path, &target).map_err(|e| format!("Couldn't copy {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// Switch a file mod on (back up the game's files, copy the mod's in) or off (put them back).
pub fn set_file_mod(game: &LibraryGame, mods: &[FileMod], id: &str, on: bool, cx: &App) -> Result<Vec<String>, String> {
    let root = mod_root(game, cx).ok_or("This game's files can't be replaced yet (see File mods)")?;
    let store = mods_store(game).ok_or("Flux's data folder isn't set up")?.join(id);
    let backups = game.art_dir().ok_or("Flux's data folder isn't set up")?.join("mod-backups").join(id);
    let this = mods.iter().find(|m| m.id == id).ok_or("Unknown mod")?;
    if on {
        let (src_root, files) = mod_files(&store);
        // Two mods replacing the same file would undo each other.
        for other in mods.iter().filter(|m| m.enabled && m.id != id) {
            if let Some(clash) = files.iter().find(|f| other.installed.contains(f)) {
                return Err(format!("{} also changes {clash}: switch it off first", other.name));
            }
        }
        let mut installed = Vec::new();
        for rel in &files {
            let target = root.join(rel);
            let backup = backups.join(rel);
            if target.is_file() && !backup.exists() {
                std::fs::create_dir_all(backup.parent().unwrap()).map_err(|e| e.to_string())?;
                std::fs::copy(&target, &backup).map_err(|e| format!("Couldn't back up {rel}: {e}"))?;
            }
            std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::copy(src_root.join(rel), &target).map_err(|e| format!("Couldn't copy {rel}: {e}"))?;
            installed.push(rel.clone());
        }
        Ok(installed)
    } else {
        for rel in &this.installed {
            let target = root.join(rel);
            let backup = backups.join(rel);
            if backup.is_file() {
                std::fs::copy(&backup, &target).map_err(|e| format!("Couldn't restore {rel}: {e}"))?;
                _ = std::fs::remove_file(&backup);
            } else {
                _ = std::fs::remove_file(&target); // the mod added it
            }
        }
        _ = std::fs::remove_dir_all(&backups);
        Ok(vec![])
    }
}

pub fn remove_file_mod(game: &LibraryGame, id: &str) {
    if let Some(store) = mods_store(game) {
        _ = std::fs::remove_dir_all(store.join(id));
    }
}

// --------------------------------------------------------------------------------- disc unpacking

/// (bytes done, bytes total) of the disc being unpacked per game id, and the last error.
pub static UNPACKING: Mutex<Vec<(String, u64, u64)>> = Mutex::new(Vec::new());
pub static UNPACK_STOP: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Finished unpacks: (game id, the unpacked default.xex or why it stopped).
pub static UNPACK_DONE: Mutex<Vec<(String, Result<PathBuf, String>)>> = Mutex::new(Vec::new());

pub fn unpack_target(game: &LibraryGame) -> Option<PathBuf> {
    let stem = game.path.file_stem()?.to_string_lossy().to_string();
    Some(game.path.parent()?.join(format!("{stem} (unpacked)")))
}

pub fn disc_size(path: &Path) -> Option<u64> {
    let mut disc = disc::Xdvdfs::open(path)?;
    Some(disc.files().iter().map(|(_, e)| e.size as u64).sum())
}

pub fn free_space(dir: &Path) -> Option<u64> {
    #[cfg(windows)]
    {
        use windows::{Win32::Storage::FileSystem::GetDiskFreeSpaceExW, core::PCWSTR};
        let mut wide: Vec<u16> = dir.as_os_str().to_string_lossy().encode_utf16().collect();
        wide.push(0);
        let mut free = 0u64;
        unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), None, None) }.ok()?;
        return Some(free);
    }
    #[allow(unreachable_code)]
    None
}

/// Unpack an Xbox 360 disc image next to it (blocking; run in the background). The image itself
/// stays as it is.
pub fn unpack_disc(id: &str, iso: &Path, dest: &Path) -> Result<PathBuf, String> {
    let mut disc = disc::Xdvdfs::open(iso).ok_or("This isn't an Xbox disc image")?;
    let id = id.to_string();
    let result = disc.extract(dest, &mut |done, total| {
        if let Some(entry) = UNPACKING.lock().unwrap().iter_mut().find(|e| e.0 == id) {
            entry.1 = done;
            entry.2 = total;
        }
        !UNPACK_STOP.lock().unwrap().contains(&id)
    });
    UNPACK_STOP.lock().unwrap().retain(|i| *i != id);
    if let Err(e) = result {
        _ = std::fs::remove_dir_all(dest);
        return Err(e);
    }
    disc::child(dest, "default.xex").ok_or_else(|| "The unpacked game has no default.xex".to_string())
}

// ----------------------------------------------------------------------------------------- saves

#[derive(Debug, Clone, PartialEq)]
pub struct SaveSet {
    /// Folders holding this game's saves (one per Xenia profile / RPCS3 save slot).
    pub folders: Vec<PathBuf>,
    pub files: usize,
    pub bytes: u64,
    pub modified: Option<std::time::SystemTime>,
}

/// Where the game's saves live right now.
pub fn save_folders(game: &LibraryGame, cx: &App) -> Vec<PathBuf> {
    let ids = ids(game, cx);
    let mut out = Vec::new();
    match game.kind {
        GameKind::Xbox360 => {
            let (Some(dir), Some(tid)) = (xenia_dir(cx), ids.title_id) else { return out };
            // content/<profile XUID>/<TitleID>/00000001 holds saved games.
            for profile in std::fs::read_dir(dir.join("content")).into_iter().flatten().flatten() {
                if let Some(title) = disc::child(&profile.path(), &tid) && let Some(saves) = disc::child(&title, "00000001") {
                    out.push(saves);
                }
            }
        },
        GameKind::Ps3 => {
            let (Some(base), Some(serial)) = (rpcs3_dir(cx), ids.serial) else { return out };
            for user in std::fs::read_dir(base.join("dev_hdd0").join("home")).into_iter().flatten().flatten() {
                for save in std::fs::read_dir(user.path().join("savedata")).into_iter().flatten().flatten() {
                    if save.file_name().to_string_lossy().to_uppercase().starts_with(&serial.to_uppercase()) {
                        out.push(save.path());
                    }
                }
            }
        },
        _ => {},
    }
    out
}

pub fn save_summary(game: &LibraryGame, cx: &App) -> SaveSet {
    let folders = save_folders(game, cx);
    let (mut files, mut bytes, mut modified) = (0, 0, None);
    for folder in &folders {
        let mut all = Vec::new();
        walk(folder, folder, &mut all);
        for rel in all {
            if let Ok(meta) = std::fs::metadata(folder.join(&rel)) {
                files += 1;
                bytes += meta.len();
                let m = meta.modified().ok();
                if m > modified {
                    modified = m;
                }
            }
        }
    }
    SaveSet { folders, files, bytes, modified }
}

fn backups_dir(game: &LibraryGame) -> Option<PathBuf> {
    Some(game.art_dir()?.join("save-backups"))
}

/// Saved backups, newest first: (folder, label).
pub fn save_backups(game: &LibraryGame) -> Vec<(PathBuf, String)> {
    let mut out: Vec<(PathBuf, String)> = backups_dir(game).and_then(|d| std::fs::read_dir(d).ok()).into_iter().flatten().flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| (e.path(), e.file_name().to_string_lossy().to_string()))
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out
}

/// Copy the game's saves into Flux's backup folder. `reason` ends up in the backup's name.
/// Each save folder is kept by its path under the emulator's data folder, so restore knows where
/// it goes.
pub fn backup_saves(game: &LibraryGame, reason: &str, cx: &App) -> Result<Option<PathBuf>, String> {
    let folders = save_folders(game, cx);
    if folders.is_empty() {
        return Ok(None);
    }
    let root = match game.kind {
        GameKind::Xbox360 => xenia_dir(cx),
        _ => rpcs3_dir(cx),
    }.ok_or("Emulator not found")?;
    let stamp = chrono::Local::now().format("%Y-%m-%d %H.%M.%S");
    let dest = backups_dir(game).ok_or("Flux's data folder isn't set up")?.join(format!("{stamp} {reason}"));
    for folder in &folders {
        let rel = folder.strip_prefix(&root).map_err(|_| "Save folder outside the emulator")?;
        copy_dir(folder, &dest.join(rel))?;
    }
    // Keep the newest 15 automatic backups.
    let autos: Vec<PathBuf> = save_backups(game).into_iter().filter(|(_, l)| l.ends_with("before playing")).map(|(p, _)| p).collect();
    for old in autos.into_iter().skip(15) {
        _ = std::fs::remove_dir_all(old);
    }
    Ok(Some(dest))
}

/// Put a backup back (the current saves are backed up first).
pub fn restore_saves(game: &LibraryGame, backup: &Path, cx: &App) -> Result<(), String> {
    let root = match game.kind {
        GameKind::Xbox360 => xenia_dir(cx),
        _ => rpcs3_dir(cx),
    }.ok_or("Emulator not found")?;
    backup_saves(game, "before restoring", cx)?;
    let mut rels = Vec::new();
    walk(backup, backup, &mut rels);
    for rel in rels {
        let target = root.join(&rel);
        std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::copy(backup.join(&rel), &target).map_err(|e| format!("Couldn't restore {rel}: {e}"))?;
    }
    Ok(())
}

/// Import someone else's save (a folder of save files) into the game's save folder.
pub fn import_save(game: &LibraryGame, folder: &Path, cx: &App) -> Result<String, String> {
    backup_saves(game, "before importing", cx)?;
    let target = match game.kind {
        GameKind::Xbox360 => {
            let folders = save_folders(game, cx);
            match folders.first() {
                Some(f) => f.clone(),
                None => return Err("Play the game and save once first (signed in to a Xenia profile), so Flux knows which profile's folder to use".into()),
            }
        },
        GameKind::Ps3 => {
            let base = rpcs3_dir(cx).ok_or("Install RPCS3 first")?;
            let name = folder.file_name().ok_or("No folder")?;
            base.join("dev_hdd0").join("home").join("00000001").join("savedata").join(name)
        },
        _ => return Err("Saves work for Xbox 360 and PS3 games".into()),
    };
    copy_dir(folder, &target)?;
    Ok(format!("Imported into {}", target.display()))
}

// ------------------------------------------------------------------------------ Skate 3 on RPCS3

/// The Skate 3 Native Menu's program: the one the player chose, else S3Native.exe in a
/// "Skate_3_Native_Menu" folder in Downloads or on the Desktop (where its zip unpacks).
pub fn native_menu_exe(cx: &App) -> Option<PathBuf> {
    if let Some(exe) = crate::interface_config::InterfaceConfig::get(cx).skate3_native_menu.clone() && exe.is_file() {
        return Some(exe);
    }
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from)?;
    for place in ["Downloads", "Desktop"] {
        let dir = home.join(place);
        let direct = dir.join("Skate_3_Native_Menu").join("S3Native.exe");
        if direct.is_file() {
            return Some(direct);
        }
        // A folder named after the menu (another version's zip name), one level down.
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let exe = entry.path().join("S3Native.exe");
            if entry.path().is_dir() && name.contains("native") && name.contains("menu") && exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

/// Where the menu's author shares it: the EA Skate Modding Discord (the invite inside S3Native.exe).
/// There's no public download page, so Flux opens this and installs the file the player gets there.
pub const NATIVE_MENU_SOURCE: &str = "https://discord.gg/b46F5PQysY";

/// Install the Native Menu from the archive (.zip / .7z / .rar) or folder the player downloaded:
/// unpacked into Flux's data folder, its S3Native.exe remembered and switched on.
pub fn install_native_menu(from: &Path, cx: &mut App) -> Result<String, String> {
    let dest = crate::LAUNCHER_DIR.get().ok_or("Flux's data folder isn't set up")?.join("skate3-native-menu");
    let stage = dest.with_extension("new");
    _ = std::fs::remove_dir_all(&stage);
    if from.is_dir() {
        copy_dir(from, &stage)?;
    } else {
        extract_archive(from, &stage)?;
    }
    fn find(dir: &Path, depth: usize) -> Option<PathBuf> {
        let entries: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();
        entries.iter().find(|p| p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("S3Native.exe"))).cloned()
            .or_else(|| (depth < 4).then(|| entries.iter().filter(|p| p.is_dir()).find_map(|d| find(d, depth + 1))).flatten())
    }
    let Some(found) = find(&stage, 0) else {
        _ = std::fs::remove_dir_all(&stage);
        return Err("That download has no S3Native.exe: pick the Native Menu's .zip / .7z (or its folder).".into());
    };
    // Keep the player's configs and challenges from an older copy.
    let rel = found.strip_prefix(&stage).map_err(|_| "Unexpected archive layout")?.to_path_buf();
    if let Some(old_exe) = native_menu_exe(cx).filter(|e| e.starts_with(&dest)) && let Some(old_dir) = old_exe.parent() {
        let new_dir = found.parent().unwrap_or(&stage).to_path_buf();
        for keep in ["configs", "custom_challenges"] {
            if old_dir.join(keep).is_dir() {
                _ = copy_dir(&old_dir.join(keep), &new_dir.join(keep));
            }
        }
    }
    _ = std::fs::remove_dir_all(&dest);
    std::fs::rename(&stage, &dest).map_err(|e| format!("Couldn't install the Native Menu: {e}"))?;
    let exe = dest.join(rel);
    let config = crate::interface_config::InterfaceConfig::get_mut(cx);
    config.skate3_native_menu = Some(exe);
    config.skate3_native_menu_on = true;
    Ok("Native Menu installed: it starts with Skate 3 (run it once by itself to let it install its 1.05 game update)".into())
}

/// Start the Native Menu beside RPCS3, from its own folder (it keeps configs and logs there). It
/// attaches to RPCS3 itself once Skate 3 has loaded.
pub fn start_native_menu(exe: &Path) -> Result<(), String> {
    let dir = exe.parent().ok_or("The Native Menu has no folder")?;
    std::process::Command::new(exe).current_dir(dir).spawn()
        .map(|_| ()).map_err(|e| format!("Couldn't start the Native Menu: {e}"))
}

/// Skate 3's skater save folder name for a serial: "BLUS30464-ALIAS_SKATER".
pub fn skater_folder_name(serial: &str) -> String {
    format!("{serial}-ALIAS_SKATER")
}

/// The skater save RPCS3 uses now.
pub fn current_skater(base: &Path, serial: &str) -> PathBuf {
    base.join("dev_hdd0").join("home").join("00000001").join("savedata").join(skater_folder_name(serial))
}

fn skater_store(game: &LibraryGame) -> Option<PathBuf> {
    Some(game.art_dir()?.join("skaters"))
}

/// Skaters kept in Flux for this game, newest first: (folder, name).
pub fn saved_skaters(game: &LibraryGame) -> Vec<(PathBuf, String)> {
    let mut out: Vec<(PathBuf, String, Option<std::time::SystemTime>)> = skater_store(game)
        .and_then(|d| std::fs::read_dir(d).ok()).into_iter().flatten().flatten()
        .filter(|e| e.path().join("SKATER.P").is_file())
        .map(|e| (e.path(), e.file_name().to_string_lossy().to_string(), e.metadata().and_then(|m| m.modified()).ok()))
        .collect();
    out.sort_by(|a, b| b.2.cmp(&a.2));
    out.into_iter().map(|(p, n, _)| (p, n)).collect()
}

/// The skater save inside a folder the player picked: the folder itself, or the
/// "<serial>-ALIAS_SKATER" folder somewhere inside it (saves are often shared as
/// "BLUS/PS3/SAVEDATA/BLUS30464-ALIAS_SKATER", with an EU copy beside it).
pub fn find_skater(folder: &Path, serial: &str) -> Result<PathBuf, String> {
    let wanted = skater_folder_name(serial).to_uppercase();
    let name_of = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_uppercase()).unwrap_or_default();
    if folder.join("SKATER.P").is_file() && (name_of(folder) == wanted || !name_of(folder).ends_with("-ALIAS_SKATER")) {
        return Ok(folder.to_path_buf());
    }
    let mut other_region = None;
    let mut stack = vec![(folder.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = name_of(&path);
            if name == wanted && path.join("SKATER.P").is_file() {
                return Ok(path);
            }
            if name.ends_with("-ALIAS_SKATER") && path.join("SKATER.P").is_file() {
                other_region = Some(name);
            }
            if depth < 6 {
                stack.push((path, depth + 1));
            }
        }
    }
    Err(match other_region {
        Some(name) => format!("That folder has a skater for another version of the game ({name}); your Skate 3 is {serial}, so it needs a {wanted} folder."),
        None => format!("No Skate 3 skater save ({wanted}, with a SKATER.P inside) in that folder."),
    })
}

/// Put a kept skater into RPCS3's save folder (the current saves are backed up first).
pub fn use_skater(game: &LibraryGame, skater: &Path, cx: &App) -> Result<String, String> {
    let base = rpcs3_dir(cx).ok_or("Install RPCS3 first")?;
    let serial = ids(game, cx).serial.ok_or("Flux couldn't read this game's serial")?;
    backup_saves(game, "before changing skater", cx)?;
    let target = current_skater(&base, &serial);
    if target.exists() {
        std::fs::remove_dir_all(&target).map_err(|e| format!("Couldn't replace the current skater: {e}"))?;
    }
    copy_dir(skater, &target)?;
    let name = skater.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(format!("Skater \"{name}\" is in place: start Skate 3 to use it (your previous saves were backed up first)"))
}

/// Keep the skater from a picked folder in Flux, then use it.
pub fn import_skater(game: &LibraryGame, folder: &Path, cx: &App) -> Result<String, String> {
    let serial = ids(game, cx).serial.ok_or("Flux couldn't read this game's serial")?;
    let found = find_skater(folder, &serial)?;
    let store = skater_store(game).ok_or("Flux's data folder isn't set up")?;
    // Named after the folder the player picked ("PS3_Skate_3_Black_Box_100_Percent_Save").
    let base_name: String = folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Skater".into())
        .chars().map(|c| if c.is_alphanumeric() || " -_().".contains(c) { c } else { '_' }).collect();
    let mut dest = store.join(&base_name);
    let mut n = 2;
    while dest.exists() {
        dest = store.join(format!("{base_name} ({n})"));
        n += 1;
    }
    copy_dir(&found, &dest)?;
    use_skater(game, &dest, cx)
}

/// Keep the skater RPCS3 has now, so it can be switched back to later.
pub fn save_current_skater(game: &LibraryGame, cx: &App) -> Result<String, String> {
    let base = rpcs3_dir(cx).ok_or("Install RPCS3 first")?;
    let serial = ids(game, cx).serial.ok_or("Flux couldn't read this game's serial")?;
    let current = current_skater(&base, &serial);
    if !current.join("SKATER.P").is_file() {
        return Err("There's no skater save yet: play Skate 3 and save once first".into());
    }
    let store = skater_store(game).ok_or("Flux's data folder isn't set up")?;
    let name = format!("My skater {}", chrono::Local::now().format("%Y-%m-%d %H.%M"));
    copy_dir(&current, &store.join(&name))?;
    Ok(format!("Kept your current skater as \"{name}\""))
}

pub fn delete_saved_skater(skater: &Path) -> Result<String, String> {
    std::fs::remove_dir_all(skater).map_err(|e| format!("Couldn't delete it: {e}"))?;
    Ok("Skater removed from the list (RPCS3's current save wasn't touched)".into())
}

// ----------------------------------------------------------------------------------------- debug

/// The emulator's log file for this game.
pub fn log_path(game: &LibraryGame, cx: &App) -> Option<PathBuf> {
    match game.kind {
        GameKind::Xbox360 => Some(xenia_dir(cx)?.join("xenia.log")),
        GameKind::Ps3 => Some(rpcs3_dir(cx)?.join("log").join("RPCS3.log")).filter(|p| p.is_file())
            .or_else(|| Some(rpcs3_dir(cx)?.join("RPCS3.log"))),
        _ => None,
    }
}

/// The last `n` lines of the log.
pub fn log_tail(path: &Path, n: usize) -> Vec<String> {
    let Ok(bytes) = std::fs::read(path) else { return vec![] };
    let start = bytes.len().saturating_sub(256 * 1024);
    let text = String::from_utf8_lossy(&bytes[start..]);
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].iter().map(|l| l.chars().take(240).collect()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const XENIA: &str = r#"title_name = "Skate 3"
title_id = "454108E6" # EA-2278
hash = "5E967D21BA33F7F5" # default.xex

[[patch]]
    name = "No Fall Damage"
    desc = "Only works while you're on your board."
    author = "illusion, Aphelion"
    is_enabled = false

    [[patch.be32]]
        address = 0x82d65eb0
        value = 0xc80b13fc

[[patch]]
    name = "60 FPS"
    author = "someone"

    [[patch.be32]]
        address = 0x1
        value = 0x2
"#;

    #[test]
    fn xenia_patch_files_parse_and_toggle() {
        let patches = parse_xenia_patches(XENIA);
        assert_eq!(patches.len(), 2);
        assert_eq!(patches[0], ("No Fall Damage".into(), "illusion, Aphelion".into(), "Only works while you're on your board.".into(), false));
        let on = set_xenia_patch_text(XENIA, 0, true);
        assert!(parse_xenia_patches(&on)[0].3);
        assert!(on.contains("address = 0x82d65eb0"), "patch writes untouched");
        // A patch without an is_enabled line gets one before its writes.
        let second = set_xenia_patch_text(&on, 1, true);
        let parsed = parse_xenia_patches(&second);
        assert!(parsed[0].3 && parsed[1].3);
        let off = set_xenia_patch_text(&second, 0, false);
        assert!(!parse_xenia_patches(&off)[0].3);
        let dir = std::env::temp_dir().join("flux-xenia-patch-test");
        _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("t.patch.toml"), XENIA).unwrap();
        assert_eq!(patch_file_hashes(&dir.join("t.patch.toml")), ["5E967D21BA33F7F5"]);
    }

    #[test]
    fn module_hash_from_xenia_log() {
        let dir = std::env::temp_dir().join("flux-xenia-log-test");
        _ = std::fs::create_dir_all(&dir);
        let log = dir.join("xenia.log");
        // Skate 3's real log: default.xex, then its EAWebkit.xex add-on with the same title ID.
        std::fs::write(&log, concat!(
            "i> 0000FAF0 Module \\Device\\Cdrom0\\default.xex:\nModule Hash: 5e967d21ba33f7f5\n    Module Flags: 00000001\n       Title ID: 454108E6\n",
            "i> F800003C Module \\Device\\Cdrom0\\data\\webkit\\EAWebkit.xex:\nModule Hash: AA75C188DE7BCA73\n       Title ID: 454108E6\n",
        )).unwrap();
        assert_eq!(hash_from_log(&log, "454108E6").as_deref(), Some("5E967D21BA33F7F5"));
        assert_eq!(hash_from_log(&log, "4D5307E6"), None);
    }

    const RPCS3: &str = r#"Version: 1.2

Anchors:
  des_US100title: &des_US100title
    "Demon's Souls":
      BLUS30443: [ 01.00 ]

  DeS_FPS_Notes: &DeS_FPS_Notes
    "Clocks scale must be default."

PPU-83681f6110d33442329073b72b8dc88a2f677172:
  "Unlock FPS":
    Games: *des_US100title
    Author: "Whatcookie, Gibbed"
    Notes: *DeS_FPS_Notes
    Patch Version: 2.1
    Patch:
      - [ load, *BLUS30443_FpsUnlock ]
  "Aspect ratio (HUD/Menus) 32:9":
    Games:
      "Demon's Souls":
        BLUS30443: [01.00, 01.04]
        BLES00932: [ 01.00 ]
    Author: "Pierre"
    Notes: "Needs \"Aspect Ratio\" on."
    Patch:
      - [be32, 0x018F0174, 0x44F00000]

PPU-c00f2b02e71f7e9b04683958bda712d66af4be15:
  "No Blur":
    Games:
      "Skate":
        BLUS30059: [ 01.01 ]
    Author: "tuukkas"
    Patch:
      - [ be32, 0x006A0B60, 0x48959DA1 ]
  "Everything":
    Games:
      All:
        All: [ All ]
    Patch:
      - [ be32, 0x0, 0x0 ]
"#;

    #[test]
    fn rpcs3_patch_list_for_one_serial() {
        let des = parse_rpcs3_patches(RPCS3, "BLUS30443");
        let names: Vec<&str> = des.iter().map(|p| p.name.as_str()).collect();
        // "All" games patches are other games' SPU fixes: only this game's own are listed.
        assert_eq!(names, ["Unlock FPS", "Aspect ratio (HUD/Menus) 32:9"]);
        assert_eq!(des[0].title, "Demon's Souls");
        assert_eq!(des[0].versions, ["01.00"]);
        assert_eq!(des[0].notes, "Clocks scale must be default.");
        assert_eq!(des[1].versions, ["01.00", "01.04"]);
        assert_eq!(des[1].notes, "Needs \"Aspect Ratio\" on.");
        assert_eq!(des[0].hash, "PPU-83681f6110d33442329073b72b8dc88a2f677172");
        let skate = parse_rpcs3_patches(RPCS3, "BLUS30059");
        assert_eq!(skate.len(), 1);
        assert_eq!(skate[0].author, "tuukkas");
        assert_eq!(rpcs3_version_key(&skate[0], Some("01.01")).as_deref(), Some("01.01"));
        assert_eq!(rpcs3_version_key(&skate[0], Some("01.02")), None);
        let any_version = Rpcs3Patch { versions: vec!["All".into()], ..skate[0].clone() };
        assert_eq!(rpcs3_version_key(&any_version, Some("01.02")).as_deref(), Some("All"));
    }

    #[test]
    fn rpcs3_patch_config_round_trip() {
        let existing = "PPU-aaa:\n  Other patch:\n    Some Game:\n      BLUS00001:\n        01.00:\n          Enabled: true\n";
        let mut config = Yaml::parse(existing);
        config.set(&["PPU-c00f", "No Blur", "Skate", "BLUS30059", "01.01", "Enabled"], Some("true"));
        let text = config.write();
        let back = Yaml::parse(&text);
        assert_eq!(back.get(&["PPU-c00f", "No Blur", "Skate", "BLUS30059", "01.01", "Enabled"]), Some(&Yaml::Val("true".into())));
        assert_eq!(back.get(&["PPU-aaa", "Other patch", "Some Game", "BLUS00001", "01.00", "Enabled"]), Some(&Yaml::Val("true".into())), "other patches kept");
        let mut off = back.clone();
        off.set(&["PPU-c00f", "No Blur", "Skate", "BLUS30059", "01.01", "Enabled"], None);
        assert_eq!(off.get(&["PPU-c00f"]), None, "empty maps pruned");
        // Keys with quotes and colons survive.
        let mut q = Yaml::Map(vec![]);
        q.set(&["PPU-1", "Aspect ratio: 21:9", "Demon's \"Souls\"", "BLUS30443", "01.00", "Enabled"], Some("true"));
        assert_eq!(Yaml::parse(&q.write()), q);
    }

    /// FLUX_RPCS3_PATCH_TEST=<patch.yml>: parse the real community file.
    #[test]
    fn real_rpcs3_patch_file() {
        let Ok(path) = std::env::var("FLUX_RPCS3_PATCH_TEST") else { return };
        let text = std::fs::read_to_string(path).unwrap();
        for serial in ["BLUS30443", "BLUS30059", "BLUS30464", "BLES00760"] {
            let found = parse_rpcs3_patches(&text, serial);
            println!("{serial}: {} patches", found.len());
            for p in found.iter().take(6) {
                println!("  {} | {} | {:?} | {}", p.name, p.title, p.versions, p.author);
            }
        }
    }
}

#[cfg(test)]
mod ps3_iso_tests {
    /// FLUX_PS3_ISO=<path to a PS3 .iso>: the serial and version come from inside the image.
    #[test]
    fn reads_param_sfo_from_an_iso() {
        let Some(path) = std::env::var_os("FLUX_PS3_ISO") else { return };
        let sfo = crate::disc::Iso::open(std::path::Path::new(&path)).unwrap().read_file("PS3_GAME/PARAM.SFO").unwrap();
        let serial = crate::disc::sfo_value(&sfo, "TITLE_ID").unwrap();
        println!("{serial} {:?} {:?}", crate::disc::sfo_value(&sfo, "APP_VER"), crate::disc::sfo_value(&sfo, "TITLE"));
        assert_eq!(serial.len(), 9);
    }
}

#[cfg(test)]
mod rpcs3_patch_list_tests {
    /// FLUX_PATCH_YML=<RPCS3 patch.yml>: what the Mods tab lists for Skate 3 (US).
    #[test]
    fn skate3_patches() {
        let Some(path) = std::env::var_os("FLUX_PATCH_YML") else { return };
        let text = std::fs::read_to_string(path).unwrap();
        let patches = super::parse_rpcs3_patches(&text, "BLUS30464");
        for p in &patches {
            println!("PATCH {} | {} | {} | {:?}", p.name, p.title, p.serial, p.versions);
        }
        assert!(!patches.is_empty() && patches.iter().all(|p| p.title == "Skate 3" && p.serial == "BLUS30464"));
    }
}

#[cfg(test)]
mod skater_tests {
    use super::find_skater;

    #[test]
    fn finds_the_right_region_and_says_why_not() {
        let root = std::env::temp_dir().join("flux-skater-test");
        _ = std::fs::remove_dir_all(&root);
        for serial in ["BLUS30464", "BLES00760"] {
            let dir = root.join(&serial[..4]).join("PS3").join("SAVEDATA").join(format!("{serial}-ALIAS_SKATER"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("SKATER.P"), b"x").unwrap();
        }
        assert!(find_skater(&root, "BLUS30464").unwrap().ends_with("BLUS30464-ALIAS_SKATER"));
        assert!(find_skater(&root, "BLES00760").unwrap().ends_with("BLES00760-ALIAS_SKATER"));
        // Picking the save folder itself works too.
        let us = root.join("BLUS").join("PS3").join("SAVEDATA").join("BLUS30464-ALIAS_SKATER");
        assert_eq!(find_skater(&us, "BLUS30464").unwrap(), us);
        // A Japanese copy finds only other regions' skaters: say so.
        assert!(find_skater(&root, "BLJM60296").unwrap_err().contains("another version"));
    }

    /// FLUX_SKATER_DIR=<a downloaded skater save>: which folder Flux would import for the US game.
    #[test]
    fn real_skater_folder() {
        let Some(dir) = std::env::var_os("FLUX_SKATER_DIR") else { return };
        println!("FOUND {:?}", find_skater(std::path::Path::new(&dir), "BLUS30464"));
    }
}
