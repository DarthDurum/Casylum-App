//! Live model lists. Each provider's list-models endpoint is called with the
//! user's key, filtered to models that can chat, and cached on disk so the
//! picker opens instantly and still works offline. If a provider can't be
//! reached we fall back to the last good list, then to the built-in one.

use super::catalog;
use crate::settings;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// How long a fetched list is trusted before it is fetched again.
const TTL_MS: i64 = 6 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    /// Display name from the provider, when it gives one. The UI prettifies
    /// the id otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelList {
    pub provider: String,
    pub models: Vec<ModelInfo>,
    /// "live" (just fetched), "cache" (recent or last good list) or
    /// "fallback" (built-in list).
    pub source: &'static str,
    pub fetched_at: Option<i64>,
    /// Why a live fetch failed, if it did.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    fetched_at: i64,
    /// Base URL the list came from (Ollama / custom); a change refetches.
    #[serde(default)]
    base: String,
    models: Vec<ModelInfo>,
}

fn cache_path() -> std::path::PathBuf {
    settings::data_dir().join("models.json")
}

fn read_cache() -> HashMap<String, CacheEntry> {
    std::fs::read_to_string(cache_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn write_cache(c: &HashMap<String, CacheEntry>) {
    let tmp = cache_path().with_extension("json.tmp");
    if let Ok(bytes) = serde_json::to_vec(c) {
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(tmp, cache_path());
        }
    }
}

/// Forget a provider's list, e.g. after its key changes.
pub fn invalidate(provider: &str) {
    let mut c = read_cache();
    if c.remove(provider).is_some() {
        write_cache(&c);
    }
}

fn fallback(provider: &str) -> Vec<ModelInfo> {
    catalog()
        .into_iter()
        .find(|p| p.id == provider)
        .map(|p| p.models.into_iter().map(|id| ModelInfo { id: id.into(), name: None }).collect())
        .unwrap_or_default()
}

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub async fn list(http: &reqwest::Client, provider: &str, key: Option<String>, base: Option<String>, refresh: bool) -> ModelList {
    let base_s = base.clone().unwrap_or_default();
    let mut cache = read_cache();
    let cached = cache.get(provider).filter(|e| e.base == base_s).cloned();
    if let Some(e) = &cached {
        if !refresh && now() - e.fetched_at < TTL_MS && !e.models.is_empty() {
            return ModelList { provider: provider.into(), models: e.models.clone(), source: "cache", fetched_at: Some(e.fetched_at), error: None };
        }
    }
    match fetch(http, provider, key, base).await {
        Ok(models) if !models.is_empty() => {
            let at = now();
            cache.insert(provider.into(), CacheEntry { fetched_at: at, base: base_s, models: models.clone() });
            write_cache(&cache);
            ModelList { provider: provider.into(), models, source: "live", fetched_at: Some(at), error: None }
        }
        res => {
            let error = Some(match res {
                Err(e) => e.to_string(),
                Ok(_) => "The provider returned no chat models.".into(),
            });
            match cached {
                Some(e) if !e.models.is_empty() => ModelList { provider: provider.into(), models: e.models, source: "cache", fetched_at: Some(e.fetched_at), error },
                _ => ModelList { provider: provider.into(), models: fallback(provider), source: "fallback", fetched_at: None, error },
            }
        }
    }
}

async fn get_json(rb: reqwest::RequestBuilder, provider: &str) -> Result<Value> {
    let resp = rb.header("Accept", "application/json").timeout(std::time::Duration::from_secs(20)).send().await?;
    Ok(super::check(resp, provider).await?.json().await?)
}

async fn fetch(http: &reqwest::Client, provider: &str, key: Option<String>, base: Option<String>) -> Result<Vec<ModelInfo>> {
    let need_key = || key.clone().ok_or_else(|| anyhow!("No API key set for {provider}."));
    let models = match provider {
        "anthropic" => {
            let base = std::env::var("CASYLUM_ANTHROPIC_BASE").unwrap_or_else(|_| "https://api.anthropic.com".into());
            let v = get_json(
                http.get(format!("{base}/v1/models?limit=1000")).header("x-api-key", need_key()?).header("anthropic-version", "2023-06-01"),
                "Anthropic",
            )
            .await?;
            parse_anthropic(&v)
        }
        "gemini" => {
            let base = std::env::var("CASYLUM_GEMINI_BASE").unwrap_or_else(|_| "https://generativelanguage.googleapis.com".into());
            let v = get_json(http.get(format!("{base}/v1beta/models?pageSize=1000")).header("x-goog-api-key", need_key()?), "Gemini").await?;
            parse_gemini(&v)
        }
        "abacus" => fetch_abacus(http, key, base).await?,
        "cohere" => {
            // Cohere's OpenAI-compatible API has no model list; use the native one.
            let base = std::env::var("CASYLUM_COHERE_BASE").unwrap_or_else(|_| "https://api.cohere.com".into());
            let v = get_json(http.get(format!("{base}/v1/models?endpoint=chat&page_size=1000")).bearer_auth(need_key()?), "Cohere").await?;
            parse_cohere(&v)
        }
        id => {
            let info = super::info(id).ok_or_else(|| anyhow!("Unknown provider: {id}"))?;
            let env = std::env::var(format!("CASYLUM_{}_BASE", id.to_uppercase())).ok();
            let base = env.or(base).filter(|b| !b.is_empty()).ok_or_else(|| anyhow!("Set a base URL for {} in Settings.", info.name))?;
            let mut rb = http.get(format!("{}/models", base.trim_end_matches('/')));
            // OpenRouter lists models without a key; everyone else needs one.
            let key = if info.needs_key && id != "openrouter" { Some(need_key()?) } else { key.clone() };
            if let Some(k) = key.filter(|k| !k.is_empty()) {
                rb = rb.bearer_auth(k);
            }
            let v = get_json(rb, info.name).await?;
            match id {
                "openai" => parse_openai(&v),
                "openrouter" => parse_openrouter(&v),
                "mistral" => parse_mistral(&v),
                _ => parse_compatible(&v),
            }
        }
    };
    Ok(models)
}

/// Abacus's list is public. Try it with the key, then without (a key it
/// doesn't like shouldn't hide the list), then Abacus's own API.
async fn fetch_abacus(http: &reqwest::Client, key: Option<String>, base: Option<String>) -> Result<Vec<ModelInfo>> {
    let base = std::env::var("CASYLUM_ABACUS_BASE").ok().or(base).unwrap_or_else(|| "https://routellm.abacus.ai/v1".into());
    let url = format!("{}/models", base.trim_end_matches('/'));
    let mut first_err = None;
    let mut tries = vec![];
    if let Some(k) = key.filter(|k| !k.is_empty()) {
        tries.push(http.get(&url).bearer_auth(k));
    }
    tries.push(http.get(&url));
    let api = std::env::var("CASYLUM_ABACUS_API").unwrap_or_else(|_| "https://api.abacus.ai".into());
    tries.push(http.get(format!("{api}/api/v0/listRouteLLMModels")));
    for rb in tries {
        match get_json(rb, "Abacus.AI").await {
            Ok(v) => {
                let models = parse_compatible(&v);
                if !models.is_empty() {
                    return Ok(models);
                }
            }
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    Err(first_err.unwrap_or_else(|| anyhow!("Abacus.AI returned no chat models.")))
}

fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str()).filter(|x| !x.is_empty())
}

fn items<'a>(v: &'a Value, k: &str) -> impl Iterator<Item = &'a Value> {
    v.get(k).and_then(|x| x.as_array()).into_iter().flatten()
}

/// Newest first, using the provider's creation time when it has one.
fn by_created(mut v: Vec<(i64, ModelInfo)>) -> Vec<ModelInfo> {
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().map(|(_, m)| m).collect()
}

fn parse_anthropic(v: &Value) -> Vec<ModelInfo> {
    // Already newest first.
    items(v, "data")
        .filter_map(|m| Some(ModelInfo { id: s(m, "id")?.into(), name: s(m, "display_name").map(String::from) }))
        .collect()
}

fn openai_chat_model(id: &str) -> bool {
    const SKIP: &[&str] = &[
        "embedding", "whisper", "tts", "dall-e", "image", "audio", "realtime", "transcribe", "moderation", "search", "davinci",
        "babbage", "instruct", "computer-use", "sora",
    ];
    let chat_family = id.starts_with("gpt-") || id.starts_with("chatgpt-") || id.starts_with("codex") || id.starts_with('o') && id[1..].starts_with(|c: char| c.is_ascii_digit());
    chat_family && !SKIP.iter().any(|w| id.contains(w))
}

fn parse_openai(v: &Value) -> Vec<ModelInfo> {
    by_created(
        items(v, "data")
            .filter_map(|m| {
                let id = s(m, "id")?;
                openai_chat_model(id).then(|| (m["created"].as_i64().unwrap_or(0), ModelInfo { id: id.into(), name: None }))
            })
            .collect(),
    )
}

fn parse_gemini(v: &Value) -> Vec<ModelInfo> {
    const SKIP: &[&str] = &["embedding", "aqa", "imagen", "veo", "tts", "image-generation", "live"];
    let mut out: Vec<ModelInfo> = items(v, "models")
        .filter_map(|m| {
            let id = s(m, "name")?.trim_start_matches("models/");
            let chat = m["supportedGenerationMethods"].as_array().map_or(true, |a| a.iter().any(|x| x == "generateContent"));
            (chat && !SKIP.iter().any(|w| id.contains(w))).then(|| ModelInfo { id: id.into(), name: s(m, "displayName").map(String::from) })
        })
        .collect();
    // Gemini lists oldest first; show the newest families at the top.
    out.sort_by(|a, b| version_key(&b.id).partial_cmp(&version_key(&a.id)).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// "gemini-2.5-pro" -> 2.5, so newer generations sort first.
fn version_key(id: &str) -> f64 {
    id.split(['-', '_']).find_map(|p| p.parse::<f64>().ok()).unwrap_or(0.0)
}

fn parse_openrouter(v: &Value) -> Vec<ModelInfo> {
    by_created(
        items(v, "data")
            .filter_map(|m| {
                let id = s(m, "id")?;
                // The agent needs tool calling; keep models that say they support it.
                let tools = m["supported_parameters"].as_array().map_or(true, |a| a.iter().any(|x| x == "tools"));
                tools.then(|| (m["created"].as_i64().unwrap_or(0), ModelInfo { id: id.into(), name: s(m, "name").map(String::from) }))
            })
            .collect(),
    )
}

/// Words in ids of models that can't chat: embeddings, speech, images,
/// safety classifiers.
fn non_chat(id: &str) -> bool {
    const SKIP: &[&str] = &[
        "embed", "whisper", "tts", "guard", "rerank", "moderation", "transcribe", "dall-e", "imagen", "image-gen", "flux", "stable-diffusion",
        "sdxl", "playai", "orpheus", "speech", "ocr", "bge-", "-audio", "audio-",
    ];
    let id = id.to_lowercase();
    SKIP.iter().any(|w| id.contains(w))
}

/// Any OpenAI-shaped list: `{data: [...]}`, `{models: [...]}` or a bare array
/// (Together). Providers that describe their models (type, tool support,
/// active) are filtered on that.
fn parse_compatible(v: &Value) -> Vec<ModelInfo> {
    let bare = v.as_array().into_iter().flatten();
    let mut out: Vec<(i64, ModelInfo)> = items(v, "data")
        .chain(items(v, "models"))
        .chain(items(v, "result"))
        .chain(bare)
        .filter_map(|m| {
            let id = s(m, "id").or_else(|| s(m, "name"))?;
            let typed_chat = s(m, "type").map_or(true, |t| matches!(t, "chat" | "language" | "code" | "llm" | "model" | "vlm"));
            // Abacus: "text_generation" vs image/audio/classification.
            let kind_chat = s(m, "model_type").map_or(true, |t| matches!(t, "text_generation" | "chat" | "llm" | "text"));
            let text_out = m["output_modalities"].as_array().map_or(true, |a| a.iter().any(|x| x == "text"));
            let ok = !non_chat(id)
                && typed_chat
                && kind_chat
                && text_out
                && m["active"].as_bool() != Some(false)
                && m["supports_chat"].as_bool() != Some(false)
                && m["supports_tools"].as_bool() != Some(false);
            let name = s(m, "display_name").filter(|n| *n != id).map(String::from);
            ok.then(|| (m["created"].as_i64().unwrap_or(0), ModelInfo { id: id.into(), name }))
        })
        .collect();
    let mut seen = std::collections::HashSet::new();
    out.retain(|(_, m)| seen.insert(m.id.clone()));
    // Keep the provider's order unless it gives creation times.
    if out.iter().any(|(c, _)| *c > 0) {
        by_created(out)
    } else {
        out.into_iter().map(|(_, m)| m).collect()
    }
}

fn parse_mistral(v: &Value) -> Vec<ModelInfo> {
    let mut seen = std::collections::HashSet::new();
    items(v, "data")
        .filter(|m| {
            let c = &m["capabilities"];
            c["completion_chat"].as_bool() != Some(false) && c["function_calling"].as_bool() != Some(false) && m["deprecation"].is_null()
        })
        .filter_map(|m| {
            let id = s(m, "id")?;
            (!non_chat(id) && seen.insert(id.to_string())).then(|| ModelInfo { id: id.into(), name: None })
        })
        .collect()
}

fn parse_cohere(v: &Value) -> Vec<ModelInfo> {
    items(v, "models")
        .filter_map(|m| {
            let id = s(m, "name")?;
            let chat = m["endpoints"].as_array().map_or(true, |a| a.iter().any(|x| x == "chat"));
            let tools = m["features"].as_array().map_or(true, |a| a.iter().any(|x| x == "tools" || x == "strict_tools"));
            (chat && tools && !non_chat(id)).then(|| ModelInfo { id: id.into(), name: None })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn anthropic_uses_display_names() {
        let v = json!({"data": [{"id": "claude-opus-5-5", "display_name": "Claude Opus 5.5"}, {"id": "claude-haiku-5-5", "display_name": ""}]});
        assert_eq!(
            parse_anthropic(&v),
            vec![
                ModelInfo { id: "claude-opus-5-5".into(), name: Some("Claude Opus 5.5".into()) },
                ModelInfo { id: "claude-haiku-5-5".into(), name: None },
            ]
        );
    }

    #[test]
    fn openai_keeps_chat_models_newest_first() {
        let v = json!({"data": [
            {"id": "gpt-4.1", "created": 1},
            {"id": "text-embedding-3-large", "created": 9},
            {"id": "gpt-5", "created": 5},
            {"id": "o4-mini", "created": 3},
            {"id": "gpt-4o-realtime-preview", "created": 7},
            {"id": "whisper-1", "created": 2},
            {"id": "omni-moderation-latest", "created": 4},
        ]});
        let ids: Vec<_> = parse_openai(&v).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["gpt-5", "o4-mini", "gpt-4.1"]);
    }

    #[test]
    fn gemini_filters_and_sorts_newest_first() {
        let v = json!({"models": [
            {"name": "models/gemini-2.5-pro", "displayName": "Gemini 2.5 Pro", "supportedGenerationMethods": ["generateContent"]},
            {"name": "models/text-embedding-004", "displayName": "Embedding", "supportedGenerationMethods": ["embedContent"]},
            {"name": "models/gemini-4-argon", "displayName": "Gemini 4 Argon", "supportedGenerationMethods": ["generateContent", "countTokens"]},
            {"name": "models/imagen-4", "supportedGenerationMethods": ["predict"]},
        ]});
        assert_eq!(
            parse_gemini(&v),
            vec![
                ModelInfo { id: "gemini-4-argon".into(), name: Some("Gemini 4 Argon".into()) },
                ModelInfo { id: "gemini-2.5-pro".into(), name: Some("Gemini 2.5 Pro".into()) },
            ]
        );
    }

    #[test]
    fn openrouter_requires_tool_support() {
        let v = json!({"data": [
            {"id": "a/old", "name": "A: Old", "created": 1, "supported_parameters": ["tools"]},
            {"id": "b/no-tools", "name": "B", "created": 3, "supported_parameters": ["temperature"]},
            {"id": "c/new", "name": "C: New", "created": 2},
        ]});
        let ids: Vec<_> = parse_openrouter(&v).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["c/new", "a/old"]);
    }

    #[test]
    fn compatible_reads_openai_shape() {
        let v = json!({"object": "list", "data": [{"id": "qwen3-coder:latest"}, {"id": "nomic-embed-text"}]});
        assert_eq!(parse_compatible(&v), vec![ModelInfo { id: "qwen3-coder:latest".into(), name: None }]);
    }

    #[test]
    fn compatible_drops_speech_and_inactive_models() {
        // Groq-style list.
        let v = json!({"data": [
            {"id": "llama-3.3-70b-versatile", "created": 1, "active": true},
            {"id": "whisper-large-v3", "created": 5},
            {"id": "playai-tts", "created": 4},
            {"id": "meta-llama/llama-guard-4-12b", "created": 3},
            {"id": "openai/gpt-oss-120b", "created": 2},
            {"id": "old-model", "created": 9, "active": false},
        ]});
        let ids: Vec<_> = parse_compatible(&v).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["openai/gpt-oss-120b", "llama-3.3-70b-versatile"]);
    }

    #[test]
    fn compatible_reads_together_array() {
        let v = json!([
            {"id": "Qwen/Qwen3-Coder", "type": "chat", "display_name": "Qwen3 Coder", "created": 2},
            {"id": "BAAI/bge-large", "type": "embedding", "created": 3},
            {"id": "black-forest-labs/FLUX.1", "type": "image", "created": 4},
        ]);
        assert_eq!(parse_compatible(&v), vec![ModelInfo { id: "Qwen/Qwen3-Coder".into(), name: Some("Qwen3 Coder".into()) }]);
    }

    #[test]
    fn compatible_reads_abacus_lists() {
        let models = json!([
            {"id": "route-llm", "display_name": "RouteLLM", "model_type": "text_generation", "output_modalities": ["text"], "tools": true},
            {"id": "claude-opus-5-5", "display_name": "Claude Opus 5.5", "model_type": "text_generation", "output_modalities": ["text"]},
            {"id": "flux-2-pro", "display_name": "FLUX.2 Pro", "model_type": "image_generation", "output_modalities": ["image"]},
            {"id": "gemini-2.5-flash-image", "model_type": "text_generation", "output_modalities": ["text", "image"]},
            {"id": "gpt-4o-transcribe", "model_type": "text_generation", "output_modalities": ["text"]},
            {"id": "gpt-audio-1.5", "model_type": "audio_generation", "output_modalities": ["audio"]},
            {"id": "gemini-decision-1", "model_type": "classification"},
        ]);
        let want = vec![
            ModelInfo { id: "route-llm".into(), name: Some("RouteLLM".into()) },
            ModelInfo { id: "claude-opus-5-5".into(), name: Some("Claude Opus 5.5".into()) },
            ModelInfo { id: "gemini-2.5-flash-image".into(), name: None },
        ];
        // routellm.abacus.ai/v1/models
        assert_eq!(parse_compatible(&json!({"data": models})), want);
        // api.abacus.ai/api/v0/listRouteLLMModels
        assert_eq!(parse_compatible(&json!({"success": true, "result": models})), want);
    }

    #[test]
    fn mistral_needs_chat_and_tools() {
        let v = json!({"data": [
            {"id": "mistral-large-latest", "capabilities": {"completion_chat": true, "function_calling": true}},
            {"id": "mistral-large-latest", "capabilities": {"completion_chat": true, "function_calling": true}},
            {"id": "mistral-embed", "capabilities": {"completion_chat": false, "function_calling": false}},
            {"id": "mistral-ocr-latest", "capabilities": {"completion_chat": false}},
            {"id": "open-mistral-7b", "capabilities": {"completion_chat": true, "function_calling": true}, "deprecation": "2025-03-30"},
        ]});
        let ids: Vec<_> = parse_mistral(&v).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["mistral-large-latest"]);
    }

    #[test]
    fn cohere_needs_chat_and_tools() {
        let v = json!({"models": [
            {"name": "command-a-03-2025", "endpoints": ["chat", "generate"], "features": ["tools", "json_mode"]},
            {"name": "embed-v4.0", "endpoints": ["embed"]},
            {"name": "command-light", "endpoints": ["chat"], "features": []},
        ]});
        let ids: Vec<_> = parse_cohere(&v).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["command-a-03-2025"]);
    }

    #[test]
    fn every_compatible_provider_has_a_base_url() {
        let s = crate::settings::Settings::default();
        for p in super::super::catalog() {
            if p.kind == super::super::Kind::OpenAi && p.id != "custom" {
                let base = super::super::base_url(p.id, &s).unwrap_or_default();
                assert!(base.starts_with("http"), "{} has no base URL", p.id);
            }
        }
    }
}
