//! Google Gemini (Generative Language API) adapter.

use super::{check, Sse, TurnRequest};
use crate::types::*;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;

fn to_wire(messages: &[Message]) -> Vec<Value> {
    // functionResponse needs the function name, so map ids to names first.
    let mut names: HashMap<&str, &str> = HashMap::new();
    for m in messages {
        for b in &m.content {
            if let Block::ToolUse { id, name, .. } = b {
                names.insert(id, name);
            }
        }
    }
    messages
        .iter()
        .filter_map(|m| {
            let parts: Vec<Value> = m
                .content
                .iter()
                .filter_map(|b| match b {
                    Block::Text { text } => Some(json!({"text": text})),
                    Block::Image { media_type, data, .. } => Some(json!({"inlineData": {"mimeType": media_type, "data": data}})),
                    Block::File { name, text } => Some(json!({"text": file_as_text(name, text)})),
                    Block::ToolUse { name, input, provider_meta, .. } => {
                        let mut p = json!({"functionCall": {"name": name, "args": input}});
                        if let Some(sig) = provider_meta.as_ref().and_then(|m| m.get("thoughtSignature")) {
                            p["thoughtSignature"] = sig.clone();
                        }
                        Some(p)
                    }
                    Block::ToolResult { tool_use_id, content, is_error, .. } => Some(json!({
                        "functionResponse": {
                            "name": names.get(tool_use_id.as_str()).copied().unwrap_or("tool"),
                            "response": if *is_error { json!({"error": content}) } else { json!({"output": content}) }
                        }
                    })),
                    _ => None,
                })
                .collect();
            if parts.is_empty() {
                return None;
            }
            Some(json!({"role": if m.role == Role::User { "user" } else { "model" }, "parts": parts}))
        })
        .collect()
}

/// Gemini takes an OpenAPI subset of JSON Schema and rejects unknown keys
/// (`$schema`, `additionalProperties`, ...), which MCP servers often send.
/// Keep only what it understands.
pub fn clean_schema(v: &Value) -> Value {
    const KEEP: &[&str] = &[
        "type", "format", "description", "nullable", "enum", "items", "properties", "required", "anyOf",
        "minItems", "maxItems", "minimum", "maximum", "minLength", "maxLength", "pattern", "title",
    ];
    let Some(obj) = v.as_object() else { return v.clone() };
    let mut out = serde_json::Map::new();
    for (k, val) in obj {
        if !KEEP.contains(&k.as_str()) {
            continue;
        }
        let val = match k.as_str() {
            "properties" => match val.as_object() {
                Some(props) => Value::Object(props.iter().map(|(n, p)| (n.clone(), clean_schema(p))).collect()),
                None => continue,
            },
            "items" => clean_schema(val),
            "anyOf" => match val.as_array() {
                Some(a) => Value::Array(a.iter().map(clean_schema).collect()),
                None => continue,
            },
            // ["string", "null"] -> "string" + nullable
            "type" => match val.as_array() {
                Some(a) => {
                    if a.iter().any(|t| t == "null") {
                        out.insert("nullable".into(), Value::Bool(true));
                    }
                    a.iter().find(|t| *t != "null").cloned().unwrap_or(Value::String("string".into()))
                }
                None => val.clone(),
            },
            "enum" => match val.as_array() {
                // Gemini only allows string enums.
                Some(a) => Value::Array(a.iter().map(|x| Value::String(x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string()))).collect()),
                None => continue,
            },
            _ => val.clone(),
        };
        out.insert(k.clone(), val);
    }
    // An object with no properties must not list required fields.
    if out.get("type") == Some(&Value::String("object".into())) {
        let props: Vec<String> = out.get("properties").and_then(|p| p.as_object()).map(|p| p.keys().cloned().collect()).unwrap_or_default();
        if let Some(req) = out.get("required").and_then(|r| r.as_array()) {
            let kept: Vec<Value> = req.iter().filter(|r| r.as_str().is_some_and(|r| props.iter().any(|p| p == r))).cloned().collect();
            out.insert("required".into(), Value::Array(kept));
        }
    }
    Value::Object(out)
}

pub async fn stream(
    http: &reqwest::Client,
    key: &str,
    req: &TurnRequest<'_>,
    on_delta: &mut (dyn FnMut(StreamDelta) + Send),
) -> Result<Turn> {
    let decls: Vec<Value> = req
        .tools
        .iter()
        .map(|t| json!({"name": t.name, "description": t.description, "parameters": clean_schema(&t.schema)}))
        .collect();
    let mut body = json!({
        "systemInstruction": {"parts": [{"text": req.system}]},
        "contents": to_wire(req.messages),
        "generationConfig": {"maxOutputTokens": 32000, "thinkingConfig": {"includeThoughts": true}},
    });
    if !decls.is_empty() {
        body["tools"] = json!([{"functionDeclarations": decls}]);
    }
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse",
        req.model
    );
    let resp = check(http.post(url).header("x-goog-api-key", key).json(&body).send().await?, "Gemini").await?;
    let mut sse = Sse::new(resp);

    let mut text = String::new();
    let mut thoughts = String::new();
    let mut tools = vec![];
    let mut usage = Usage::default();
    let mut finish = String::new();

    while let Some(ev) = sse.next().await? {
        let v: Value = match serde_json::from_str(&ev.data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(err) = v.get("error") {
            return Err(anyhow!("Gemini stream error: {}", err["message"].as_str().unwrap_or("unknown")));
        }
        if let Some(u) = v.get("usageMetadata") {
            usage.input_tokens = u["promptTokenCount"].as_u64().unwrap_or(usage.input_tokens);
            usage.output_tokens = u["candidatesTokenCount"].as_u64().unwrap_or(0) + u["thoughtsTokenCount"].as_u64().unwrap_or(0);
        }
        let Some(cand) = v["candidates"].get(0) else { continue };
        if let Some(f) = cand["finishReason"].as_str() {
            finish = f.to_string();
        }
        for p in cand["content"]["parts"].as_array().into_iter().flatten() {
            if let Some(fc) = p.get("functionCall") {
                let name = fc["name"].as_str().unwrap_or("").to_string();
                on_delta(StreamDelta::ToolStarted { name: name.clone() });
                let meta = p.get("thoughtSignature").map(|s| json!({"thoughtSignature": s}));
                tools.push(Block::ToolUse {
                    id: fc["id"].as_str().map(String::from).unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4().simple())),
                    name,
                    input: fc.get("args").cloned().unwrap_or(json!({})),
                    provider_meta: meta,
                });
            } else if let Some(s) = p["text"].as_str() {
                if p["thought"].as_bool() == Some(true) {
                    thoughts.push_str(s);
                    on_delta(StreamDelta::Thinking(s.to_string()));
                } else {
                    text.push_str(s);
                    on_delta(StreamDelta::Text(s.to_string()));
                }
            }
        }
    }

    let mut content = vec![];
    if !thoughts.is_empty() {
        content.push(Block::Thinking { thinking: thoughts, signature: None });
    }
    if !text.is_empty() {
        content.push(Block::Text { text });
    }
    let has_tools = !tools.is_empty();
    content.extend(tools);
    let stop = match finish.as_str() {
        _ if has_tools => StopReason::ToolUse,
        "MAX_TOKENS" => StopReason::MaxTokens,
        "SAFETY" | "PROHIBITED_CONTENT" => StopReason::Refusal,
        _ => StopReason::EndTurn,
    };
    Ok(Turn { content, stop, usage })
}

#[cfg(test)]
mod schema_tests {
    use serde_json::json;

    #[test]
    fn cleans_mcp_schema_for_gemini() {
        let s = json!({"$schema": "x", "type": "object", "additionalProperties": false,
            "properties": {"q": {"type": ["string", "null"], "default": "a"}, "n": {"type": "integer", "enum": [1, 2]}},
            "required": ["q", "missing"]});
        let c = super::clean_schema(&s);
        assert!(c.get("$schema").is_none() && c.get("additionalProperties").is_none());
        assert_eq!(c["properties"]["q"]["type"], "string");
        assert_eq!(c["properties"]["q"]["nullable"], true);
        assert_eq!(c["properties"]["n"]["enum"], json!(["1", "2"]));
        assert_eq!(c["required"], json!(["q"]));
    }
}
