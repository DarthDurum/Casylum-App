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

#[derive(Debug, Clone, Serialize)]
pub struct Imported {
    pub name: String,
    pub dir: String,
    /// A skill with this name was already there and has been replaced.
    pub replaced: bool,
}

fn folder_name(raw: &str) -> String {
    let mut s: String = raw.trim().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let s = s.trim_matches('-').chars().take(64).collect::<String>();
    s
}

const MAX_ZIP_BYTES: u64 = 100 * 1024 * 1024;
const MAX_ZIP_FILES: usize = 5000;

/// Install skills from a .zip (or Claude's .skill, which is a zip). The
/// archive may hold one skill at its root, one skill folder, or several.
pub fn import_zip(zip_path: &str, scope: &str, project_root: Option<&str>) -> Result<Vec<Imported>> {
    use std::io::Read;
    let base = match scope {
        "project" => project_dir(project_root.filter(|r| !r.is_empty()).ok_or_else(|| anyhow!("Choose a folder for this chat first to add project skills."))?),
        _ => global_dir(),
    };
    let file = std::fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| anyhow!("Not a valid zip file: {e}"))?;
    if zip.len() > MAX_ZIP_FILES {
        return Err(anyhow!("The zip has too many files ({}).", zip.len()));
    }
    // Every file, with a safe relative path.
    let mut entries: Vec<(usize, PathBuf)> = vec![];
    let mut total = 0u64;
    for i in 0..zip.len() {
        let f = zip.by_index(i)?;
        let Some(p) = f.enclosed_name() else { continue };
        if f.is_dir() || p.components().any(|c| c.as_os_str() == "__MACOSX") || p.file_name().is_some_and(|n| n == ".DS_Store") {
            continue;
        }
        total += f.size();
        if total > MAX_ZIP_BYTES {
            return Err(anyhow!("The zip is too large once unpacked (over 100 MB)."));
        }
        entries.push((i, p));
    }
    // Skill roots: folders holding a SKILL.md, shallowest first.
    let mut roots: Vec<(usize, PathBuf)> = entries
        .iter()
        .filter(|(_, p)| p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("SKILL.md")))
        .map(|(i, p)| (*i, p.parent().map(Path::to_path_buf).unwrap_or_default()))
        .collect();
    roots.sort_by_key(|(_, p)| p.components().count());
    let mut kept: Vec<(usize, PathBuf)> = vec![];
    for r in roots {
        if !kept.iter().any(|(_, k)| r.1.starts_with(k)) {
            kept.push(r);
        }
    }
    if kept.is_empty() {
        return Err(anyhow!("No SKILL.md found in this file."));
    }
    let stem = Path::new(zip_path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "skill".into());
    let mut out = vec![];
    for (md_index, root) in kept {
        let mut text = String::new();
        zip.by_index(md_index)?.read_to_string(&mut text).map_err(|e| anyhow!("SKILL.md isn't readable text: {e}"))?;
        let (name, _, _) = parse(&text);
        let fallback = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| stem.clone());
        let folder = name.as_deref().map(folder_name).filter(|n| !n.is_empty()).unwrap_or_else(|| folder_name(&fallback));
        if !valid_name(&folder) {
            return Err(anyhow!("Couldn't work out a name for the skill in {}.", root.display()));
        }
        let dir = base.join(&folder);
        let replaced = dir.exists();
        if replaced {
            if !dir.join("SKILL.md").is_file() {
                return Err(anyhow!("{} exists and isn't a skill; not replacing it.", dir.display()));
            }
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        for (i, p) in &entries {
            let Ok(rel) = p.strip_prefix(&root) else { continue };
            // Only files inside this skill (not a nested skill's own root).
            let target = dir.join(rel);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut f = zip.by_index(*i)?;
            let mut w = std::fs::File::create(&target)?;
            std::io::copy(&mut f, &mut w)?;
        }
        // Keep the canonical file name even if the zip used another case.
        let md = dir.join("SKILL.md");
        if !md.is_file() {
            std::fs::write(&md, &text)?;
        }
        out.push(Imported { name: name.unwrap_or(folder), dir: dir.to_string_lossy().to_string(), replaced });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn imports_skills_from_zip() {
        use std::io::Write;
        let tmp = std::env::temp_dir().join(format!("casylum-zip-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let zp = tmp.join("pack.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zp).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("pack/pdf-tools/SKILL.md", o).unwrap();
            z.write_all(b"---\nname: pdf-tools\ndescription: Work with PDFs\n---\nBody").unwrap();
            z.start_file("pack/pdf-tools/scripts/run.py", o).unwrap();
            z.write_all(b"print(1)").unwrap();
            z.start_file("pack/other/SKILL.md", o).unwrap();
            z.write_all(b"no frontmatter").unwrap();
            z.start_file("__MACOSX/pack/._x", o).unwrap();
            z.write_all(b"junk").unwrap();
            z.finish().unwrap();
        }
        let project = tmp.join("proj");
        std::fs::create_dir_all(&project).unwrap();
        let got = super::import_zip(&zp.to_string_lossy(), "project", Some(&project.to_string_lossy())).unwrap();
        assert_eq!(got.len(), 2);
        let skills = super::project_dir(&project.to_string_lossy());
        assert!(skills.join("pdf-tools/scripts/run.py").is_file());
        assert!(skills.join("other/SKILL.md").is_file());
        assert!(!got[0].replaced);
        // Importing again replaces.
        let again = super::import_zip(&zp.to_string_lossy(), "project", Some(&project.to_string_lossy())).unwrap();
        assert!(again.iter().all(|i| i.replaced));
        let _ = std::fs::remove_dir_all(&tmp);
    }

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
