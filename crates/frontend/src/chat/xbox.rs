//! Xbox friends and messages through Xbox Live, using the Microsoft account the player signed
//! into Flux with (the backend turns that sign-in into an Xbox Live pass).

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::sync::Mutex;

use super::{Friend, Message, Network, Request, fail, http, notice, store, update};

const NET: Network = Network::Xbox;
const PEOPLE: &str = "https://peoplehub.xboxlive.com/users/me/people";
const SOCIAL: &str = "https://social.xboxlive.com/users/me/people";
// Lower-case "xbox": the messaging service rejects some requests that spell it "Xbox".
const MESSAGES: &str = "https://xblmessaging.xboxlive.com/network/xbox/users/me";

struct Pass {
    authorization: String,
    xuid: String,
    fetched: Instant,
}

static PASS: Mutex<Option<Pass>> = Mutex::const_new(None);
static POLLING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

async fn fetch_pass() -> Result<Pass, String> {
    let Some(backend) = crate::games::backend() else { return Err("Flux isn't ready yet".into()) };
    let (send, recv) = tokio::sync::oneshot::channel();
    backend.send(bridge::message::MessageToBackend::XboxLiveToken { result: send });
    let token = recv.await.map_err(|_| "Flux stopped".to_string())?.map_err(|e| e.to_string())?;
    Ok(Pass { authorization: token.authorization.to_string(), xuid: token.xuid.to_string(), fetched: Instant::now() })
}

/// The Xbox Live pass (renewed every few hours).
async fn pass() -> Result<(String, String), String> {
    let mut guard = PASS.lock().await;
    if guard.as_ref().is_none_or(|p| p.fetched.elapsed() > Duration::from_secs(3 * 3600)) {
        *guard = Some(fetch_pass().await?);
    }
    let pass = guard.as_ref().unwrap();
    Ok((pass.authorization.clone(), pass.xuid.clone()))
}

async fn request(method: reqwest::Method, url: &str, contract: &str, body: Option<Value>) -> Result<Value, String> {
    let (authorization, _) = pass().await?;
    let mut request = http().request(method, url)
        .header("Authorization", authorization)
        .header("x-xbl-contract-version", contract)
        .header("Accept-Language", "en-US");
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.map_err(|e| format!("Couldn't reach Xbox Live: {e}"))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        log::warn!("Xbox Live {status} for {}: {}", url.split('?').next().unwrap_or(url), text.chars().take(300).collect::<String>());
    }
    if status.is_success() {
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    } else if status.as_u16() == 401 || status.as_u16() == 403 {
        *PASS.lock().await = None;
        Err(format!("Xbox Live didn't allow that ({status}). If this keeps happening, it needs Flux's own Microsoft app (waiting on approval)."))
    } else {
        // Xbox Live's own explanation, e.g. {"errorMessage": "..."}.
        let reason = serde_json::from_str::<Value>(&text).ok()
            .and_then(|j| j["errorMessage"].as_str().or(j["message"].as_str()).map(String::from))
            .unwrap_or_default();
        Err(if reason.is_empty() { format!("Xbox Live said {status}") } else { format!("Xbox Live said {status}: {reason}") })
    }
}

pub async fn connect() {
    update(NET, |s| {
        s.connecting = true;
        s.error = None;
        s.notice = None;
    });
    match fetch_pass().await {
        Ok(fresh) => {
            *PASS.lock().await = Some(fresh);
            store::edit(|s| s.xbox = true);
            let me = request(reqwest::Method::GET, "https://profile.xboxlive.com/users/me/profile/settings?settings=Gamertag", "2", None).await.ok()
                .and_then(|j| j["profileUsers"][0]["settings"][0]["value"].as_str().map(String::from)).unwrap_or_default();
            update(NET, |s| {
                s.connected = true;
                s.connecting = false;
                s.me = me;
            });
            refresh_friends().await;
            if !POLLING.swap(true, std::sync::atomic::Ordering::SeqCst) {
                super::spawn(async {
                    loop {
                        tokio::time::sleep(Duration::from_secs(90)).await;
                        if !super::state(NET).connected {
                            POLLING.store(false, std::sync::atomic::Ordering::SeqCst);
                            return;
                        }
                        refresh_friends().await;
                    }
                });
            }
        },
        Err(e) => fail(NET, e),
    }
}

pub fn disconnect() {
    super::spawn(async {
        *PASS.lock().await = None;
    });
    store::edit(|s| s.xbox = false);
    update(NET, |s| *s = Default::default());
}

fn person(p: &Value) -> Option<Friend> {
    let id = p["xuid"].as_str()?.to_string();
    let name = p["gamertag"].as_str().or(p["displayName"].as_str()).unwrap_or("Player").to_string();
    let online = p["presenceState"] == "Online";
    let status = p["presenceText"].as_str().filter(|t| !t.is_empty()).map(String::from)
        .unwrap_or_else(|| if online { "Online".into() } else { "Offline".into() });
    Some(Friend { id, name, status, online, avatar: p["displayPicRaw"].as_str().map(String::from) })
}

pub async fn refresh_friends() {
    let result: Result<(), String> = async {
        let friends = request(reqwest::Method::GET, &format!("{PEOPLE}/friends/decoration/detail,presencedetail"), "7", None).await?;
        let mut list: Vec<Friend> = friends["people"].as_array().into_iter().flatten().filter_map(person).collect();
        list.sort_by_key(|f| (!f.online, f.name.to_lowercase()));
        // People who added you that you haven't added back.
        let followers = request(reqwest::Method::GET, &format!("{PEOPLE}/followers/decoration/detail"), "7", None).await.unwrap_or(Value::Null);
        let requests: Vec<Request> = followers["people"].as_array().into_iter().flatten()
            .filter(|p| p["isFollowingCaller"] == true && p["isFollowedByCaller"] != true)
            .filter_map(person).map(|f| Request { id: f.id, name: f.name, incoming: true }).collect();
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

/// Every piece of text in a message, whatever shape Xbox Live sent it in.
fn texts(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (key, value) in map {
                match (key.as_str(), value) {
                    ("text", Value::String(s)) => out.push(s.clone()),
                    _ => texts(value, out),
                }
            }
        },
        Value::Array(list) => list.iter().for_each(|v| texts(v, out)),
        _ => {},
    }
}

fn parse_messages(json: &Value, me: &str) -> Vec<Message> {
    let list = json["messages"].as_array().or(json["conversation"]["messages"].as_array());
    list.into_iter().flatten().filter_map(|m| {
        let mut parts = Vec::new();
        texts(&m["content"], &mut parts);
        if parts.is_empty() {
            texts(&m["contentPayload"], &mut parts);
        }
        if parts.is_empty() {
            return None;
        }
        let sender = m["sender"].as_str().map(String::from).or_else(|| m["sender"]["xuid"].as_str().map(String::from)).unwrap_or_default();
        let time = ["timestamp", "sent", "messageTime"].iter().find_map(|k| m[*k].as_str())
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|t| t.timestamp_millis()).unwrap_or(0);
        Some(Message { mine: sender == me, sender, text: parts.join(" "), time })
    }).collect()
}

pub async fn load_messages(friend: String) {
    let result: Result<(), String> = async {
        let (_, me) = pass().await?;
        // Find your one-to-one conversation with them in the inbox (or message requests), then
        // load it by its id.
        let mut conversation = None;
        for folder in ["primary", "secondary"] {
            let inbox = request(reqwest::Method::GET, &format!("{MESSAGES}/inbox/{folder}"), "2", None).await?;
            conversation = inbox["conversations"].as_array().into_iter().flatten().find(|c| {
                c["type"] == "OneToOne" && c["participants"].as_array().is_some_and(|p| p.iter().any(|x| x.as_str() == Some(friend.as_str())))
            }).and_then(|c| c["conversationId"].as_str().map(String::from));
            if conversation.is_some() {
                break;
            }
        }
        let messages = match conversation {
            Some(id) => {
                let json = request(reqwest::Method::GET, &format!("{MESSAGES}/conversations/OneToOne/{id}?maxItems=100"), "2", None).await?;
                parse_messages(&json, &me)
            },
            None => Vec::new(),
        };
        super::set_messages(NET, &friend, messages, false);
        Ok(())
    }.await;
    if let Err(e) = result {
        fail(NET, e);
    }
}

pub async fn send(friend: String, text: String) {
    let body = json!({"parts": [{"contentType": "text", "version": 0, "text": text}]});
    match request(reqwest::Method::POST, &format!("{MESSAGES}/conversations/users/xuid({friend})"), "1", Some(body)).await {
        Ok(_) => {
            log::info!("Xbox message sent");
            load_messages(friend).await
        },
        Err(e) => fail(NET, e),
    }
}

pub async fn add_friend(gamertag: String) {
    let url = format!("https://profile.xboxlive.com/users/gt({})/profile/settings?settings=Gamertag", urlencoding::encode(&gamertag));
    let xuid = match request(reqwest::Method::GET, &url, "2", None).await {
        Ok(json) => json["profileUsers"][0]["id"].as_str().map(String::from),
        Err(_) => None,
    };
    let Some(xuid) = xuid else { return fail(NET, format!("There's no Xbox player called {gamertag}")) };
    match request(reqwest::Method::PUT, &format!("{SOCIAL}/xuid({xuid})"), "2", None).await {
        Ok(_) => {
            notice(NET, format!("Added {gamertag}"));
            refresh_friends().await;
        },
        Err(e) => fail(NET, e),
    }
}

pub async fn answer(xuid: String, accept: bool) {
    if accept {
        match request(reqwest::Method::PUT, &format!("{SOCIAL}/xuid({xuid})"), "2", None).await {
            Ok(_) => refresh_friends().await,
            Err(e) => fail(NET, e),
        }
    } else {
        // Xbox has no "decline": just hide it here.
        update(NET, |s| s.requests.retain(|r| r.id != xuid));
    }
}

pub async fn remove_friend(xuid: String) {
    match request(reqwest::Method::DELETE, &format!("{SOCIAL}/xuid({xuid})"), "2", None).await {
        Ok(_) => refresh_friends().await,
        Err(e) => fail(NET, e),
    }
}

#[cfg(test)]
#[test]
fn reads_message_shapes() {
    let json = json!({"messages": [
        {"sender": "111", "timestamp": "2026-10-11T10:00:00Z", "content": {"parts": [{"contentType": "text", "text": "hi"}]}},
        {"sender": "222", "timestamp": "2026-10-11T10:01:00Z", "contentPayload": {"content": {"parts": [{"text": "yo"}]}}},
    ]});
    let list = parse_messages(&json, "111");
    assert_eq!(list.len(), 2);
    assert!(list[0].mine && list[0].text == "hi");
    assert!(!list[1].mine && list[1].text == "yo");
}
