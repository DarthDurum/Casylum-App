//! The agent's tools. Every path is resolved inside the project root, and
//! every file change records the original content so a run can be undone.

use crate::types::ToolSpec;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

const MAX_OUTPUT: usize = 30_000;
const MAX_META: usize = 200_000;

pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "read_file",
            description: "Read a text file from the project. Returns numbered lines. Use offset/limit for large files.",
            schema: json!({"type": "object", "properties": {
                "path": {"type": "string", "description": "Path relative to the project root"},
                "offset": {"type": "integer", "description": "1-based line to start at"},
                "limit": {"type": "integer", "description": "Max lines to return (default 2000)"}
            }, "required": ["path"]}),
        },
        ToolSpec {
            name: "list_dir",
            description: "List files and folders in a project directory (respects .gitignore).",
            schema: json!({"type": "object", "properties": {
                "path": {"type": "string", "description": "Directory relative to the project root; defaults to the root"},
                "depth": {"type": "integer", "description": "How many levels deep to list (default 1, max 4)"}
            }}),
        },
        ToolSpec {
            name: "search",
            description: "Search file contents with a regular expression (ripgrep-style, respects .gitignore). Returns path:line: text matches.",
            schema: json!({"type": "object", "properties": {
                "pattern": {"type": "string", "description": "Regular expression"},
                "path": {"type": "string", "description": "Directory to search, relative to root"},
                "glob": {"type": "string", "description": "Only files matching this glob, e.g. **/*.ts"}
            }, "required": ["pattern"]}),
        },
        ToolSpec {
            name: "write_file",
            description: "Create a file or overwrite it entirely. Prefer edit_file for changes to existing files.",
            schema: json!({"type": "object", "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"}
            }, "required": ["path", "content"]}),
        },
        ToolSpec {
            name: "edit_file",
            description: "Replace an exact string in a file. old_string must match exactly once unless replace_all is true. Read the file first.",
            schema: json!({"type": "object", "properties": {
                "path": {"type": "string"},
                "old_string": {"type": "string"},
                "new_string": {"type": "string"},
                "replace_all": {"type": "boolean"}
            }, "required": ["path", "old_string", "new_string"]}),
        },
        ToolSpec {
            name: "run_command",
            description: "Run a shell command in the project root and return its output. Use for builds, tests, git and package managers. Not for interactive programs.",
            schema: json!({"type": "object", "properties": {
                "command": {"type": "string"},
                "timeout_secs": {"type": "integer", "description": "Default 120, max 600"}
            }, "required": ["command"]}),
        },
        ToolSpec {
            name: "update_plan",
            description: "Show the user your plan as a checklist. Call at the start of multi-step work and whenever a step changes status. Send the full list each time. Give each step a short title and, where it helps, a one-sentence description of what the step involves (which files, what you'll check or change).",
            schema: json!({"type": "object", "properties": {
                "items": {"type": "array", "items": {"type": "object", "properties": {
                    "text": {"type": "string", "description": "Short step title, a few words"},
                    "description": {"type": "string", "description": "Optional. One sentence on what this step involves"},
                    "status": {"type": "string", "enum": ["pending", "in_progress", "done"]}
                }, "required": ["text", "status"]}}
            }, "required": ["items"]}),
        },
        ToolSpec {
            name: "use_skill",
            description: "Load the full instructions of one of the available skills listed in the system prompt. Call this before doing a task a skill covers.",
            schema: json!({"type": "object", "properties": {
                "name": {"type": "string"}
            }, "required": ["name"]}),
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    Read,
    Edit,
    Command,
}

pub fn risk(name: &str) -> Risk {
    match name {
        "write_file" | "edit_file" => Risk::Edit,
        "run_command" => Risk::Command,
        _ => Risk::Read,
    }
}

pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
    pub meta: Option<Value>,
}

impl ToolOutput {
    fn ok(content: impl Into<String>) -> Self {
        Self { content: content.into(), is_error: false, meta: None }
    }
}

/// Original file contents captured before the first change in a run.
/// `None` means the file did not exist.
pub type Checkpoint = HashMap<PathBuf, Option<String>>;

pub struct ToolCtx<'a> {
    pub root: &'a Path,
    pub disabled_skills: &'a [String],
    pub checkpoint: &'a mut Checkpoint,
}

pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf> {
    let p = Path::new(rel);
    let joined = if p.is_absolute() { p.to_path_buf() } else { root.join(p) };
    // Normalize lexically so files that don't exist yet can be checked too.
    let mut norm = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                norm.pop();
            }
            Component::CurDir => {}
            other => norm.push(other),
        }
    }
    let root_canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    // Resolve symlinks on the deepest existing ancestor.
    let mut existing = norm.clone();
    let mut tail = vec![];
    while !existing.exists() {
        match (existing.file_name().map(|f| f.to_owned()), existing.parent()) {
            (Some(f), Some(par)) => {
                tail.push(f);
                existing = par.to_path_buf();
            }
            _ => break,
        }
    }
    let mut real = existing.canonicalize().unwrap_or(existing);
    for t in tail.into_iter().rev() {
        real.push(t);
    }
    if !real.starts_with(&root_canon) {
        return Err(anyhow!("Path '{rel}' is outside the project"));
    }
    Ok(real)
}

fn rel_display(root: &Path, p: &Path) -> String {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    p.strip_prefix(&root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

fn truncate(mut s: String, max: usize) -> String {
    if s.len() > max {
        let mut cut = max;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        let dropped = s.len() - cut;
        s.truncate(cut);
        s.push_str(&format!("\n… [truncated {dropped} bytes]"));
    }
    s
}

fn str_arg<'a>(input: &'a Value, k: &str) -> Result<&'a str> {
    input.get(k).and_then(|v| v.as_str()).ok_or_else(|| anyhow!("Missing string argument '{k}'"))
}

fn diff_meta(path: &str, before: Option<&str>, after: &str) -> Value {
    let cap = |s: &str| if s.len() > MAX_META { None } else { Some(s.to_string()) };
    let before_lines = before.map(|b| b.lines().count()).unwrap_or(0);
    let after_lines = after.lines().count();
    json!({
        "kind": "diff",
        "path": path,
        "before": before.and_then(cap),
        "after": cap(after),
        "created": before.is_none(),
        "added": after_lines.saturating_sub(before_lines),
        "removed": before_lines.saturating_sub(after_lines),
    })
}

pub async fn run(name: &str, input: &Value, ctx: &mut ToolCtx<'_>) -> ToolOutput {
    if let Some(raw) = input.get("__invalid") {
        return ToolOutput {
            content: format!("The tool input was not valid JSON (it may have been cut off). Please retry. Received: {}", truncate(raw.to_string(), 500)),
            is_error: true,
            meta: None,
        };
    }
    let res = match name {
        "read_file" => read_file(input, ctx),
        "list_dir" => list_dir(input, ctx),
        "search" => search(input, ctx),
        "write_file" => write_file(input, ctx),
        "edit_file" => edit_file(input, ctx),
        "run_command" => run_command(input, ctx).await,
        "update_plan" => Ok(ToolOutput { content: "Plan updated.".into(), is_error: false, meta: Some(json!({"kind": "plan", "items": input["items"]})) }),
        "use_skill" => str_arg(input, "name").and_then(|n| {
            crate::skills::load(&ctx.root.to_string_lossy(), ctx.disabled_skills, n).map(ToolOutput::ok)
        }),
        other => Err(anyhow!("Unknown tool '{other}'")),
    };
    res.unwrap_or_else(|e| ToolOutput { content: format!("Error: {e}"), is_error: true, meta: None })
}

fn read_file(input: &Value, ctx: &ToolCtx) -> Result<ToolOutput> {
    let rel = str_arg(input, "path")?;
    let p = resolve(ctx.root, rel)?;
    let bytes = std::fs::read(&p).map_err(|e| anyhow!("Cannot read {rel}: {e}"))?;
    if bytes.iter().take(8000).any(|b| *b == 0) {
        return Ok(ToolOutput::ok(format!("{rel} looks like a binary file ({} bytes).", bytes.len())));
    }
    let text = String::from_utf8_lossy(&bytes);
    let offset = input["offset"].as_u64().unwrap_or(1).max(1) as usize;
    let limit = input["limit"].as_u64().unwrap_or(2000).clamp(1, 5000) as usize;
    let total = text.lines().count();
    let mut out = String::new();
    for (i, line) in text.lines().enumerate().skip(offset - 1).take(limit) {
        let line = if line.len() > 2000 { &line[..line.char_indices().nth(2000).map(|(i, _)| i).unwrap_or(line.len())] } else { line };
        out.push_str(&format!("{:>5}\t{}\n", i + 1, line));
    }
    if offset - 1 + limit < total {
        out.push_str(&format!("… ({} more lines; use offset to read further)\n", total - (offset - 1 + limit)));
    }
    if out.is_empty() {
        out = "(empty file)".into();
    }
    Ok(ToolOutput::ok(truncate(out, MAX_OUTPUT * 2)))
}

fn list_dir(input: &Value, ctx: &ToolCtx) -> Result<ToolOutput> {
    let rel = input["path"].as_str().unwrap_or(".");
    let dir = resolve(ctx.root, rel)?;
    let depth = input["depth"].as_u64().unwrap_or(1).clamp(1, 4) as usize;
    let mut lines = vec![];
    for e in ignore::WalkBuilder::new(&dir).max_depth(Some(depth)).hidden(false).filter_entry(|e| e.file_name() != ".git").build().flatten() {
        if e.depth() == 0 {
            continue;
        }
        let is_dir = e.file_type().is_some_and(|t| t.is_dir());
        let indent = "  ".repeat(e.depth() - 1);
        let name = e.file_name().to_string_lossy();
        lines.push(format!("{indent}{name}{}", if is_dir { "/" } else { "" }));
        if lines.len() >= 1000 {
            lines.push("… (listing truncated)".into());
            break;
        }
    }
    if lines.is_empty() {
        return Ok(ToolOutput::ok("(empty directory)"));
    }
    Ok(ToolOutput::ok(lines.join("\n")))
}

fn search(input: &Value, ctx: &ToolCtx) -> Result<ToolOutput> {
    let pattern = str_arg(input, "pattern")?;
    let re = regex::Regex::new(pattern).map_err(|e| anyhow!("Invalid regex: {e}"))?;
    let dir = resolve(ctx.root, input["path"].as_str().unwrap_or("."))?;
    let glob = match input["glob"].as_str().filter(|g| !g.is_empty()) {
        Some(g) => Some(globset::Glob::new(g)?.compile_matcher()),
        None => None,
    };
    let mut out = vec![];
    let mut files = 0;
    'walk: for e in ignore::WalkBuilder::new(&dir).hidden(false).filter_entry(|e| e.file_name() != ".git").build().flatten() {
        if !e.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let rel = rel_display(ctx.root, e.path());
        if let Some(g) = &glob {
            if !g.is_match(&rel) && !g.is_match(e.file_name()) {
                continue;
            }
        }
        if e.metadata().map(|m| m.len() > 2_000_000).unwrap_or(true) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
        let mut hit = false;
        for (i, line) in text.lines().enumerate() {
            if re.is_match(line) {
                hit = true;
                let l: String = line.chars().take(300).collect();
                out.push(format!("{rel}:{}: {}", i + 1, l.trim_end()));
                if out.len() >= 300 {
                    out.push("… (more matches; narrow the search)".into());
                    break 'walk;
                }
            }
        }
        files += hit as usize;
    }
    if out.is_empty() {
        return Ok(ToolOutput::ok("No matches."));
    }
    Ok(ToolOutput::ok(truncate(format!("{} matches in {files} files\n{}", out.len(), out.join("\n")), MAX_OUTPUT)))
}

fn remember(ctx: &mut ToolCtx, p: &Path) {
    if !ctx.checkpoint.contains_key(p) {
        ctx.checkpoint.insert(p.to_path_buf(), std::fs::read_to_string(p).ok());
    }
}

fn write_file(input: &Value, ctx: &mut ToolCtx) -> Result<ToolOutput> {
    let rel = str_arg(input, "path")?;
    let content = str_arg(input, "content")?;
    let p = resolve(ctx.root, rel)?;
    let before = std::fs::read_to_string(&p).ok();
    remember(ctx, &p);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&p, content)?;
    let shown = rel_display(ctx.root, &p);
    Ok(ToolOutput {
        content: format!("{} {shown} ({} lines).", if before.is_some() { "Overwrote" } else { "Created" }, content.lines().count()),
        is_error: false,
        meta: Some(diff_meta(&shown, before.as_deref(), content)),
    })
}

fn edit_file(input: &Value, ctx: &mut ToolCtx) -> Result<ToolOutput> {
    let rel = str_arg(input, "path")?;
    let old = str_arg(input, "old_string")?;
    let new = str_arg(input, "new_string")?;
    let all = input["replace_all"].as_bool().unwrap_or(false);
    let p = resolve(ctx.root, rel)?;
    let before = std::fs::read_to_string(&p).map_err(|e| anyhow!("Cannot read {rel}: {e}"))?;
    if old.is_empty() {
        return Err(anyhow!("old_string is empty; use write_file to create files"));
    }
    let count = before.matches(old).count();
    let after = match (count, all) {
        (0, _) => return Err(anyhow!("old_string was not found in {rel}. Read the file again and copy the text exactly.")),
        (1, _) | (_, true) => before.replace(old, new),
        (n, false) => return Err(anyhow!("old_string matches {n} places in {rel}. Add surrounding lines to make it unique, or set replace_all.")),
    };
    remember(ctx, &p);
    std::fs::write(&p, &after)?;
    let shown = rel_display(ctx.root, &p);
    let mut meta = diff_meta(&shown, Some(&before), &after);
    meta["added"] = json!(new.lines().count() * count);
    meta["removed"] = json!(old.lines().count() * count);
    Ok(ToolOutput { content: format!("Edited {shown} ({count} replacement{}).", if count == 1 { "" } else { "s" }), is_error: false, meta: Some(meta) })
}

async fn run_command(input: &Value, ctx: &ToolCtx<'_>) -> Result<ToolOutput> {
    let cmd = str_arg(input, "command")?;
    let secs = input["timeout_secs"].as_u64().unwrap_or(120).clamp(1, 600);
    let mut c = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    } else {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let mut c = tokio::process::Command::new(shell);
        c.arg("-lc").arg(cmd);
        c
    };
    c.current_dir(ctx.root).stdin(std::process::Stdio::null()).kill_on_drop(true);
    let started = std::time::Instant::now();
    let out = match tokio::time::timeout(Duration::from_secs(secs), c.output()).await {
        Ok(r) => r?,
        Err(_) => return Ok(ToolOutput { content: format!("Command timed out after {secs}s and was stopped."), is_error: true, meta: None }),
    };
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&err);
    }
    let code = out.status.code().unwrap_or(-1);
    let body = truncate(text, MAX_OUTPUT);
    Ok(ToolOutput {
        content: format!("Exit code {code}\n{body}"),
        is_error: !out.status.success(),
        meta: Some(json!({"kind": "command", "exit_code": code, "duration_ms": started.elapsed().as_millis() as u64})),
    })
}

/// Restore every file captured in a checkpoint. Returns the paths restored.
pub fn restore(cp: &Checkpoint) -> Vec<String> {
    let mut done = vec![];
    for (p, orig) in cp {
        let ok = match orig {
            Some(text) => std::fs::write(p, text).is_ok(),
            None => std::fs::remove_file(p).is_ok() || !p.exists(),
        };
        if ok {
            done.push(p.to_string_lossy().to_string());
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("casylum-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn rejects_paths_outside_root() {
        let root = tmp();
        assert!(resolve(&root, "../etc/passwd").is_err());
        assert!(resolve(&root, "/etc/passwd").is_err());
        assert!(resolve(&root, "a/../../x").is_err());
        assert!(resolve(&root, "src/new/file.rs").is_ok());
    }

    #[tokio::test]
    async fn edit_write_and_restore() {
        let root = tmp();
        std::fs::write(root.join("a.txt"), "hello world\nhello again\n").unwrap();
        let mut cp = Checkpoint::new();
        let disabled: Vec<String> = vec![];
        let mut ctx = ToolCtx { root: &root, disabled_skills: &disabled, checkpoint: &mut cp };

        // Ambiguous match is refused.
        let out = run("edit_file", &json!({"path": "a.txt", "old_string": "hello", "new_string": "bye"}), &mut ctx).await;
        assert!(out.is_error, "{}", out.content);

        let out = run("edit_file", &json!({"path": "a.txt", "old_string": "hello world", "new_string": "bye world"}), &mut ctx).await;
        assert!(!out.is_error, "{}", out.content);
        assert_eq!(out.meta.as_ref().unwrap()["kind"], "diff");

        let out = run("write_file", &json!({"path": "sub/new.txt", "content": "x\n"}), &mut ctx).await;
        assert!(!out.is_error, "{}", out.content);

        let out = run("search", &json!({"pattern": "bye"}), &mut ctx).await;
        assert!(out.content.contains("a.txt:1"), "{}", out.content);

        let out = run("read_file", &json!({"path": "a.txt"}), &mut ctx).await;
        assert!(out.content.contains("bye world"));

        // Undo puts everything back, including deleting the created file.
        restore(&cp);
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "hello world\nhello again\n");
        assert!(!root.join("sub/new.txt").exists());
    }

    #[tokio::test]
    async fn run_command_reports_exit_code() {
        let root = tmp();
        let mut cp = Checkpoint::new();
        let disabled: Vec<String> = vec![];
        let mut ctx = ToolCtx { root: &root, disabled_skills: &disabled, checkpoint: &mut cp };
        let out = run("run_command", &json!({"command": "echo hi && exit 3"}), &mut ctx).await;
        assert!(out.is_error);
        assert!(out.content.contains("Exit code 3") && out.content.contains("hi"), "{}", out.content);
    }

    #[test]
    fn invalid_json_input_is_reported() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let root = tmp();
        let mut cp = Checkpoint::new();
        let disabled: Vec<String> = vec![];
        let mut ctx = ToolCtx { root: &root, disabled_skills: &disabled, checkpoint: &mut cp };
        let out = rt.block_on(run("write_file", &json!({"__invalid": "{\"path\": \"a"}), &mut ctx));
        assert!(out.is_error);
    }
}
