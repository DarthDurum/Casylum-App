//! API keys live in the OS credential store (macOS Keychain, Windows
//! Credential Manager, Secret Service on Linux). They are read only by the
//! Rust core and are never sent to the webview.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::OnceLock;

const SERVICE: &str = "app.casylum.desktop";

fn cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static C: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

fn entry(provider: &str) -> anyhow::Result<keyring::Entry> {
    Ok(keyring::Entry::new(SERVICE, provider)?)
}

pub fn get(provider: &str) -> Option<String> {
    if let Some(v) = cache().lock().get(provider) {
        return v.clone();
    }
    let v = entry(provider).ok().and_then(|e| e.get_password().ok()).filter(|k| !k.is_empty());
    cache().lock().insert(provider.to_string(), v.clone());
    v
}

pub fn set(provider: &str, key: &str) -> anyhow::Result<()> {
    let key = key.trim();
    entry(provider)?.set_password(key)?;
    cache().lock().insert(provider.to_string(), Some(key.to_string()));
    Ok(())
}

pub fn delete(provider: &str) -> anyhow::Result<()> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(e) => return Err(e.into()),
    }
    cache().lock().insert(provider.to_string(), None);
    Ok(())
}

/// Masked hint like "sk-…a1b2" so the UI can show which key is stored.
pub fn hint(provider: &str) -> Option<String> {
    get(provider).map(|k| {
        let tail: String = k.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
        let head: String = k.chars().take(3).collect();
        format!("{head}…{tail}")
    })
}
