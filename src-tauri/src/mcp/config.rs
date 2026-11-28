//! MCP server configuration, in the same JSON shape Claude Desktop and
//! Claude Code use, so existing configs can be pasted as they are:
//!
//! ```json
//! { "mcpServers": {
//!     "files":  { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path"], "env": {} },
//!     "remote": { "type": "http", "url": "https://example.com/mcp", "headers": { "Authorization": "Bearer ..." } },
//!     "legacy": { "type": "sse",  "url": "https://example.com/sse" }
//! } }
//! ```
//!
//! Casylum also reads `"disabled": true`, `"autoApprove": true | ["tool", ...]`,
//! `"cwd"` and `"timeout"` (seconds per tool call). Unknown keys are kept.

use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum Transport {
    Stdio { command: String, args: Vec<String>, env: BTreeMap<String, String>, cwd: Option<String> },
    /// Streamable HTTP (falls back to legacy SSE if the server only speaks that).
    Http { url: String, headers: BTreeMap<String, String> },
    /// Legacy HTTP+SSE transport.
    Sse { url: String, headers: BTreeMap<String, String> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AutoApprove {
    None,
    All,
    Tools(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerConfig {
    pub transport: Transport,
    pub disabled: bool,
    pub auto_approve: AutoApprove,
    pub timeout_secs: u64,
}

impl AutoApprove {
    pub fn allows(&self, tool: &str) -> bool {
        match self {
            AutoApprove::None => false,
            AutoApprove::All => true,
            AutoApprove::Tools(t) => t.iter().any(|x| x == tool),
        }
    }
}

pub fn path() -> PathBuf {
    crate::settings::data_dir().join("mcp.json")
}

pub const EMPTY: &str = "{\n  \"mcpServers\": {}\n}\n";

/// The config file as the user wrote it.
pub fn read_text() -> String {
    std::fs::read_to_string(path()).unwrap_or_else(|_| EMPTY.to_string())
}

/// Find the server map in what the user pasted: `{"mcpServers": {...}}`
/// (Claude), `{"servers": {...}}` (VS Code), or a bare `{name: {...}}`.
pub fn servers_of(root: &Value) -> Result<Map<String, Value>> {
    let obj = root.as_object().ok_or_else(|| anyhow!("The config must be a JSON object."))?;
    for key in ["mcpServers", "servers", "mcp_servers"] {
        if let Some(v) = obj.get(key) {
            return v.as_object().cloned().ok_or_else(|| anyhow!("\"{key}\" must be an object of servers."));
        }
    }
    if obj.is_empty() {
        return Ok(Map::new());
    }
    // A bare map of servers: every value looks like a server.
    if obj.values().all(|v| v.get("command").is_some() || v.get("url").is_some() || v.get("serverUrl").is_some()) {
        return Ok(obj.clone());
    }
    // A single server pasted on its own is ambiguous (no name), so say so.
    Err(anyhow!("Couldn't find \"mcpServers\" in the config."))
}

/// Normalise any accepted shape to `{"mcpServers": {...}}`, keeping extra keys.
pub fn normalise(text: &str) -> Result<Value> {
    let root: Value = serde_json::from_str(text).map_err(|e| anyhow!("Invalid JSON: {e}"))?;
    let servers = servers_of(&root)?;
    for (name, v) in &servers {
        parse_server(name, v)?;
    }
    let mut out = root.as_object().cloned().unwrap_or_default();
    out.remove("servers");
    out.remove("mcp_servers");
    if !out.contains_key("mcpServers") {
        // It was a bare map: drop the server entries from the top level.
        out.retain(|k, _| !servers.contains_key(k));
    }
    out.insert("mcpServers".into(), Value::Object(servers));
    Ok(Value::Object(out))
}

pub fn parse_all(text: &str) -> Result<BTreeMap<String, ServerConfig>> {
    let root: Value = serde_json::from_str(text).map_err(|e| anyhow!("Invalid JSON: {e}"))?;
    let mut out = BTreeMap::new();
    for (name, v) in servers_of(&root)? {
        out.insert(name.clone(), parse_server(&name, &v)?);
    }
    Ok(out)
}

fn str_map(v: Option<&Value>) -> BTreeMap<String, String> {
    v.and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), expand(&v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))))
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_server(name: &str, v: &Value) -> Result<ServerConfig> {
    if name.trim().is_empty() {
        return Err(anyhow!("Every server needs a name."));
    }
    let obj = v.as_object().ok_or_else(|| anyhow!("Server \"{name}\" must be an object."))?;
    let kind = obj.get("type").or_else(|| obj.get("transport")).and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
    let url = obj.get("url").or_else(|| obj.get("serverUrl")).and_then(|u| u.as_str()).map(expand);
    let transport = if let Some(cmd) = obj.get("command").and_then(|c| c.as_str()) {
        if cmd.trim().is_empty() {
            return Err(anyhow!("Server \"{name}\" has an empty command."));
        }
        let args = obj
            .get("args")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().map(|x| expand(&x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string()))).collect())
            .unwrap_or_default();
        Transport::Stdio {
            command: expand(cmd),
            args,
            env: str_map(obj.get("env")),
            cwd: obj.get("cwd").and_then(|c| c.as_str()).filter(|c| !c.is_empty()).map(expand),
        }
    } else if let Some(url) = url {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(anyhow!("Server \"{name}\": url must start with http:// or https://"));
        }
        let headers = str_map(obj.get("headers"));
        if kind == "sse" {
            Transport::Sse { url, headers }
        } else {
            Transport::Http { url, headers }
        }
    } else {
        return Err(anyhow!("Server \"{name}\" needs a \"command\" (local) or a \"url\" (remote)."));
    };
    let auto_approve = match obj.get("autoApprove").or_else(|| obj.get("alwaysAllow")) {
        Some(Value::Bool(true)) => AutoApprove::All,
        Some(Value::Array(a)) => AutoApprove::Tools(a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()),
        _ => AutoApprove::None,
    };
    let timeout_secs = obj.get("timeout").and_then(|t| t.as_u64()).map(|t| if t > 10_000 { t / 1000 } else { t }).unwrap_or(600).clamp(5, 3600);
    Ok(ServerConfig {
        transport,
        disabled: obj.get("disabled").and_then(|d| d.as_bool()).unwrap_or(false),
        auto_approve,
        timeout_secs,
    })
}

/// Expand `${VAR}` and `${VAR:-default}` from the environment, like Claude Code.
pub fn expand(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("${") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[i..]);
            return out;
        };
        let inner = &after[..end];
        let (var, default) = match inner.split_once(":-") {
            Some((v, d)) => (v, Some(d)),
            None => (inner, None),
        };
        match std::env::var(var) {
            Ok(v) if !v.is_empty() => out.push_str(&v),
            _ => match default {
                Some(d) => out.push_str(d),
                None => {
                    out.push_str("${");
                    out.push_str(inner);
                    out.push('}');
                }
            },
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

pub fn save_value(v: &Value) -> Result<()> {
    let p = path();
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(v)?)?;
    std::fs::rename(tmp, p)?;
    Ok(())
}

/// Save pasted or edited config text after checking it.
pub fn save_text(text: &str) -> Result<()> {
    save_value(&normalise(text)?)
}

fn load_value() -> Result<Value> {
    normalise(&read_text())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 48 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Add or replace one server. `previous` renames an existing entry.
pub fn put_server(name: &str, server: Value, previous: Option<&str>) -> Result<()> {
    if !valid_name(name) {
        return Err(anyhow!("Server names may use letters, numbers, '-', '_' and '.' (max 48)."));
    }
    parse_server(name, &server)?;
    let mut root = load_value()?;
    let servers = root["mcpServers"].as_object_mut().expect("normalised");
    if let Some(prev) = previous.filter(|p| *p != name) {
        servers.remove(prev);
    }
    servers.insert(name.to_string(), server);
    save_value(&root)
}

pub fn remove_server(name: &str) -> Result<()> {
    let mut root = load_value()?;
    root["mcpServers"].as_object_mut().expect("normalised").remove(name);
    save_value(&root)
}

pub fn set_disabled(name: &str, disabled: bool) -> Result<()> {
    let mut root = load_value()?;
    let s = root["mcpServers"].get_mut(name).and_then(|s| s.as_object_mut()).ok_or_else(|| anyhow!("No server named {name}"))?;
    if disabled {
        s.insert("disabled".into(), json!(true));
    } else {
        s.remove("disabled");
    }
    save_value(&root)
}

/// Config files from other apps whose servers can be imported.
fn other_configs() -> Vec<(&'static str, PathBuf)> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut out = vec![];
    // Claude Desktop
    #[cfg(target_os = "macos")]
    out.push(("Claude Desktop", home.join("Library/Application Support/Claude/claude_desktop_config.json")));
    #[cfg(windows)]
    if let Some(d) = dirs::config_dir() {
        out.push(("Claude Desktop", d.join("Claude").join("claude_desktop_config.json")));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    out.push(("Claude Desktop", home.join(".config/Claude/claude_desktop_config.json")));
    out.push(("Claude Code", home.join(".claude.json")));
    out.push(("Cursor", home.join(".cursor/mcp.json")));
    out.push(("Windsurf", home.join(".codeium/windsurf/mcp_config.json")));
    out
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportReport {
    pub added: Vec<String>,
    pub skipped: Vec<String>,
    pub sources: Vec<String>,
}

/// Copy servers from Claude Desktop, Claude Code, Cursor and Windsurf into
/// Casylum's config. Servers whose name is already used are left alone.
pub fn import_from_apps() -> Result<ImportReport> {
    let mut root = load_value()?;
    let mut report = ImportReport { added: vec![], skipped: vec![], sources: vec![] };
    for (app, path) in other_configs() {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
        let Some(found) = v.get("mcpServers").and_then(|m| m.as_object()) else { continue };
        if found.is_empty() {
            continue;
        }
        report.sources.push(app.to_string());
        let servers = root["mcpServers"].as_object_mut().expect("normalised");
        for (name, server) in found {
            if servers.contains_key(name) || parse_server(name, server).is_err() {
                report.skipped.push(name.clone());
                continue;
            }
            servers.insert(name.clone(), server.clone());
            report.added.push(name.clone());
        }
    }
    if !report.added.is_empty() {
        save_value(&root)?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_desktop_config() {
        let text = r#"{"mcpServers": {
            "fs": {"command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]},
            "web": {"type": "http", "url": "https://x.dev/mcp", "headers": {"Authorization": "Bearer t"}},
            "old": {"type": "sse", "url": "https://x.dev/sse", "disabled": true, "autoApprove": ["read"]}
        }}"#;
        let all = parse_all(text).unwrap();
        assert!(matches!(&all["fs"].transport, Transport::Stdio { command, args, .. } if command == "npx" && args.len() == 3));
        assert!(matches!(&all["web"].transport, Transport::Http { headers, .. } if headers["Authorization"] == "Bearer t"));
        assert!(matches!(&all["old"].transport, Transport::Sse { .. }));
        assert!(all["old"].disabled);
        assert!(all["old"].auto_approve.allows("read") && !all["old"].auto_approve.allows("write"));
    }

    #[test]
    fn accepts_bare_and_vscode_shapes() {
        let bare = normalise(r#"{"fs": {"command": "uvx", "args": ["mcp-server-git"]}}"#).unwrap();
        assert!(bare["mcpServers"]["fs"].is_object());
        assert!(bare.get("fs").is_none());
        let vs = normalise(r#"{"servers": {"gh": {"type": "http", "url": "https://api.githubcopilot.com/mcp/"}}}"#).unwrap();
        assert!(vs["mcpServers"]["gh"].is_object());
        assert!(normalise(r#"{"mcpServers": {"bad": {"args": []}}}"#).is_err());
    }

    #[test]
    fn expands_env_vars() {
        std::env::set_var("CASYLUM_TEST_TOKEN", "abc");
        assert_eq!(expand("Bearer ${CASYLUM_TEST_TOKEN}"), "Bearer abc");
        assert_eq!(expand("${CASYLUM_NOPE:-def}/x"), "def/x");
        assert_eq!(expand("${CASYLUM_NOPE}"), "${CASYLUM_NOPE}");
    }
}
