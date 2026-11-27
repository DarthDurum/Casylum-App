//! Skills: folders containing a SKILL.md with `name` and `description` in
//! YAML-style frontmatter. Only descriptions go into the system prompt; the
//! agent loads a skill's full body on demand with the `use_skill` tool.
//!
//! Global skills: ~/.casylum/skills/<name>/SKILL.md
//! Project skills: <project>/.casylum/skills/<name>/SKILL.md

use anyhow::{anyhow, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub scope: String, // "global" | "project"
    pub dir: String,
    pub enabled: bool,
}

pub fn global_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".casylum").join("skills")
}

pub fn project_dir(root: &str) -> PathBuf {
    Path::new(root).join(".casylum").join("skills")
}

/// Parse `---\nkey: value\n---\nbody`. Returns (name, description, body).
pub fn parse(text: &str) -> (Option<String>, Option<String>, String) {
    let t = text.trim_start_matches('\u{feff}');
    let Some(rest) = t.strip_prefix("---") else { return (None, None, t.to_string()) };
    let Some(end) = rest.find("\n---") else { return (None, None, t.to_string()) };
    let front = &rest[..end];
    let body = rest[end + 4..].trim_start_matches(['\r', '\n']).to_string();
    let mut name = None;
    let mut desc = None;
    for line in front.lines() {
        if let Some((k, v)) = line.split_once(':') {
            let v = v.trim().trim_matches(['"', '\'']).to_string();
            match k.trim() {
                "name" => name = Some(v),
                "description" => desc = Some(v),
                _ => {}
            }
        }
    }
    (name, desc, body)
}

fn scan(dir: &Path, scope: &str, disabled: &[String], out: &mut Vec<Skill>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let md = p.join("SKILL.md");
        let Ok(text) = std::fs::read_to_string(&md) else { continue };
        let folder = p.file_name().unwrap_or_default().to_string_lossy().to_string();
        let (name, desc, _) = parse(&text);
        let name = name.unwrap_or(folder);
        let enabled = !disabled.contains(&format!("{scope}:{name}"));
        out.push(Skill {
            name,
            description: desc.unwrap_or_default(),
            scope: scope.to_string(),
            dir: p.to_string_lossy().to_string(),
            enabled,
        });
    }
}

pub fn list(project_root: Option<&str>, disabled: &[String]) -> Vec<Skill> {
    let mut out = vec![];
    if let Some(r) = project_root.filter(|r| !r.is_empty()) {
        scan(&project_dir(r), "project", disabled, &mut out);
    }
    scan(&global_dir(), "global", disabled, &mut out);
    out.sort_by(|a, b| a.scope.cmp(&b.scope).then(a.name.cmp(&b.name)));
    out
}

/// Full instructions for the agent, plus the other files it may read.
pub fn load(project_root: &str, disabled: &[String], name: &str) -> Result<String> {
    let skill = list(Some(project_root), disabled)
        .into_iter()
        .find(|s| s.enabled && s.name == name)
        .ok_or_else(|| anyhow!("No enabled skill named '{name}'"))?;
    let text = std::fs::read_to_string(Path::new(&skill.dir).join("SKILL.md"))?;
    let (_, _, body) = parse(&text);
    let mut files = vec![];
    for e in ignore::WalkBuilder::new(&skill.dir).max_depth(Some(3)).build().flatten() {
        if e.file_type().is_some_and(|t| t.is_file()) && e.file_name() != "SKILL.md" {
            files.push(e.path().to_string_lossy().to_string());
        }
    }
    let mut out = format!("# Skill: {}\nLocation: {}\n\n{}", skill.name, skill.dir, body);
    if !files.is_empty() {
        out.push_str("\n\nSupporting files in this skill (read them with read_file using the absolute path if needed):\n");
        for f in files.iter().take(50) {
            out.push_str(&format!("- {f}\n"));
        }
    }
    Ok(out)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn save(scope: &str, project_root: Option<&str>, name: &str, content: &str) -> Result<String> {
    if !valid_name(name) {
        return Err(anyhow!("Skill names may use letters, numbers, '-' and '_' (max 64)."));
    }
    let base = match scope {
        "project" => project_dir(project_root.filter(|r| !r.is_empty()).ok_or_else(|| anyhow!("Open a project first"))?),
        _ => global_dir(),
    };
    let dir = base.join(name);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("SKILL.md"), content)?;
    Ok(dir.to_string_lossy().to_string())
}

pub fn read(dir: &str) -> Result<String> {
    Ok(std::fs::read_to_string(Path::new(dir).join("SKILL.md"))?)
}

pub fn delete(dir: &str) -> Result<()> {
    let p = Path::new(dir);
    // Only ever delete folders that are actually skills.
    if !p.join("SKILL.md").is_file() {
        return Err(anyhow!("Not a skill folder"));
    }
    let parent_ok = p.parent().is_some_and(|par| par.ends_with(Path::new(".casylum").join("skills")));
    if !parent_ok {
        return Err(anyhow!("Refusing to delete outside a skills folder"));
    }
    std::fs::remove_dir_all(p)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_frontmatter() {
        let (n, d, b) = super::parse("---\nname: deploy\ndescription: \"Ship it safely\"\n---\n\n# Steps\n1. go");
        assert_eq!(n.as_deref(), Some("deploy"));
        assert_eq!(d.as_deref(), Some("Ship it safely"));
        assert!(b.starts_with("# Steps"));
        let (n, _, b) = super::parse("no frontmatter");
        assert!(n.is_none());
        assert_eq!(b, "no frontmatter");
    }
}
