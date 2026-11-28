//! A minimal MCP client: JSON-RPC 2.0 over stdio, Streamable HTTP, or the
//! legacy HTTP+SSE transport. It initialises the session, lists tools,
//! calls tools, and answers the few requests a server may send back.

use super::config::Transport;
use crate::providers::Sse;
use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

pub const PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_LOG_LINES: usize = 300;

pub type Logs = Arc<Mutex<VecDeque<String>>>;

pub fn log(logs: &Logs, line: impl Into<String>) {
    let mut l = logs.lock();
    let line: String = line.into();
    let line = if line.len() > 2000 { format!("{}…", &line[..line.char_indices().nth(1990).map(|(i, _)| i).unwrap_or(line.len())]) } else { line };
    l.push_back(line);
    while l.len() > MAX_LOG_LINES {
        l.pop_front();
    }
}

enum Link {
    Stdio { stdin: tokio::sync::Mutex<tokio::process::ChildStdin>, child: Mutex<Option<tokio::process::Child>> },
    Http { url: String, headers: BTreeMap<String, String>, session: Mutex<Option<String>> },
    Sse { post_url: String, headers: BTreeMap<String, String> },
}

pub struct Client {
    link: Link,
    http: reqwest::Client,
    pending: Mutex<HashMap<u64, oneshot::Sender<Value>>>,
    next_id: AtomicU64,
    initialized: AtomicBool,
    dead: Mutex<Option<String>>,
    cancel: CancellationToken,
    logs: Logs,
    /// Called when the server says its tool list changed.
    on_tools_changed: Box<dyn Fn() + Send + Sync>,
}

impl Drop for Client {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Link::Stdio { child, .. } = &self.link {
            if let Some(mut c) = child.lock().take() {
                let _ = c.start_kill();
            }
        }
    }
}

fn header_map(h: &BTreeMap<String, String>) -> reqwest::header::HeaderMap {
    let mut m = reqwest::header::HeaderMap::new();
    for (k, v) in h {
        if let (Ok(k), Ok(v)) = (reqwest::header::HeaderName::from_bytes(k.as_bytes()), reqwest::header::HeaderValue::from_str(v)) {
            m.insert(k, v);
        }
    }
    m
}

/// GUI apps on macOS and Linux don't inherit the shell's PATH, so `npx`,
/// `uvx` and friends often aren't found. Add the usual places.
#[cfg(not(windows))]
fn augmented_path() -> String {
    let mut dirs: Vec<String> = std::env::var("PATH").unwrap_or_default().split(':').filter(|s| !s.is_empty()).map(String::from).collect();
    let home = dirs::home_dir().unwrap_or_default();
    let mut extra: Vec<std::path::PathBuf> = vec![
        "/opt/homebrew/bin".into(),
        "/usr/local/bin".into(),
        home.join(".local/bin"),
        home.join(".cargo/bin"),
        home.join(".bun/bin"),
        home.join(".deno/bin"),
        home.join(".volta/bin"),
        home.join(".nix-profile/bin"),
    ];
    // The newest Node installed with nvm.
    if let Ok(rd) = std::fs::read_dir(home.join(".nvm/versions/node")) {
        let mut v: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        v.sort();
        if let Some(last) = v.last() {
            extra.push(last.join("bin"));
        }
    }
    for d in extra {
        let s = d.to_string_lossy().to_string();
        if d.is_dir() && !dirs.contains(&s) {
            dirs.push(s);
        }
    }
    dirs.join(":")
}

fn stdio_command(command: &str, args: &[String], env: &BTreeMap<String, String>, cwd: Option<&str>) -> tokio::process::Command {
    #[cfg(windows)]
    let mut c = {
        // `npx`, `uvx` etc. are .cmd shims that CreateProcess can't start
        // directly, so anything that isn't plainly an .exe goes through cmd.
        let lower = command.to_lowercase();
        let mut c = if lower.ends_with(".exe") {
            let mut c = tokio::process::Command::new(command);
            c.args(args);
            c
        } else {
            let mut c = tokio::process::Command::new("cmd");
            c.arg("/C").arg(command).args(args);
            c
        };
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    };
    #[cfg(not(windows))]
    let mut c = {
        let mut c = tokio::process::Command::new(command);
        c.args(args);
        c.env("PATH", augmented_path());
        c
    };
    for (k, v) in env {
        c.env(k, v);
    }
    let dir = cwd.map(std::path::PathBuf::from).filter(|d| d.is_dir()).or_else(dirs::home_dir);
    if let Some(d) = dir {
        c.current_dir(d);
    }
    c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    c
}

impl Client {
    /// Start the transport (spawn the process or open the stream). Call
    /// `initialize` next.
    pub async fn connect(transport: &Transport, http: reqwest::Client, logs: Logs, on_tools_changed: Box<dyn Fn() + Send + Sync>) -> Result<Arc<Self>> {
        match transport {
            Transport::Stdio { command, args, env, cwd } => {
                log(&logs, format!("$ {} {}", command, args.join(" ")));
                let mut child = stdio_command(command, args, env, cwd.as_deref())
                    .spawn()
                    .map_err(|e| anyhow!("Couldn't start \"{command}\": {e}. Check that it is installed and on your PATH."))?;
                let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
                let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
                let stderr = child.stderr.take();
                let client = Arc::new(Client {
                    link: Link::Stdio { stdin: tokio::sync::Mutex::new(stdin), child: Mutex::new(Some(child)) },
                    http,
                    pending: Mutex::new(HashMap::new()),
                    next_id: AtomicU64::new(1),
                    initialized: AtomicBool::new(false),
                    dead: Mutex::new(None),
                    cancel: CancellationToken::new(),
                    logs: logs.clone(),
                    on_tools_changed,
                });
                if let Some(err) = stderr {
                    let logs = logs.clone();
                    let cancel = client.cancel.clone();
                    tokio::spawn(async move {
                        let mut lines = BufReader::new(err).lines();
                        loop {
                            tokio::select! {
                                _ = cancel.cancelled() => break,
                                l = lines.next_line() => match l {
                                    Ok(Some(l)) => log(&logs, l),
                                    _ => break,
                                },
                            }
                        }
                    });
                }
                let weak = Arc::downgrade(&client);
                let cancel = client.cancel.clone();
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stdout).lines();
                    let why = loop {
                        tokio::select! {
                            _ = cancel.cancelled() => return,
                            l = lines.next_line() => match l {
                                Ok(Some(line)) => {
                                    let Some(c) = weak.upgrade() else { return };
                                    let t = line.trim();
                                    if t.is_empty() { continue; }
                                    match serde_json::from_str::<Value>(t) {
                                        Ok(v) => c.dispatch(v),
                                        // Some servers print logs to stdout; keep them visible.
                                        Err(_) => log(&c.logs, line),
                                    }
                                }
                                Ok(None) => break "The server process exited.".to_string(),
                                Err(e) => break format!("Reading from the server failed: {e}"),
                            },
                        }
                    };
                    if let Some(c) = weak.upgrade() {
                        c.mark_dead(why);
                    }
                });
                Ok(client)
            }
            Transport::Http { url, headers } => Ok(Arc::new(Client {
                link: Link::Http { url: url.clone(), headers: headers.clone(), session: Mutex::new(None) },
                http,
                pending: Mutex::new(HashMap::new()),
                next_id: AtomicU64::new(1),
                initialized: AtomicBool::new(false),
                dead: Mutex::new(None),
                cancel: CancellationToken::new(),
                logs,
                on_tools_changed,
            })),
            Transport::Sse { url, headers } => Self::connect_sse(url, headers, http, logs, on_tools_changed).await,
        }
    }

    async fn connect_sse(url: &str, headers: &BTreeMap<String, String>, http: reqwest::Client, logs: Logs, on_tools_changed: Box<dyn Fn() + Send + Sync>) -> Result<Arc<Self>> {
        log(&logs, format!("GET {url} (SSE)"));
        let resp = http.get(url).headers(header_map(headers)).header("Accept", "text/event-stream").send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("{url} returned {status}: {}", body.chars().take(300).collect::<String>()));
        }
        let mut sse = Sse::new(resp);
        // The first event names the URL to POST messages to.
        let endpoint = tokio::time::timeout(Duration::from_secs(30), async {
            while let Some(ev) = sse.next().await? {
                if ev.event == "endpoint" {
                    return Ok::<_, anyhow::Error>(ev.data);
                }
            }
            Err(anyhow!("The SSE stream closed before sending its endpoint."))
        })
        .await
        .map_err(|_| anyhow!("The server didn't send an SSE endpoint within 30s."))??;
        let base = reqwest::Url::parse(url)?;
        let post_url = base.join(endpoint.trim())?.to_string();
        let client = Arc::new(Client {
            link: Link::Sse { post_url, headers: headers.clone() },
            http,
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            initialized: AtomicBool::new(false),
            dead: Mutex::new(None),
            cancel: CancellationToken::new(),
            logs,
            on_tools_changed,
        });
        let weak: Weak<Client> = Arc::downgrade(&client);
        let cancel = client.cancel.clone();
        tokio::spawn(async move {
            let why = loop {
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    ev = sse.next() => match ev {
                        Ok(Some(ev)) => {
                            let Some(c) = weak.upgrade() else { return };
                            if ev.event.is_empty() || ev.event == "message" {
                                if let Ok(v) = serde_json::from_str::<Value>(&ev.data) { c.dispatch(v); }
                            }
                        }
                        Ok(None) => break "The SSE stream closed.".to_string(),
                        Err(e) => break format!("The SSE stream failed: {e}"),
                    },
                }
            };
            if let Some(c) = weak.upgrade() {
                c.mark_dead(why);
            }
        });
        Ok(client)
    }

    pub fn is_dead(&self) -> Option<String> {
        self.dead.lock().clone()
    }

    fn mark_dead(&self, why: String) {
        log(&self.logs, why.clone());
        *self.dead.lock() = Some(why);
        // Wake every waiting request with an error.
        for (_, tx) in self.pending.lock().drain() {
            let _ = tx.send(json!({"error": {"code": -32000, "message": "The server disconnected."}}));
        }
    }

    /// Route one incoming JSON-RPC message (or a batch).
    fn dispatch(self: &Arc<Self>, v: Value) {
        if let Value::Array(items) = v {
            for i in items {
                self.dispatch(i);
            }
            return;
        }
        let has_method = v.get("method").and_then(|m| m.as_str()).is_some();
        let id = v.get("id").cloned().filter(|i| !i.is_null());
        if !has_method {
            if let Some(n) = id.as_ref().and_then(|i| i.as_u64().or_else(|| i.as_str().and_then(|s| s.parse().ok()))) {
                if let Some(tx) = self.pending.lock().remove(&n) {
                    let _ = tx.send(v);
                }
            }
            return;
        }
        let method = v["method"].as_str().unwrap_or_default().to_string();
        match id {
            // A request from the server: answer what we can.
            Some(id) => {
                let reply = match method.as_str() {
                    "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
                    "roots/list" => json!({"jsonrpc": "2.0", "id": id, "result": {"roots": []}}),
                    _ => json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("Casylum doesn't support {method}")}}),
                };
                let me = self.clone();
                tokio::spawn(async move {
                    let _ = me.send_raw(&reply, false).await;
                });
            }
            None => match method.as_str() {
                "notifications/tools/list_changed" => (self.on_tools_changed)(),
                "notifications/message" => {
                    let p = &v["params"];
                    let data = p.get("data").map(|d| d.as_str().map(str::to_string).unwrap_or_else(|| d.to_string())).unwrap_or_default();
                    log(&self.logs, format!("[{}] {}", p["level"].as_str().unwrap_or("log"), data));
                }
                _ => {}
            },
        }
    }

    /// Write one message. For Streamable HTTP this POSTs and, when
    /// `want_reply` is set, dispatches whatever comes back.
    async fn send_raw(self: &Arc<Self>, msg: &Value, want_reply: bool) -> Result<()> {
        match &self.link {
            Link::Stdio { stdin, .. } => {
                let mut line = serde_json::to_vec(msg)?;
                line.push(b'\n');
                let mut w = stdin.lock().await;
                w.write_all(&line).await.map_err(|e| anyhow!("Couldn't write to the server: {e}"))?;
                w.flush().await?;
                Ok(())
            }
            Link::Sse { post_url, headers } => {
                let resp = self.http.post(post_url).headers(header_map(headers)).json(msg).timeout(Duration::from_secs(60)).send().await?;
                if !resp.status().is_success() {
                    let s = resp.status();
                    return Err(anyhow!("The server returned {s}: {}", resp.text().await.unwrap_or_default().chars().take(300).collect::<String>()));
                }
                Ok(())
            }
            Link::Http { url, headers, session } => {
                let mut rb = self
                    .http
                    .post(url)
                    .headers(header_map(headers))
                    .header("Accept", "application/json, text/event-stream")
                    .json(msg);
                if let Some(s) = session.lock().clone() {
                    rb = rb.header("Mcp-Session-Id", s);
                }
                if self.initialized.load(Ordering::Relaxed) {
                    rb = rb.header("MCP-Protocol-Version", PROTOCOL_VERSION);
                }
                let resp = rb.send().await?;
                if let Some(s) = resp.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()) {
                    *session.lock() = Some(s.to_string());
                }
                let status = resp.status();
                if !status.is_success() {
                    let body = resp.text().await.unwrap_or_default();
                    return Err(HttpError { status: status.as_u16(), body: body.chars().take(400).collect() }.into());
                }
                if !want_reply || status == reqwest::StatusCode::ACCEPTED {
                    return Ok(());
                }
                let ctype = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_lowercase();
                if ctype.contains("text/event-stream") {
                    // Read events until our response arrives; the stream may
                    // carry notifications and requests first.
                    let id = msg.get("id").and_then(|i| i.as_u64());
                    let mut sse = Sse::new(resp);
                    while let Some(ev) = sse.next().await? {
                        if ev.data.trim().is_empty() {
                            continue;
                        }
                        if let Ok(v) = serde_json::from_str::<Value>(&ev.data) {
                            let done = id.is_some() && v.get("id").and_then(|i| i.as_u64()) == id && v.get("method").is_none();
                            self.dispatch(v);
                            if done {
                                break;
                            }
                        }
                    }
                } else {
                    let text = resp.text().await?;
                    if !text.trim().is_empty() {
                        let v: Value = serde_json::from_str(&text).map_err(|e| anyhow!("The server sent invalid JSON: {e}"))?;
                        self.dispatch(v);
                    }
                }
                Ok(())
            }
        }
    }

    pub async fn request(self: &Arc<Self>, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        if let Some(why) = self.is_dead() {
            return Err(anyhow!(why));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        // Over Streamable HTTP the reply arrives inside the POST itself, so
        // the timeout covers sending as well as waiting.
        let is_http = matches!(self.link, Link::Http { .. });
        let waited = tokio::time::timeout(timeout, async {
            self.send_raw(&msg, true).await?;
            if is_http && self.pending.lock().contains_key(&id) {
                return Err(anyhow!("The server closed the request without answering."));
            }
            rx.await.map_err(|_| anyhow!("The server disconnected."))
        })
        .await;
        let resp = match waited {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                self.pending.lock().remove(&id);
                return Err(e);
            }
            Err(_) => {
                self.pending.lock().remove(&id);
                let _ = self.notify("notifications/cancelled", json!({"requestId": id, "reason": "timeout"})).await;
                return Err(anyhow!("The server didn't answer {method} within {}s.", timeout.as_secs()));
            }
        };
        if let Some(err) = resp.get("error") {
            let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("unknown error");
            return Err(anyhow!("{msg}"));
        }
        Ok(resp.get("result").cloned().unwrap_or(Value::Null))
    }

    pub async fn notify(self: &Arc<Self>, method: &str, params: Value) -> Result<()> {
        self.send_raw(&json!({"jsonrpc": "2.0", "method": method, "params": params}), false).await
    }

    pub async fn initialize(self: &Arc<Self>, timeout: Duration) -> Result<Value> {
        let result = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {"roots": {"listChanged": false}},
                    "clientInfo": {"name": "casylum", "title": "Casylum", "version": env!("CARGO_PKG_VERSION")},
                }),
                timeout,
            )
            .await?;
        self.initialized.store(true, Ordering::Relaxed);
        self.notify("notifications/initialized", json!({})).await?;
        Ok(result)
    }

    pub async fn list_tools(self: &Arc<Self>) -> Result<Vec<Value>> {
        let mut out = vec![];
        let mut cursor: Option<String> = None;
        for _ in 0..50 {
            let params = match &cursor {
                Some(c) => json!({"cursor": c}),
                None => json!({}),
            };
            let r = self.request("tools/list", params, Duration::from_secs(60)).await?;
            if let Some(t) = r.get("tools").and_then(|t| t.as_array()) {
                out.extend(t.iter().cloned());
            }
            cursor = r.get("nextCursor").and_then(|c| c.as_str()).filter(|c| !c.is_empty()).map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(out)
    }

    pub async fn call_tool(self: &Arc<Self>, name: &str, args: Value, timeout: Duration) -> Result<Value> {
        let args = if args.is_object() { args } else { json!({}) };
        self.request("tools/call", json!({"name": name, "arguments": args}), timeout).await
    }

    pub fn shutdown(&self) {
        self.cancel.cancel();
        if let Link::Stdio { child, .. } = &self.link {
            if let Some(mut c) = child.lock().take() {
                let _ = c.start_kill();
            }
        }
        if let Link::Http { url, headers, session } = &self.link {
            // End the server-side session; best effort.
            if let Some(s) = session.lock().take() {
                let rb = self.http.delete(url).headers(header_map(headers)).header("Mcp-Session-Id", s);
                // Runs at app exit too, outside any Tokio context.
                tauri::async_runtime::spawn(async move {
                    let _ = rb.timeout(Duration::from_secs(5)).send().await;
                });
            }
        }
    }
}

/// A non-2xx answer from a Streamable HTTP server. 404/405 on the first
/// request usually means the server only speaks legacy SSE.
#[derive(Debug)]
pub struct HttpError {
    pub status: u16,
    pub body: String,
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status {
            401 | 403 => write!(f, "The server refused access ({}). Add an Authorization header with a token for this server.", self.status),
            _ => write!(f, "The server returned HTTP {}: {}", self.status, self.body),
        }
    }
}

impl std::error::Error for HttpError {}

/// Turn a tools/call result into text for the model.
pub fn result_text(r: &Value) -> String {
    let mut parts: Vec<String> = vec![];
    if let Some(items) = r.get("content").and_then(|c| c.as_array()) {
        for it in items {
            match it.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                "text" => parts.push(it["text"].as_str().unwrap_or_default().to_string()),
                "image" | "audio" => {
                    let kind = it["type"].as_str().unwrap_or("media");
                    let mime = it["mimeType"].as_str().unwrap_or("unknown");
                    let size = it["data"].as_str().map(|d| d.len() * 3 / 4).unwrap_or(0);
                    parts.push(format!("[{kind} {mime}, {size} bytes]"));
                }
                "resource" => {
                    let res = &it["resource"];
                    match res.get("text").and_then(|t| t.as_str()) {
                        Some(t) => parts.push(format!("[resource {}]\n{t}", res["uri"].as_str().unwrap_or(""))),
                        None => parts.push(format!("[resource {} ({})]", res["uri"].as_str().unwrap_or(""), res["mimeType"].as_str().unwrap_or("binary"))),
                    }
                }
                "resource_link" => parts.push(format!("[link {} {}]", it["name"].as_str().unwrap_or(""), it["uri"].as_str().unwrap_or(""))),
                _ => parts.push(it.to_string()),
            }
        }
    }
    if parts.is_empty() {
        if let Some(s) = r.get("structuredContent") {
            parts.push(serde_json::to_string_pretty(s).unwrap_or_default());
        }
    }
    if parts.is_empty() {
        "(no output)".into()
    } else {
        parts.join("\n")
    }
}
