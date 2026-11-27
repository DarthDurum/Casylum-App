//! Provider layer: one entry point (`stream_turn`) with adapters for each
//! API family. All adapters stream, report live deltas, and return a
//! normalized `Turn`.

pub mod anthropic;
pub mod gemini;
pub mod openai;

use crate::types::{Message, StreamDelta, ToolSpec, Turn};
use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub needs_key: bool,
    pub models: Vec<&'static str>,
    pub key_url: &'static str,
}

pub fn catalog() -> Vec<ProviderInfo> {
    vec![
        ProviderInfo {
            id: "anthropic",
            name: "Anthropic",
            needs_key: true,
            models: vec!["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5", "claude-fable-5-2"],
            key_url: "https://console.anthropic.com/settings/keys",
        },
        ProviderInfo {
            id: "openai",
            name: "OpenAI",
            needs_key: true,
            models: vec!["gpt-5", "gpt-5-mini", "o4-mini", "gpt-4.1"],
            key_url: "https://platform.openai.com/api-keys",
        },
        ProviderInfo {
            id: "gemini",
            name: "Google Gemini",
            needs_key: true,
            models: vec!["gemini-2.5-pro", "gemini-2.5-flash"],
            key_url: "https://aistudio.google.com/apikey",
        },
        ProviderInfo {
            id: "openrouter",
            name: "OpenRouter",
            needs_key: true,
            models: vec!["anthropic/claude-sonnet-5-5", "openai/gpt-5", "google/gemini-2.5-pro"],
            key_url: "https://openrouter.ai/keys",
        },
        ProviderInfo {
            id: "ollama",
            name: "Ollama (local)",
            needs_key: false,
            models: vec!["qwen3-coder", "llama3.3", "gpt-oss"],
            key_url: "https://ollama.com/library",
        },
        ProviderInfo {
            id: "custom",
            name: "Custom (OpenAI-compatible)",
            needs_key: false,
            models: vec![],
            key_url: "",
        },
    ]
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
    let key = req.api_key.clone();
    let need_key = || key.clone().ok_or_else(|| anyhow!("No API key set for {}. Add one in Settings.", req.provider));
    match req.provider {
        "anthropic" => anthropic::stream(http, &need_key()?, &req, on_delta).await,
        "gemini" => gemini::stream(http, &need_key()?, &req, on_delta).await,
        "openai" => openai::stream(http, "https://api.openai.com/v1", Some(&need_key()?), true, &req, on_delta).await,
        "openrouter" => openai::stream(http, "https://openrouter.ai/api/v1", Some(&need_key()?), false, &req, on_delta).await,
        "ollama" | "custom" => {
            let base = req.base_url.clone().filter(|b| !b.is_empty()).ok_or_else(|| anyhow!("Set a base URL for {} in Settings.", req.provider))?;
            openai::stream(http, base.trim_end_matches('/'), key.as_deref(), false, &req, on_delta).await
        }
        other => Err(anyhow!("Unknown provider: {other}")),
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
