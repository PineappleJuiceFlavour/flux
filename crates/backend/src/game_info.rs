//! Details for library games (genre, developer, publisher, release date, tags, links,
//! description), for the Games home screen. Steam games use Steam's store data; everything else
//! uses Wikipedia (description) and Wikidata (the structured facts). No API keys needed.

use std::sync::Arc;

use bridge::message::FoundGameInfo;

use crate::{BackendState, web_art::{steam_search, title_words, web_json}};

const UA_SAFE_WIKI: &str = "https://en.wikipedia.org/w/api.php";

fn strings(value: &serde_json::Value, key: &str) -> Vec<String> {
    value[key].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect()
}

async fn from_steam(client: &reqwest::Client, appid: u32) -> Option<FoundGameInfo> {
    let url = url::Url::parse_with_params("https://store.steampowered.com/api/appdetails", &[("appids", appid.to_string().as_str()), ("l", "english")]).ok()?;
    let json = web_json(client, url).await?;
    let data = &json[appid.to_string()]["data"];
    if data.is_null() {
        return None;
    }
    let names = |key: &str| -> Vec<String> {
        data[key].as_array().into_iter().flatten().filter_map(|v| v["description"].as_str().map(str::to_string)).collect()
    };
    let mut links = vec![("Steam store".to_string(), format!("https://store.steampowered.com/app/{appid}"))];
    if let Some(site) = data["website"].as_str().filter(|s| !s.is_empty()) {
        links.push(("Homepage".into(), site.to_string()));
    }
    let name = data["name"].as_str().unwrap_or_default();
    links.push(("PCGamingWiki".into(), format!("https://www.pcgamingwiki.com/api/appid.php?appid={appid}")));
    let _ = name;
    let description = data["short_description"].as_str().map(|s| html_to_text(s)).unwrap_or_default();
    let about = data["about_the_game"].as_str().map(html_to_text).unwrap_or_default();
    Some(FoundGameInfo {
        description: if about.len() > description.len() && about.len() < 1500 { about } else { description },
        genres: names("genres"),
        developers: strings(data, "developers"),
        publishers: strings(data, "publishers"),
        release_date: data["release_date"]["date"].as_str().map(str::to_string),
        tags: names("categories").into_iter().take(8).collect(),
        links,
        source: "Steam".into(),
    })
}

/// Strip Steam's HTML down to plain text.
fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            },
            c if !in_tag => out.push(c),
            _ => {},
        }
    }
    let out = out.replace("&quot;", "\"").replace("&amp;", "&").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The game's Wikipedia article (same matching as the art lookup): (title, Wikidata id).
async fn wikipedia_page(client: &reqwest::Client, name: &str, platform: Option<&str>) -> Option<(String, Option<String>)> {
    let query = format!("{} video game {}", title_words(name).join(" "), platform.unwrap_or(""));
    let url = url::Url::parse_with_params(UA_SAFE_WIKI, &[
        ("action", "query"), ("format", "json"), ("generator", "search"), ("gsrsearch", query.trim()), ("gsrlimit", "6"),
        ("prop", "pageprops"), ("ppprop", "wikibase_item"),
    ]).ok()?;
    let json = web_json(client, url).await?;
    let wanted = title_words(name);
    let mut pages: Vec<&serde_json::Value> = json["query"]["pages"].as_object()?.values().collect();
    pages.sort_by_key(|p| p["index"].as_u64().unwrap_or(99));
    let page = pages.into_iter().find(|p| p["title"].as_str().is_some_and(|t| title_words(t) == wanted))?;
    Some((page["title"].as_str()?.to_string(), page["pageprops"]["wikibase_item"].as_str().map(str::to_string)))
}

async fn from_wiki(client: &reqwest::Client, name: &str, platform: Option<&str>) -> Option<FoundGameInfo> {
    let (title, qid) = wikipedia_page(client, name, platform).await?;
    let summary_url = url::Url::parse(&format!("https://en.wikipedia.org/api/rest_v1/page/summary/{}", urlencoding::encode(&title.replace(' ', "_")))).ok()?;
    let summary = web_json(client, summary_url).await;
    let description = summary.as_ref().and_then(|s| s["extract"].as_str()).unwrap_or_default().to_string();
    let mut info = FoundGameInfo {
        description,
        genres: vec![],
        developers: vec![],
        publishers: vec![],
        release_date: None,
        tags: vec![],
        links: vec![("Wikipedia".into(), format!("https://en.wikipedia.org/wiki/{}", urlencoding::encode(&title.replace(' ', "_"))))],
        source: "Wikipedia".into(),
    };
    let Some(qid) = qid else { return Some(info) };

    let entity_url = url::Url::parse(&format!("https://www.wikidata.org/wiki/Special:EntityData/{qid}.json")).ok()?;
    let Some(entity) = web_json(client, entity_url).await else { return Some(info) };
    let claims = &entity["entities"][&qid]["claims"];
    let ids = |prop: &str| -> Vec<String> {
        claims[prop].as_array().into_iter().flatten()
            .filter_map(|c| c["mainsnak"]["datavalue"]["value"]["id"].as_str().map(str::to_string))
            .collect()
    };
    let (genres, developers, publishers, modes) = (ids("P136"), ids("P178"), ids("P123"), ids("P404"));
    // One request for the names of everything referenced.
    let all: Vec<String> = genres.iter().chain(&developers).chain(&publishers).chain(&modes).take(50).cloned().collect();
    let mut labels = std::collections::HashMap::new();
    if !all.is_empty()
        && let Ok(url) = url::Url::parse_with_params("https://www.wikidata.org/w/api.php", &[
            ("action", "wbgetentities"), ("format", "json"), ("props", "labels"), ("languages", "en|mul"), ("languagefallback", "1"), ("ids", all.join("|").as_str()),
        ])
        && let Some(json) = web_json(client, url).await
    {
        for id in &all {
            let labels_of = &json["entities"][id]["labels"];
            if let Some(label) = labels_of["en"]["value"].as_str().or_else(|| labels_of["mul"]["value"].as_str()) {
                labels.insert(id.clone(), label.to_string());
            }
        }
    }
    let named = |list: &[String]| -> Vec<String> {
        let mut out: Vec<String> = list.iter().filter_map(|id| labels.get(id).cloned()).collect();
        out.dedup();
        out
    };
    // "skateboarding video game" -> "Skateboarding"
    info.genres = named(&genres).into_iter().map(|g| {
        let g = g.trim_end_matches(" video game").trim_end_matches(" game").to_string();
        let mut c = g.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    }).collect();
    info.developers = named(&developers);
    info.publishers = named(&publishers);
    info.tags = named(&modes).into_iter().map(|m| {
        let m = m.trim_end_matches(" video game").to_string();
        let mut c = m.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    }).collect();
    // The first full date (day precision = 11), else the first year/month given.
    let dates: Vec<(i64, &str)> = claims["P577"].as_array().into_iter().flatten()
        .filter_map(|c| Some((c["mainsnak"]["datavalue"]["value"]["precision"].as_i64().unwrap_or(0), c["mainsnak"]["datavalue"]["value"]["time"].as_str()?)))
        .collect();
    info.release_date = dates.iter().filter(|(p, _)| *p >= 11).map(|d| d.1).min()
        .or_else(|| dates.iter().map(|d| d.1).min())
        .map(|t| t.trim_start_matches('+').split('T').next().unwrap_or("").replace("-00", ""));
    if let Some(site) = claims["P856"][0]["mainsnak"]["datavalue"]["value"].as_str() {
        info.links.insert(0, ("Homepage".into(), site.to_string()));
    }
    info.links.push(("PCGamingWiki".into(), format!("https://www.pcgamingwiki.com/w/index.php?search={}", urlencoding::encode(name))));
    info.source = "Wikipedia + Wikidata".into();
    Some(info)
}

pub async fn find_game_info(client: &reqwest::Client, name: &str, platform: Option<&str>, steam_appid: Option<u32>) -> Result<FoundGameInfo, Arc<str>> {
    if title_words(name).is_empty() {
        return Err("The game needs a name to look up".into());
    }
    let appid = match steam_appid {
        Some(id) => Some(id),
        None => steam_search(client, name).await,
    };
    if let Some(appid) = appid && let Some(info) = from_steam(client, appid).await {
        return Ok(info);
    }
    if let Some(info) = from_wiki(client, name, platform).await {
        return Ok(info);
    }
    // "Grand Theft Auto V Legacy" -> "Grand Theft Auto V": edition words that Wikipedia titles leave out.
    let edition = ["legacy", "enhanced", "remastered", "definitive", "edition", "goty", "complete", "deluxe", "ultimate", "anniversary", "directors", "cut"];
    let words = title_words(name);
    let base: Vec<&str> = words.iter().map(String::as_str).rev().skip_while(|w| edition.contains(w)).collect::<Vec<_>>().into_iter().rev().collect();
    if base.len() < words.len() && !base.is_empty()
        && let Some(info) = from_wiki(client, &base.join(" "), platform).await
    {
        return Ok(info);
    }
    Err(Arc::from(format!("Couldn't find details for \"{name}\" online")))
}

impl BackendState {
    pub async fn find_game_info(&self, name: &str, platform: Option<&str>, steam_appid: Option<u32>) -> Result<FoundGameInfo, Arc<str>> {
        find_game_info(&self.http_client_provider.redirecting(), name, platform, steam_appid).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FLUX_GAME_INFO_TEST=1: look up a few real games.
    #[tokio::test]
    async fn real_lookups() {
        if std::env::var_os("FLUX_GAME_INFO_TEST").is_none() {
            return;
        }
        let client = reqwest::Client::builder().user_agent("FluxLauncher/1.0 (https://github.com/PineappleJuiceFlavour/flux)").build().unwrap();
        for (name, platform) in [("Skate3", Some("Xbox 360")), ("Simpsons Hit and Run", Some("PlayStation 2")), ("Among Us", None), ("Grand Theft Auto V Legacy", None), ("The Legend of Zelda Tears of the Kingdom", Some("Nintendo Switch"))] {
            match find_game_info(&client, name, platform, None).await {
                Ok(i) => println!("{name}: [{}] genres {:?} dev {:?} pub {:?} date {:?} tags {:?}\n  links {:?}\n  {}", i.source, i.genres, i.developers, i.publishers, i.release_date, i.tags, i.links, i.description.chars().take(140).collect::<String>()),
                Err(e) => println!("{name}: ERROR {e}"),
            }
        }
    }
}
