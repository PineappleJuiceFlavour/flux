//! SK8V (github.com/becaring/SK8V): Skate 3's skating in GTA V Legacy Story Mode, a fan mod by
//! becaring. This side finds what it needs (the player's Skate 3 for Xbox 360 in the library, GTA V
//! Legacy and its version, ScriptHookV, a controller, space), knows what's installed, and starts it.
//! Installing is the backend's (backend/src/sk8v.rs).
//!
//! Flux only starts GTA V for SK8V in Story Mode, with Rockstar's own `-nobattleye` option, which
//! GTA V needs for single-player mods. GTA Online isn't available while it runs that way, and
//! starting GTA V normally (from its own tile or launcher) has BattlEye and Online as usual.

use std::{path::{Path, PathBuf}, sync::Mutex, time::Instant};

use gpui::App;

use crate::{games::{GameKind, LibraryGame}, win_native::{self, Hive}};

/// The GTA V Legacy version SK8V and its ScriptHookV are made for (SK8V's setup_engine.REQUIRED_GTA).
pub const REQUIRED_GTA: &str = "1.0.3889.0";
/// Skate 3 for Xbox 360 (the disc SK8V builds its skating from).
pub const SKATE3_XBOX_TITLE: &str = "454108E6";
pub const STEAM_LEGACY_APP: u32 = 271590;

#[derive(Debug, Clone, PartialEq)]
pub struct Gta {
    pub dir: PathBuf,
    pub version: Option<String>,
    pub steam: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Installed {
    /// Flux's record of the last install (backend sk8v::INSTALLED_FILE).
    pub version: Option<String>,
    pub stance: Option<String>,
    /// SK8V's own manifest in the GTA folder: it's really there.
    pub in_gta: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Status {
    /// Skate 3 for Xbox 360 in the library: (game, disc image or folder).
    pub skate_xbox: Option<(LibraryGame, PathBuf)>,
    /// Skate 3 for PS3 in the library (SK8V can't use it).
    pub skate_ps3: Option<LibraryGame>,
    pub gta: Option<Gta>,
    /// GTA V Enhanced found (SK8V doesn't support it).
    pub enhanced: bool,
    pub scripthook: bool,
    pub controller: bool,
    pub free_bytes: Option<u64>,
    pub installed: Installed,
}

impl Status {
    pub fn gta_version_ok(&self) -> bool {
        self.gta.as_ref().is_some_and(|g| g.version.as_deref() == Some(REQUIRED_GTA))
    }

    /// Both games are here and right for SK8V.
    pub fn can_install(&self) -> bool {
        self.skate_xbox.is_some() && self.gta_version_ok()
    }

    pub fn ready(&self) -> bool {
        self.installed.in_gta && self.gta_version_ok()
    }
}

/// The SK8V folder in Flux's data folder (backend sk8v::sk8v_dir).
pub fn data_dir() -> Option<PathBuf> {
    crate::LAUNCHER_DIR.get().map(|d| d.join("sk8v"))
}

static CACHE: Mutex<Option<(Instant, Status)>> = Mutex::new(None);

/// What's here now (looked up again every few seconds, not every frame).
pub fn status(cx: &App) -> Status {
    if let Some((at, status)) = CACHE.lock().unwrap().as_ref() && at.elapsed().as_secs() < 4 {
        return status.clone();
    }
    let status = look(cx);
    *CACHE.lock().unwrap() = Some((Instant::now(), status.clone()));
    status
}

/// Look again on the next `status` (after installing, picking a folder...).
pub fn refresh() {
    *CACHE.lock().unwrap() = None;
}

fn look(cx: &App) -> Status {
    let games = crate::games::games(cx);
    let mut status = Status::default();
    for game in games {
        match game.kind {
            GameKind::Xbox360 if status.skate_xbox.is_none() => {
                let ids = crate::game_mods::ids(game, cx);
                if ids.title_id.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(SKATE3_XBOX_TITLE)) {
                    // SK8V takes the disc image, or the unpacked disc folder (the one with default.xex).
                    let disc = if game.path.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("default.xex")) {
                        game.path.parent().map(Path::to_path_buf).unwrap_or_else(|| game.path.clone())
                    } else {
                        game.path.clone()
                    };
                    status.skate_xbox = Some((game.clone(), disc));
                }
            },
            GameKind::Ps3 if status.skate_ps3.is_none() => {
                let ids = crate::game_mods::ids(game, cx);
                if ids.serial.as_deref().is_some_and(crate::rpcs3_config::is_skate3) {
                    status.skate_ps3 = Some(game.clone());
                }
            },
            _ => {},
        }
    }
    let chosen = crate::interface_config::InterfaceConfig::get(cx).sk8v_gta_dir.clone();
    let (legacy, enhanced) = find_gta(chosen.as_deref(), games);
    status.enhanced = enhanced;
    status.gta = legacy.map(|dir| Gta {
        version: win_native::file_version(&dir.join("GTA5.exe")),
        steam: dir.to_string_lossy().to_lowercase().contains("steamapps"),
        dir,
    });
    if let Some(gta) = &status.gta {
        status.scripthook = gta.dir.join("ScriptHookV.dll").is_file();
        status.free_bytes = win_native::free_space(&gta.dir);
        status.installed.in_gta = gta.dir.join("SkateVLegacy.installed.json").is_file() && gta.dir.join("SkateVLegacy.asi").is_file();
    }
    if let Some(record) = data_dir().and_then(|d| std::fs::read(d.join("flux-installed.json")).ok())
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
    {
        status.installed.version = record["version"].as_str().map(str::to_string);
        status.installed.stance = record["stance"].as_str().map(str::to_string);
    }
    status.controller = crate::controller_nav::connected() || !crate::controller::status().pads.is_empty();
    status
}

/// GTA V Legacy's folder (the one with GTA5.exe), and whether Enhanced is installed.
fn find_gta(chosen: Option<&Path>, games: &[LibraryGame]) -> (Option<PathBuf>, bool) {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = chosen {
        candidates.push(dir.to_path_buf());
    }
    for (key, value) in [
        (r"SOFTWARE\WOW6432Node\Rockstar Games\Grand Theft Auto V", "InstallFolder"),
        (r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Steam App 271590", "InstallLocation"),
        (r"SOFTWARE\WOW6432Node\Rockstar Games\GTAV", "InstallFolder"),
    ] {
        if let Some(dir) = win_native::registry_string(Hive::LocalMachine, key, value) {
            candidates.push(PathBuf::from(dir));
        }
    }
    let steam_common: Vec<PathBuf> = crate::importers::steam_root()
        .map(|root| crate::importers::steam_library_folders(&root))
        .unwrap_or_default()
        .into_iter()
        .map(|lib| lib.join("steamapps").join("common"))
        .collect();
    for common in &steam_common {
        candidates.push(common.join("Grand Theft Auto V"));
    }
    for game in games {
        if game.path.file_name().is_some_and(|n| ["gta5.exe", "playgtav.exe"].contains(&n.to_string_lossy().to_lowercase().as_str())) {
            if let Some(dir) = game.path.parent() {
                candidates.push(dir.to_path_buf());
            }
        }
    }
    let legacy = candidates.iter().find(|d| d.join("GTA5.exe").is_file()).cloned();
    let enhanced = candidates.iter().chain(steam_common.iter().map(|c| c.join("Grand Theft Auto V Enhanced")).collect::<Vec<_>>().iter())
        .any(|d| d.join("GTA5_Enhanced.exe").is_file());
    (legacy, enhanced)
}

/// Start GTA V Legacy Story Mode for SK8V: Rockstar's `-nobattleye` option (single-player mods
/// need it; Online isn't available that session). Steam copies start through Steam.
pub fn launch(gta: &Gta, cx: &mut App) -> Result<(), String> {
    if gta.version.as_deref() != Some(REQUIRED_GTA) {
        return Err(format!("SK8V needs GTA V Legacy {REQUIRED_GTA}; this copy is {}", gta.version.as_deref().unwrap_or("unknown")));
    }
    if gta.steam {
        cx.open_url(&format!("steam://run/{STEAM_LEGACY_APP}//-nobattleye"));
        return Ok(());
    }
    let launcher = ["PlayGTAV.exe", "GTAVLauncher.exe"].iter().map(|n| gta.dir.join(n)).find(|p| p.is_file())
        .ok_or("Couldn't find PlayGTAV.exe in the GTA V folder")?;
    std::process::Command::new(&launcher).arg("-nobattleye").current_dir(&gta.dir).spawn()
        .map(|_| ()).map_err(|e| format!("Couldn't start GTA V: {e}"))
}

/// Take SK8V out of the GTA folder, as its README says: the files its manifest lists, its INI and
/// manifest, the SK8V data folder and the skatev pack. ScriptHookV stays (other mods use it).
pub fn uninstall(gta: &Path) -> Result<usize, String> {
    let manifest = gta.join("SkateVLegacy.installed.json");
    let mut removed = 0;
    let mut remove = |p: PathBuf| -> Result<(), String> {
        if p.is_dir() {
            std::fs::remove_dir_all(&p).map_err(|e| format!("Couldn't remove {}: {e}", p.display()))?;
            removed += 1;
        } else if p.is_file() {
            std::fs::remove_file(&p).map_err(|e| format!("Couldn't remove {}: {e}", p.display()))?;
            removed += 1;
        }
        Ok(())
    };
    if let Some(files) = std::fs::read(&manifest).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|m| m["files"].as_object().cloned())
    {
        for rel in files.keys() {
            // Only paths inside the GTA folder.
            if Path::new(rel).components().all(|c| matches!(c, std::path::Component::Normal(_))) {
                remove(gta.join(rel))?;
            }
        }
    }
    for name in ["SkateVLegacy.asi", "SkateVRuntime.dll", "SkateVLegacy.ini", "SkateVLegacy.installed.json", "SkateVLegacy.installed.txt", "SK8V"] {
        remove(gta.join(name))?;
    }
    remove(gta.join("update").join("x64").join("dlcpacks").join("skatev"))?;
    if let Some(dir) = data_dir() {
        _ = std::fs::remove_file(dir.join("flux-installed.json"));
    }
    refresh();
    Ok(removed)
}

static LATEST: Mutex<Option<(Instant, Option<String>)>> = Mutex::new(None);

/// The newest SK8V release's version, looked up on GitHub at most every 30 minutes (None until known).
pub fn latest(cx: &mut App) -> Option<String> {
    if let Some((at, version)) = LATEST.lock().unwrap().as_ref() && at.elapsed().as_secs() < 1800 {
        return version.clone();
    }
    *LATEST.lock().unwrap() = Some((Instant::now(), None));
    let receiver = crate::games::fetch("https://api.github.com/repos/becaring/SK8V/releases/latest");
    cx.spawn(async move |cx| {
        let Ok(Ok(bytes)) = receiver.await else { return };
        let tag = serde_json::from_slice::<serde_json::Value>(&bytes).ok().and_then(|v| v["tag_name"].as_str().map(str::to_string));
        *LATEST.lock().unwrap() = Some((Instant::now(), tag));
        _ = cx.update(|cx| cx.refresh_windows());
    }).detach();
    None
}
