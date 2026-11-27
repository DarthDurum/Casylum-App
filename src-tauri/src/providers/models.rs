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
    let resp = rb.timeout(std::time::Duration::from_secs(15)).send().await?;
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
        "openai" => {
            let base = std::env::var("CASYLUM_OPENAI_BASE").unwrap_or_else(|_| "https://api.openai.com/v1".into());
            let v = get_json(http.get(format!("{base}/models")).bearer_auth(need_key()?), "OpenAI").await?;
            parse_openai(&v)
        }
        "gemini" => {
            let base = std::env::var("CASYLUM_GEMINI_BASE").unwrap_or_else(|_| "https://generativelanguage.googleapis.com".into());
            let v = get_json(http.get(format!("{base}/v1beta/models?pageSize=1000")).header("x-goog-api-key", need_key()?), "Gemini").await?;
            parse_gemini(&v)
        }
        "openrouter" => {
            let base = std::env::var("CASYLUM_OPENROUTER_BASE").unwrap_or_else(|_| "https://openrouter.ai/api/v1".into());
            let mut rb = http.get(format!("{base}/models"));
            if let Some(k) = &key {
                rb = rb.bearer_auth(k);
            }
            parse_openrouter(&get_json(rb, "OpenRouter").await?)
        }
        "ollama" | "custom" => {
            let base = base.filter(|b| !b.is_empty()).ok_or_else(|| anyhow!("Set a base URL for {provider} in Settings."))?;
            let mut rb = http.get(format!("{}/models", base.trim_end_matches('/')));
            if let Some(k) = &key {
                rb = rb.bearer_auth(k);
            }
            parse_compatible(&get_json(rb, provider).await?)
        }
        other => return Err(anyhow!("Unknown provider: {other}")),
    };
    Ok(models)
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

fn parse_compatible(v: &Value) -> Vec<ModelInfo> {
    let mut out: Vec<ModelInfo> = items(v, "data")
        .chain(items(v, "models"))
        .filter_map(|m| {
            let id = s(m, "id").or_else(|| s(m, "name"))?;
            (!id.contains("embed")).then(|| ModelInfo { id: id.into(), name: None })
        })
        .collect();
    out.dedup_by(|a, b| a.id == b.id);
    out
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
}
