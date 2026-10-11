//! Reads and edits Xenia Canary's `xenia-canary.config.toml` in place: only the value on a setting's
//! line changes, so Xenia's own comments and every other setting stay as they are.

use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Val {
    Int(i64),
    Str(&'static str),
    Bool(bool),
}

impl Val {
    fn toml(self) -> String {
        match self {
            Val::Int(v) => v.to_string(),
            Val::Str(v) => format!("\"{v}\""),
            Val::Bool(v) => v.to_string(),
        }
    }

    fn matches(self, raw: &str) -> bool {
        match self {
            Val::Int(v) => raw.parse::<i64>().ok() == Some(v),
            Val::Str(v) => raw.trim_matches('"').eq_ignore_ascii_case(v),
            Val::Bool(v) => raw.parse::<bool>().ok() == Some(v),
        }
    }
}

pub struct Setting {
    pub label: &'static str,
    pub help: &'static str,
    pub section: &'static str,
    /// Keys written together (resolution scale is an x and a y key).
    pub keys: &'static [&'static str],
    /// (label, one value per key)
    pub options: &'static [(&'static str, &'static [Val])],
}

pub const SETTINGS: &[Setting] = &[
    Setting {
        label: "Resolution",
        help: "Renders the game at a multiple of its native 720p. The biggest quality boost; needs a strong GPU above 2x.",
        section: "GPU",
        keys: &["draw_resolution_scale_x", "draw_resolution_scale_y"],
        options: &[("720p (1x)", &[Val::Int(1), Val::Int(1)]), ("1440p (2x)", &[Val::Int(2), Val::Int(2)]), ("4K (3x)", &[Val::Int(3), Val::Int(3)]), ("5K (4x)", &[Val::Int(4), Val::Int(4)])],
    },
    Setting {
        label: "Anti-aliasing",
        help: "Smooths jagged edges.",
        section: "Display",
        keys: &["postprocess_antialiasing"],
        options: &[("Off", &[Val::Str("")]), ("FXAA", &[Val::Str("fxaa")]), ("FXAA Extreme", &[Val::Str("fxaa_extreme")])],
    },
    Setting {
        label: "Upscaling and sharpening",
        help: "How the picture is stretched to your window. FSR looks sharpest when the window is bigger than the render resolution.",
        section: "Display",
        keys: &["postprocess_scaling_and_sharpening"],
        options: &[("Bilinear", &[Val::Str("")]), ("AMD CAS", &[Val::Str("cas")]), ("AMD FSR", &[Val::Str("fsr")])],
    },
    Setting {
        label: "Texture filtering",
        help: "Keeps textures sharp at an angle (the ground and ramps).",
        section: "GPU",
        keys: &["anisotropic_override"],
        options: &[("Game default", &[Val::Int(-1)]), ("4x", &[Val::Int(3)]), ("8x", &[Val::Int(4)]), ("16x", &[Val::Int(5)])],
    },
    Setting {
        label: "Graphics API",
        help: "Direct3D 12 is the most tested on Windows; try Vulkan if something looks wrong.",
        section: "GPU",
        keys: &["gpu"],
        options: &[("Auto", &[Val::Str("any")]), ("Direct3D 12", &[Val::Str("d3d12")]), ("Vulkan", &[Val::Str("vulkan")])],
    },
    Setting {
        label: "VSync",
        help: "Stops screen tearing.",
        section: "GPU",
        keys: &["vsync"],
        options: &[("On", &[Val::Bool(true)]), ("Off", &[Val::Bool(false)])],
    },
    Setting {
        label: "Frame rate limit",
        help: "Skate 3 runs at 30 on a real Xbox 360; higher can speed up or break some game logic.",
        section: "GPU",
        keys: &["framerate_limit"],
        options: &[("Auto", &[Val::Int(0)]), ("30", &[Val::Int(30)]), ("60", &[Val::Int(60)])],
    },
    Setting {
        label: "Fullscreen",
        help: "Start in fullscreen. F11 toggles it in game.",
        section: "Display",
        keys: &["fullscreen"],
        options: &[("Off", &[Val::Bool(false)]), ("On", &[Val::Bool(true)])],
    },
    Setting {
        label: "In-game photos",
        help: "Skate 3's photos and some replays need render-to-texture readback. Fast fixes them for a small performance cost.",
        section: "GPU",
        keys: &["readback_resolve"],
        options: &[("Off", &[Val::Str("none")]), ("Fast", &[Val::Str("fast")]), ("Accurate (slow)", &[Val::Str("full")])],
    },
    Setting {
        label: "Audio cutting out fix",
        help: "Uses the older audio decoder without a separate thread, which the Xenia Skate 3 community recommends.",
        section: "APU",
        keys: &["use_dedicated_xma_thread", "xma_decoder"],
        options: &[("Off", &[Val::Bool(true), Val::Str("new")]), ("On", &[Val::Bool(false), Val::Str("old")])],
    },
    Setting {
        label: "Controller",
        help: "Xbox mode reads Xbox controllers (and PlayStation ones through Flux's Controllers page); PlayStation mode reads PS4/PS5 controllers directly.",
        section: "HID",
        keys: &["hid"],
        options: &[("Auto", &[Val::Str("any")]), ("Xbox", &[Val::Str("xinput")]), ("PlayStation & others", &[Val::Str("sdl")])],
    },
    Setting {
        label: "Widescreen",
        help: "16:9 instead of 4:3.",
        section: "Video",
        keys: &["widescreen"],
        options: &[("16:9", &[Val::Bool(true)]), ("4:3", &[Val::Bool(false)])],
    },
];

/// Presets: (name, description, [(setting label, option label)]).
pub const PRESETS: &[(&str, &str, &[(&str, &str)])] = &[
    ("Original", "Like a real Xbox 360", &[
        ("Resolution", "720p (1x)"), ("Anti-aliasing", "Off"), ("Upscaling and sharpening", "Bilinear"), ("Texture filtering", "Game default"),
    ]),
    ("High", "1440p, FXAA, 16x filtering, FSR", &[
        ("Resolution", "1440p (2x)"), ("Anti-aliasing", "FXAA"), ("Upscaling and sharpening", "AMD FSR"), ("Texture filtering", "16x"),
    ]),
    ("Skate 3 fixes", "Audio fix + working photos (community settings)", &[
        ("Audio cutting out fix", "On"), ("In-game photos", "Fast"),
    ]),
    ("Ultra", "4K, FXAA Extreme, 16x filtering, FSR", &[
        ("Resolution", "4K (3x)"), ("Anti-aliasing", "FXAA Extreme"), ("Upscaling and sharpening", "AMD FSR"), ("Texture filtering", "16x"),
    ]),
];

pub fn config_path() -> Option<PathBuf> {
    Some(crate::games::xenia_exe()?.parent()?.join("xenia-canary.config.toml"))
}

fn read() -> String {
    config_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default()
}

fn key_of(line: &str) -> Option<&str> {
    let (key, _) = line.split_once('=')?;
    let key = key.trim();
    (!key.is_empty() && !key.starts_with('#') && !key.starts_with('[')).then_some(key)
}

fn raw_value(text: &str, key: &str) -> Option<String> {
    text.lines().find(|l| key_of(l) == Some(key)).map(|line| {
        let value = line.split_once('=').unwrap().1;
        let value = value.split('#').next().unwrap_or("").trim();
        value.to_string()
    })
}

/// The option currently set, by label (None if the file doesn't say, or says something else).
pub fn current(setting: &Setting) -> Option<&'static str> {
    let text = read();
    let raws: Vec<String> = setting.keys.iter().map(|k| raw_value(&text, k)).collect::<Option<_>>()?;
    setting.options.iter()
        .find(|(_, values)| values.iter().zip(&raws).all(|(v, raw)| v.matches(raw)))
        .map(|(label, _)| *label)
}

pub fn set(setting: &Setting, values: &[Val]) -> Result<(), String> {
    let path = config_path().ok_or("The emulator isn't installed")?;
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    for (key, value) in setting.keys.iter().zip(values) {
        text = set_key(&text, setting.section, key, *value);
    }
    let tmp = path.with_extension("toml.flux-tmp");
    std::fs::write(&tmp, &text).map_err(|e| format!("Couldn't save the emulator settings: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("Couldn't save the emulator settings: {e}"))
}

pub fn apply_preset(name: &str) -> Result<(), String> {
    let Some((_, _, picks)) = PRESETS.iter().find(|(n, _, _)| *n == name) else { return Ok(()) };
    for (label, option) in *picks {
        let setting = SETTINGS.iter().find(|s| s.label == *label).unwrap();
        let values = setting.options.iter().find(|(o, _)| o == option).unwrap().1;
        set(setting, values)?;
    }
    Ok(())
}

/// Set one key (for settings outside `SETTINGS`, e.g. log_level, allow_plugins).
pub fn set_value(section: &str, key: &str, value: Val) -> Result<(), String> {
    let path = config_path().ok_or("Xenia isn't installed")?;
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let tmp = path.with_extension("toml.flux-tmp");
    std::fs::write(&tmp, set_key(&text, section, key, value)).map_err(|e| format!("Couldn't save Xenia's settings: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("Couldn't save Xenia's settings: {e}"))
}

/// A key's raw value ("2", "true", "\"fxaa\"").
pub fn get_value(key: &str) -> Option<String> {
    raw_value(&read(), key)
}

/// Replace `key`'s value on its line (keeping the comment), or add it under `[section]`.
fn set_key(text: &str, section: &str, key: &str, value: Val) -> String {
    let new_value = value.toml();
    let mut found = false;
    let mut lines: Vec<String> = text.lines().map(|line| {
        if found || key_of(line) != Some(key) {
            return line.to_string();
        }
        found = true;
        let comment = line.find('#').map(|i| &line[i..]).unwrap_or("");
        let head = format!("{key} = {new_value}");
        if comment.is_empty() { head } else { format!("{head:<50}\t{comment}") }
    }).collect();
    if !found {
        let header = format!("[{section}]");
        match lines.iter().position(|l| l.trim() == header) {
            Some(i) => lines.insert(i + 1, format!("{key} = {new_value}")),
            None => {
                lines.push(String::new());
                lines.push(header);
                lines.push(format!("{key} = {new_value}"));
            },
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_value_keeps_comment_and_adds_missing() {
        let text = "[GPU]\nvsync = true                       \t# Enable VSYNC.\n";
        let out = set_key(text, "GPU", "vsync", Val::Bool(false));
        assert!(out.contains("vsync = false"));
        assert!(out.contains("# Enable VSYNC."));
        let out = set_key(&out, "GPU", "draw_resolution_scale_x", Val::Int(2));
        assert!(out.contains("[GPU]\ndraw_resolution_scale_x = 2\n"));
        assert_eq!(raw_value(&out, "vsync").as_deref(), Some("false"));
        let out = set_key("[Display]\npostprocess_antialiasing = \"\"   # x\n", "Display", "postprocess_antialiasing", Val::Str("fxaa"));
        assert_eq!(raw_value(&out, "postprocess_antialiasing").as_deref(), Some("\"fxaa\""));
        assert!(Val::Str("fxaa").matches("\"fxaa\""));
        assert!(Val::Str("").matches("\"\""));
    }
}

#[cfg(test)]
mod real_file {
    use super::*;

    /// XENIA_CONFIG_TEST=<path to xenia-canary.config.toml> checks every setting reads a known option.
    #[test]
    fn every_setting_reads_from_real_config() {
        let Ok(path) = std::env::var("XENIA_CONFIG_TEST") else { return };
        let text = std::fs::read_to_string(path).unwrap();
        for setting in SETTINGS {
            let raws: Vec<Option<String>> = setting.keys.iter().map(|k| raw_value(&text, k)).collect();
            let found = setting.options.iter().find(|(_, values)| values.iter().zip(&raws).all(|(v, raw)| raw.as_deref().is_some_and(|r| v.matches(r))));
            println!("{:<24} {:?} -> {:?}", setting.label, raws, found.map(|f| f.0));
        }
    }
}
