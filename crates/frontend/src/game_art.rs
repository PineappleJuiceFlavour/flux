//! Art for library games. Steam games get their own banner and logo from the Steam client's local
//! image cache (`appcache/librarycache/<appid>`); any other Windows game gets a background generated
//! from its program icon. Everything is copied into Flux's data folder, so it survives Steam clearing
//! its cache. Nothing is downloaded.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SteamInfo {
    pub appid: u32,
    pub name: String,
    pub library_cache: Option<PathBuf>,
}

fn steam_root() -> Option<PathBuf> {
    crate::win_native::user_registry_string(r"Software\Valve\Steam", "SteamPath")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| Some(PathBuf::from(r"C:\Program Files (x86)\Steam")).filter(|p| p.is_dir()))
}

/// `"key"\t\t"value"` lines of a Valve KeyValues (.acf) file.
fn acf_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut parts = line.split('"').filter(|p| !p.trim().is_empty());
        (parts.next()? == key).then(|| parts.next().map(str::to_string)).flatten()
    })
}

/// If `path` (a program or folder) is inside a Steam library, which Steam game is it?
pub fn steam_info(path: &Path) -> Option<SteamInfo> {
    let mut game_dir = None;
    for ancestor in path.ancestors() {
        let parent = ancestor.parent()?;
        if parent.file_name().is_some_and(|n| n.eq_ignore_ascii_case("common"))
            && parent.parent().and_then(|p| p.file_name()).is_some_and(|n| n.eq_ignore_ascii_case("steamapps"))
        {
            game_dir = Some(ancestor.to_path_buf());
            break;
        }
    }
    let game_dir = game_dir?;
    let install_dir = game_dir.file_name()?.to_string_lossy().to_string();
    let steamapps = game_dir.parent()?.parent()?;
    for entry in std::fs::read_dir(steamapps).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
        if acf_value(&text, "installdir").is_some_and(|d| d.eq_ignore_ascii_case(&install_dir)) {
            let appid: u32 = acf_value(&text, "appid")?.parse().ok()?;
            let name = acf_value(&text, "name").unwrap_or(install_dir.clone());
            let library_cache = steam_root().map(|r| r.join("appcache").join("librarycache").join(appid.to_string())).filter(|p| p.is_dir());
            return Some(SteamInfo { appid, name, library_cache });
        }
    }
    None
}

/// Find `file` in a Steam library-cache folder (newer clients nest files one level deeper).
fn find_cached(dir: &Path, file: &str) -> Option<PathBuf> {
    let direct = dir.join(file);
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).find_map(|sub| {
        let p = sub.join(file);
        p.is_file().then_some(p)
    })
}

#[derive(Debug, Clone, Default)]
pub struct Art {
    pub background: Option<PathBuf>,
    pub logo: Option<PathBuf>,
    pub icon: Option<PathBuf>,
}

/// Copy or make the art for a game into `dest` (Flux's `games/<id>` folder).
pub fn prepare(exe_or_folder: &Path, steam: Option<&SteamInfo>, dest: &Path) -> Art {
    let _ = std::fs::create_dir_all(dest);
    let mut art = Art::default();

    if let Some(cache) = steam.and_then(|s| s.library_cache.as_deref()) {
        let copy = |names: &[&str], to: &str| -> Option<PathBuf> {
            let src = names.iter().find_map(|n| find_cached(cache, n))?;
            let ext = src.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_else(|| "jpg".into());
            let out = dest.join(format!("{to}.{ext}"));
            std::fs::copy(&src, &out).ok()?;
            Some(out)
        };
        art.background = copy(&["library_hero.jpg", "library_hero.png", "header.jpg"], "background");
        art.logo = copy(&["logo.png"], "logo");
    }

    let exe = if exe_or_folder.is_file() { Some(exe_or_folder.to_path_buf()) } else { None };
    if let Some(exe) = exe {
        let icon = dest.join("icon.png");
        if extract_icon(&exe, &icon) {
            art.icon = Some(icon.clone());
            if art.background.is_none() {
                let out = dest.join("background.png");
                if generate_background(&icon, &out).is_some() {
                    art.background = Some(out);
                }
            }
        }
    }
    art
}

/// The program's largest icon (up to 256x256) as a PNG, via Windows' own icon loader.
fn extract_icon(exe: &Path, out: &Path) -> bool {
    crate::win_native::save_icon_png(exe, out) && out.is_file()
}

/// A 1920x1080 background from an icon: a gradient of the icon's two main colors, the icon blown up
/// and softened on the right, and a dark fade on the left where the title sits.
fn generate_background(icon: &Path, out: &Path) -> Option<()> {
    use image::{Rgba, RgbaImage, imageops};
    let icon = image::open(icon).ok()?.to_rgba8();

    // Two dominant colors: average of the brighter and of the darker opaque pixels.
    let opaque: Vec<[f32; 3]> = icon.pixels().filter(|p| p[3] > 200).map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    if opaque.is_empty() {
        return None;
    }
    let luma = |c: &[f32; 3]| 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
    let mean_luma = opaque.iter().map(luma).sum::<f32>() / opaque.len() as f32;
    let average = |pred: &dyn Fn(f32) -> bool| {
        let picked: Vec<&[f32; 3]> = opaque.iter().filter(|c| pred(luma(c))).collect();
        if picked.is_empty() {
            return [40.0, 40.0, 60.0];
        }
        let n = picked.len() as f32;
        [0, 1, 2].map(|i| picked.iter().map(|c| c[i]).sum::<f32>() / n)
    };
    let bright = average(&|l| l >= mean_luma);
    let dark = average(&|l| l < mean_luma);

    let (w, h) = (1920u32, 1080u32);
    let mut bg = RgbaImage::from_fn(w, h, |x, y| {
        let t = (x as f32 / w as f32 * 0.6 + y as f32 / h as f32 * 0.4).clamp(0.0, 1.0);
        let c = [0, 1, 2].map(|i| (dark[i] * 0.5 * (1.0 - t) + bright[i] * 0.7 * t) * 0.85);
        Rgba([c[0] as u8, c[1] as u8, c[2] as u8, 255])
    });

    // The icon, big and soft, on the right.
    let big = imageops::resize(&icon, 900, 900, imageops::FilterType::CatmullRom);
    let soft = imageops::blur(&imageops::resize(&big, 300, 300, imageops::FilterType::Triangle), 6.0);
    let soft = imageops::resize(&soft, 1100, 1100, imageops::FilterType::Triangle);
    overlay_alpha(&mut bg, &soft, 1150 - 550 + 260, 540 - 550, 0.55);
    overlay_alpha(&mut bg, &big, 1150 - 450 + 260, 540 - 450, 0.9);

    // Darken the left for the title and the bottom for the PLAY bar.
    for (x, y, p) in bg.enumerate_pixels_mut() {
        let left = (1.0 - x as f32 / 1100.0).clamp(0.0, 1.0) * 0.55;
        let bottom = ((y as f32 - 700.0) / 380.0).clamp(0.0, 1.0) * 0.5;
        let k = 1.0 - left.max(bottom);
        for i in 0..3 {
            p[i] = (p[i] as f32 * k) as u8;
        }
    }
    bg.save(out).ok()
}

fn overlay_alpha(base: &mut image::RgbaImage, top: &image::RgbaImage, x0: i64, y0: i64, opacity: f32) {
    for (x, y, p) in top.enumerate_pixels() {
        let (bx, by) = (x0 + x as i64, y0 + y as i64);
        if bx < 0 || by < 0 || bx >= base.width() as i64 || by >= base.height() as i64 {
            continue;
        }
        let a = p[3] as f32 / 255.0 * opacity;
        let b = base.get_pixel_mut(bx as u32, by as u32);
        for i in 0..3 {
            b[i] = (b[i] as f32 * (1.0 - a) + p[i] as f32 * a) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_acf_values() {
        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"1174180\"\n\t\"name\"\t\t\"Red Dead Redemption 2\"\n\t\"installdir\"\t\t\"Red Dead Redemption 2\"\n}";
        assert_eq!(acf_value(acf, "appid").as_deref(), Some("1174180"));
        assert_eq!(acf_value(acf, "installdir").as_deref(), Some("Red Dead Redemption 2"));
    }

    /// FLUX_ART_TEST=<exe path>;<out dir> runs the whole art step on a real game.
    #[test]
    fn art_for_real_game() {
        let Ok(spec) = std::env::var("FLUX_ART_TEST") else { return };
        let (exe, out) = spec.split_once(';').unwrap();
        let steam = steam_info(Path::new(exe));
        println!("steam: {steam:?}");
        println!("art: {:?}", prepare(Path::new(exe), steam.as_ref(), Path::new(out)));
    }
}
