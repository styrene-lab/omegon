//! Discovery-layer producers for the dynamic inference inventory.
//!
//! Populates `InventorySource::Discovery` from live provider model enumeration.
//! Fetchers are keyed by endpoint *protocol*, not provider: one OpenAI-compatible
//! fetcher covers every `/v1/models`-shaped endpoint; dedicated parsers exist only
//! where the wire contract differs (OpenRouter, Anthropic, Google, GitHub Copilot,
//! local Ollama). Endpoints without an enumeration contract are `None` — a
//! supported state, not an error.
//!
//! Network fetching is separated from response parsing so every parser is
//! testable against canned fixtures without I/O.
//!
//! OpenSpec change: `inference-discovery-producers` (specs: inference/discovery).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::inference_inventory::{
    EndpointId, EvidenceKind, InventoryLayer, InventorySource, Modality, OfferingId, OfferingPatch,
};

/// Conservative context defaults for offerings discovery knows exist but the
/// registry has never curated. Selectable explicitly, excluded from autonomous
/// routing (no grade is ever synthesized from discovery).
pub const UNCURATED_CONTEXT_INPUT: usize = 128_000;
pub const UNCURATED_CONTEXT_OUTPUT: usize = 16_000;

/// Default refresh interval when the provider does not supply its own expiry.
pub const DEFAULT_TTL_SECS: u64 = 3600;

/// How an endpoint's live model list is enumerated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoveryContract {
    /// `GET {base_url}/models` with a bearer credential (OpenAI wire shape).
    OpenAiCompatible,
    /// OpenRouter's enriched `GET /api/v1/models` listing.
    OpenRouter,
    /// Anthropic `GET /v1/models`.
    Anthropic,
    /// Google Generative Language `GET /v1beta/models`.
    Google,
    /// GitHub Copilot token exchange + `{copilot_base}/models`.
    GithubCopilot,
    /// Local Ollama daemon (`ollama list` / `/api/tags`).
    OllamaLocal,
}

/// Resolve the discovery contract for a registry endpoint id.
///
/// This mapping is code-resident until the registry endpoints block grows a
/// `discovery` field (tasks 4.1/4.2 of the OpenSpec change record per-endpoint
/// verification). `None` means non-enumerable: perplexity has no listing API,
/// and openai-codex's ChatGPT-backend token is unverified against `/v1/models`.
pub fn contract_for_endpoint(endpoint_id: &str) -> Option<DiscoveryContract> {
    match endpoint_id {
        "openai" | "groq" | "mistral" | "xai" | "huggingface-router" | "ollama-cloud"
        | "gemini-openai" => Some(DiscoveryContract::OpenAiCompatible),
        "openrouter" => Some(DiscoveryContract::OpenRouter),
        "anthropic" => Some(DiscoveryContract::Anthropic),
        "google" => Some(DiscoveryContract::Google),
        "github-copilot" => Some(DiscoveryContract::GithubCopilot),
        "ollama" => Some(DiscoveryContract::OllamaLocal),
        _ => None,
    }
}

/// One model reported by a live enumeration endpoint.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredModel {
    pub id: String,
    pub display_name: Option<String>,
    pub context_input: Option<usize>,
    pub context_output: Option<usize>,
    /// Provider-asserted capability flags (e.g. Copilot `capabilities.supports`).
    pub capabilities: BTreeMap<String, bool>,
    /// True when the id is structurally an embedding/internal model that can
    /// never serve chat traffic.
    pub non_chat: bool,
}

/// A completed enumeration of one endpoint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredModels {
    pub endpoint_id: String,
    pub models: Vec<DiscoveredModel>,
    /// Unix seconds when the fetch completed.
    pub fetched_at: u64,
    /// Seconds this result stays fresh (provider-supplied expiry wins).
    pub ttl_secs: u64,
    /// True when this result was loaded from the persisted cache rather than
    /// fetched live in this process.
    pub cached: bool,
}

#[derive(Debug)]
pub struct DiscoveryError {
    pub endpoint_id: String,
    /// Redacted, operator-safe description. Never contains credentials.
    pub detail: String,
}

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "discovery({}): {}", self.endpoint_id, self.detail)
    }
}

impl std::error::Error for DiscoveryError {}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Ids that enumerate but can never serve chat: embeddings and provider-internal
/// auxiliary models. Verified against the live Copilot listing (2026-07-14),
/// which returns e.g. `text-embedding-3-small-inference` and
/// `trajectory-compaction` alongside chat models.
fn classify_non_chat(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    id.contains("embedding")
        || id.contains("embed-")
        || id.starts_with("embed")
        || id.contains("trajectory-compaction")
        || id.contains("moderation")
        || id.contains("whisper")
        || id.contains("tts")
}

// ── Parsers (pure, fixture-testable) ────────────────────────────────────────

/// OpenAI wire shape: `{"data": [{"id": "...", "owned_by": "..."}]}`.
/// Groq extends entries with `context_window`; parse it when present.
pub fn parse_openai_compatible(body: &Value) -> Vec<DiscoveredModel> {
    let Some(entries) = body.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str()?.to_string();
            let context_input = entry
                .get("context_window")
                .or_else(|| entry.get("context_length"))
                .and_then(Value::as_u64)
                .map(|v| v as usize);
            Some(DiscoveredModel {
                non_chat: classify_non_chat(&id),
                id,
                context_input,
                ..Default::default()
            })
        })
        .collect()
}

/// OpenRouter: `{"data": [{"id", "name", "context_length", "architecture": {...}, "top_provider": {"max_completion_tokens"}}]}`.
pub fn parse_openrouter(body: &Value) -> Vec<DiscoveredModel> {
    let Some(entries) = body.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str()?.to_string();
            let display_name = entry
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string);
            let context_input = entry
                .get("context_length")
                .and_then(Value::as_u64)
                .map(|v| v as usize);
            let context_output = entry
                .pointer("/top_provider/max_completion_tokens")
                .and_then(Value::as_u64)
                .map(|v| v as usize);
            let output_modalities = entry
                .pointer("/architecture/output_modalities")
                .and_then(Value::as_array);
            let non_chat = classify_non_chat(&id)
                || output_modalities.is_some_and(|mods| {
                    !mods.iter().any(|m| m.as_str() == Some("text"))
                });
            Some(DiscoveredModel {
                id,
                display_name,
                context_input,
                context_output,
                capabilities: BTreeMap::new(),
                non_chat,
            })
        })
        .collect()
}

/// Anthropic: `{"data": [{"id", "display_name"}]}`.
pub fn parse_anthropic(body: &Value) -> Vec<DiscoveredModel> {
    let Some(entries) = body.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str()?.to_string();
            let display_name = entry
                .get("display_name")
                .and_then(Value::as_str)
                .map(str::to_string);
            Some(DiscoveredModel {
                non_chat: classify_non_chat(&id),
                id,
                display_name,
                ..Default::default()
            })
        })
        .collect()
}

/// Google: `{"models": [{"name": "models/<id>", "displayName", "inputTokenLimit", "outputTokenLimit", "supportedGenerationMethods": [...]}]}`.
pub fn parse_google(body: &Value) -> Vec<DiscoveredModel> {
    let Some(entries) = body.get("models").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let name = entry.get("name")?.as_str()?;
            let id = name.strip_prefix("models/").unwrap_or(name).to_string();
            let display_name = entry
                .get("displayName")
                .and_then(Value::as_str)
                .map(str::to_string);
            let context_input = entry
                .get("inputTokenLimit")
                .and_then(Value::as_u64)
                .map(|v| v as usize);
            let context_output = entry
                .get("outputTokenLimit")
                .and_then(Value::as_u64)
                .map(|v| v as usize);
            let methods = entry
                .get("supportedGenerationMethods")
                .and_then(Value::as_array);
            let non_chat = classify_non_chat(&id)
                || methods.is_some_and(|m| {
                    !m.iter()
                        .any(|v| v.as_str() == Some("generateContent"))
                });
            Some(DiscoveredModel {
                id,
                display_name,
                context_input,
                context_output,
                capabilities: BTreeMap::new(),
                non_chat,
            })
        })
        .collect()
}

/// GitHub Copilot `{copilot_base}/models`: `{"data": [{"id", "name", "capabilities": {"limits": {"max_context_window_tokens", "max_output_tokens"}, "supports": {...}}, "model_picker_enabled"}]}`.
pub fn parse_copilot(body: &Value) -> Vec<DiscoveredModel> {
    let Some(entries) = body.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str()?.to_string();
            let display_name = entry
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string);
            let context_input = entry
                .pointer("/capabilities/limits/max_context_window_tokens")
                .and_then(Value::as_u64)
                .map(|v| v as usize);
            let context_output = entry
                .pointer("/capabilities/limits/max_output_tokens")
                .and_then(Value::as_u64)
                .map(|v| v as usize);
            let mut capabilities = BTreeMap::new();
            if let Some(supports) = entry
                .pointer("/capabilities/supports")
                .and_then(Value::as_object)
            {
                if supports.get("tool_calls").and_then(Value::as_bool) == Some(true) {
                    capabilities.insert("tools".to_string(), true);
                }
                if supports.get("vision").and_then(Value::as_bool) == Some(true) {
                    capabilities.insert("vision".to_string(), true);
                }
            }
            let model_type = entry
                .pointer("/capabilities/type")
                .and_then(Value::as_str);
            let non_chat = classify_non_chat(&id)
                || matches!(model_type, Some(t) if t != "chat");
            Some(DiscoveredModel {
                id,
                display_name,
                context_input,
                context_output,
                capabilities,
                non_chat,
            })
        })
        .collect()
}

/// Ollama `/api/tags`: `{"models": [{"name": "llama3.2:latest", ...}]}`.
pub fn parse_ollama_tags(body: &Value) -> Vec<DiscoveredModel> {
    let Some(entries) = body.get("models").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let id = entry.get("name")?.as_str()?.to_string();
            Some(DiscoveredModel {
                non_chat: classify_non_chat(&id),
                id,
                ..Default::default()
            })
        })
        .collect()
}

pub fn parse_for_contract(contract: DiscoveryContract, body: &Value) -> Vec<DiscoveredModel> {
    match contract {
        DiscoveryContract::OpenAiCompatible => parse_openai_compatible(body),
        DiscoveryContract::OpenRouter => parse_openrouter(body),
        DiscoveryContract::Anthropic => parse_anthropic(body),
        DiscoveryContract::Google => parse_google(body),
        DiscoveryContract::GithubCopilot => parse_copilot(body),
        DiscoveryContract::OllamaLocal => parse_ollama_tags(body),
    }
}

// ── Layer construction ───────────────────────────────────────────────────────

/// Build the Discovery-source inventory layer from enumeration results.
///
/// Semantics (spec: inference/discovery):
/// - every discovered id becomes an offering patch asserting availability;
///   ids unknown to lower layers get conservative ungraded defaults
/// - `registry_ids` (per endpoint) that are *absent* from a successful live
///   enumeration are patched `enabled: false` — unavailable-on-endpoint
/// - discovery never writes capability grades
pub fn build_discovery_layer(
    results: &[DiscoveredModels],
    registry_ids: &BTreeMap<String, BTreeSet<String>>,
) -> InventoryLayer {
    let mut layer = InventoryLayer::new(InventorySource::Discovery, EvidenceKind::Discovered);
    for result in results {
        let discovered_ids: BTreeSet<&str> =
            result.models.iter().map(|m| m.id.as_str()).collect();
        for model in &result.models {
            let offering_id = OfferingId(format!("{}:{}", result.endpoint_id, model.id));
            let known = registry_ids
                .get(&result.endpoint_id)
                .is_some_and(|ids| ids.contains(&model.id));
            let output_modality = if model.non_chat && model.id.to_ascii_lowercase().contains("embed")
            {
                Modality(Modality::EMBEDDING.into())
            } else {
                Modality(Modality::TEXT.into())
            };
            let mut capabilities = model.capabilities.clone();
            let mut patch = OfferingPatch {
                endpoint: Some(EndpointId(result.endpoint_id.clone())),
                native_model_id: Some(model.id.clone()),
                enabled: Some(true),
                ..Default::default()
            };
            if let Some(name) = &model.display_name {
                patch.display_name = Some(name.clone());
            }
            if let Some(ctx) = model.context_input {
                patch.context_input = Some(Some(ctx));
            }
            if let Some(ctx) = model.context_output {
                patch.context_output = Some(Some(ctx));
            }
            if !known {
                // Uncurated: conservative defaults, no grade. Lower layers have
                // nothing for this id, so discovery must supply a full shape.
                patch.display_name = patch.display_name.or_else(|| Some(model.id.clone()));
                patch.context_input =
                    Some(Some(model.context_input.unwrap_or(UNCURATED_CONTEXT_INPUT)));
                patch.context_output = Some(Some(
                    model.context_output.unwrap_or(UNCURATED_CONTEXT_OUTPUT),
                ));
                if !model.non_chat {
                    capabilities.entry("coding".to_string()).or_insert(true);
                }
                patch.input_modalities =
                    Some([Modality(Modality::TEXT.into())].into());
                patch.output_modalities = Some([output_modality].into());
                patch.conceptual_model = Some(None);
                patch.extensions = Some(BTreeMap::new());
            } else if model.non_chat {
                patch.output_modalities = Some([output_modality].into());
            }
            patch.capabilities = capabilities;
            layer.offerings.insert(offering_id, patch);
        }
        // Registry-listed ids missing from a successful live enumeration are
        // unavailable on this endpoint.
        if let Some(known_ids) = registry_ids.get(&result.endpoint_id) {
            for known_id in known_ids {
                if !discovered_ids.contains(known_id.as_str()) {
                    layer.offerings.insert(
                        OfferingId(format!("{}:{}", result.endpoint_id, known_id)),
                        OfferingPatch {
                            endpoint: Some(EndpointId(result.endpoint_id.clone())),
                            native_model_id: Some(known_id.clone()),
                            enabled: Some(false),
                            ..Default::default()
                        },
                    );
                }
            }
        }
    }
    layer
}

// ── Persistence ──────────────────────────────────────────────────────────────

pub const CACHE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DiscoveryCache {
    #[serde(default)]
    pub schema_version: u32,
    /// Last-known-good enumeration per endpoint id.
    #[serde(default)]
    pub endpoints: BTreeMap<String, DiscoveredModels>,
}

impl DiscoveryCache {
    pub fn load(path: &Path) -> Self {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match serde_json::from_str::<Self>(&raw) {
            Ok(cache) if cache.schema_version == CACHE_SCHEMA_VERSION => {
                let mut cache = cache;
                for result in cache.endpoints.values_mut() {
                    result.cached = true;
                }
                cache
            }
            _ => Self::default(),
        }
    }

    pub fn store(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut on_disk = self.clone();
        on_disk.schema_version = CACHE_SCHEMA_VERSION;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&on_disk)?)?;
        std::fs::rename(&tmp, path)
    }

    pub fn record(&mut self, result: DiscoveredModels) {
        self.endpoints.insert(result.endpoint_id.clone(), result);
    }

    /// Endpoints whose cached result has expired (or was never fetched),
    /// filtered to those with a discovery contract.
    pub fn due_endpoints<'a>(
        &self,
        candidates: impl IntoIterator<Item = &'a str>,
        now: u64,
        force: bool,
    ) -> Vec<String> {
        candidates
            .into_iter()
            .filter(|id| contract_for_endpoint(id).is_some())
            .filter(|id| {
                if force {
                    return true;
                }
                match self.endpoints.get(*id) {
                    None => true,
                    Some(result) => now >= result.fetched_at.saturating_add(result.ttl_secs),
                }
            })
            .map(str::to_string)
            .collect()
    }

    pub fn results(&self) -> Vec<DiscoveredModels> {
        self.endpoints.values().cloned().collect()
    }
}

/// Default cache location under the user config dir.
pub fn default_cache_path() -> PathBuf {
    crate::paths::user_config_dir().join("discovery-cache.json")
}

// ── Fetching ─────────────────────────────────────────────────────────────────

fn redact(detail: &str) -> String {
    let mut out = detail.replace('\n', " ");
    out.truncate(300);
    out
}

/// Enumerate one endpoint. Credential resolution uses the same paths as the
/// live bridges (`resolve_api_key_sync`); Copilot goes through the shared
/// token-exchange transport in `github_copilot.rs`.
pub async fn fetch_endpoint(
    endpoint_id: &str,
    base_url: Option<&str>,
) -> Result<DiscoveredModels, DiscoveryError> {
    let contract = contract_for_endpoint(endpoint_id).ok_or_else(|| DiscoveryError {
        endpoint_id: endpoint_id.to_string(),
        detail: "no discovery contract".into(),
    })?;
    let err = |detail: String| DiscoveryError {
        endpoint_id: endpoint_id.to_string(),
        detail: redact(&detail),
    };
    let (body, ttl_secs) = match contract {
        DiscoveryContract::GithubCopilot => {
            let payload = crate::github_copilot::fetch_models_payload()
                .await
                .map_err(|e| err(e.to_string()))?;
            let ttl = payload
                .token_refresh_in
                .map(|v| v.max(60) as u64)
                .unwrap_or(DEFAULT_TTL_SECS);
            (payload.body, ttl)
        }
        DiscoveryContract::OllamaLocal => {
            let base = base_url.unwrap_or("http://127.0.0.1:11434");
            let url = format!("{}/api/tags", base.trim_end_matches('/'));
            let body = reqwest::Client::new()
                .get(url)
                .send()
                .await
                .map_err(|e| err(e.to_string()))?
                .error_for_status()
                .map_err(|e| err(e.to_string()))?
                .json::<Value>()
                .await
                .map_err(|e| err(e.to_string()))?;
            (body, DEFAULT_TTL_SECS)
        }
        DiscoveryContract::OpenAiCompatible
        | DiscoveryContract::OpenRouter
        | DiscoveryContract::Anthropic
        | DiscoveryContract::Google => {
            let base = base_url.ok_or_else(|| err("endpoint has no base_url".into()))?;
            let (key, _) = crate::providers::resolve_api_key_sync(endpoint_id)
                .ok_or_else(|| err("no credential resolves".into()))?;
            let url = match contract {
                DiscoveryContract::OpenRouter => {
                    // OpenRouter's enumeration lives at /api/v1/models; the
                    // registry base_url already ends in /api/v1.
                    format!("{}/models", base.trim_end_matches('/'))
                }
                DiscoveryContract::Google => {
                    format!(
                        "https://generativelanguage.googleapis.com/v1beta/models?key={key}"
                    )
                }
                DiscoveryContract::Anthropic => {
                    format!("{}/v1/models", base.trim_end_matches('/'))
                }
                DiscoveryContract::OpenAiCompatible
                | DiscoveryContract::GithubCopilot
                | DiscoveryContract::OllamaLocal => {
                    format!("{}/models", base.trim_end_matches('/'))
                }
            };
            let client = reqwest::Client::new();
            let mut request = client.get(&url).header("Accept", "application/json");
            match contract {
                DiscoveryContract::Anthropic => {
                    request = request
                        .header("x-api-key", &key)
                        .header("anthropic-version", "2023-06-01");
                }
                DiscoveryContract::Google => {}
                _ => {
                    request = request.header("Authorization", format!("Bearer {key}"));
                }
            }
            let body = request
                .send()
                .await
                .map_err(|e| err(e.to_string()))?
                .error_for_status()
                .map_err(|e| err(e.to_string()))?
                .json::<Value>()
                .await
                .map_err(|e| err(e.to_string()))?;
            (body, DEFAULT_TTL_SECS)
        }
    };
    Ok(DiscoveredModels {
        endpoint_id: endpoint_id.to_string(),
        models: parse_for_contract(contract, &body),
        fetched_at: unix_now(),
        ttl_secs,
        cached: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn copilot_fixture() -> Value {
        json!({"data": [
            {"id": "claude-opus-4.8", "name": "Claude Opus 4.8",
             "capabilities": {"type": "chat",
                "limits": {"max_context_window_tokens": 200000, "max_output_tokens": 32000},
                "supports": {"tool_calls": true, "vision": true}}},
            {"id": "gpt-5.6-sol", "name": "GPT-5.6 Sol",
             "capabilities": {"type": "chat",
                "limits": {"max_context_window_tokens": 400000, "max_output_tokens": 64000},
                "supports": {"tool_calls": true}}},
            {"id": "text-embedding-3-small-inference",
             "capabilities": {"type": "embeddings", "limits": {}}},
            {"id": "trajectory-compaction",
             "capabilities": {"type": "chat", "limits": {}}}
        ]})
    }

    #[test]
    fn copilot_parser_extracts_limits_and_capabilities() {
        let models = parse_copilot(&copilot_fixture());
        assert_eq!(models.len(), 4);
        let opus = &models[0];
        assert_eq!(opus.id, "claude-opus-4.8");
        assert_eq!(opus.context_input, Some(200000));
        assert_eq!(opus.context_output, Some(32000));
        assert_eq!(opus.capabilities.get("tools"), Some(&true));
        assert_eq!(opus.capabilities.get("vision"), Some(&true));
        assert!(!opus.non_chat);
        assert!(models[2].non_chat, "embeddings type is non-chat");
        assert!(models[3].non_chat, "internal aux model is non-chat");
    }

    #[test]
    fn openai_compatible_parser_handles_ids_and_groq_context() {
        let body = json!({"data": [
            {"id": "gpt-5.5", "owned_by": "openai"},
            {"id": "llama-3.3-70b-versatile", "context_window": 131072},
            {"id": "text-embedding-3-large"}
        ]});
        let models = parse_openai_compatible(&body);
        assert_eq!(models.len(), 3);
        assert_eq!(models[0].context_input, None);
        assert_eq!(models[1].context_input, Some(131072));
        assert!(models[2].non_chat);
    }

    #[test]
    fn openrouter_parser_reads_rich_metadata() {
        let body = json!({"data": [
            {"id": "anthropic/claude-sonnet-4.6", "name": "Claude Sonnet 4.6",
             "context_length": 1000000,
             "architecture": {"output_modalities": ["text"]},
             "top_provider": {"max_completion_tokens": 64000}},
            {"id": "openai/dall-e-image", "name": "Image Model",
             "architecture": {"output_modalities": ["image"]}}
        ]});
        let models = parse_openrouter(&body);
        assert_eq!(models[0].context_input, Some(1000000));
        assert_eq!(models[0].context_output, Some(64000));
        assert!(!models[0].non_chat);
        assert!(models[1].non_chat, "image-only output is non-chat");
    }

    #[test]
    fn google_parser_strips_prefix_and_filters_methods() {
        let body = json!({"models": [
            {"name": "models/gemini-3.1-pro-preview", "displayName": "Gemini 3.1 Pro",
             "inputTokenLimit": 2097152, "outputTokenLimit": 65536,
             "supportedGenerationMethods": ["generateContent", "countTokens"]},
            {"name": "models/embedding-001",
             "supportedGenerationMethods": ["embedContent"]}
        ]});
        let models = parse_google(&body);
        assert_eq!(models[0].id, "gemini-3.1-pro-preview");
        assert_eq!(models[0].context_input, Some(2097152));
        assert!(!models[0].non_chat);
        assert!(models[1].non_chat);
    }

    #[test]
    fn malformed_and_empty_bodies_yield_no_models() {
        for contract in [
            DiscoveryContract::OpenAiCompatible,
            DiscoveryContract::OpenRouter,
            DiscoveryContract::Anthropic,
            DiscoveryContract::Google,
            DiscoveryContract::GithubCopilot,
            DiscoveryContract::OllamaLocal,
        ] {
            assert!(parse_for_contract(contract, &json!({"unexpected": true})).is_empty());
            assert!(parse_for_contract(contract, &json!("not an object")).is_empty());
        }
    }

    fn registry_ids(endpoint: &str, ids: &[&str]) -> BTreeMap<String, BTreeSet<String>> {
        let mut map = BTreeMap::new();
        map.insert(
            endpoint.to_string(),
            ids.iter().map(|s| s.to_string()).collect(),
        );
        map
    }

    #[test]
    fn uncurated_discovered_model_gets_conservative_defaults_no_grade() {
        let results = vec![DiscoveredModels {
            endpoint_id: "github-copilot".into(),
            models: vec![DiscoveredModel {
                id: "gpt-5.6-sol".into(),
                ..Default::default()
            }],
            fetched_at: 1,
            ttl_secs: DEFAULT_TTL_SECS,
            cached: false,
        }];
        let layer = build_discovery_layer(&results, &registry_ids("github-copilot", &[]));
        let patch = &layer.offerings[&OfferingId("github-copilot:gpt-5.6-sol".into())];
        assert_eq!(patch.context_input, Some(Some(UNCURATED_CONTEXT_INPUT)));
        assert_eq!(patch.context_output, Some(Some(UNCURATED_CONTEXT_OUTPUT)));
        assert_eq!(patch.capabilities.get("coding"), Some(&true));
        assert!(
            patch.capability_grades.is_empty(),
            "discovery must never synthesize grades"
        );
        assert_eq!(patch.enabled, Some(true));
    }

    #[test]
    fn curated_discovered_model_patches_availability_without_overriding_metadata() {
        let results = vec![DiscoveredModels {
            endpoint_id: "github-copilot".into(),
            models: vec![DiscoveredModel {
                id: "claude-sonnet-4.6".into(),
                ..Default::default()
            }],
            fetched_at: 1,
            ttl_secs: DEFAULT_TTL_SECS,
            cached: false,
        }];
        let layer =
            build_discovery_layer(&results, &registry_ids("github-copilot", &["claude-sonnet-4.6"]));
        let patch = &layer.offerings[&OfferingId("github-copilot:claude-sonnet-4.6".into())];
        assert_eq!(patch.enabled, Some(true));
        assert_eq!(
            patch.context_input, None,
            "known ids keep registry metadata unless the provider reported limits"
        );
    }

    #[test]
    fn registry_id_absent_from_live_enumeration_is_disabled() {
        let results = vec![DiscoveredModels {
            endpoint_id: "github-copilot".into(),
            models: vec![DiscoveredModel {
                id: "gpt-5.5".into(),
                ..Default::default()
            }],
            fetched_at: 1,
            ttl_secs: DEFAULT_TTL_SECS,
            cached: false,
        }];
        let layer = build_discovery_layer(
            &results,
            &registry_ids("github-copilot", &["gpt-5.5", "retired-model"]),
        );
        let retired = &layer.offerings[&OfferingId("github-copilot:retired-model".into())];
        assert_eq!(retired.enabled, Some(false));
        let live = &layer.offerings[&OfferingId("github-copilot:gpt-5.5".into())];
        assert_eq!(live.enabled, Some(true));
    }

    #[test]
    fn ttl_expiry_and_force_control_due_endpoints() {
        let mut cache = DiscoveryCache::default();
        cache.record(DiscoveredModels {
            endpoint_id: "openai".into(),
            models: vec![],
            fetched_at: 1000,
            ttl_secs: 3600,
            cached: false,
        });
        let candidates = ["openai", "groq", "perplexity"];
        // Unexpired: openai skipped; groq never fetched → due; perplexity has
        // no contract → never due.
        let due = cache.due_endpoints(candidates, 2000, false);
        assert_eq!(due, vec!["groq".to_string()]);
        // Expired.
        let due = cache.due_endpoints(candidates, 1000 + 3600, false);
        assert_eq!(due, vec!["openai".to_string(), "groq".to_string()]);
        // Force bypasses TTL but not the contract filter.
        let due = cache.due_endpoints(candidates, 2000, true);
        assert_eq!(due, vec!["openai".to_string(), "groq".to_string()]);
    }

    #[test]
    fn cache_round_trips_and_marks_loaded_results_cached() {
        let dir = std::env::temp_dir().join(format!("omegon-disc-test-{}", std::process::id()));
        let path = dir.join("discovery-cache.json");
        let mut cache = DiscoveryCache::default();
        cache.record(DiscoveredModels {
            endpoint_id: "github-copilot".into(),
            models: vec![DiscoveredModel {
                id: "gpt-5.5".into(),
                ..Default::default()
            }],
            fetched_at: 42,
            ttl_secs: 1500,
            cached: false,
        });
        cache.store(&path).expect("store cache");
        let loaded = DiscoveryCache::load(&path);
        let result = &loaded.endpoints["github-copilot"];
        assert!(result.cached, "loaded results must carry cached evidence");
        assert_eq!(result.fetched_at, 42);
        assert_eq!(result.ttl_secs, 1500);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_schema_version_discards_cache() {
        let dir = std::env::temp_dir().join(format!("omegon-disc-ver-{}", std::process::id()));
        let path = dir.join("discovery-cache.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, r#"{"schema_version": 999, "endpoints": {}}"#).unwrap();
        assert!(DiscoveryCache::load(&path).endpoints.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
