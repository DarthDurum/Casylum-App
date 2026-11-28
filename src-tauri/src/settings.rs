//! Non-secret app settings, stored as JSON in the app config directory.
//! API keys never live here: see `keys.rs`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Ask before every edit and command.
    #[default]
    Ask,
    /// Apply file edits automatically, ask before commands.
    AutoEdit,
    /// Never ask.
    FullAuto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub provider: String,
    pub model: String,
    pub permission_mode: PermissionMode,
    pub theme: String,
    pub ollama_base_url: String,
    pub custom_base_url: String,
    pub lmstudio_base_url: String,
    pub recent_projects: Vec<String>,
    /// Skills switched off, as "<scope>:<name>".
    pub disabled_skills: Vec<String>,
    /// Download updates in the background and install them on quit.
    pub auto_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: "anthropic".into(),
            model: "claude-opus-5-5".into(),
            permission_mode: PermissionMode::Ask,
            theme: "dark".into(),
            ollama_base_url: "http://localhost:11434/v1".into(),
            custom_base_url: String::new(),
            lmstudio_base_url: "http://localhost:1234/v1".into(),
            recent_projects: vec![],
            disabled_skills: vec![],
            auto_update: true,
        }
    }
}

impl Settings {
    /// A user-set base URL by its settings field name.
    pub fn base_url(&self, field: &str) -> String {
        match field {
            "ollama_base_url" => self.ollama_base_url.clone(),
            "lmstudio_base_url" => self.lmstudio_base_url.clone(),
            "custom_base_url" => self.custom_base_url.clone(),
            _ => String::new(),
        }
    }
}

pub fn data_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("casylum");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn path() -> PathBuf {
    data_dir().join("settings.json")
}

pub fn load() -> Settings {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(s: &Settings) -> anyhow::Result<()> {
    let tmp = path().with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(s)?)?;
    std::fs::rename(tmp, path())?;
    Ok(())
}
