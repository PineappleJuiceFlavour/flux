//! Privacy mode: hides who's who on screen (for streaming or screenshots): friends' names and
//! pictures in chat, your account names, email addresses, and your Windows user name in paths.
//! Messages show as asterisks.

use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(false);
/// The setting.
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Hide names, emails and the like on screen.
pub fn masking() -> bool {
    on()
}

/// A message as shown: all asterisks in privacy mode.
pub fn message(text: &str) -> String {
    if on() { "*".repeat(text.chars().count().clamp(6, 24)) } else { text.to_string() }
}

/// Follow the setting (called when it changes, and at startup).
pub fn set(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// A stand-in name for someone, the same each time ("Friend 4F2").
pub fn name(id: &str, real: &str) -> String {
    if !masking() {
        return real.to_string();
    }
    let hash = id.bytes().fold(0x811c9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x01000193));
    format!("Friend {:03X}", hash & 0xfff)
}

/// Your own name.
pub fn me(real: &str) -> String {
    if masking() { "You".into() } else { real.to_string() }
}

/// Text with email addresses and chat ids ("@sam:matrix.org") hidden.
pub fn text(text: &str) -> String {
    if !masking() {
        return text.to_string();
    }
    mask_text(text)
}

fn mask_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, word) in text.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let core = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '@' && c != '.' && c != ':' && c != '_' && c != '-' && c != '+');
        let is_email = core.split_once('@').is_some_and(|(user, domain)| !user.is_empty() && domain.contains('.'));
        let is_chat_id = core.starts_with('@') && core.contains(':');
        if is_email || is_chat_id {
            out.push_str(&word.replace(core, if is_email { "•••@•••" } else { "@•••" }));
        } else {
            out.push_str(word);
        }
    }
    out
}

/// A file path for display, without your Windows user name.
pub fn path(path: &std::path::Path) -> String {
    let shown = path.display().to_string();
    if !masking() {
        return shown;
    }
    match std::env::var("USERPROFILE") {
        Ok(home) if !home.is_empty() && shown.to_lowercase().starts_with(&home.to_lowercase()) => format!("%USERPROFILE%{}", &shown[home.len()..]),
        _ => shown,
    }
}

#[cfg(test)]
#[test]
fn masks() {
    assert_eq!(mask_text("mail sam@example.com now"), "mail •••@••• now");
    assert_eq!(mask_text("Friend request sent to @sam:matrix.org"), "Friend request sent to @•••");
    assert_eq!(mask_text("(sam@example.com)"), "(•••@•••)");
    assert_eq!(mask_text("hi @ you"), "hi @ you");
    set(true);
    assert_eq!(name("abc", "Sam"), name("abc", "Other"));
    assert_ne!(name("abc", "Sam"), "Sam");
    assert_eq!(message("hey what's up"), "*************");
    assert!(!message("hi").contains('h'));
    set(false);
    assert_eq!(name("abc", "Sam"), "Sam");
}
