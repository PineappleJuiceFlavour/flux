//! PlayStation friends and messages through the API the official PlayStation App uses (not an
//! official, published API: Sony can change it at any time). Sign-in is the player's NPSSO
//! token from playstation.com; Flux keeps only the refresh token it's traded for.

use std::{collections::HashMap, time::{Duration, Instant}};

use serde_json::{Value, json};
use tokio::sync::Mutex;

use super::{Friend, Message, Network, Request, fail, http, notice, store, update};

const NET: Network = Network::PlayStation;
const AUTH: &str = "https://ca.account.sony.com/api/authz/v3/oauth";
const PROFILE: &str = "https://m.np.playstation.com/api/userProfile/v1/internal/users";
const LOUNGE: &str = "https://m.np.playstation.com/api/gamingLoungeGroups/v1";
const CLIENT_ID: &str = "09515159-7237-4370-9b40-3806e67c0891";
const BASIC: &str = "Basic MDk1MTUxNTktNzIzNy00MzcwLTliNDAtMzgwNmU2N2MwODkxOnVjUGprYTV0bnRCMktxc1A=";
const REDIRECT: &str = "com.scee.psxandroid.scecompcall://redirect";
const SCOPE: &str = "psn:mobile.v2.core psn:clientapp";

struct Session {
    access: String,
    expires: Instant,
    refresh: String,
    me: String,
    /// Friend account id -> their chat group.
    groups: HashMap<String, String>,
    names: HashMap<String, String>,
}

static SESSION: Mutex<Option<Session>> = Mutex::const_new(None);
static POLLING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

async fn token_request(form: &[(&str, &str)]) -> Result<(String, u64, String), String> {
    let response = http().post(format!("{AUTH}/token"))
        .header("Authorization", BASIC)
        .form(form)
        .send().await.map_err(|e| format!("Couldn't reach PlayStation: {e}"))?;
    let json: Value = response.json().await.map_err(|e| e.to_string())?;
    match (json["access_token"].as_str(), json["refresh_token"].as_str()) {
        (Some(access), Some(refresh)) => Ok((access.into(), json["expires_in"].as_u64().unwrap_or(3600), refresh.into())),
        _ => Err(json["error_description"].as_str().or(json["error"].as_str()).unwrap_or("PlayStation didn't sign you in").to_string()),
    }
}

pub async fn connect(npsso: String) {
    update(NET, |s| {
        s.connecting = true;
        s.error = None;
        s.notice = None;
    });
    let npsso = npsso.trim().trim_matches('"').to_string();
    // Pasting the whole {"npsso":"..."} page works too.
    let npsso = serde_json::from_str::<Value>(&npsso).ok().and_then(|j| j["npsso"].as_str().map(String::from)).unwrap_or(npsso);
    let result: Result<(), String> = async {
        let url = format!(
            "{AUTH}/authorize?access_type=offline&client_id={CLIENT_ID}&redirect_uri={}&response_type=code&scope={}",
            urlencoding::encode(REDIRECT), urlencoding::encode(SCOPE),
        );
        let response = http().get(url).header("Cookie", format!("npsso={npsso}")).send().await.map_err(|e| format!("Couldn't reach PlayStation: {e}"))?;
        let location = response.headers().get("location").and_then(|l| l.to_str().ok()).unwrap_or("").to_string();
        let code = location.split_once('?').and_then(|(_, q)| q.split('&').find_map(|kv| kv.strip_prefix("code=")))
            .ok_or("PlayStation didn't accept that token. Get a fresh one (step 2) and paste it again.")?;
        let (access, expires, refresh) = token_request(&[
            ("code", code), ("redirect_uri", REDIRECT), ("grant_type", "authorization_code"), ("scope", SCOPE), ("token_format", "jwt"),
        ]).await?;
        start(access, expires, refresh).await
    }.await;
    if let Err(e) = result {
        fail(NET, e);
    }
}

pub async fn resume(refresh: String) {
    update(NET, |s| s.connecting = true);
    match token_request(&[("refresh_token", &refresh), ("grant_type", "refresh_token"), ("scope", SCOPE), ("token_format", "jwt")]).await {
        Ok((access, expires, refresh)) => {
            if let Err(e) = start(access, expires, refresh).await {
                fail(NET, e);
            }
        },
        Err(e) => {
            store::edit(|s| s.psn_refresh = None);
            fail(NET, format!("PlayStation sign-in expired ({e}). Connect again."));
        },
    }
}

async fn start(access: String, expires: u64, refresh: String) -> Result<(), String> {
    store::edit(|s| s.psn_refresh = Some(refresh.clone()));
    let me = http().get("https://dms.api.playstation.com/api/v1/devices/accounts/me").bearer_auth(&access).send().await
        .map_err(|e| e.to_string())?.json::<Value>().await.map_err(|e| e.to_string())?;
    let me = me["accountId"].as_str().map(String::from).or_else(|| me["accountId"].as_u64().map(|n| n.to_string()))
        .ok_or("PlayStation didn't say who you are")?;
    *SESSION.lock().await = Some(Session {
        access,
        expires: Instant::now() + Duration::from_secs(expires.saturating_sub(60)),
        refresh,
        me: me.clone(),
        groups: HashMap::new(),
        names: HashMap::new(),
    });
    let my_name = get(&format!("{PROFILE}/{me}/profiles")).await.ok().and_then(|p| p["onlineId"].as_str().map(String::from)).unwrap_or_default();
    update(NET, |s| {
        s.connected = true;
        s.connecting = false;
        s.me = my_name;
        s.error = None;
    });
    refresh_friends().await;
    if !POLLING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        super::spawn(async {
            loop {
                tokio::time::sleep(Duration::from_secs(90)).await;
                if SESSION.lock().await.is_none() {
                    POLLING.store(false, std::sync::atomic::Ordering::SeqCst);
                    return;
                }
                refresh_friends().await;
            }
        });
    }
    Ok(())
}

pub fn disconnect() {
    super::spawn(async {
        *SESSION.lock().await = None;
    });
    store::edit(|s| s.psn_refresh = None);
    update(NET, |s| *s = Default::default());
}

/// A valid access token (refreshed when old).
async fn access() -> Result<String, String> {
    let mut guard = SESSION.lock().await;
    let session = guard.as_mut().ok_or("Connect PlayStation first")?;
    if Instant::now() >= session.expires {
        let (access, expires, refresh) = token_request(&[
            ("refresh_token", &session.refresh), ("grant_type", "refresh_token"), ("scope", SCOPE), ("token_format", "jwt"),
        ]).await?;
        session.access = access;
        session.expires = Instant::now() + Duration::from_secs(expires.saturating_sub(60));
        session.refresh = refresh.clone();
        store::edit(|s| s.psn_refresh = Some(refresh));
    }
    Ok(session.access.clone())
}

async fn send_json(method: reqwest::Method, url: &str, body: Option<Value>) -> Result<Value, String> {
    let token = access().await?;
    let mut request = http().request(method, url).bearer_auth(token).header("Accept-Language", "en-US");
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.map_err(|e| format!("Couldn't reach PlayStation: {e}"))?;
    let status = response.status();
    let json: Value = response.json().await.unwrap_or(Value::Null);
    if status.is_success() {
        Ok(json)
    } else {
        Err(json["error"]["message"].as_str().map(String::from).unwrap_or_else(|| format!("PlayStation said {status}")))
    }
}

async fn get(url: &str) -> Result<Value, String> {
    send_json(reqwest::Method::GET, url, None).await
}

fn id_of(v: &Value) -> Option<String> {
    v.as_str().map(String::from).or_else(|| v.as_u64().map(|n| n.to_string()))
}

/// Names and pictures for account ids (100 at a time).
async fn profiles(ids: &[String]) -> HashMap<String, (String, Option<String>)> {
    let mut out = HashMap::new();
    for chunk in ids.chunks(100) {
        let Ok(json) = get(&format!("{PROFILE}/profiles?accountIds={}", chunk.join(","))).await else { continue };
        for (id, profile) in chunk.iter().zip(json["profiles"].as_array().into_iter().flatten()) {
            let name = profile["onlineId"].as_str().unwrap_or("Player").to_string();
            let avatar = profile["avatars"].as_array().and_then(|a| a.iter().find(|x| x["size"] == "m").or(a.first()))
                .and_then(|a| a["url"].as_str().map(String::from));
            out.insert(id.clone(), (name, avatar));
        }
    }
    out
}

pub async fn refresh_friends() {
    let result: Result<(), String> = async {
        let friends = get(&format!("{PROFILE}/me/friends?limit=1000")).await?;
        let ids: Vec<String> = friends["friends"].as_array().into_iter().flatten().filter_map(id_of).collect();
        let received = get(&format!("{PROFILE}/me/friends/receivedRequests?limit=100")).await.unwrap_or(Value::Null);
        let request_ids: Vec<String> = received["receivedRequests"].as_array().into_iter().flatten().filter_map(|r| id_of(&r["accountId"])).collect();
        let all: Vec<String> = ids.iter().chain(request_ids.iter()).cloned().collect();
        let profiles = profiles(&all).await;

        let mut presence: HashMap<String, (bool, String)> = HashMap::new();
        for chunk in ids.chunks(100) {
            let Ok(json) = get(&format!("{PROFILE}/basicPresences?type=primary&accountIds={}", chunk.join(","))).await else { continue };
            for p in json["basicPresences"].as_array().into_iter().flatten() {
                let Some(id) = id_of(&p["accountId"]) else { continue };
                let online = p["primaryPlatformInfo"]["onlineStatus"] == "online";
                let game = p["gameTitleInfoList"].as_array().and_then(|g| g.first()).and_then(|g| g["titleName"].as_str());
                let status = match (online, game) {
                    (true, Some(game)) => format!("Playing {game}"),
                    (true, None) => "Online".into(),
                    _ => "Offline".into(),
                };
                presence.insert(id, (online, status));
            }
        }

        let mut list: Vec<Friend> = ids.iter().map(|id| {
            let (name, avatar) = profiles.get(id).cloned().unwrap_or_else(|| ("Player".into(), None));
            let (online, status) = presence.get(id).cloned().unwrap_or((false, "Offline".into()));
            Friend { id: id.clone(), name, status, online, avatar }
        }).collect();
        list.sort_by_key(|f| (!f.online, f.name.to_lowercase()));
        let requests: Vec<Request> = request_ids.iter().map(|id| Request {
            id: id.clone(), name: profiles.get(id).map(|p| p.0.clone()).unwrap_or_else(|| "Player".into()), incoming: true,
        }).collect();
        if let Some(session) = SESSION.lock().await.as_mut() {
            session.names = profiles.into_iter().map(|(id, (name, _))| (id, name)).collect();
        }
        update(NET, |s| {
            s.friends = list;
            s.requests = requests;
        });
        Ok(())
    }.await;
    if let Err(e) = result {
        fail(NET, e);
    }
}

/// The chat group with a friend (made the first time).
async fn group_with(friend: &str) -> Result<String, String> {
    if let Some(group) = SESSION.lock().await.as_ref().and_then(|s| s.groups.get(friend).cloned()) {
        return Ok(group);
    }
    let json = send_json(reqwest::Method::POST, &format!("{LOUNGE}/groups"), Some(json!({"invitees": [{"accountId": friend}]}))).await
        .map_err(|e| format!("Couldn't open a chat with them: {e}"))?;
    let group = json["groupId"].as_str().ok_or("PlayStation didn't open the chat")?.to_string();
    if let Some(session) = SESSION.lock().await.as_mut() {
        session.groups.insert(friend.to_string(), group.clone());
    }
    Ok(group)
}

pub async fn load_messages(friend: String) {
    let result: Result<(), String> = async {
        let group = group_with(&friend).await?;
        let json = get(&format!("{LOUNGE}/members/me/groups/{group}/threads/{group}/messages?limit=50")).await?;
        let me = SESSION.lock().await.as_ref().map(|s| s.me.clone()).unwrap_or_default();
        let messages = json["messages"].as_array().into_iter().flatten().filter_map(|m| {
            let text = m["body"].as_str()?.to_string();
            let sender = id_of(&m["sender"]["accountId"]).unwrap_or_default();
            let time = m["createdTimestamp"].as_str().and_then(|t| t.parse().ok()).or(m["createdTimestamp"].as_i64()).unwrap_or(0);
            Some(Message { mine: sender == me, sender: m["sender"]["onlineId"].as_str().unwrap_or("").to_string(), text, time })
        }).collect();
        super::set_messages(NET, &friend, messages, false);
        Ok(())
    }.await;
    if let Err(e) = result {
        fail(NET, e);
    }
}

pub async fn send(friend: String, text: String) {
    let result: Result<(), String> = async {
        let group = group_with(&friend).await?;
        send_json(reqwest::Method::POST, &format!("{LOUNGE}/groups/{group}/threads/{group}/messages"), Some(json!({"messageType": 1, "body": text}))).await?;
        Ok(())
    }.await;
    match result {
        Ok(()) => load_messages(friend).await,
        Err(e) => fail(NET, e),
    }
}

pub async fn answer(id: String, accept: bool) {
    let method = if accept { reqwest::Method::PUT } else { reqwest::Method::DELETE };
    match send_json(method, &format!("{PROFILE}/me/friends/{id}"), None).await {
        Ok(_) => refresh_friends().await,
        Err(e) => fail(NET, e),
    }
}

pub async fn remove_friend(id: String) {
    answer(id, false).await;
}

/// Sony's app API can't send friend requests, so look the player up and open their profile
/// page, where the request can be sent.
pub async fn add_friend(online_id: String) {
    let url = format!("https://us-prof.np.community.playstation.net/userProfile/v1/users/{}/profile2?fields=accountId,onlineId", urlencoding::encode(&online_id));
    match get(&url).await {
        Ok(json) if json["profile"]["accountId"].is_string() => {
            _ = open::that_detached(format!("https://profile.playstation.com/{}", urlencoding::encode(&online_id)));
            notice(NET, format!("Found {online_id}. PlayStation only lets its own apps send friend requests, so their profile is open in your browser: send it from there (or your PS5 / the PlayStation App)."));
        },
        _ => fail(NET, format!("There's no PlayStation player called {online_id}")),
    }
}
