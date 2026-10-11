//! Flux chat on Matrix. Sign-in happens on the homeserver's own page in the browser (where new
//! players can also create a free account); Flux only gets an access token back, through a
//! one-off local address. Friends are Matrix direct chats ("m.direct").

use std::{collections::HashMap, time::Duration};

use serde_json::{Value, json};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, sync::Mutex};

use super::{Friend, Message, Network, Request, fail, http, notice, store, update};

const NET: Network = Network::Flux;

struct Session {
    base: String,
    token: String,
    user: String,
    /// Friend user id -> direct chat rooms with them (the "m.direct" list).
    direct: HashMap<String, Vec<String>>,
    /// Room -> member -> membership ("join", "invite", "leave").
    members: HashMap<String, HashMap<String, String>>,
    names: HashMap<String, String>,
    online: HashMap<String, bool>,
    /// Room -> messages, oldest first.
    timeline: HashMap<String, Vec<Message>>,
    /// Invites to you: room -> who sent it.
    invites: HashMap<String, String>,
    /// Bumped on sign-out, so an old sync loop stops.
    generation: u64,
}

static SESSION: Mutex<Option<Session>> = Mutex::const_new(None);
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn enc(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

/// The homeserver's API address ("matrix.org" -> "https://matrix-client.matrix.org").
async fn discover(server: &str) -> Result<String, String> {
    let server = server.trim().trim_end_matches('/');
    let server = if server.is_empty() { "matrix.org" } else { server };
    let host = server.trim_start_matches("https://").trim_start_matches("http://");
    let well_known = format!("https://{host}/.well-known/matrix/client");
    if let Ok(response) = http().get(&well_known).send().await
        && response.status().is_success()
        && let Ok(json) = response.json::<Value>().await
        && let Some(base) = json["m.homeserver"]["base_url"].as_str()
    {
        return Ok(base.trim_end_matches('/').to_string());
    }
    Ok(format!("https://{host}"))
}

async fn api(method: reqwest::Method, base: &str, token: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let mut request = http().request(method, format!("{base}/_matrix/client/v3{path}")).bearer_auth(token).timeout(Duration::from_secs(60));
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.map_err(|e| format!("Couldn't reach the chat server: {e}"))?;
    let status = response.status();
    let json: Value = response.json().await.unwrap_or(Value::Null);
    if status.is_success() {
        Ok(json)
    } else if status.as_u16() == 401 {
        Err("UNAUTHORIZED".into())
    } else {
        Err(json["error"].as_str().map(String::from).unwrap_or_else(|| format!("The chat server said {status}")))
    }
}

/// Run an API call with the current session.
async fn call(method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value, String> {
    let (base, token) = {
        let session = SESSION.lock().await;
        let session = session.as_ref().ok_or("Not signed in to Flux chat")?;
        (session.base.clone(), session.token.clone())
    };
    api(method, &base, &token, path, body).await
}

/// Sign in (or sign up) in the browser, then start chatting.
pub async fn login(homeserver: String) {
    update(NET, |s| {
        s.connecting = true;
        s.error = None;
        s.notice = Some("Finish signing in in your browser (you can create a free account there too)...".into());
    });
    let result: Result<(), String> = async {
        let base = discover(&homeserver).await?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let redirect = format!("http://127.0.0.1:{port}/flux-chat");
        let url = format!("{base}/_matrix/client/v3/login/sso/redirect?redirectUrl={}", enc(&redirect));
        open::that_detached(&url).map_err(|e| format!("Couldn't open your browser: {e}"))?;

        // Wait for the browser to come back with a one-time login token.
        let login_token = tokio::time::timeout(Duration::from_secs(600), async {
            loop {
                let (mut stream, _) = listener.accept().await.map_err(|e| e.to_string())?;
                let mut buf = vec![0u8; 8192];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("").to_string();
                let token = path.split_once('?').and_then(|(_, q)| q.split('&').find_map(|kv| kv.strip_prefix("loginToken=")))
                    .map(|t| urlencoding::decode(t).map(|t| t.into_owned()).unwrap_or_default());
                let page = if token.is_some() {
                    "<h2 style='font-family:sans-serif'>Signed in to Flux chat. You can close this tab.</h2>"
                } else {
                    "<h2 style='font-family:sans-serif'>Waiting for sign-in...</h2>"
                };
                let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len());
                _ = stream.write_all(reply.as_bytes()).await;
                if let Some(token) = token.filter(|t| !t.is_empty()) {
                    return Ok::<String, String>(token);
                }
            }
        }).await.map_err(|_| "Sign-in timed out (10 minutes)".to_string())??;

        let response = http().post(format!("{base}/_matrix/client/v3/login"))
            .json(&json!({"type": "m.login.token", "token": login_token, "initial_device_display_name": "Flux"}))
            .send().await.map_err(|e| e.to_string())?;
        let json: Value = response.json().await.map_err(|e| e.to_string())?;
        let (Some(token), Some(user)) = (json["access_token"].as_str(), json["user_id"].as_str()) else {
            return Err(json["error"].as_str().unwrap_or("The chat server didn't sign you in").to_string());
        };
        let session = store::MatrixSession { base_url: base, access_token: token.into(), user_id: user.into() };
        store::edit(|s| s.matrix = Some(session.clone()));
        start(session).await;
        Ok(())
    }.await;
    if let Err(e) = result {
        fail(NET, e);
    }
}

/// Sign back in with the saved session.
pub async fn resume(session: store::MatrixSession) {
    update(NET, |s| s.connecting = true);
    start(session).await;
}

async fn start(saved: store::MatrixSession) {
    let generation = GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    *SESSION.lock().await = Some(Session {
        base: saved.base_url,
        token: saved.access_token,
        user: saved.user_id.clone(),
        direct: HashMap::new(),
        members: HashMap::new(),
        names: HashMap::new(),
        online: HashMap::new(),
        timeline: HashMap::new(),
        invites: HashMap::new(),
        generation,
    });
    update(NET, |s| {
        s.me = saved.user_id;
        s.error = None;
        s.notice = None;
    });
    super::spawn(sync_loop(generation));
}

pub async fn logout() {
    GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    _ = call(reqwest::Method::POST, "/logout", Some(json!({}))).await;
    *SESSION.lock().await = None;
    store::edit(|s| s.matrix = None);
    update(NET, |s| *s = Default::default());
}

async fn sync_loop(generation: u64) {
    let filter = enc(&json!({"room": {"timeline": {"limit": 40}}, "presence": {"types": ["m.presence"]}}).to_string());
    let mut since: Option<String> = None;
    let mut failures = 0u32;
    loop {
        if GENERATION.load(std::sync::atomic::Ordering::SeqCst) != generation {
            return;
        }
        let path = match &since {
            Some(since) => format!("/sync?timeout=30000&filter={filter}&since={}", enc(since)),
            None => format!("/sync?timeout=0&filter={filter}"),
        };
        match call(reqwest::Method::GET, &path, None).await {
            Ok(json) => {
                failures = 0;
                since = json["next_batch"].as_str().map(String::from);
                let first = !matches!(update_session(&json, generation).await, Some(false));
                publish(first).await;
                fetch_missing_names().await;
            },
            Err(e) if e == "UNAUTHORIZED" => {
                *SESSION.lock().await = None;
                store::edit(|s| s.matrix = None);
                update(NET, |s| *s = Default::default());
                fail(NET, "You were signed out of Flux chat. Sign in again.");
                return;
            },
            Err(e) => {
                failures += 1;
                if failures == 3 {
                    fail(NET, e);
                }
                tokio::time::sleep(Duration::from_secs((failures * 5).min(60) as u64)).await;
            },
        }
    }
}

/// Take in one sync. Returns whether it was the session's first.
async fn update_session(json: &Value, generation: u64) -> Option<bool> {
    let mut guard = SESSION.lock().await;
    let session = guard.as_mut().filter(|s| s.generation == generation)?;
    let first = session.members.is_empty() && session.direct.is_empty();
    let me = session.user.clone();

    for event in json["account_data"]["events"].as_array().into_iter().flatten() {
        if event["type"] == "m.direct" && let Some(map) = event["content"].as_object() {
            session.direct = map.iter().map(|(user, rooms)| {
                (user.clone(), rooms.as_array().into_iter().flatten().filter_map(|r| r.as_str().map(String::from)).collect())
            }).collect();
        }
    }
    for event in json["presence"]["events"].as_array().into_iter().flatten() {
        if let Some(user) = event["sender"].as_str() {
            session.online.insert(user.to_string(), event["content"]["presence"] == "online");
            if let Some(name) = event["content"]["displayname"].as_str() {
                session.names.insert(user.to_string(), name.to_string());
            }
        }
    }
    if let Some(joined) = json["rooms"]["join"].as_object() {
        for (room, data) in joined {
            session.invites.remove(room);
            let events = data["state"]["events"].as_array().into_iter().flatten().chain(data["timeline"]["events"].as_array().into_iter().flatten());
            for event in events {
                match event["type"].as_str() {
                    Some("m.room.member") => {
                        if let (Some(user), Some(membership)) = (event["state_key"].as_str(), event["content"]["membership"].as_str()) {
                            session.members.entry(room.clone()).or_default().insert(user.to_string(), membership.to_string());
                            if let Some(name) = event["content"]["displayname"].as_str() {
                                session.names.insert(user.to_string(), name.to_string());
                            }
                        }
                    },
                    Some(kind @ ("m.room.message" | "m.room.encrypted")) => {
                        let sender = event["sender"].as_str().unwrap_or("").to_string();
                        let text = if kind == "m.room.encrypted" {
                            "🔒 Encrypted message: open it in a Matrix app like Element".to_string()
                        } else {
                            event["content"]["body"].as_str().unwrap_or("").to_string()
                        };
                        let time = event["origin_server_ts"].as_i64().unwrap_or(0);
                        session.timeline.entry(room.clone()).or_default().push(Message { mine: sender == me, sender, text, time });
                    },
                    _ => {},
                }
            }
        }
    }
    if let Some(invited) = json["rooms"]["invite"].as_object() {
        for (room, data) in invited {
            let inviter = data["invite_state"]["events"].as_array().into_iter().flatten()
                .find(|e| e["type"] == "m.room.member" && e["state_key"] == me.as_str())
                .and_then(|e| e["sender"].as_str()).unwrap_or("someone").to_string();
            session.invites.insert(room.clone(), inviter);
        }
    }
    if let Some(left) = json["rooms"]["leave"].as_object() {
        for room in left.keys() {
            session.invites.remove(room);
            session.members.entry(room.clone()).or_default().insert(me.clone(), "leave".into());
        }
    }
    Some(first)
}

/// Turn the session into the panel's friends, requests and conversations.
async fn publish(first: bool) {
    let guard = SESSION.lock().await;
    let Some(session) = guard.as_ref() else { return };
    let me = &session.user;
    let name = |user: &str| session.names.get(user).cloned().unwrap_or_else(|| user.trim_start_matches('@').split(':').next().unwrap_or(user).to_string());
    let mut friends = Vec::new();
    let mut requests = Vec::new();
    let mut conversations = Vec::new();
    for (user, rooms) in &session.direct {
        // The room you're in with them, and whether they've joined it yet.
        let room = rooms.iter().find(|r| session.members.get(*r).and_then(|m| m.get(me)).is_some_and(|m| m == "join"));
        let Some(room) = room else { continue };
        match session.members.get(room).and_then(|m| m.get(user)).map(String::as_str) {
            Some("join") => {
                let online = session.online.get(user).copied().unwrap_or(false);
                friends.push(Friend { id: user.clone(), name: name(user), status: if online { "Online".into() } else { user.clone() }, online, avatar: None });
                let mut messages = session.timeline.get(room).cloned().unwrap_or_default();
                for m in &mut messages {
                    m.sender = name(&m.sender);
                }
                conversations.push((user.clone(), messages));
            },
            Some("invite") => requests.push(Request { id: room.clone(), name: name(user), incoming: false }),
            _ => {},
        }
    }
    for (room, inviter) in &session.invites {
        requests.push(Request { id: room.clone(), name: name(inviter), incoming: true });
    }
    friends.sort_by_key(|f| (!f.online, f.name.to_lowercase()));
    let me_name = name(me);
    drop(guard);
    update(NET, |s| {
        s.connected = true;
        s.connecting = false;
        s.me = me_name;
        s.friends = friends;
        s.requests = requests;
    });
    for (friend, messages) in conversations {
        super::set_messages(NET, &friend, messages, !first);
    }
}

async fn fetch_missing_names() {
    let missing: Vec<String> = {
        let guard = SESSION.lock().await;
        let Some(session) = guard.as_ref() else { return };
        session.direct.keys().chain(session.invites.values()).filter(|u| !session.names.contains_key(*u)).cloned().collect()
    };
    for user in missing {
        let name = call(reqwest::Method::GET, &format!("/profile/{}/displayname", enc(&user)), None).await.ok()
            .and_then(|j| j["displayname"].as_str().map(String::from))
            .unwrap_or_else(|| user.trim_start_matches('@').split(':').next().unwrap_or(&user).to_string());
        if let Some(session) = SESSION.lock().await.as_mut() {
            session.names.insert(user, name);
        }
    }
    publish(false).await;
}

async fn room_with(friend: &str) -> Option<String> {
    let guard = SESSION.lock().await;
    let session = guard.as_ref()?;
    session.direct.get(friend)?.iter().find(|r| session.members.get(*r).and_then(|m| m.get(&session.user)).is_some_and(|m| m == "join")).cloned()
}

pub async fn send(friend: String, text: String) {
    let Some(room) = room_with(&friend).await else {
        return fail(NET, "That chat isn't ready yet");
    };
    let txn = format!("flux{}", super::now_ms());
    if let Err(e) = call(reqwest::Method::PUT, &format!("/rooms/{}/send/m.room.message/{txn}", enc(&room)), Some(json!({"msgtype": "m.text", "body": text}))).await {
        fail(NET, e);
    }
}

async fn save_direct() -> Result<(), String> {
    let (user, direct) = {
        let guard = SESSION.lock().await;
        let session = guard.as_ref().ok_or("Not signed in")?;
        (session.user.clone(), session.direct.clone())
    };
    call(reqwest::Method::PUT, &format!("/user/{}/account_data/m.direct", enc(&user)), Some(json!(direct))).await.map(|_| ())
}

/// "name", "@name" or "@name:server" -> a full Matrix user id (your own server by default).
fn full_id(name: &str, me: &str) -> String {
    let server = me.split_once(':').map(|(_, s)| s).unwrap_or("matrix.org");
    let name = name.trim().trim_start_matches('@');
    if name.contains(':') { format!("@{name}") } else { format!("@{name}:{server}") }
}

pub async fn add_friend(name: String) {
    let me = match SESSION.lock().await.as_ref() {
        Some(s) => s.user.clone(),
        None => return fail(NET, "Sign in to Flux chat first"),
    };
    let user = full_id(&name, &me);
    if user == me {
        return fail(NET, "That's you!");
    }
    if room_with(&user).await.is_some() {
        return notice(NET, format!("{user} is already your friend"));
    }
    if call(reqwest::Method::GET, &format!("/profile/{}", enc(&user)), None).await.is_err() {
        return fail(NET, format!("There's no Flux chat user called {user}"));
    }
    let created = call(reqwest::Method::POST, "/createRoom", Some(json!({
        "preset": "trusted_private_chat", "is_direct": true, "invite": [user], "name": "Flux chat",
    }))).await;
    match created {
        Ok(json) => {
            let Some(room) = json["room_id"].as_str().map(String::from) else { return fail(NET, "Couldn't start the chat") };
            if let Some(session) = SESSION.lock().await.as_mut() {
                session.direct.entry(user.clone()).or_default().push(room.clone());
                let members = session.members.entry(room).or_default();
                members.insert(me, "join".into());
                members.insert(user.clone(), "invite".into());
            }
            match save_direct().await {
                Ok(()) => notice(NET, format!("Friend request sent to {user}")),
                Err(e) => fail(NET, e),
            }
            publish(false).await;
        },
        Err(e) => fail(NET, e),
    }
}

pub async fn answer(room: String, accept: bool) {
    let inviter = SESSION.lock().await.as_ref().and_then(|s| s.invites.get(&room).cloned());
    if !accept {
        match call(reqwest::Method::POST, &format!("/rooms/{}/leave", enc(&room)), Some(json!({}))).await {
            Ok(_) => {
                if let Some(session) = SESSION.lock().await.as_mut() {
                    session.invites.remove(&room);
                    // An outgoing request you cancelled.
                    for rooms in session.direct.values_mut() {
                        rooms.retain(|r| r != &room);
                    }
                }
                _ = save_direct().await;
            },
            Err(e) => fail(NET, e),
        }
        return publish(false).await;
    }
    match call(reqwest::Method::POST, &format!("/join/{}", enc(&room)), Some(json!({}))).await {
        Ok(_) => {
            if let (Some(inviter), Some(session)) = (inviter, SESSION.lock().await.as_mut()) {
                session.invites.remove(&room);
                let rooms = session.direct.entry(inviter).or_default();
                if !rooms.contains(&room) {
                    rooms.push(room.clone());
                }
                let me = session.user.clone();
                session.members.entry(room).or_default().insert(me, "join".into());
            }
            if let Err(e) = save_direct().await {
                fail(NET, e);
            }
        },
        Err(e) => fail(NET, e),
    }
    publish(false).await;
}

pub async fn remove_friend(user: String) {
    let rooms = SESSION.lock().await.as_mut().and_then(|s| s.direct.remove(&user)).unwrap_or_default();
    for room in rooms {
        _ = call(reqwest::Method::POST, &format!("/rooms/{}/leave", enc(&room)), Some(json!({}))).await;
    }
    if let Err(e) = save_direct().await {
        fail(NET, e);
    }
    publish(false).await;
}

#[cfg(test)]
#[test]
fn friend_ids() {
    assert_eq!(full_id("sam", "@me:matrix.org"), "@sam:matrix.org");
    assert_eq!(full_id("@sam", "@me:matrix.org"), "@sam:matrix.org");
    assert_eq!(full_id("@sam:example.com", "@me:matrix.org"), "@sam:example.com");
}
