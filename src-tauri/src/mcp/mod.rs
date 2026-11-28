//! MCP (Model Context Protocol) support: connect the servers in `mcp.json`,
//! keep their tool lists, and expose those tools to the agent as
//! `mcp__<server>__<tool>` for every provider.

pub mod client;
pub mod config;

use crate::types::ToolSpec;
use client::{Client, Logs};
use config::{ServerConfig, Transport};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    #[serde(skip)]
    pub schema: Value,
    pub read_only: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Disabled,
    Connecting,
    Connected,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerStatus {
    pub name: String,
    /// "stdio" | "http" | "sse"
    pub transport: String,
    /// Command line or URL, for display.
    pub target: String,
    pub status: Status,
    pub error: Option<String>,
    pub tools: Vec<McpTool>,
    pub server_name: Option<String>,
    pub server_version: Option<String>,
    pub auto_approve_all: bool,
}

struct Server {
    config: ServerConfig,
    status: Status,
    error: Option<String>,
    tools: Vec<McpTool>,
    instructions: Option<String>,
    info: Option<(String, String)>,
    client: Option<Arc<Client>>,
    logs: Logs,
    /// Bumped on every (re)connect so stale tasks can tell they lost.
    generation: u64,
}

pub struct Manager {
    servers: Mutex<BTreeMap<String, Server>>,
    http: reqwest::Client,
    app: Mutex<Option<AppHandle>>,
    next_gen: Mutex<u64>,
}

/// `mcp__<server>__<tool>`, limited to what every provider accepts
/// (letters, digits, `_` and `-`, at most 64 characters).
pub fn tool_name(server: &str, tool: &str) -> String {
    let clean = |s: &str| s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect::<String>();
    let full = format!("mcp__{}__{}", clean(server), clean(tool));
    if full.len() <= 64 {
        return full;
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    full.hash(&mut h);
    format!("{}_{:08x}", &full[..55], h.finish() as u32)
}

pub fn is_mcp_tool(name: &str) -> bool {
    name.starts_with("mcp__")
}

fn describe(t: &Transport) -> (String, String) {
    match t {
        Transport::Stdio { command, args, .. } => ("stdio".into(), format!("{} {}", command, args.join(" ")).trim().to_string()),
        Transport::Http { url, .. } => ("http".into(), url.clone()),
        Transport::Sse { url, .. } => ("sse".into(), url.clone()),
    }
}

/// Providers want an object schema with properties.
fn normalise_schema(s: Option<&Value>) -> Value {
    let mut s = s.cloned().filter(|v| v.is_object()).unwrap_or_else(|| json!({"type": "object"}));
    if s.get("type").is_none() {
        s["type"] = json!("object");
    }
    if s.get("properties").map_or(true, |p| !p.is_object()) {
        s["properties"] = json!({});
    }
    s
}

fn parse_tools(list: Vec<Value>) -> Vec<McpTool> {
    list.into_iter()
        .filter_map(|t| {
            let name = t.get("name")?.as_str()?.to_string();
            let mut description = t.get("description").and_then(|d| d.as_str()).unwrap_or("").trim().to_string();
            if description.is_empty() {
                description = t.get("title").and_then(|d| d.as_str()).unwrap_or(&name).to_string();
            }
            if description.chars().count() > 1024 {
                description = description.chars().take(1020).collect::<String>() + "…";
            }
            let read_only = t.pointer("/annotations/readOnlyHint").and_then(|v| v.as_bool()).unwrap_or(false);
            Some(McpTool { name, description, schema: normalise_schema(t.get("inputSchema")), read_only })
        })
        .collect()
}

impl Manager {
    pub fn new(http: reqwest::Client) -> Arc<Self> {
        Arc::new(Self { servers: Mutex::new(BTreeMap::new()), http, app: Mutex::new(None), next_gen: Mutex::new(0) })
    }

    pub fn set_app(&self, app: AppHandle) {
        *self.app.lock() = Some(app);
    }

    fn changed(&self) {
        if let Some(app) = self.app.lock().as_ref() {
            let _ = app.emit("mcp", ());
        }
    }

    /// Bring running servers in line with `mcp.json`: start new or changed
    /// ones, stop removed or disabled ones. Returns a config error, if any.
    pub fn sync(self: &Arc<Self>) -> Option<String> {
        let parsed = config::parse_all(&config::read_text());
        let (wanted, err) = match parsed {
            Ok(w) => (w, None),
            Err(e) => (BTreeMap::new(), Some(e.to_string())),
        };
        if err.is_some() {
            // Keep what's running rather than tearing everything down over a typo.
            return err;
        }
        let mut to_start = vec![];
        {
            let mut servers = self.servers.lock();
            servers.retain(|name, s| {
                let keep = wanted.get(name).is_some_and(|c| *c == s.config);
                if !keep {
                    if let Some(c) = s.client.take() {
                        c.shutdown();
                    }
                }
                keep
            });
            for (name, cfg) in wanted {
                if servers.contains_key(&name) {
                    continue;
                }
                let disabled = cfg.disabled;
                servers.insert(
                    name.clone(),
                    Server {
                        config: cfg,
                        status: if disabled { Status::Disabled } else { Status::Connecting },
                        error: None,
                        tools: vec![],
                        instructions: None,
                        info: None,
                        client: None,
                        logs: Arc::new(Mutex::new(VecDeque::new())),
                        generation: 0,
                    },
                );
                if !disabled {
                    to_start.push(name);
                }
            }
        }
        for name in to_start {
            self.start(&name);
        }
        self.changed();
        None
    }

    /// (Re)connect one server in the background.
    pub fn start(self: &Arc<Self>, name: &str) {
        let gen = {
            let mut g = self.next_gen.lock();
            *g += 1;
            *g
        };
        let (cfg, logs) = {
            let mut servers = self.servers.lock();
            let Some(s) = servers.get_mut(name) else { return };
            if s.config.disabled {
                return;
            }
            if let Some(c) = s.client.take() {
                c.shutdown();
            }
            s.status = Status::Connecting;
            s.error = None;
            s.generation = gen;
            (s.config.clone(), s.logs.clone())
        };
        self.changed();
        let me = self.clone();
        let name = name.to_string();
        tauri::async_runtime::spawn(async move {
            let res = me.connect(&name, &cfg, logs.clone(), gen).await;
            let mut servers = me.servers.lock();
            let Some(s) = servers.get_mut(&name) else { return };
            if s.generation != gen {
                return;
            }
            match res {
                Ok((client, tools, instructions, info)) => {
                    s.client = Some(client);
                    s.tools = tools;
                    s.instructions = instructions;
                    s.info = info;
                    s.status = Status::Connected;
                    s.error = None;
                }
                Err(e) => {
                    s.client = None;
                    s.tools.clear();
                    s.status = Status::Error;
                    let mut msg = e.to_string();
                    // The last stderr lines usually say what went wrong.
                    let tail: Vec<String> = logs.lock().iter().rev().take(4).cloned().collect::<Vec<_>>().into_iter().rev().collect();
                    if matches!(cfg.transport, Transport::Stdio { .. }) && !tail.is_empty() {
                        msg = format!("{msg}\n{}", tail.join("\n"));
                    }
                    client::log(&logs, format!("Error: {e}"));
                    s.error = Some(msg);
                }
            }
            drop(servers);
            me.changed();
        });
    }

    async fn connect(
        self: &Arc<Self>,
        name: &str,
        cfg: &ServerConfig,
        logs: Logs,
        gen: u64,
    ) -> anyhow::Result<(Arc<Client>, Vec<McpTool>, Option<String>, Option<(String, String)>)> {
        let weak = Arc::downgrade(self);
        let server = name.to_string();
        let make_cb = move || -> Box<dyn Fn() + Send + Sync> {
            let weak = weak.clone();
            let server = server.clone();
            Box::new(move || {
                if let Some(m) = weak.upgrade() {
                    let s = server.clone();
                    tauri::async_runtime::spawn(async move { m.refresh_tools(&s, gen).await });
                }
            })
        };
        // npx/uvx may download the package on first run, so allow time.
        let init_timeout = Duration::from_secs(if matches!(cfg.transport, Transport::Stdio { .. }) { 120 } else { 45 });
        let client = Client::connect(&cfg.transport, self.http.clone(), logs.clone(), make_cb()).await?;
        let init = match client.initialize(init_timeout).await {
            Ok(r) => r,
            // A server that only speaks the older HTTP+SSE transport.
            Err(e) if matches!(&cfg.transport, Transport::Http { .. }) && e.downcast_ref::<client::HttpError>().is_some_and(|h| matches!(h.status, 400 | 404 | 405)) => {
                let Transport::Http { url, headers } = &cfg.transport else { unreachable!() };
                client::log(&logs, format!("Streamable HTTP failed ({e}); trying SSE"));
                let sse = Client::connect(&Transport::Sse { url: url.clone(), headers: headers.clone() }, self.http.clone(), logs.clone(), make_cb()).await?;
                let r = sse.initialize(init_timeout).await?;
                return self.finish_connect(sse, r).await;
            }
            Err(e) => {
                client.shutdown();
                return Err(e);
            }
        };
        self.finish_connect(client, init).await
    }

    async fn finish_connect(
        &self,
        client: Arc<Client>,
        init: Value,
    ) -> anyhow::Result<(Arc<Client>, Vec<McpTool>, Option<String>, Option<(String, String)>)> {
        let has_tools = init.pointer("/capabilities/tools").is_some();
        let tools = if has_tools { parse_tools(client.list_tools().await?) } else { client.list_tools().await.map(parse_tools).unwrap_or_default() };
        let instructions = init.get("instructions").and_then(|i| i.as_str()).filter(|i| !i.trim().is_empty()).map(|i| i.chars().take(4000).collect());
        let info = init.get("serverInfo").map(|i| (i["name"].as_str().unwrap_or("").to_string(), i["version"].as_str().unwrap_or("").to_string()));
        Ok((client, tools, instructions, info))
    }

    async fn refresh_tools(self: &Arc<Self>, name: &str, gen: u64) {
        let client = {
            let servers = self.servers.lock();
            match servers.get(name) {
                Some(s) if s.generation == gen => s.client.clone(),
                _ => None,
            }
        };
        let Some(client) = client else { return };
        if let Ok(list) = client.list_tools().await {
            if let Some(s) = self.servers.lock().get_mut(name) {
                if s.generation == gen {
                    s.tools = parse_tools(list);
                }
            }
            self.changed();
        }
    }

    pub fn statuses(&self) -> Vec<ServerStatus> {
        self.servers
            .lock()
            .iter()
            .map(|(name, s)| {
                let (transport, target) = describe(&s.config.transport);
                ServerStatus {
                    name: name.clone(),
                    transport,
                    target,
                    status: s.status,
                    error: s.error.clone(),
                    tools: s.tools.clone(),
                    server_name: s.info.as_ref().map(|i| i.0.clone()).filter(|n| !n.is_empty()),
                    server_version: s.info.as_ref().map(|i| i.1.clone()).filter(|n| !n.is_empty()),
                    auto_approve_all: s.config.auto_approve == config::AutoApprove::All,
                }
            })
            .collect()
    }

    pub fn logs(&self, name: &str) -> Vec<String> {
        self.servers.lock().get(name).map(|s| s.logs.lock().iter().cloned().collect()).unwrap_or_default()
    }

    /// Tool definitions for every connected server.
    pub fn specs(&self) -> Vec<ToolSpec> {
        let servers = self.servers.lock();
        let mut out = vec![];
        for (name, s) in servers.iter() {
            if s.status != Status::Connected {
                continue;
            }
            for t in &s.tools {
                out.push(ToolSpec {
                    name: tool_name(name, &t.name),
                    description: format!("[MCP server: {name}] {}", t.description),
                    schema: t.schema.clone(),
                });
            }
        }
        out
    }

    /// Lines for the system prompt: connected servers and their instructions.
    pub fn prompt_section(&self) -> String {
        let servers = self.servers.lock();
        let live: Vec<_> = servers.iter().filter(|(_, s)| s.status == Status::Connected && !s.tools.is_empty()).collect();
        if live.is_empty() {
            return String::new();
        }
        let mut out = String::from("\nConnected MCP servers. Their tools are named mcp__<server>__<tool>; use them when they fit the task:\n");
        for (name, s) in live {
            out.push_str(&format!("- {name} ({} tools)\n", s.tools.len()));
            if let Some(i) = &s.instructions {
                out.push_str(&format!("  <server_instructions server=\"{name}\">\n  {}\n  </server_instructions>\n", i.replace('\n', "\n  ")));
            }
        }
        out
    }

    fn find(&self, full: &str) -> Option<(String, String, Arc<Client>, ServerConfig, bool)> {
        let servers = self.servers.lock();
        for (name, s) in servers.iter() {
            if s.status != Status::Connected {
                continue;
            }
            if let Some(t) = s.tools.iter().find(|t| tool_name(name, &t.name) == full) {
                return Some((name.clone(), t.name.clone(), s.client.clone()?, s.config.clone(), t.read_only));
            }
        }
        None
    }

    /// Whether a tool may run without asking: the server marks it read-only,
    /// or the user auto-approved it.
    pub fn is_safe(&self, full: &str) -> bool {
        self.find(full).is_some_and(|(_, tool, _, cfg, ro)| ro || cfg.auto_approve.allows(&tool))
    }

    pub async fn call(&self, full: &str, input: &Value) -> crate::tools::ToolOutput {
        let Some((server, tool, client, cfg, _)) = self.find(full) else {
            return crate::tools::ToolOutput::err(format!("The MCP tool {full} isn't available. Its server may have disconnected."));
        };
        let meta = Some(json!({"kind": "mcp", "server": server, "tool": tool}));
        match client.call_tool(&tool, input.clone(), Duration::from_secs(cfg.timeout_secs)).await {
            Ok(r) => {
                let mut text = client::result_text(&r);
                if text.len() > 30_000 {
                    let cut = text.char_indices().nth(30_000).map(|(i, _)| i).unwrap_or(text.len());
                    text.truncate(cut);
                    text.push_str("\n… (output truncated)");
                }
                let is_error = r.get("isError").and_then(|e| e.as_bool()).unwrap_or(false);
                crate::tools::ToolOutput { content: text, is_error, meta }
            }
            Err(e) => crate::tools::ToolOutput { content: format!("MCP server {server} failed: {e}"), is_error: true, meta },
        }
    }

    /// Give servers that are still starting a moment, so the first message
    /// of a session can use their tools.
    pub async fn wait_ready(&self, max: Duration) {
        let start = std::time::Instant::now();
        while start.elapsed() < max && self.servers.lock().values().any(|s| s.status == Status::Connecting) {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// Restart a server (after it crashed, or to pick up a new version).
    pub fn restart(self: &Arc<Self>, name: &str) {
        self.start(name);
    }

    pub fn shutdown_all(&self) {
        for s in self.servers.lock().values_mut() {
            if let Some(c) = s.client.take() {
                c.shutdown();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny stdio MCP server in Python, to test the client end to end.
    const FAKE_SERVER: &str = r#"
import json, sys
print("starting up (not JSON)", flush=True)
for line in sys.stdin:
    msg = json.loads(line)
    mid, method = msg.get("id"), msg.get("method")
    if method == "initialize":
        # Ask the client something first, as real servers may.
        print(json.dumps({"jsonrpc": "2.0", "id": "srv-1", "method": "ping"}), flush=True)
        res = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}}, "serverInfo": {"name": "fake", "version": "1.0"}, "instructions": "Be nice."}
    elif method == "tools/list":
        if msg.get("params", {}).get("cursor") == "p2":
            res = {"tools": [{"name": "echo", "description": "Echo text", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}}]}
        else:
            res = {"tools": [{"name": "now", "inputSchema": {"type": "object"}, "annotations": {"readOnlyHint": True}}], "nextCursor": "p2"}
    elif method == "tools/call":
        a = msg["params"]["arguments"]
        if msg["params"]["name"] == "echo":
            res = {"content": [{"type": "text", "text": "echo: " + a.get("text", "")}]}
        else:
            res = {"content": [{"type": "text", "text": "boom"}], "isError": True}
    elif mid is None:
        continue
    elif method is None:
        sys.stderr.write("client answered: " + line)
        sys.stderr.flush()
        continue
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "nope"}}), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": mid, "result": res}), flush=True)
"#;

    #[tokio::test]
    async fn talks_to_a_stdio_server() {
        let dir = std::env::temp_dir().join(format!("casylum-mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("server.py");
        std::fs::write(&script, FAKE_SERVER).unwrap();
        let python = if cfg!(windows) { "python" } else { "python3" };
        let t = Transport::Stdio { command: python.into(), args: vec![script.to_string_lossy().to_string()], env: Default::default(), cwd: None };
        let logs: Logs = Arc::new(Mutex::new(VecDeque::new()));
        let c = Client::connect(&t, reqwest::Client::new(), logs.clone(), Box::new(|| {})).await.unwrap();
        let init = c.initialize(Duration::from_secs(20)).await.unwrap();
        assert_eq!(init["serverInfo"]["name"], "fake");
        let tools = parse_tools(c.list_tools().await.unwrap());
        assert_eq!(tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["now", "echo"]);
        assert!(tools[0].read_only);
        let r = c.call_tool("echo", json!({"text": "hi"}), Duration::from_secs(10)).await.unwrap();
        assert_eq!(client::result_text(&r), "echo: hi");
        let r = c.call_tool("now", json!(null), Duration::from_secs(10)).await.unwrap();
        assert_eq!(r["isError"], true);
        // The client answered the server's ping, and the stray stdout line was logged.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let l: Vec<String> = logs.lock().iter().cloned().collect();
        assert!(l.iter().any(|x| x.contains("starting up")), "{l:?}");
        assert!(l.iter().any(|x| x.contains("client answered") && x.contains("srv-1")), "{l:?}");
        c.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn talks_to_http_and_sse_servers() {
        use tokio::io::AsyncBufReadExt;
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/src/mcp/fake_http_server.py");
        let python = if cfg!(windows) { "python" } else { "python3" };
        let mut child = tokio::process::Command::new(python).arg(script).stdout(std::process::Stdio::piped()).kill_on_drop(true).spawn().unwrap();
        let mut out = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
        let port = out.next_line().await.unwrap().unwrap();
        let base = format!("http://127.0.0.1:{port}");
        let m = Manager::new(reqwest::Client::builder().no_proxy().build().unwrap());
        // Streamable HTTP, with a session id and SSE-framed replies.
        let cfg = config::parse_server("h", &json!({"type": "http", "url": format!("{base}/mcp")})).unwrap();
        let logs: Logs = Arc::new(Mutex::new(VecDeque::new()));
        let (c, tools, _, info) = m.connect("h", &cfg, logs.clone(), 1).await.unwrap();
        assert_eq!(info.unwrap().0, "fake-http");
        assert_eq!(tools[0].name, "echo");
        let r = c.call_tool("echo", json!({"message": "over http"}), Duration::from_secs(10)).await.unwrap();
        assert_eq!(client::result_text(&r), "echo: over http");
        assert!(logs.lock().iter().any(|l| l.contains("working")));
        // An SSE-only server reached with a plain url falls back to SSE.
        let cfg = config::parse_server("s", &json!({"url": format!("{base}/sse")})).unwrap();
        let (c, tools, _, _) = m.connect("s", &cfg, logs.clone(), 2).await.unwrap();
        assert_eq!(tools.len(), 1);
        let r = c.call_tool("echo", json!({"message": "over sse"}), Duration::from_secs(10)).await.unwrap();
        assert_eq!(client::result_text(&r), "echo: over sse");
        c.shutdown();
    }

    /// Run against a real remote server: CASYLUM_MCP_URL=http://localhost:3001/mcp cargo test remote -- --ignored
    #[tokio::test]
    #[ignore]
    async fn remote_server() {
        let url = std::env::var("CASYLUM_MCP_URL").unwrap();
        let sse = url.ends_with("/sse");
        let t = if sse { Transport::Sse { url, headers: Default::default() } } else { Transport::Http { url, headers: Default::default() } };
        let logs: Logs = Arc::new(Mutex::new(VecDeque::new()));
        let c = Client::connect(&t, reqwest::Client::new(), logs.clone(), Box::new(|| {})).await.unwrap();
        let init = c.initialize(Duration::from_secs(20)).await.unwrap();
        println!("init: {init}");
        let tools = parse_tools(c.list_tools().await.unwrap());
        println!("tools: {:?}", tools.iter().map(|t| &t.name).collect::<Vec<_>>());
        let r = c.call_tool("echo", json!({"message": "hello"}), Duration::from_secs(10)).await.unwrap();
        println!("echo: {}", client::result_text(&r));
        assert!(client::result_text(&r).contains("hello"));
    }

    #[test]
    fn tool_names_fit_every_provider() {
        assert_eq!(tool_name("github", "create_issue"), "mcp__github__create_issue");
        assert_eq!(tool_name("my server", "do.thing"), "mcp__my_server__do_thing");
        let long = tool_name("a-very-long-server-name-indeed", "and_an_even_longer_tool_name_that_goes_on");
        assert!(long.len() <= 64 && long.starts_with("mcp__a-very-long"));
        assert_eq!(long, tool_name("a-very-long-server-name-indeed", "and_an_even_longer_tool_name_that_goes_on"));
    }

    #[test]
    fn normalises_schemas_and_reads_hints() {
        let tools = parse_tools(vec![
            json!({"name": "a", "inputSchema": {"type": "object"}, "annotations": {"readOnlyHint": true}}),
            json!({"name": "b", "description": "B"}),
        ]);
        assert!(tools[0].read_only && tools[0].schema["properties"].is_object());
        assert_eq!(tools[1].schema["type"], "object");
        assert_eq!(tools[1].description, "B");
        assert_eq!(tools[0].description, "a");
    }
}
