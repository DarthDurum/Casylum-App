//! Projects group chats. A project can have a folder (new chats in it start
//! there) and instructions the agent follows in every chat of the project.
//! Stored as JSON in the app data directory.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub created_at: i64,
}

fn path() -> PathBuf {
    crate::settings::data_dir().join("projects.json")
}

pub fn list() -> Vec<Project> {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_all(all: &[Project]) -> Result<()> {
    let p = path();
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(all)?)?;
    std::fs::rename(tmp, p)?;
    Ok(())
}

pub fn get(id: &str) -> Option<Project> {
    list().into_iter().find(|p| p.id == id)
}

fn clean_name(name: &str) -> Result<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(anyhow!("Give the project a name."));
    }
    Ok(n.chars().take(80).collect())
}

pub fn create(name: &str, folder: &str, instructions: &str) -> Result<Project> {
    let p = Project {
        id: uuid::Uuid::new_v4().to_string(),
        name: clean_name(name)?,
        folder: folder.trim().to_string(),
        instructions: instructions.to_string(),
        created_at: chrono::Utc::now().timestamp_millis(),
    };
    let mut all = list();
    all.push(p.clone());
    save_all(&all)?;
    Ok(p)
}

pub fn update(id: &str, name: &str, folder: &str, instructions: &str) -> Result<Project> {
    let mut all = list();
    let p = all.iter_mut().find(|p| p.id == id).ok_or_else(|| anyhow!("That project no longer exists."))?;
    p.name = clean_name(name)?;
    p.folder = folder.trim().to_string();
    p.instructions = instructions.to_string();
    let out = p.clone();
    save_all(&all)?;
    Ok(out)
}

/// Remove a project. Its chats stay, outside any project.
pub fn delete(id: &str) -> Result<()> {
    let mut all = list();
    all.retain(|p| p.id != id);
    save_all(&all)?;
    for s in crate::agent::list_sessions(None) {
        if s.project_id.as_deref() == Some(id) {
            if let Ok(mut full) = crate::agent::load_session(&s.id) {
                full.project_id = None;
                let _ = crate::agent::save_session(&full);
            }
        }
    }
    Ok(())
}

/// What the agent sees about the chat's project.
pub fn prompt_section(p: &Project) -> String {
    let mut s = format!("\nThis chat is part of the project \"{}\".\n", p.name);
    let text = p.instructions.trim();
    if !text.is_empty() {
        let text: String = text.chars().take(20_000).collect();
        s.push_str(&format!("Instructions for every chat in this project:\n<project_instructions>\n{text}\n</project_instructions>\n"));
    }
    s
}
