//! A PS3 game's own RPCS3 settings: `config/custom_configs/config_<SERIAL>.yml`, the file RPCS3's
//! "Create Custom Configuration" makes. Only the settings changed here are written; anything not
//! in it comes from RPCS3's main `config.yml`, so the page shows that value until it's changed.

use std::path::{Path, PathBuf};

use crate::game_mods::Yaml;

pub struct Setting {
    pub label: &'static str,
    pub help: &'static str,
    /// Section and key in RPCS3's YAML.
    pub path: [&'static str; 2],
    /// (label, value as RPCS3 writes it)
    pub options: &'static [(&'static str, &'static str)],
}

pub const SETTINGS: &[Setting] = &[
    Setting {
        label: "Resolution",
        help: "Renders the game above the PS3's 720p. The biggest quality boost; needs a strong GPU above 1440p.",
        path: ["Video", "Resolution Scale"],
        options: &[("720p", "100"), ("1080p", "150"), ("1440p", "200"), ("4K", "300")],
    },
    Setting {
        label: "Renderer",
        help: "Vulkan is the fastest on most PCs; try OpenGL if the game crashes or looks wrong.",
        path: ["Video", "Renderer"],
        options: &[("Vulkan", "Vulkan"), ("OpenGL", "OpenGL")],
    },
    Setting {
        label: "Anti-aliasing",
        help: "Smooths edges. Turning it off is faster.",
        path: ["Video", "MSAA"],
        options: &[("Auto", "Auto"), ("Off", "Disabled")],
    },
    Setting {
        label: "Texture filtering",
        help: "Keeps textures sharp at an angle (the ground and ramps).",
        path: ["Video", "Anisotropic Filter Override"],
        options: &[("Game default", "0"), ("4x", "4"), ("8x", "8"), ("16x", "16")],
    },
    Setting {
        label: "Frame limit",
        help: "PS3 Native keeps the game at its own speed and fixes audio crackle some PCs get with Write Color Buffers on.",
        path: ["Video", "Frame limit"],
        options: &[("Auto", "Auto"), ("PS3 Native", "PS3 Native"), ("60", "60"), ("Off", "Off")],
    },
    Setting {
        label: "VSync",
        help: "Stops screen tearing.",
        path: ["Video", "VSync Mode"],
        options: &[("Off", "Disabled"), ("On", "Synchronous")],
    },
    Setting {
        label: "Write Color Buffers",
        help: "Draws effects the game reads back. Skate 3 needs it for skater previews, photo missions and bloom lighting. A little slower.",
        path: ["Video", "Write Color Buffers"],
        options: &[("Off", "false"), ("On", "true")],
    },
    Setting {
        label: "Stretch to window",
        help: "Fills the window instead of keeping 16:9. Widescreen (aspect ratio) patches need it on.",
        path: ["Video", "Stretch To Display Area"],
        options: &[("Off", "false"), ("On", "true")],
    },
    Setting {
        label: "SPU accuracy",
        help: "How the PS3's SPU maths is done. Skate 3 players use Approximate; everyone in an online lobby needs the same.",
        path: ["Core", "SPU XFloat Accuracy"],
        options: &[("Approximate", "Approximate"), ("Relaxed", "Relaxed"), ("Accurate", "Accurate")],
    },
    Setting {
        label: "SPU loop detection",
        help: "A speed-up that changes timing. Skate 3 players keep it off so online physics stays in sync.",
        path: ["Core", "SPU loop detection"],
        options: &[("Off", "false"), ("On", "true")],
    },
    Setting {
        label: "SPU threads",
        help: "1 to 3 can fix choppy audio or slow motion on CPUs with few cores.",
        path: ["Core", "Preferred SPU Threads"],
        options: &[("Auto", "0"), ("1", "1"), ("2", "2"), ("3", "3")],
    },
    Setting {
        label: "SPU block size",
        help: "Mega is faster. Players report it also stops Skate 3 freezing on the cutscene before the open world.",
        path: ["Core", "SPU Block Size"],
        options: &[("Safe", "Safe"), ("Mega", "Mega"), ("Giga", "Giga")],
    },
];

/// (name, description, only for Skate 3, picks)
pub const PRESETS: &[(&str, &str, bool, &[(&str, &str)])] = &[
    ("Original", "Like a real PS3", false, &[
        ("Resolution", "720p"), ("Anti-aliasing", "Auto"), ("Texture filtering", "Game default"),
    ]),
    ("High", "1440p, 16x filtering", false, &[
        ("Resolution", "1440p"), ("Anti-aliasing", "Auto"), ("Texture filtering", "16x"),
    ]),
    ("Skate 3 fixes", "Previews, photos, lighting, audio and online sync (community settings)", true, &[
        ("Write Color Buffers", "On"), ("Frame limit", "PS3 Native"), ("SPU accuracy", "Approximate"), ("SPU loop detection", "Off"),
    ]),
    ("Ultra", "4K, 16x filtering", false, &[
        ("Resolution", "4K"), ("Anti-aliasing", "Auto"), ("Texture filtering", "16x"),
    ]),
];

/// Skate 3's serials: US, Europe, Japan.
pub const SKATE3_SERIALS: &[&str] = &["BLUS30464", "BLES00760", "BLJM60296"];

pub fn is_skate3(serial: &str) -> bool {
    SKATE3_SERIALS.contains(&serial)
}

pub fn custom_config_path(base: &Path, serial: &str) -> PathBuf {
    base.join("config").join("custom_configs").join(format!("config_{serial}.yml"))
}

fn read(path: &Path) -> Yaml {
    Yaml::parse(&std::fs::read_to_string(path).unwrap_or_default())
}

fn value(yaml: &Yaml, setting: &Setting) -> Option<String> {
    match yaml.get(&setting.path)? {
        Yaml::Val(v) => Some(v.trim_matches('"').to_string()),
        Yaml::Map(_) => None,
    }
}

/// The option in effect: this game's own setting, else RPCS3's main one. `true` when the game has
/// its own value.
pub fn current(base: &Path, serial: &str, setting: &Setting) -> Option<(&'static str, bool)> {
    let own = value(&read(&custom_config_path(base, serial)), setting);
    let raw = own.clone().or_else(|| value(&read(&base.join("config").join("config.yml")), setting))?;
    let label = setting.options.iter().find(|(_, v)| v.eq_ignore_ascii_case(&raw))?.0;
    Some((label, own.is_some()))
}

pub fn set(base: &Path, serial: &str, setting: &Setting, option: &str) -> Result<(), String> {
    let raw = setting.options.iter().find(|(label, _)| *label == option).ok_or("Unknown option")?.1;
    let path = custom_config_path(base, serial);
    let mut yaml = read(&path);
    yaml.set(&setting.path, Some(raw));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't save the settings: {e}"))?;
    }
    let tmp = path.with_extension("yml.flux-tmp");
    std::fs::write(&tmp, yaml.write()).map_err(|e| format!("Couldn't save the settings: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("Couldn't save the settings: {e}"))
}

pub fn apply_preset(base: &Path, serial: &str, name: &str) -> Result<(), String> {
    let Some((_, _, _, picks)) = PRESETS.iter().find(|(n, ..)| *n == name) else { return Ok(()) };
    for (label, option) in *picks {
        let setting = SETTINGS.iter().find(|s| s.label == *label).ok_or("Unknown setting")?;
        set(base, serial, setting, option)?;
    }
    Ok(())
}

/// Folders a PS3 game copies its data into on first run (Skate 3: `BLUS30464_INSTALL`). Stale
/// copies keep old files loading after a mod changes them.
pub fn install_caches(base: &Path, serial: &str) -> Vec<PathBuf> {
    let game = base.join("dev_hdd0").join("game");
    std::fs::read_dir(&game).into_iter().flatten().flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| {
            let n = n.to_string_lossy().to_uppercase();
            n.starts_with(serial) && n != serial && n.contains("INSTALL")
        }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_only_what_changed_and_reads_main_config_otherwise() {
        let base = std::env::temp_dir().join("flux-rpcs3-config-test");
        _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("config")).unwrap();
        std::fs::write(base.join("config").join("config.yml"), "Core:\n  SPU Block Size: Safe\nVideo:\n  Renderer: Vulkan\n  Write Color Buffers: false\n").unwrap();
        let wcb = SETTINGS.iter().find(|s| s.label == "Write Color Buffers").unwrap();
        assert_eq!(current(&base, "BLUS30464", wcb), Some(("Off", false)));
        apply_preset(&base, "BLUS30464", "Skate 3 fixes").unwrap();
        assert_eq!(current(&base, "BLUS30464", wcb), Some(("On", true)));
        let text = std::fs::read_to_string(custom_config_path(&base, "BLUS30464")).unwrap();
        assert!(text.contains("Write Color Buffers: true"), "{text}");
        assert!(text.contains("SPU XFloat Accuracy: Approximate"), "{text}");
        assert!(!text.contains("Renderer"), "{text}");
        // The main config is untouched.
        assert!(std::fs::read_to_string(base.join("config").join("config.yml")).unwrap().contains("Write Color Buffers: false"));
    }

    #[test]
    fn finds_install_caches() {
        let base = std::env::temp_dir().join("flux-rpcs3-cache-test");
        _ = std::fs::remove_dir_all(&base);
        for d in ["BLUS30464", "BLUS30464_INSTALL", "BLES00760_INSTALL"] {
            std::fs::create_dir_all(base.join("dev_hdd0").join("game").join(d)).unwrap();
        }
        let found = install_caches(&base, "BLUS30464");
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("BLUS30464_INSTALL"));
    }
}
