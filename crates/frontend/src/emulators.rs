//! The emulators Flux can start games with. Most are managed (installed by the backend from their
//! official releases, see `backend::emulator::SOURCES`); the Switch one is the player's own copy.
//! Every emulator can also be pointed at a copy the player already has.

use std::path::{Path, PathBuf};

use gpui::App;

use crate::interface_config::InterfaceConfig;

pub struct EmuDef {
    pub id: &'static str,
    pub name: &'static str,
    pub consoles: &'static str,
    /// Flux can download and update it.
    pub managed: bool,
    /// Lowercase file-name prefix of its program (to find it in its folder).
    pub exe_prefix: &'static str,
    /// What the player has to provide, in plain words.
    pub needs: &'static str,
    pub license: &'static str,
}

pub const XENIA: &str = "xenia";
pub const DUCKSTATION: &str = "duckstation";
pub const PCSX2: &str = "pcsx2";
pub const RPCS3: &str = "rpcs3";
pub const SHADPS4: &str = "shadps4";
pub const PPSSPP: &str = "ppsspp";
pub const XEMU: &str = "xemu";
pub const SWITCH: &str = "switch";

pub const EMULATORS: &[EmuDef] = &[
    EmuDef {
        id: DUCKSTATION, name: "DuckStation", consoles: "PlayStation", managed: true, exe_prefix: "duckstation-qt",
        needs: "A PS1 BIOS dumped from your own console (its first-run setup asks where it is).", license: "CC BY-NC-ND",
    },
    EmuDef {
        id: PCSX2, name: "PCSX2", consoles: "PlayStation 2", managed: true, exe_prefix: "pcsx2-qt",
        needs: "A PS2 BIOS dumped from your own console (its first-run setup asks where it is).", license: "GPL-3.0",
    },
    EmuDef {
        id: RPCS3, name: "RPCS3", consoles: "PlayStation 3", managed: true, exe_prefix: "rpcs3",
        needs: "The PS3 system software (PS3UPDAT.PUP, free from PlayStation's website); Flux can install it for you.", license: "GPL-2.0",
    },
    EmuDef {
        id: SHADPS4, name: "shadPS4", consoles: "PlayStation 4", managed: true, exe_prefix: "shadps4",
        needs: "A PS4 game dumped from your own console (a folder with eboot.bin). Early days: many games don't run yet.", license: "GPL-2.0",
    },
    EmuDef {
        id: PPSSPP, name: "PPSSPP", consoles: "PSP", managed: true, exe_prefix: "ppssppwindows64",
        needs: "Nothing else: just your PSP game (.iso or .cso).", license: "GPL-2.0",
    },
    EmuDef {
        id: XEMU, name: "xemu", consoles: "Original Xbox", managed: true, exe_prefix: "xemu",
        needs: "The Xbox BIOS, MCPX boot ROM and a hard-disk image from your own console (set in xemu's settings).", license: "GPL-2.0",
    },
    EmuDef {
        id: XENIA, name: "Xenia Canary", consoles: "Xbox 360", managed: true, exe_prefix: "xenia_canary",
        needs: "Nothing else: just your Xbox 360 game.", license: "BSD-3-Clause",
    },
    EmuDef {
        id: SWITCH, name: "Switch emulator", consoles: "Nintendo Switch", managed: false, exe_prefix: "",
        needs: "Your own Switch emulator (for example Ryubing, Eden or Citron) set up with keys and firmware from your own Switch. Choose its .exe here.", license: "",
    },
];

pub fn emulator(id: &str) -> Option<&'static EmuDef> {
    EMULATORS.iter().find(|e| e.id == id)
}

pub fn managed_dir(id: &str) -> Option<PathBuf> {
    let folder = if id == XENIA { "xenia-canary" } else { id };
    Some(crate::LAUNCHER_DIR.get()?.join("emulators").join(folder))
}

fn find_exe(dir: &Path, prefix: &str) -> Option<PathBuf> {
    let matches = |p: &Path| {
        let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        name.starts_with(prefix) && name.ends_with(".exe")
    };
    let entries: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();
    entries.iter().find(|p| p.is_file() && matches(p)).cloned()
        .or_else(|| entries.iter().filter(|p| p.is_dir()).find_map(|d| find_exe(d, prefix)))
}

/// The program to run: the player's own copy if they chose one, else Flux's managed install.
pub fn exe(id: &str, cx: &App) -> Option<PathBuf> {
    if let Some(custom) = InterfaceConfig::get(cx).emulator_paths.get(id).filter(|p| p.is_file()) {
        return Some(custom.clone());
    }
    let def = emulator(id)?;
    if !def.managed {
        return None;
    }
    find_exe(&managed_dir(id)?, def.exe_prefix)
}

pub fn is_custom(id: &str, cx: &App) -> bool {
    InterfaceConfig::get(cx).emulator_paths.get(id).is_some_and(|p| p.is_file())
}

/// Arguments that start `game` straight away (not the emulator's menu).
pub fn launch_args(id: &str, game: &Path) -> Vec<std::ffi::OsString> {
    let g = game.as_os_str().to_owned();
    match id {
        // -batch: close the emulator when the game is closed, like a console.
        DUCKSTATION => vec!["-batch".into(), g],
        PCSX2 => vec!["-batch".into(), "--".into(), g],
        RPCS3 => vec!["--no-gui".into(), g],
        SHADPS4 => vec!["-g".into(), g],
        XEMU => vec!["-dvd_path".into(), g],
        _ => vec![g],
    }
}
