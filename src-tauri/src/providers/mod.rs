//! Provider layer: one entry point (`stream_turn`) with adapters for each
//! API family. All adapters stream, report live deltas, and return a
//! normalized `Turn`.

pub mod anthropic;
pub mod gemini;
pub mod models;
pub mod openai;

use crate::types::{Message, StreamDelta, ToolSpec, Turn};
use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use serde::Serialize;

/// How a provider is called.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Anthropic,
    Gemini,
    /// OpenAI Chat Completions, or an API that copies it.
    OpenAi,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub needs_key: bool,
    /// Built-in model list, used until the live list arrives or when the
    /// provider can't be reached.
    pub models: Vec<&'static str>,
    pub key_url: &'static str,
    #[serde(skip)]
    pub kind: Kind,
    /// API base for OpenAI-compatible providers. For providers whose address
    /// the user sets (local servers, custom endpoints) this is the default.
    pub base_url: &'static str,
    /// Settings field holding a user-set base URL, if the address can change.
    pub url_setting: Option<&'static str>,
    /// Runs on the user's machine (no key, shown as connected).
    pub local: bool,
    /// Supports tool calling. Without it the model can chat but not act.
    pub tools: bool,
}

fn p(id: &'static str, name: &'static str, kind: Kind, base_url: &'static str, key_url: &'static str, models: &[&'static str]) -> ProviderInfo {
    ProviderInfo { id, name, needs_key: true, models: models.to_vec(), key_url, kind, base_url, url_setting: None, local: false, tools: true }
}

fn local(id: &'static str, name: &'static str, url_setting: &'static str, base_url: &'static str, key_url: &'static str, models: &[&'static str]) -> ProviderInfo {
    ProviderInfo { needs_key: false, url_setting: Some(url_setting), local: true, ..p(id, name, Kind::OpenAi, base_url, key_url, models) }
}

pub fn catalog() -> Vec<ProviderInfo> {
    use Kind::*;
    vec![
        p("anthropic", "Anthropic", Anthropic, "", "https://console.anthropic.com/settings/keys", &["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5", "claude-fable-5-2"]),
        p("openai", "OpenAI", OpenAi, "https://api.openai.com/v1", "https://platform.openai.com/api-keys", &["gpt-5", "gpt-5-mini", "o4-mini", "gpt-4.1"]),
        p("gemini", "Google Gemini", Gemini, "", "https://aistudio.google.com/apikey", &["gemini-2.5-pro", "gemini-2.5-flash"]),
        p("openrouter", "OpenRouter", OpenAi, "https://openrouter.ai/api/v1", "https://openrouter.ai/keys", &["anthropic/claude-sonnet-5-5", "openai/gpt-5", "google/gemini-2.5-pro"]),
        p("groq", "Groq", OpenAi, "https://api.groq.com/openai/v1", "https://console.groq.com/keys", &["openai/gpt-oss-120b", "moonshotai/kimi-k2-instruct", "llama-3.3-70b-versatile", "qwen/qwen3-32b"]),
        p("abacus", "Abacus.AI (RouteLLM)", OpenAi, "https://routellm.abacus.ai/v1", "https://abacus.ai/app/route-llm-apis", &["route-llm", "gpt-5", "claude-sonnet-4-5", "gemini-2.5-pro"]),
        p("mistral", "Mistral", OpenAi, "https://api.mistral.ai/v1", "https://console.mistral.ai/api-keys", &["mistral-large-latest", "mistral-medium-latest", "codestral-latest", "devstral-medium-latest"]),
        p("deepseek", "DeepSeek", OpenAi, "https://api.deepseek.com/v1", "https://platform.deepseek.com/api_keys", &["deepseek-chat", "deepseek-reasoner"]),
        p("xai", "xAI (Grok)", OpenAi, "https://api.x.ai/v1", "https://console.x.ai", &["grok-4", "grok-code-fast-1", "grok-3-mini"]),
        p("together", "Together AI", OpenAi, "https://api.together.xyz/v1", "https://api.together.ai/settings/api-keys", &["moonshotai/Kimi-K2-Instruct", "Qwen/Qwen3-Coder-480B-A35B-Instruct-FP8", "meta-llama/Llama-3.3-70B-Instruct-Turbo"]),
        p("fireworks", "Fireworks AI", OpenAi, "https://api.fireworks.ai/inference/v1", "https://fireworks.ai/account/api-keys", &["accounts/fireworks/models/kimi-k2-instruct", "accounts/fireworks/models/qwen3-coder-480b-a35b-instruct", "accounts/fireworks/models/deepseek-v3p1"]),
        p("cerebras", "Cerebras", OpenAi, "https://api.cerebras.ai/v1", "https://cloud.cerebras.ai", &["gpt-oss-120b", "qwen-3-coder-480b", "llama-3.3-70b"]),
        ProviderInfo { tools: false, ..p("perplexity", "Perplexity", OpenAi, "https://api.perplexity.ai", "https://www.perplexity.ai/account/api/keys", &["sonar-pro", "sonar", "sonar-reasoning-pro"]) },
        p("cohere", "Cohere", OpenAi, "https://api.cohere.ai/compatibility/v1", "https://dashboard.cohere.com/api-keys", &["command-a-03-2025", "command-r-plus-08-2024", "command-r-08-2024"]),
        p("deepinfra", "DeepInfra", OpenAi, "https://api.deepinfra.com/v1/openai", "https://deepinfra.com/dash/api_keys", &["Qwen/Qwen3-Coder-480B-A35B-Instruct", "moonshotai/Kimi-K2-Instruct", "meta-llama/Llama-3.3-70B-Instruct"]),
        p("huggingface", "Hugging Face", OpenAi, "https://router.huggingface.co/v1", "https://huggingface.co/settings/tokens", &["openai/gpt-oss-120b", "Qwen/Qwen3-Coder-480B-A35B-Instruct", "deepseek-ai/DeepSeek-V3.1"]),
        p("sambanova", "SambaNova", OpenAi, "https://api.sambanova.ai/v1", "https://cloud.sambanova.ai/apis", &["DeepSeek-V3.1", "Meta-Llama-3.3-70B-Instruct", "gpt-oss-120b"]),
        p("nebius", "Nebius AI Studio", OpenAi, "https://api.studio.nebius.com/v1", "https://studio.nebius.com/settings/api-keys", &["Qwen/Qwen3-Coder-480B-A35B-Instruct", "deepseek-ai/DeepSeek-V3", "openai/gpt-oss-120b"]),
        p("novita", "Novita AI", OpenAi, "https://api.novita.ai/openai/v1", "https://novita.ai/settings/key-management", &["moonshotai/kimi-k2-instruct", "qwen/qwen3-coder-480b-a35b-instruct", "deepseek/deepseek-v3.1"]),
        p("moonshot", "Moonshot (Kimi)", OpenAi, "https://api.moonshot.ai/v1", "https://platform.moonshot.ai/console/api-keys", &["kimi-k2-turbo-preview", "kimi-k2-0905-preview", "kimi-latest"]),
        p("zai", "Z.ai (GLM)", OpenAi, "https://api.z.ai/api/paas/v4", "https://z.ai/manage-apikey/apikey-list", &["glm-4.6", "glm-4.5", "glm-4.5-air"]),
        p("qwen", "Alibaba Qwen", OpenAi, "https://dashscope-intl.aliyuncs.com/compatible-mode/v1", "https://modelstudio.console.alibabacloud.com/?tab=playground#/api-key", &["qwen3-coder-plus", "qwen3-max", "qwen-plus"]),
        local("ollama", "Ollama (local)", "ollama_base_url", "http://localhost:11434/v1", "https://ollama.com/library", &["qwen3-coder", "llama3.3", "gpt-oss"]),
        local("lmstudio", "LM Studio (local)", "lmstudio_base_url", "http://localhost:1234/v1", "https://lmstudio.ai/docs/app/api", &[]),
        ProviderInfo { local: false, ..local("custom", "Custom (OpenAI-compatible)", "custom_base_url", "", "", &[]) },
    ]
}

pub fn info(id: &str) -> Option<ProviderInfo> {
    catalog().into_iter().find(|p| p.id == id)
}

/// The base URL to call for a provider: the user's setting when the address
/// can change, otherwise the built-in one.
pub fn base_url(id: &str, s: &crate::settings::Settings) -> Option<String> {
    let p = info(id)?;
    let set = p.url_setting.map(|f| s.base_url(f)).unwrap_or_default();
    let url = if set.trim().is_empty() { p.base_url.to_string() } else { set.trim().to_string() };
    (!url.is_empty()).then(|| url.trim_end_matches('/').to_string())
}

pub struct TurnRequest<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub system: &'a str,
    pub messages: &'a [Message],
    pub tools: &'a [ToolSpec],
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

pub async fn stream_turn(
    http: &reqwest::Client,
    req: TurnRequest<'_>,
    on_delta: &mut (dyn FnMut(StreamDelta) + Send),
) -> Result<Turn> {
    let p = info(req.provider).ok_or_else(|| anyhow!("Unknown provider: {}", req.provider))?;
    let key = req.api_key.clone().filter(|k| !k.is_empty());
    let need_key = || key.clone().ok_or_else(|| anyhow!("No API key set for {}. Add one in Settings.", p.name));
    match p.kind {
        Kind::Anthropic => anthropic::stream(http, &need_key()?, &req, on_delta).await,
        Kind::Gemini => gemini::stream(http, &need_key()?, &req, on_delta).await,
        Kind::OpenAi => {
            let base = req
                .base_url
                .clone()
                .filter(|b| !b.is_empty())
                .or_else(|| (!p.base_url.is_empty()).then(|| p.base_url.to_string()))
                .ok_or_else(|| anyhow!("Set a base URL for {} in Settings.", p.name))?;
            let key = if p.needs_key { Some(need_key()?) } else { key.clone() };
            // A provider without tool calling can still chat.
            let no_tools: [ToolSpec; 0] = [];
            let req = TurnRequest { tools: if p.tools { req.tools } else { &no_tools }, ..req };
            openai::stream(http, base.trim_end_matches('/'), key.as_deref(), p.id == "openai", &req, on_delta).await
        }
    }
}

/// Minimal Server-Sent Events reader over a reqwest response.
pub struct Sse {
    stream: futures_util::stream::BoxStream<'static, reqwest::Result<bytes::Bytes>>,
    buf: Vec<u8>,
}

#[allow(dead_code)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

impl Sse {
    pub fn new(resp: reqwest::Response) -> Self {
        Self { stream: resp.bytes_stream().boxed(), buf: Vec::new() }
    }

    pub async fn next(&mut self) -> Result<Option<SseEvent>> {
        loop {
            if let Some(ev) = self.take_event() {
                return Ok(Some(ev));
            }
            match self.stream.next().await {
                Some(chunk) => self.buf.extend_from_slice(&chunk?),
                None => {
                    // Flush a trailing event without a blank line.
                    if !self.buf.is_empty() {
                        self.buf.extend_from_slice(b"\n\n");
                        if let Some(ev) = self.take_event() {
                            return Ok(Some(ev));
                        }
                    }
                    return Ok(None);
                }
            }
        }
    }

    fn take_event(&mut self) -> Option<SseEvent> {
        loop {
            let (end, sep) = find_boundary(&self.buf)?;
            let raw: Vec<u8> = self.buf.drain(..end + sep).collect();
            let text = String::from_utf8_lossy(&raw[..end]);
            let mut event = String::new();
            let mut data = String::new();
            for line in text.lines() {
                if let Some(v) = line.strip_prefix("event:") {
                    event = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("data:") {
                    if !data.is_empty() {
                        data.push('\n');
                    }
                    data.push_str(v.strip_prefix(' ').unwrap_or(v));
                }
            }
            if !data.is_empty() || !event.is_empty() {
                return Some(SseEvent { event, data });
            }
        }
    }
}

fn find_boundary(buf: &[u8]) -> Option<(usize, usize)> {
    let a = buf.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2));
    let b = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| (i, 4));
    match (a, b) {
        (Some(x), Some(y)) => Some(if x.0 <= y.0 { x } else { y }),
        (x, y) => x.or(y),
    }
}

/// Turn a non-2xx response into a readable error.
pub async fn check(resp: reqwest::Response, provider: &str) -> Result<reqwest::Response> {
    if resp.status().is_success() {
        return Ok(resp);
    }
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let msg = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.pointer("/0/error/message"))
                .or_else(|| v.get("message"))
                .and_then(|m| m.as_str().map(String::from))
        })
        .unwrap_or(body);
    Err(anyhow!("{provider} returned {status}: {msg}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use std::io::{Read, Write};

    /// Serve one canned SSE response on a random port; returns the base URL.
    fn serve(body: &'static str) -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = [0u8; 65536];
            let _ = s.read(&mut buf);
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
            s.write_all(resp.as_bytes()).unwrap();
        });
        format!("http://{addr}")
    }

    fn req<'a>(provider: &'a str, base: Option<String>, msgs: &'a [Message], tools: &'a [ToolSpec]) -> TurnRequest<'a> {
        TurnRequest { provider, model: "m", system: "sys", messages: msgs, tools, api_key: Some("k".into()), base_url: base }
    }

    #[tokio::test]
    async fn openai_compatible_stream_with_tool_call() {
        let base = serve(concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Let me look\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"pa\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"th\\\":\\\"a.rs\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\n",
            "data: [DONE]\n\n"
        ));
        let msgs = vec![Message::user_text("hi")];
        let tools = crate::tools::specs();
        let mut text = String::new();
        let turn = stream_turn(&reqwest::Client::new(), req("custom", Some(base), &msgs, &tools), &mut |d| {
            if let StreamDelta::Text(t) = d {
                text.push_str(&t)
            }
        })
        .await
        .unwrap();
        assert_eq!(text, "Let me look");
        assert_eq!(turn.stop, StopReason::ToolUse);
        assert_eq!(turn.usage.output_tokens, 5);
        match &turn.content[1] {
            Block::ToolUse { id, name, input, .. } => {
                assert_eq!(id, "c1");
                assert_eq!(name, "read_file");
                assert_eq!(input["path"], "a.rs");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn anthropic_stream_with_thinking_and_tool() {
        let base = serve(concat!(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":12}}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"plan\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"SIG\"}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"On it.\"}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tu1\",\"name\":\"list_dir\",\"input\":{}}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\": \\\"src\\\"}\"}}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":40}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
        ));
        std::env::set_var("CASYLUM_ANTHROPIC_BASE", base);
        let msgs = vec![Message::user_text("hi")];
        let tools = crate::tools::specs();
        let turn = stream_turn(&reqwest::Client::new(), req("anthropic", None, &msgs, &tools), &mut |_| {}).await.unwrap();
        assert_eq!(turn.stop, StopReason::ToolUse);
        assert_eq!(turn.usage.input_tokens, 12);
        assert!(matches!(&turn.content[0], Block::Thinking { thinking, signature } if thinking == "plan" && signature.as_deref() == Some("SIG")));
        assert!(matches!(&turn.content[1], Block::Text { text } if text == "On it."));
        assert!(matches!(&turn.content[2], Block::ToolUse { input, .. } if input["path"] == "src"));
    }
}
