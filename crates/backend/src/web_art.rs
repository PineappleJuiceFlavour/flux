//! Finds art for library games on the web, so a game added from a disc image or a folder gets a real
//! background instead of a Flux scene. Sources, best first:
//! - Steam's store (the game's own hero banner and logo) for games sold on Steam;
//! - libretro's thumbnail archive (in-game screenshots and box art, named after the No-Intro/Redump
//!   databases) for console games;
//! - Wikipedia (the article's cover art and gameplay screenshot) for anything else.
//! Screenshots and covers are turned into a 1920x1080 background: the screenshot (or cover) blown up
//! and softened, the cover sharp on the right, darkened where Flux puts the title and PLAY bar.

use std::{path::Path, sync::Arc};

use bridge::message::{FoundGameArt, SgdbImage};
use image::{RgbaImage, imageops};

use crate::BackendState;

const LIBRETRO: &str = "https://thumbnails.libretro.com";

/// Words of a title, for comparing names written differently: "The Simpsons: Hit & Run",
/// "Simpsons, The - Hit _ Run (USA)" and "simpsons hit and run" all become [simpsons, hit, run].
pub fn title_words(title: &str) -> Vec<String> {
    let mut plain = String::new();
    let mut depth = 0u32;
    for c in title.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => {
                // "Skate3" and "Skate 3" are the same title.
                if plain.chars().last().is_some_and(|p| p.is_alphabetic() != c.is_alphabetic() && p.is_alphanumeric() && c.is_alphanumeric()) {
                    plain.push(' ');
                }
                plain.push(c);
            },
            _ => {},
        }
    }
    plain.to_lowercase()
        .replace('\'', "")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !matches!(*w, "the" | "and" | "a" | "video" | "game" | "tm" | "r"))
        .map(|w| match w {
            "ii" => "2", "iii" => "3", "iv" => "4", "v" => "5",
            w => w,
        }.to_string())
        .collect()
}

/// The best file in a libretro listing for `name`: same title words, preferring USA, then Europe,
/// and skipping betas, demos and prototypes.
pub fn pick_libretro(files: &[String], name: &str) -> Option<String> {
    let wanted = title_words(name);
    if wanted.is_empty() {
        return None;
    }
    files.iter()
        .filter(|f| title_words(f.trim_end_matches(".png")) == wanted)
        .filter(|f| !["(Beta", "(Demo", "(Proto", "(Kiosk", "(Sample"].iter().any(|bad| f.contains(bad)))
        .min_by_key(|f| if f.contains("(USA") { 0 } else if f.contains("Europe") || f.contains("World") { 1 } else { 2 })
        .cloned()
}

/// File names in an Apache-style directory listing.
fn listing_files(html: &str) -> Vec<String> {
    html.split("href=\"").skip(1)
        .filter_map(|rest| rest.split('"').next())
        .filter(|href| href.ends_with(".png"))
        .filter_map(|href| urlencoding::decode(href).ok().map(|s| s.into_owned()))
        .collect()
}

pub(crate) async fn web_get(client: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.bytes().await.ok().map(|b| b.to_vec())
}

pub(crate) async fn web_json(client: &reqwest::Client, url: url::Url) -> Option<serde_json::Value> {
    serde_json::from_slice(&web_get(client, url.as_str()).await?).ok()
}

async fn web_image(client: &reqwest::Client, url: &str) -> Option<RgbaImage> {
    let bytes = web_get(client, url).await?;
    Some(image::load_from_memory(&bytes).ok()?.to_rgba8())
}

/// A Steam app with exactly this title (by its words).
pub(crate) async fn steam_search(client: &reqwest::Client, name: &str) -> Option<u32> {
    let url = url::Url::parse_with_params("https://store.steampowered.com/api/storesearch/", &[("term", name), ("l", "english"), ("cc", "US")]).ok()?;
    let json = web_json(client, url).await?;
    let wanted = title_words(name);
    json["items"].as_array()?.iter()
        .find(|item| item["name"].as_str().is_some_and(|n| title_words(n) == wanted))
        .and_then(|item| item["id"].as_u64())
        .map(|id| id as u32)
}

async fn libretro(client: &reqwest::Client, system: &str, folder: &str, name: &str) -> Option<RgbaImage> {
    let base = format!("{LIBRETRO}/{}/{folder}/", urlencoding::encode(system));
    let html = String::from_utf8(web_get(client, &base).await?).ok()?;
    let file = pick_libretro(&listing_files(&html), name)?;
    web_image(client, &format!("{base}{}", urlencoding::encode(&file))).await
}

/// The Wikipedia article for the game: its cover (the infobox picture) and a gameplay screenshot.
async fn wikipedia(client: &reqwest::Client, name: &str, platform: Option<&str>) -> (Option<RgbaImage>, Option<RgbaImage>) {
    const API: &str = "https://en.wikipedia.org/w/api.php";
    let query = format!("{} video game {}", title_words(name).join(" "), platform.unwrap_or(""));
    let Ok(url) = url::Url::parse_with_params(API, &[
        ("action", "query"), ("format", "json"), ("generator", "search"), ("gsrsearch", query.trim()), ("gsrlimit", "6"),
        ("prop", "pageimages"), ("piprop", "original"), ("pilicense", "any"),
    ]) else { return (None, None) };
    let Some(json) = web_json(client, url).await else { return (None, None) };
    let wanted = title_words(name);
    let mut pages: Vec<&serde_json::Value> = json["query"]["pages"].as_object().map(|p| p.values().collect()).unwrap_or_default();
    pages.sort_by_key(|p| p["index"].as_u64().unwrap_or(99));
    let Some(page) = pages.into_iter().find(|p| p["title"].as_str().is_some_and(|t| title_words(t) == wanted)) else { return (None, None) };
    let title = page["title"].as_str().unwrap_or_default().to_string();

    let cover = match page["original"]["source"].as_str() {
        Some(src) => web_image(client, src).await,
        None => None,
    };

    // A gameplay screenshot from the article, if it has one.
    let mut screenshot = None;
    if let Ok(url) = url::Url::parse_with_params(API, &[
        ("action", "query"), ("format", "json"), ("titles", title.as_str()), ("generator", "images"), ("gimlimit", "30"),
        ("prop", "imageinfo"), ("iiprop", "url"),
    ]) {
        if let Some(json) = web_json(client, url).await {
            let shot = json["query"]["pages"].as_object().into_iter().flat_map(|p| p.values()).find_map(|p| {
                let file = p["title"].as_str()?.to_lowercase();
                let looks_like_play = ["screenshot", "gameplay", "screen", "ingame", "in-game"].iter().any(|w| file.contains(w));
                (looks_like_play && (file.ends_with(".jpg") || file.ends_with(".png") || file.ends_with(".jpeg")))
                    .then(|| p["imageinfo"][0]["url"].as_str().map(str::to_string)).flatten()
            });
            if let Some(src) = shot {
                screenshot = web_image(client, &src).await;
            }
        }
    }
    (cover, screenshot)
}

/// SteamGridDB (needs the player's free API key): the best-fitting picture of a kind for a game.
/// `target` is "steam/<appid>" or "game/<id>".
async fn sgdb_url(client: &reqwest::Client, key: &str, kind: &str, target: &str, query: &str) -> Option<String> {
    let url = format!("https://www.steamgriddb.com/api/v2/{kind}/{target}?{query}");
    let response = client.get(&url).bearer_auth(key).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let json: serde_json::Value = response.json().await.ok()?;
    json["data"].as_array()?.first()?["url"].as_str().map(str::to_string)
}

/// Every choice of one kind of art for a game, best rated first, a page (50) at a time.
pub async fn sgdb_options(client: &reqwest::Client, key: &str, name: &str, steam_appid: Option<u32>, kind: &str, page: u32) -> Result<Vec<SgdbImage>, Arc<str>> {
    let key = key.trim();
    if key.is_empty() {
        return Err("Add your SteamGridDB key first (Games > Art)".into());
    }
    let target = sgdb_target(client, key, name, steam_appid).await.ok_or_else(|| Arc::<str>::from(format!("SteamGridDB doesn't know \"{name}\"")))?;
    // Tiles are square; the other kinds take any size.
    let query = match kind {
        "grids" => "dimensions=1024x1024,512x512&nsfw=false&humor=any",
        _ => "nsfw=false&humor=any",
    };
    let url = format!("https://www.steamgriddb.com/api/v2/{kind}/{target}?{query}&page={page}");
    let response = client.get(&url).bearer_auth(key).send().await.map_err(|e| Arc::<str>::from(format!("Couldn't reach SteamGridDB: {e}")))?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("SteamGridDB didn't accept your key: check it in Games > Art".into());
    }
    let json: serde_json::Value = response.json().await.map_err(|_| Arc::<str>::from("Unexpected answer from SteamGridDB"))?;
    let images = json["data"].as_array().cloned().unwrap_or_default().into_iter().filter_map(|i| Some(SgdbImage {
        thumb: i["thumb"].as_str().or(i["url"].as_str())?.into(),
        url: i["url"].as_str()?.into(),
        width: i["width"].as_u64().unwrap_or(0) as u32,
        height: i["height"].as_u64().unwrap_or(0) as u32,
        author: i["author"]["name"].as_str().unwrap_or("").into(),
    })).collect();
    Ok(images)
}

async fn sgdb_target(client: &reqwest::Client, key: &str, name: &str, steam_appid: Option<u32>) -> Option<String> {
    if let Some(appid) = steam_appid {
        return Some(format!("steam/{appid}"));
    }
    let term = title_words(name).join(" ");
    let url = format!("https://www.steamgriddb.com/api/v2/search/autocomplete/{}", urlencoding::encode(&term));
    let response = client.get(&url).bearer_auth(key).send().await.ok()?;
    let json: serde_json::Value = response.json().await.ok()?;
    let results = json["data"].as_array()?;
    let wanted = title_words(name);
    // Same title first, else SteamGridDB's best guess.
    let pick = results.iter().find(|g| g["name"].as_str().is_some_and(|n| title_words(n) == wanted)).or(results.first())?;
    Some(format!("game/{}", pick["id"].as_u64()?))
}

async fn save(client: &reqwest::Client, url: &str, path: PathBufLike) -> Option<std::path::PathBuf> {
    let bytes = web_get(client, url).await.filter(|b| image::load_from_memory(b).is_ok())?;
    std::fs::write(&path.0, bytes).ok().map(|_| path.0)
}

/// A file name to save into (keeps `save`'s signature short).
struct PathBufLike(std::path::PathBuf);

/// Look up art for one game and save what's found into `dest`. The best of each kind wins:
/// - background: SteamGridDB hero, else Steam's library hero, else a picture made from a
///   screenshot and the box art (libretro thumbnails / Wikipedia);
/// - cover (the tile): SteamGridDB square grid, else Steam's 2x box art, else libretro's
///   1024-pixel box art, else Wikipedia's;
/// - logo: SteamGridDB, else Steam's.
pub async fn find_game_art(client: &reqwest::Client, name: &str, system: Option<&str>, platform: Option<&str>, steam_appid: Option<u32>, sgdb_key: Option<&str>, dest: &Path) -> Result<FoundGameArt, Arc<str>> {
    let name = name.trim();
    if title_words(name).is_empty() {
        return Err("The game needs a name to look up".into());
    }
    std::fs::create_dir_all(dest).map_err(|_| Arc::<str>::from("Couldn't create the game's art folder"))?;
    // Each lookup writes new file names, so the window never shows a stale cached picture.
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let file = |what: &str, ext: &str| PathBufLike(dest.join(format!("web-{what}-{stamp}.{ext}")));
    let (mut background, mut cover, mut logo, mut icon) = (None, None, None, None);
    let mut sources: Vec<&str> = Vec::new();

    let appid = match steam_appid {
        Some(id) => Some(id),
        None => steam_search(client, name).await,
    };

    // 1. SteamGridDB, sized for exactly these spots.
    if let Some(key) = sgdb_key.filter(|k| !k.trim().is_empty()).map(str::trim)
        && let Some(target) = sgdb_target(client, key, name, appid).await
    {
        if let Some(url) = sgdb_url(client, key, "heroes", &target, "dimensions=3840x1240,1920x620&types=static&nsfw=false").await {
            background = save(client, &url, file("background", "png")).await;
        }
        if let Some(url) = sgdb_url(client, key, "grids", &target, "dimensions=1024x1024,512x512&types=static&nsfw=false").await {
            cover = save(client, &url, file("cover", "png")).await;
        }
        if let Some(url) = sgdb_url(client, key, "logos", &target, "types=static&nsfw=false").await {
            logo = save(client, &url, file("logo", "png")).await;
        }
        if let Some(url) = sgdb_url(client, key, "icons", &target, "types=static&nsfw=false").await {
            icon = save(client, &url, file("icon", "png")).await;
        }
        if background.is_some() || cover.is_some() || logo.is_some() {
            sources.push("SteamGridDB");
        }
    }

    // 2. Steam's own library art.
    if let Some(appid) = appid {
        let assets = format!("https://shared.steamstatic.com/store_item_assets/steam/apps/{appid}");
        let before = (background.is_some(), cover.is_some(), logo.is_some());
        if background.is_none() {
            background = save(client, &format!("{assets}/library_hero.jpg"), file("background", "jpg")).await;
        }
        if cover.is_none() {
            cover = match save(client, &format!("{assets}/library_600x900_2x.jpg"), file("cover", "jpg")).await {
                Some(c) => Some(c),
                None => save(client, &format!("{assets}/library_600x900.jpg"), file("cover", "jpg")).await,
            };
        }
        if logo.is_none() {
            logo = save(client, &format!("{assets}/logo.png"), file("logo", "png")).await;
        }
        if before != (background.is_some(), cover.is_some(), logo.is_some()) {
            sources.push("Steam");
        }
    }

    // 3. libretro thumbnails (console games), 4. Wikipedia: box art and a screenshot.
    if background.is_none() || cover.is_none() {
        let (mut screenshot, mut box_art) = (None, None);
        if let Some(system) = system {
            screenshot = libretro(client, system, "Named_Snaps", name).await;
            // The 1024-pixel box art when the archive has it, else the standard size.
            box_art = match libretro(client, system, "Named_Boxarts_1024", name).await {
                Some(big) => Some(big),
                None => libretro(client, system, "Named_Boxarts", name).await,
            };
            if screenshot.is_some() || box_art.is_some() {
                sources.push("libretro thumbnails");
            }
        }
        if screenshot.is_none() || box_art.is_none() {
            let (wiki_cover, wiki_shot) = wikipedia(client, name, platform).await;
            if (box_art.is_none() && wiki_cover.is_some()) || (screenshot.is_none() && wiki_shot.is_some()) {
                sources.push("Wikipedia");
            }
            box_art = box_art.or(wiki_cover);
            screenshot = screenshot.or(wiki_shot);
        }
        if background.is_none() && (screenshot.is_some() || box_art.is_some()) {
            let made = compose_background(screenshot.as_ref(), box_art.as_ref()).ok_or_else(|| Arc::<str>::from("Couldn't make a background"))?;
            let path = file("background", "png").0;
            made.save(&path).map_err(|_| Arc::<str>::from("Couldn't save the background"))?;
            background = Some(path);
        }
        if cover.is_none() && let Some(b) = &box_art {
            let path = file("cover", "png").0;
            cover = b.save(&path).ok().map(|_| path);
        }
    }

    if background.is_none() && cover.is_none() && logo.is_none() {
        return Err(format!("Couldn't find any art for \"{name}\" online").into());
    }
    sources.dedup();
    Ok(FoundGameArt { background, logo, cover, icon, source: sources.join(" + ").into() })
}

impl BackendState {
    pub async fn find_game_art(&self, name: &str, system: Option<&str>, platform: Option<&str>, steam_appid: Option<u32>, sgdb_key: Option<&str>, dest: &Path) -> Result<FoundGameArt, Arc<str>> {
        find_game_art(&self.http_client_provider.redirecting(), name, system, platform, steam_appid, sgdb_key, dest).await
    }
}

/// Scale `img` to fill w x h, cropping the overflow evenly.
fn cover_fill(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let scale = (w as f32 / img.width() as f32).max(h as f32 / img.height() as f32);
    let (sw, sh) = ((img.width() as f32 * scale).ceil() as u32, (img.height() as f32 * scale).ceil() as u32);
    let scaled = imageops::resize(img, sw.max(w), sh.max(h), imageops::FilterType::CatmullRom);
    imageops::crop_imm(&scaled, (scaled.width() - w) / 2, (scaled.height() - h) / 2, w, h).to_image()
}

fn compose_background(screenshot: Option<&RgbaImage>, cover: Option<&RgbaImage>) -> Option<RgbaImage> {
    let (w, h) = (1920u32, 1080u32);
    // Base: the screenshot lightly softened (they're small), or the cover heavily blurred.
    let mut bg = match (screenshot, cover) {
        (Some(shot), _) => {
            let soft = (1920.0 / shot.width() as f32).max(1.0);
            let small = cover_fill(shot, 960, 540);
            let small = if soft > 2.5 { imageops::blur(&small, 1.2) } else { small };
            imageops::resize(&small, w, h, imageops::FilterType::Triangle)
        },
        (None, Some(cover)) => {
            let small = imageops::blur(&cover_fill(cover, 480, 270), 9.0);
            imageops::resize(&small, w, h, imageops::FilterType::Triangle)
        },
        (None, None) => return None,
    };

    // The cover, sharp, on the right (at most twice its own size, so it stays crisp).
    if let Some(cover) = cover {
        let height = (cover.height() as f32 * 2.0).min(720.0).max(300.0);
        let width = cover.width() as f32 * height / cover.height() as f32;
        let (cw, ch) = (width as u32, height as u32);
        let sharp = imageops::resize(cover, cw, ch, imageops::FilterType::CatmullRom);
        let (x0, y0) = (w as i64 - cw as i64 - 200, (h as i64 - ch as i64) / 2 - 60);
        // Soft shadow behind it.
        for (x, y, p) in bg.enumerate_pixels_mut() {
            let dx = ((x0 - x as i64).max(x as i64 - (x0 + cw as i64)).max(0)) as f32;
            let dy = ((y0 - y as i64).max(y as i64 - (y0 + ch as i64)).max(0)) as f32;
            let d = (dx * dx + dy * dy).sqrt();
            if d < 60.0 {
                let k = 1.0 - 0.6 * (1.0 - d / 60.0);
                for i in 0..3 {
                    p[i] = (p[i] as f32 * k) as u8;
                }
            }
        }
        imageops::overlay(&mut bg, &sharp, x0, y0);
    }

    // Darken the left for the title and the bottom for the PLAY bar, a little everywhere.
    for (x, y, p) in bg.enumerate_pixels_mut() {
        let left = (1.0 - x as f32 / 1100.0).clamp(0.0, 1.0) * 0.6;
        let bottom = ((y as f32 - 680.0) / 400.0).clamp(0.0, 1.0) * 0.55;
        let k = (1.0 - left.max(bottom)) * 0.9;
        for i in 0..3 {
            p[i] = (p[i] as f32 * k) as u8;
        }
        p[3] = 255;
    }
    Some(bg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_words_match_across_spellings() {
        assert_eq!(title_words("The Simpsons: Hit & Run"), title_words("Simpsons, The - Hit _ Run (USA)"));
        assert_eq!(title_words("Grand Theft Auto: San Andreas™"), ["grand", "theft", "auto", "san", "andreas"]);
        assert_eq!(title_words("Skate 3 (USA, Europe)"), ["skate", "3"]);
        assert_eq!(title_words("Skate3"), ["skate", "3"]);
        assert_eq!(title_words("Ratchet & Clank II"), title_words("Ratchet and Clank 2"));
    }

    #[test]
    fn picks_the_right_libretro_file() {
        let files: Vec<String> = [
            "Simpsons, The - Road Rage (USA).png",
            "Simpsons, The - Hit _ Run (USA) (Beta).png",
            "Simpsons, The - Hit _ Run (Europe, Australia) (En,Fr,De,Es).png",
            "Simpsons, The - Hit _ Run (USA).png",
        ].map(String::from).to_vec();
        assert_eq!(pick_libretro(&files, "The Simpsons: Hit & Run").as_deref(), Some("Simpsons, The - Hit _ Run (USA).png"));
        assert_eq!(pick_libretro(&files, "Skate 3"), None);
    }

    #[test]
    fn reads_directory_listings() {
        let html = r#"<a href="?C=N;O=D">Name</a><a href="Simpsons,%20The%20-%20Hit%20_%20Run%20(USA).png">x</a>"#;
        assert_eq!(listing_files(html), ["Simpsons, The - Hit _ Run (USA).png"]);
    }

    /// FLUX_WEB_ART_TEST=<out dir>: look up a few real games online and save what's found.
    #[tokio::test]
    async fn real_lookups() {
        let Ok(out) = std::env::var("FLUX_WEB_ART_TEST") else { return };
        let client = reqwest::Client::builder().user_agent("FluxLauncher/0.1 (game art test)").build().unwrap();
        for (name, system, platform) in [
            ("Skate3", Some("Microsoft - Xbox 360"), Some("Xbox 360")),
            ("Tomodachi Life Living the Dream", None, Some("Nintendo Switch")),
            ("The Simpsons: Hit & Run", Some("Sony - PlayStation 2"), Some("PlayStation 2")),
            ("Simpsons Road Rage", Some("Sony - PlayStation 2"), Some("PlayStation 2")),
            ("Grounded", None, None),
        ] {
            let dest = Path::new(&out).join(title_words(name).join("-"));
            println!("{name}: {:?}", find_game_art(&client, name, system, platform, None, std::env::var("FLUX_SGDB_KEY").ok().as_deref(), &dest).await);
        }
    }
}

#[cfg(test)]
mod sgdb_tests {
    /// FLUX_SGDB_KEY=<key>: list the art choices for one game.
    #[tokio::test]
    async fn options_for_a_real_game() {
        let Ok(key) = std::env::var("FLUX_SGDB_KEY") else { return };
        let client = reqwest::Client::builder().user_agent("FluxLauncher/1.0").build().unwrap();
        for kind in ["grids", "heroes", "logos", "icons"] {
            let list = super::sgdb_options(&client, &key, "Skate 3", None, kind, 0).await.unwrap();
            println!("{kind}: {} choices, first {:?}", list.len(), list.first().map(|i| (i.width, i.height, i.thumb.to_string())));
            assert!(!list.is_empty());
        }
    }
}
