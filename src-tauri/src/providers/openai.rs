//! OpenAI Chat Completions adapter. Also used for OpenRouter, Ollama and any
//! OpenAI-compatible endpoint.

use super::{check, Sse, TurnRequest};
use crate::types::*;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn to_wire(system: &str, messages: &[Message]) -> Vec<Value> {
    let mut out = vec![json!({"role": "system", "content": system})];
    for m in messages {
        match m.role {
            Role::User => {
                // Tool results become separate `tool` messages; text stays as a user message.
                let mut text = String::new();
                for b in &m.content {
                    match b {
                        Block::ToolResult { tool_use_id, content, .. } => {
                            out.push(json!({"role": "tool", "tool_call_id": tool_use_id, "content": content}));
                        }
                        Block::Text { text: t } => {
                            if !text.is_empty() {
                                text.push('\n');
                            }
                            text.push_str(t);
                        }
                        _ => {}
                    }
                }
                if !text.is_empty() {
                    out.push(json!({"role": "user", "content": text}));
                }
            }
            Role::Assistant => {
                let mut text = String::new();
                let mut calls = vec![];
                for b in &m.content {
                    match b {
                        Block::Text { text: t } => text.push_str(t),
                        Block::ToolUse { id, name, input, .. } => calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": {"name": name, "arguments": input.to_string()}
                        })),
                        _ => {}
                    }
                }
                let mut msg = json!({"role": "assistant", "content": if text.is_empty() { Value::Null } else { json!(text) }});
                if !calls.is_empty() {
                    msg["tool_calls"] = json!(calls);
                }
                out.push(msg);
            }
        }
    }
    out
}

#[derive(Default)]
struct CallAcc {
    id: String,
    name: String,
    args: String,
}

pub async fn stream(
    http: &reqwest::Client,
    base: &str,
    key: Option<&str>,
    is_openai: bool,
    req: &TurnRequest<'_>,
    on_delta: &mut (dyn FnMut(StreamDelta) + Send),
) -> Result<Turn> {
    let tools: Vec<Value> = req
        .tools
        .iter()
        .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.schema}}))
        .collect();
    let mut body = json!({
        "model": req.model,
        "messages": to_wire(req.system, req.messages),
        "stream": true,
        "stream_options": {"include_usage": true},
    });
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    if is_openai {
        body["max_completion_tokens"] = json!(32000);
    }

    let mut b = http.post(format!("{base}/chat/completions")).json(&body);
    if let Some(k) = key.filter(|k| !k.is_empty()) {
        b = b.bearer_auth(k);
    }
    if base.contains("openrouter.ai") {
        b = b.header("X-Title", "Casylum");
    }
    let resp = check(b.send().await?, req.provider).await?;
    let mut sse = Sse::new(resp);

    let mut text = String::new();
    let mut reasoning = String::new();
    let mut calls: BTreeMap<u64, CallAcc> = BTreeMap::new();
    let mut usage = Usage::default();
    let mut finish = String::new();

    while let Some(ev) = sse.next().await? {
        if ev.data == "[DONE]" {
            break;
        }
        let v: Value = match serde_json::from_str(&ev.data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(err) = v.get("error") {
            return Err(anyhow!("{} stream error: {}", req.provider, err["message"].as_str().unwrap_or(&err.to_string())));
        }
        if let Some(u) = v.get("usage").filter(|u| !u.is_null()) {
            usage.input_tokens = u["prompt_tokens"].as_u64().unwrap_or(usage.input_tokens);
            usage.output_tokens = u["completion_tokens"].as_u64().unwrap_or(usage.output_tokens);
        }
        let Some(choice) = v["choices"].get(0) else { continue };
        let d = &choice["delta"];
        if let Some(s) = d["content"].as_str() {
            text.push_str(s);
            on_delta(StreamDelta::Text(s.to_string()));
        }
        // OpenRouter / some local servers stream reasoning separately.
        if let Some(s) = d["reasoning"].as_str().or_else(|| d["reasoning_content"].as_str()) {
            reasoning.push_str(s);
            on_delta(StreamDelta::Thinking(s.to_string()));
        }
        if let Some(arr) = d["tool_calls"].as_array() {
            for tc in arr {
                let idx = tc["index"].as_u64().unwrap_or(0);
                let acc = calls.entry(idx).or_default();
                if let Some(id) = tc["id"].as_str() {
                    acc.id = id.to_string();
                }
                if let Some(n) = tc["function"]["name"].as_str() {
                    if acc.name.is_empty() {
                        on_delta(StreamDelta::ToolStarted { name: n.to_string() });
                    }
                    acc.name.push_str(n);
                }
                if let Some(a) = tc["function"]["arguments"].as_str() {
                    acc.args.push_str(a);
                }
            }
        }
        if let Some(f) = choice["finish_reason"].as_str() {
            finish = f.to_string();
        }
    }

    let mut content = vec![];
    if !reasoning.is_empty() {
        content.push(Block::Thinking { thinking: reasoning, signature: None });
    }
    if !text.is_empty() {
        content.push(Block::Text { text });
    }
    for (_, c) in calls {
        let input = if c.args.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&c.args).unwrap_or_else(|_| json!({"__invalid": c.args}))
        };
        let id = if c.id.is_empty() { format!("call_{}", uuid::Uuid::new_v4().simple()) } else { c.id };
        content.push(Block::ToolUse { id, name: c.name, input, provider_meta: None });
    }
    let has_tools = content.iter().any(|b| matches!(b, Block::ToolUse { .. }));
    let stop = match finish.as_str() {
        _ if has_tools => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        "content_filter" => StopReason::Refusal,
        _ => StopReason::EndTurn,
    };
    Ok(Turn { content, stop, usage })
}
