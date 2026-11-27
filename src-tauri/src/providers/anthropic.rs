//! Anthropic Messages API adapter (raw HTTP; there is no official Rust SDK).

use super::{check, Sse, TurnRequest};
use crate::types::*;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

/// Models from the 4.6 generation onward take adaptive thinking and effort.
fn modern(model: &str) -> bool {
    let legacy = ["claude-3", "-4-5", "-4-1", "-4-0", "sonnet-4-2", "opus-4-2", "claude-2"];
    !legacy.iter().any(|p| model.contains(p))
}

/// Models that support the server-side refusal fallback.
fn supports_fallback(model: &str) -> bool {
    ["claude-opus-5-5", "claude-opus-5", "claude-sonnet-5-5", "claude-fable-5-1", "claude-fable-5-2"]
        .iter()
        .any(|m| model == *m)
}

fn to_wire(messages: &[Message]) -> Vec<Value> {
    messages
        .iter()
        .map(|m| {
            let content: Vec<Value> = m
                .content
                .iter()
                .map(|b| match b {
                    Block::Text { text } => json!({"type": "text", "text": text}),
                    Block::Thinking { thinking, signature } => {
                        json!({"type": "thinking", "thinking": thinking, "signature": signature.clone().unwrap_or_default()})
                    }
                    Block::RedactedThinking { data } => json!({"type": "redacted_thinking", "data": data}),
                    Block::ToolUse { id, name, input, .. } => json!({"type": "tool_use", "id": id, "name": name, "input": input}),
                    Block::ToolResult { tool_use_id, content, is_error, .. } => {
                        json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content, "is_error": is_error})
                    }
                })
                .collect();
            json!({"role": if m.role == Role::User { "user" } else { "assistant" }, "content": content})
        })
        .collect()
}

enum Partial {
    Text(String),
    Thinking { text: String, sig: String },
    Redacted(String),
    Tool { id: String, name: String, json: String },
}

pub async fn stream(
    http: &reqwest::Client,
    key: &str,
    req: &TurnRequest<'_>,
    on_delta: &mut (dyn FnMut(StreamDelta) + Send),
) -> Result<Turn> {
    let tools: Vec<Value> = req
        .tools
        .iter()
        .map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.schema, "eager_input_streaming": true}))
        .collect();

    let mut body = json!({
        "model": req.model,
        "max_tokens": 64000,
        "system": [{"type": "text", "text": req.system, "cache_control": {"type": "ephemeral"}}],
        "messages": to_wire(req.messages),
        "stream": true,
    });
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    if modern(req.model) {
        body["thinking"] = json!({"type": "adaptive", "display": "summarized"});
        body["output_config"] = json!({"effort": "high"});
    }

    let mut builder = http
        .post(format!("{}/v1/messages", std::env::var("CASYLUM_ANTHROPIC_BASE").unwrap_or_else(|_| "https://api.anthropic.com".into())))
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json");
    if supports_fallback(req.model) {
        builder = builder.header("anthropic-beta", "server-side-fallback-2026-07-01");
        body["fallbacks"] = json!("default");
    }

    let resp = check(builder.json(&body).send().await?, "Anthropic").await?;
    let mut sse = Sse::new(resp);

    let mut blocks: Vec<Option<Partial>> = Vec::new();
    let mut usage = Usage::default();
    let mut stop = StopReason::Other;

    while let Some(ev) = sse.next().await? {
        if ev.data.is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(&ev.data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ty = v["type"].as_str().unwrap_or("");
        match ty {
            "message_start" => {
                let u = &v["message"]["usage"];
                usage.input_tokens = u["input_tokens"].as_u64().unwrap_or(0)
                    + u["cache_read_input_tokens"].as_u64().unwrap_or(0)
                    + u["cache_creation_input_tokens"].as_u64().unwrap_or(0);
            }
            "content_block_start" => {
                let idx = v["index"].as_u64().unwrap_or(0) as usize;
                while blocks.len() <= idx {
                    blocks.push(None);
                }
                let cb = &v["content_block"];
                blocks[idx] = match cb["type"].as_str().unwrap_or("") {
                    "text" => Some(Partial::Text(cb["text"].as_str().unwrap_or("").to_string())),
                    "thinking" => Some(Partial::Thinking { text: String::new(), sig: String::new() }),
                    "redacted_thinking" => Some(Partial::Redacted(cb["data"].as_str().unwrap_or("").to_string())),
                    "tool_use" => {
                        let name = cb["name"].as_str().unwrap_or("").to_string();
                        on_delta(StreamDelta::ToolStarted { name: name.clone() });
                        Some(Partial::Tool { id: cb["id"].as_str().unwrap_or("").to_string(), name, json: String::new() })
                    }
                    _ => None,
                };
            }
            "content_block_delta" => {
                let idx = v["index"].as_u64().unwrap_or(0) as usize;
                let d = &v["delta"];
                if let Some(Some(p)) = blocks.get_mut(idx) {
                    match (p, d["type"].as_str().unwrap_or("")) {
                        (Partial::Text(t), "text_delta") => {
                            let s = d["text"].as_str().unwrap_or("");
                            t.push_str(s);
                            on_delta(StreamDelta::Text(s.to_string()));
                        }
                        (Partial::Thinking { text, .. }, "thinking_delta") => {
                            let s = d["thinking"].as_str().unwrap_or("");
                            text.push_str(s);
                            on_delta(StreamDelta::Thinking(s.to_string()));
                        }
                        (Partial::Thinking { sig, .. }, "signature_delta") => sig.push_str(d["signature"].as_str().unwrap_or("")),
                        (Partial::Tool { json, .. }, "input_json_delta") => json.push_str(d["partial_json"].as_str().unwrap_or("")),
                        _ => {}
                    }
                }
            }
            "message_delta" => {
                if let Some(o) = v["usage"]["output_tokens"].as_u64() {
                    usage.output_tokens = o;
                }
                if let Some(r) = v["delta"]["stop_reason"].as_str() {
                    stop = match r {
                        "end_turn" | "stop_sequence" => StopReason::EndTurn,
                        "tool_use" => StopReason::ToolUse,
                        "max_tokens" => StopReason::MaxTokens,
                        "refusal" => StopReason::Refusal,
                        _ => StopReason::Other,
                    };
                }
            }
            "error" => {
                let msg = v["error"]["message"].as_str().unwrap_or("unknown error");
                return Err(anyhow!("Anthropic stream error: {msg}"));
            }
            _ => {}
        }
    }

    let content = blocks
        .into_iter()
        .flatten()
        .map(|p| match p {
            Partial::Text(text) => Block::Text { text },
            Partial::Thinking { text, sig } => Block::Thinking { thinking: text, signature: Some(sig) },
            Partial::Redacted(data) => Block::RedactedThinking { data },
            Partial::Tool { id, name, json } => {
                // With eager input streaming the API does not validate input,
                // so a broken payload is surfaced to the tool layer as `__invalid`.
                let input = if json.trim().is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(&json).unwrap_or_else(|_| json!({"__invalid": json}))
                };
                Block::ToolUse { id, name, input, provider_meta: None }
            }
        })
        .filter(|b| !matches!(b, Block::Text { text } if text.is_empty()))
        .collect();

    Ok(Turn { content, stop, usage })
}
