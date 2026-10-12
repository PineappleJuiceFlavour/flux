//! Xbox chat sign-in, done the way Xbox's own apps do it (as in the open-source prismarine-auth
//! "sisu" flow): Microsoft sign-in as the Xbox app with a code at microsoft.com/link, then a device
//! token and a combined user + device + title pass from Xbox's sign-in service, each request
//! signed with this PC's own P-256 key. Xbox Live only delivers messages sent with such a pass.
//! Not an official API for other apps: it can stop working when Microsoft changes it.

use std::time::Duration;

use base64::Engine;
use ring::{
    rand::SystemRandom,
    signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair},
};
use serde_json::{Value, json};

use super::http;

/// The Xbox app's Microsoft sign-in id (iOS), and the Xbox Live scope it asks for.
const CLIENT_ID: &str = "000000004c12ae6f";
const SCOPE: &str = "service::user.auth.xboxlive.com::MBI_SSL";
const DEVICE_CODE_URL: &str = "https://login.live.com/oauth20_connect.srf";
const TOKEN_URL: &str = "https://login.live.com/oauth20_token.srf";
const DEVICE_AUTH_URL: &str = "https://device.auth.xboxlive.com/device/authenticate";
const SISU_URL: &str = "https://sisu.xboxlive.com/authorize";

/// A sign-in waiting for the player to enter `user_code` at microsoft.com/link.
#[derive(Debug, Clone)]
pub struct DeviceCode {
    pub user_code: String,
    pub link: String,
    device_code: String,
    interval: u64,
    expires_in: u64,
    cookie: Option<String>,
}

/// What Xbox Live requests are signed in with.
#[derive(Debug, Clone)]
pub struct Pass {
    pub authorization: String,
    pub xuid: String,
    pub gamertag: String,
}

/// Step 1: get a code for the player to enter at microsoft.com/link.
pub async fn start_sign_in() -> Result<DeviceCode, String> {
    let response = http().post(DEVICE_CODE_URL)
        .form(&[("scope", SCOPE), ("client_id", CLIENT_ID), ("response_type", "device_code")])
        .send().await.map_err(|e| format!("Couldn't reach Microsoft: {e}"))?;
    let cookie = response.headers().get("set-cookie").and_then(|c| c.to_str().ok())
        .and_then(|c| c.split(';').next()).map(String::from);
    let json: Value = response.json().await.map_err(|e| e.to_string())?;
    let user_code = json["user_code"].as_str().ok_or("Microsoft didn't give a sign-in code")?.to_string();
    Ok(DeviceCode {
        link: format!("https://www.microsoft.com/link?otc={user_code}"),
        user_code,
        device_code: json["device_code"].as_str().unwrap_or_default().to_string(),
        interval: json["interval"].as_u64().unwrap_or(5).max(1),
        expires_in: json["expires_in"].as_u64().unwrap_or(900),
        cookie,
    })
}

/// Step 2: wait for the player to finish signing in; returns the Microsoft refresh token.
pub async fn finish_sign_in(code: &DeviceCode) -> Result<String, String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(code.expires_in);
    while std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_secs(code.interval)).await;
        let mut request = http().post(format!("{TOKEN_URL}?client_id={CLIENT_ID}")).form(&[
            ("client_id", CLIENT_ID),
            ("device_code", code.device_code.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ]);
        if let Some(cookie) = &code.cookie {
            request = request.header("Cookie", cookie);
        }
        let Ok(response) = request.send().await else { continue };
        let json: Value = response.json().await.unwrap_or(Value::Null);
        if let Some(refresh) = json["refresh_token"].as_str() {
            return Ok(refresh.to_string());
        }
        match json["error"].as_str() {
            Some("authorization_pending") | None => continue,
            Some(error) => return Err(json["error_description"].as_str().unwrap_or(error).to_string()),
        }
    }
    Err("The sign-in code expired. Try again.".into())
}

/// A fresh pass from the saved Microsoft refresh token and this PC's key (pkcs8, base64).
/// Returns the pass and the new refresh token to save.
pub async fn pass(refresh: &str, key_pkcs8: &str) -> Result<(Pass, String), String> {
    let json: Value = http().post(TOKEN_URL)
        .form(&[("scope", SCOPE), ("client_id", CLIENT_ID), ("grant_type", "refresh_token"), ("refresh_token", refresh)])
        .send().await.map_err(|e| format!("Couldn't reach Microsoft: {e}"))?
        .json().await.map_err(|e| e.to_string())?;
    let access = json["access_token"].as_str().ok_or("Your Xbox chat sign-in has expired: connect Xbox again")?;
    let new_refresh = json["refresh_token"].as_str().unwrap_or(refresh).to_string();

    let key = load_key(key_pkcs8)?;
    let jwk = jwk(&key);
    let id = || {
        let mut b = [0u8; 16];
        _ = ring::rand::SecureRandom::fill(&SystemRandom::new(), &mut b);
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        let h = hex::encode_upper(b);
        format!("{{{}-{}-{}-{}-{}}}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
    };

    let device = signed_post(&key, DEVICE_AUTH_URL, &json!({
        "Properties": {
            "AuthMethod": "ProofOfPossession",
            "Id": id(),
            "DeviceType": "iOS",
            "SerialNumber": id(),
            "Version": "0.0.0",
            "ProofKey": jwk,
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT",
    })).await?;
    let device_token = device["Token"].as_str().ok_or("Xbox didn't register this PC")?;

    let sisu = signed_post(&key, SISU_URL, &json!({
        "AccessToken": format!("t={access}"),
        "AppId": CLIENT_ID,
        "DeviceToken": device_token,
        "Sandbox": "RETAIL",
        "UseModernGamertag": true,
        "SiteName": "user.auth.xboxlive.com",
        "RelyingParty": "http://xboxlive.com",
        "ProofKey": jwk,
    })).await?;
    let token = sisu["AuthorizationToken"]["Token"].as_str().ok_or("Xbox didn't sign you in")?;
    let claims = &sisu["AuthorizationToken"]["DisplayClaims"]["xui"][0];
    let pass = Pass {
        authorization: format!("XBL3.0 x={};{token}", claims["uhs"].as_str().unwrap_or_default()),
        xuid: claims["xid"].as_str().unwrap_or_default().to_string(),
        gamertag: claims["mgt"].as_str().or(claims["gtg"].as_str()).unwrap_or_default().to_string(),
    };
    Ok((pass, new_refresh))
}

/// A new key for this PC (pkcs8, base64), kept with the sign-in.
pub fn new_key() -> Result<String, String> {
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new()).map_err(|_| "Couldn't make a key")?;
    Ok(base64::engine::general_purpose::STANDARD.encode(pkcs8.as_ref()))
}

fn load_key(pkcs8: &str) -> Result<EcdsaKeyPair, String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(pkcs8).map_err(|_| "Saved Xbox key is damaged: connect Xbox again")?;
    EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &bytes, &SystemRandom::new()).map_err(|_| "Saved Xbox key is damaged: connect Xbox again".into())
}

/// The public half as a JSON Web Key.
fn jwk(key: &EcdsaKeyPair) -> Value {
    let point = key.public_key().as_ref(); // 0x04 || x || y
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    json!({"kty": "EC", "crv": "P-256", "x": b64.encode(&point[1..33]), "y": b64.encode(&point[33..65]), "alg": "ES256", "use": "sig"})
}

/// Xbox's request signature: policy version, Windows-epoch timestamp, then an ES256 signature over
/// those plus the method, path, authorization and body (each NUL-terminated).
fn signature(key: &EcdsaKeyPair, url: &str, authorization: &str, body: &str, unix_seconds: u64) -> Result<String, String> {
    let timestamp = (unix_seconds + 11_644_473_600) * 10_000_000;
    let path = url.split("://").nth(1).and_then(|rest| rest.find('/').map(|i| &rest[i..])).unwrap_or("/");
    let path = path.split('?').next().unwrap_or(path);
    let mut signed = Vec::with_capacity(body.len() + 64);
    signed.extend_from_slice(&1i32.to_be_bytes());
    signed.push(0);
    signed.extend_from_slice(&timestamp.to_be_bytes());
    signed.push(0);
    for part in ["POST", path, authorization, body] {
        signed.extend_from_slice(part.as_bytes());
        signed.push(0);
    }
    let sig = key.sign(&SystemRandom::new(), &signed).map_err(|_| "Couldn't sign the request")?;
    let mut header = Vec::with_capacity(12 + 64);
    header.extend_from_slice(&1i32.to_be_bytes());
    header.extend_from_slice(&timestamp.to_be_bytes());
    header.extend_from_slice(sig.as_ref());
    Ok(base64::engine::general_purpose::STANDARD.encode(header))
}

async fn signed_post(key: &EcdsaKeyPair, url: &str, body: &Value) -> Result<Value, String> {
    let body = body.to_string();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let response = http().post(url)
        .header("Content-Type", "application/json")
        .header("Cache-Control", "no-store, must-revalidate, no-cache")
        .header("x-xbl-contract-version", "1")
        .header("Signature", signature(key, url, "", &body, now)?)
        .body(body)
        .send().await.map_err(|e| format!("Couldn't reach Xbox Live: {e}"))?;
    let status = response.status();
    let error_code = response.headers().get("x-err").and_then(|v| v.to_str().ok()).map(String::from);
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        log::warn!("Xbox sign-in {status} at {url}: {}", text.chars().take(300).collect::<String>());
        let code = error_code.or_else(|| serde_json::from_str::<Value>(&text).ok().and_then(|j| j["XErr"].as_u64().map(|n| n.to_string())));
        return Err(match code.as_deref() {
            Some("2148916233") => "This Microsoft account has no Xbox profile yet: create one at xbox.com first".into(),
            Some("2148916238") => "Xbox needs an adult to add this account to a Microsoft family first".into(),
            Some(code) => format!("Xbox sign-in failed ({status}, error {code})"),
            None => format!("Xbox sign-in failed ({status})"),
        });
    }
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

#[cfg(test)]
#[test]
fn signs_like_xbox_expects() {
    let key = load_key(&new_key().unwrap()).unwrap();
    let header = base64::engine::general_purpose::STANDARD.decode(signature(&key, "https://sisu.xboxlive.com/authorize?x=1", "", "{}", 1_700_000_000).unwrap()).unwrap();
    // policy version 1, Windows-epoch timestamp, 64-byte (r || s) signature
    assert_eq!(header.len(), 4 + 8 + 64);
    assert_eq!(&header[..4], &1i32.to_be_bytes());
    assert_eq!(u64::from_be_bytes(header[4..12].try_into().unwrap()), (1_700_000_000 + 11_644_473_600) * 10_000_000);
    // The signature checks out against the public key over the same bytes.
    let mut signed = Vec::new();
    signed.extend_from_slice(&1i32.to_be_bytes());
    signed.push(0);
    signed.extend_from_slice(&header[4..12]);
    signed.push(0);
    for part in ["POST", "/authorize", "", "{}"] {
        signed.extend_from_slice(part.as_bytes());
        signed.push(0);
    }
    let public = ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, key.public_key().as_ref());
    public.verify(&signed, &header[12..]).expect("signature verifies");
    let jwk = jwk(&key);
    assert_eq!(jwk["crv"], "P-256");
    assert_eq!(jwk["x"].as_str().unwrap().len(), 43);
}

/// FLUX_XBOX_LIVE_TEST=1: ask Microsoft for a sign-in code and Xbox for a device token (no account
/// involved), to check the requests and their signatures against the real services.
#[cfg(test)]
#[test]
fn live_services_accept_requests() {
    if std::env::var_os("FLUX_XBOX_LIVE_TEST").is_none() {
        return;
    }
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let code = start_sign_in().await.expect("Microsoft gives a sign-in code");
        println!("sign-in code ok ({} characters)", code.user_code.len());
        let key = load_key(&new_key().unwrap()).unwrap();
        let device = signed_post(&key, DEVICE_AUTH_URL, &json!({
            "Properties": {"AuthMethod": "ProofOfPossession", "Id": "{11111111-2222-4333-8444-555555555555}", "DeviceType": "iOS",
                "SerialNumber": "{66666666-7777-4888-8999-000000000000}", "Version": "0.0.0", "ProofKey": jwk(&key)},
            "RelyingParty": "http://auth.xboxlive.com", "TokenType": "JWT",
        })).await.expect("Xbox accepts the signed device request");
        assert!(device["Token"].is_string());
        println!("device token ok");
    });
}
