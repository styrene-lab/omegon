//! Unified model catalog — cloud and local inference models with metadata.
//!
//! This is the single source of truth for all available models across providers.
//! It supports:
//! - Dynamic discovery (Ollama, OpenRouter live queries)
//! - Static fallback (hardcoded model lists for known providers)
//! - Symmetric representation: cloud and local models are peers
//! - Context limits, capability tags, and hardware requirements

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Capability tags for a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    Reasoning,    // Extended thinking, CoT-optimized
    Coding,       // Good at code generation
    Fast,         // Low latency responses
    Vision,       // Can process images
    Instruction,  // Instruction-following optimized
    Multilingual, // Strong across languages
}

impl Capability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reasoning => "reasoning",
            Self::Coding => "coding",
            Self::Fast => "fast",
            Self::Vision => "vision",
            Self::Instruction => "instruction",
            Self::Multilingual => "multilingual",
        }
    }
}

/// A single model's metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// `provider:model-id` (e.g. `openrouter:qwen/qwen-qwq-32b`)
    pub id: String,
    /// Human-readable name (e.g. "Qwen QwQ 32B")
    pub name: String,
    /// Provider name (e.g. "OpenRouter", "Ollama", "Anthropic")
    pub provider: String,
    /// Max input tokens
    pub context_input: usize,
    /// Max output tokens
    pub context_output: usize,
    /// Capability tags
    pub capabilities: Vec<Capability>,
    /// Brief description
    pub description: String,
    /// Whether it's available (authenticated, installed, etc.)
    pub available: bool,
    /// Stable semantic model identity shared across provider routes.
    pub conceptual_model_id: Option<String>,
    /// Model producer/vendor independent of serving provider route.
    pub producer: Option<String>,
    /// Trust/deployment/commercial execution class for the route.
    pub execution_class: Option<String>,
}

impl ModelInfo {
    /// Format context window as "200k in / 8k out"
    pub fn context_str(&self) -> String {
        format!(
            "{}k in / {}k out",
            self.context_input / 1000,
            self.context_output / 1000
        )
    }

    /// Format capabilities as comma-separated tags
    pub fn capability_str(&self) -> String {
        self.capabilities
            .iter()
            .map(|c| c.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Model catalog — grouped by provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCatalog {
    /// Models keyed by provider, then by model ID
    pub providers: BTreeMap<String, Vec<ModelInfo>>,
    /// Per-provider discovery freshness (e.g. "live, confirmed 3m ago",
    /// "stale, confirmed 5h ago"). Empty for bootstrap-only sections.
    #[serde(default)]
    pub freshness: BTreeMap<String, String>,
}

impl ModelCatalog {
    pub fn find_by_id(&self, model_id: &str) -> Option<&ModelInfo> {
        self.providers
            .values()
            .flat_map(|models| models.iter())
            .find(|model| model.id == model_id)
    }

    /// Discover the live model catalog.
    ///
    /// - Cloud sections project from an inventory snapshot (embedded registry
    ///   bootstrap + persisted discovery layer), so live-enumerated provider
    ///   models appear and chat-incompatible offerings are filtered. Reads the
    ///   persisted discovery cache only — never the network (spec:
    ///   inference/catalog-unification "Catalog read is non-blocking").
    /// - Ollama falls back to a live `ollama list` when discovery has not yet
    ///   cached local results.
    pub fn discover() -> Self {
        let cache = crate::inference_discovery::DiscoveryCache::load(
            &crate::inference_discovery::default_cache_path(),
        );
        let mut cat = Self::project_with_gate(&cache, |provider| {
            crate::providers::resolve_api_key_sync(provider).is_some()
        });

        if !cache.endpoints.contains_key("ollama") {
            let ollama_models = Self::query_ollama();
            if !ollama_models.is_empty() {
                cat.providers.insert("Ollama".to_string(), ollama_models);
                cat.freshness
                    .insert("Ollama".to_string(), "live query".to_string());
            }
        }

        cat
    }

    /// Project the catalog from an inventory snapshot built from the embedded
    /// registry plus the given discovery cache. `gate` decides which provider
    /// sections are included (credential resolution in production; injectable
    /// for tests). Falls back to the registry-only catalog if the snapshot
    /// cannot be built.
    pub(crate) fn project_with_gate(
        cache: &crate::inference_discovery::DiscoveryCache,
        gate: impl Fn(&str) -> bool,
    ) -> Self {
        use crate::inference_discovery as discovery;
        use crate::inference_inventory::{InventoryLayer, InventorySnapshot, Modality};

        let reg = crate::model_registry::ModelRegistry::global();
        let mut layers = vec![InventoryLayer::embedded_registry(reg)];
        let results = cache.results();
        if !results.is_empty() {
            layers.push(discovery::build_discovery_layer(
                &results,
                &discovery::registry_ids_by_endpoint(reg),
            ));
        }
        let snapshot = match InventorySnapshot::build(1, layers) {
            Ok(snapshot) => snapshot,
            Err(errors) => {
                debug_assert!(false, "catalog projection snapshot rejected: {errors:?}");
                return Self::cloud_only();
            }
        };

        let provider_display: BTreeMap<&str, &str> = [
            ("anthropic", "Anthropic"),
            ("openai", "OpenAI"),
            ("openai-codex", "OpenAI Codex"),
            ("github-copilot", "GitHub Copilot"),
            ("ollama-cloud", "Ollama Cloud"),
            ("ollama", "Ollama"),
            ("groq", "Groq"),
            ("xai", "xAI"),
            ("mistral", "Mistral"),
            ("google", "Google Gemini"),
            ("gemini-openai", "Google Gemini"),
            ("openrouter", "OpenRouter"),
            ("huggingface-router", "Hugging Face Router"),
        ]
        .into();

        let registry_descriptions: BTreeMap<String, String> = reg
            .all_models()
            .map(|m| (format!("{}:{}", m.provider, m.id), m.description.clone()))
            .collect();

        let mut providers: BTreeMap<String, Vec<ModelInfo>> = BTreeMap::new();
        let mut freshness: BTreeMap<String, String> = BTreeMap::new();
        let now = discovery::unix_now();

        for (offering_id, offering) in &snapshot.offerings {
            let endpoint_id = offering.endpoint.value.0.as_str();
            let Some(&display_name) = provider_display.get(endpoint_id) else {
                continue;
            };
            if !gate(endpoint_id) {
                continue;
            }
            // Disabled offerings include registry ids absent from a successful
            // live enumeration — excluded from selection by default.
            if !offering.enabled.value {
                continue;
            }
            // Chat-selection filter: modality plus the non-chat marker that
            // catches text-output internal aux models.
            let text_out = offering
                .output_modalities
                .value
                .iter()
                .any(|m| m.0 == Modality::TEXT);
            let non_chat_marked = offering
                .extensions
                .value
                .get(discovery::EXT_NON_CHAT)
                .is_some_and(|v| v == "true");
            if !text_out || non_chat_marked {
                continue;
            }

            let capabilities = offering
                .capabilities
                .iter()
                .filter(|(_, enabled)| enabled.value)
                .filter_map(|(name, _)| match name.as_str() {
                    "reasoning" => Some(Capability::Reasoning),
                    "coding" => Some(Capability::Coding),
                    "vision" => Some(Capability::Vision),
                    "fast" => Some(Capability::Fast),
                    "instruction" => Some(Capability::Instruction),
                    "multilingual" => Some(Capability::Multilingual),
                    _ => None,
                })
                .collect();
            let description = registry_descriptions
                .get(&offering_id.0)
                .cloned()
                .unwrap_or_else(|| {
                    format!("Discovered live on {display_name}; not yet curated (ungraded)")
                });
            providers
                .entry(display_name.to_string())
                .or_default()
                .push(ModelInfo {
                    id: offering_id.0.clone(),
                    name: offering.display_name.value.clone(),
                    provider: display_name.to_string(),
                    context_input: offering
                        .context_input
                        .as_ref()
                        .map(|c| c.value)
                        .unwrap_or(0),
                    context_output: offering
                        .context_output
                        .as_ref()
                        .map(|c| c.value)
                        .unwrap_or(0),
                    capabilities,
                    description,
                    available: true,
                    conceptual_model_id: offering
                        .conceptual_model
                        .as_ref()
                        .map(|c| c.value.0.clone()),
                    producer: reg.producer_for_route(&offering_id.0).map(str::to_string),
                    execution_class: reg
                        .execution_class_for_route(&offering_id.0)
                        .map(str::to_string),
                });
        }

        for (endpoint_id, result) in &cache.endpoints {
            let Some(&display_name) = provider_display.get(endpoint_id.as_str()) else {
                continue;
            };
            let age_secs = now.saturating_sub(result.fetched_at);
            let age = if age_secs < 120 {
                format!("{age_secs}s ago")
            } else if age_secs < 7200 {
                format!("{}m ago", age_secs / 60)
            } else {
                format!("{}h ago", age_secs / 3600)
            };
            let stale = age_secs > result.ttl_secs;
            // live/cached is process-relative (any fresh process loads from
            // disk), so age + staleness is the operator signal, not source.
            let state = if stale {
                format!("stale, last confirmed {age}")
            } else {
                format!("confirmed {age}")
            };
            freshness.insert(display_name.to_string(), state);
        }

        if providers.is_empty() {
            return Self::cloud_only();
        }
        ModelCatalog {
            providers,
            freshness,
        }
    }

    /// Query `ollama list` and parse installed models into ModelInfo entries.
    fn query_ollama() -> Vec<ModelInfo> {
        let output = std::process::Command::new("ollama").arg("list").output();
        let output = match output {
            Ok(o) if o.status.success() => o,
            _ => return vec![],
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut models = Vec::new();
        for line in stdout.lines().skip(1) {
            // Format: "NAME   ID   SIZE   MODIFIED"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }
            let full_name = parts[0]; // e.g. "glm-4.7-flash:latest"
            // Title-case display name from the name portion (before the colon tag)
            let raw = full_name.split(':').next().unwrap_or(full_name);
            let display_name = raw
                .replace('-', " ")
                .split_whitespace()
                .map(|w| {
                    let mut c = w.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            let size_str = parts.get(2).copied().unwrap_or("");
            let description = if size_str.is_empty() {
                format!("Ollama: {full_name}")
            } else {
                format!("Ollama: {full_name} ({size_str})")
            };
            models.push(ModelInfo {
                id: format!("ollama:{full_name}"),
                name: display_name,
                provider: "Ollama".to_string(),
                context_input: 128_000,
                context_output: 32_768,
                capabilities: vec![Capability::Instruction, Capability::Coding],
                description,
                available: true,
                conceptual_model_id: None,
                producer: None,
                execution_class: Some("local".to_string()),
            });
        }
        models
    }

    /// Cloud-only catalog — includes a provider section only when an API key
    /// is present for it.  Call `discover()` for the full live catalog.
    pub fn cloud_only() -> Self {
        let mut providers = BTreeMap::new();

        fn has_key(provider: &str) -> bool {
            crate::providers::resolve_api_key_sync(provider).is_some()
        }

        // ─── Cloud Providers (from model registry, auth-gated) ────────
        let reg = crate::model_registry::ModelRegistry::global();
        let provider_display: &[(&str, &str)] = &[
            ("anthropic", "Anthropic"),
            ("openai", "OpenAI"),
            ("openai-codex", "OpenAI Codex"),
            ("github-copilot", "GitHub Copilot"),
            ("ollama-cloud", "Ollama Cloud"),
            ("groq", "Groq"),
            ("xai", "xAI"),
            ("mistral", "Mistral"),
            ("google", "Google Gemini"),
            ("openrouter", "OpenRouter"),
        ];

        for &(provider_id, display_name) in provider_display {
            if !has_key(provider_id) {
                continue;
            }
            let models: Vec<ModelInfo> = reg
                .models_for_provider(provider_id)
                .into_iter()
                .map(|m| {
                    let capabilities = m
                        .capabilities
                        .iter()
                        .filter_map(|c| match c.as_str() {
                            "reasoning" => Some(Capability::Reasoning),
                            "coding" => Some(Capability::Coding),
                            "vision" => Some(Capability::Vision),
                            "fast" => Some(Capability::Fast),
                            "instruction" => Some(Capability::Instruction),
                            "multilingual" => Some(Capability::Multilingual),
                            _ => None,
                        })
                        .collect();
                    ModelInfo {
                        id: format!("{}:{}", provider_id, m.id),
                        name: m.name.clone(),
                        provider: display_name.to_string(),
                        context_input: m.context_input,
                        context_output: m.context_output,
                        capabilities,
                        description: m.description.clone(),
                        available: true,
                        conceptual_model_id: m.conceptual_model_id.clone(),
                        producer: m.producer.clone(),
                        execution_class: m.execution_class.clone(),
                    }
                })
                .collect();
            if !models.is_empty() {
                providers.insert(display_name.to_string(), models);
            }
        }

        // Google Antigravity (show as unavailable if no API key)
        if has_key("google-antigravity") && !has_key("google") {
            let models: Vec<ModelInfo> = reg
                .models_for_provider("google")
                .into_iter()
                .map(|m| ModelInfo {
                    id: format!("google-antigravity:{}", m.id),
                    name: m.name.clone(),
                    provider: "Google Antigravity".to_string(),
                    context_input: m.context_input,
                    context_output: m.context_output,
                    capabilities: m
                        .capabilities
                        .iter()
                        .filter_map(|c| match c.as_str() {
                            "reasoning" => Some(Capability::Reasoning),
                            "coding" => Some(Capability::Coding),
                            "vision" => Some(Capability::Vision),
                            "fast" => Some(Capability::Fast),
                            "instruction" => Some(Capability::Instruction),
                            "multilingual" => Some(Capability::Multilingual),
                            _ => None,
                        })
                        .collect(),
                    description: format!("{} via Antigravity subscription", m.name),
                    available: false,
                    conceptual_model_id: m.conceptual_model_id.clone(),
                    producer: m.producer.clone(),
                    execution_class: m.execution_class.clone(),
                })
                .collect();
            if !models.is_empty() {
                providers.insert(
                    "Google Antigravity (use GOOGLE_API_KEY instead)".to_string(),
                    models,
                );
            }
        }

        // In unauthenticated CI / first-run environments, no provider keys and no
        // Ollama daemon may be present. Keep the selector usable and tests
        // deterministic by exposing the registry-backed default provider as
        // unavailable options instead of returning an empty catalog.
        if providers.is_empty() {
            let models: Vec<ModelInfo> = reg
                .models_for_provider("anthropic")
                .into_iter()
                .map(|m| ModelInfo {
                    id: format!("anthropic:{}", m.id),
                    name: m.name.clone(),
                    provider: "Anthropic".to_string(),
                    context_input: m.context_input,
                    context_output: m.context_output,
                    capabilities: m
                        .capabilities
                        .iter()
                        .filter_map(|c| match c.as_str() {
                            "reasoning" => Some(Capability::Reasoning),
                            "coding" => Some(Capability::Coding),
                            "vision" => Some(Capability::Vision),
                            "fast" => Some(Capability::Fast),
                            "instruction" => Some(Capability::Instruction),
                            "multilingual" => Some(Capability::Multilingual),
                            _ => None,
                        })
                        .collect(),
                    description: m.description.clone(),
                    available: false,
                    conceptual_model_id: m.conceptual_model_id.clone(),
                    producer: m.producer.clone(),
                    execution_class: m.execution_class.clone(),
                })
                .collect();
            if !models.is_empty() {
                providers.insert("Anthropic".to_string(), models);
            }
        }

        ModelCatalog {
            providers,
            freshness: BTreeMap::new(),
        }
    }

    // ── Legacy hardcoded blocks removed — all model data now comes from
    // data/model-registry.json via crate::model_registry::ModelRegistry.
    // To add a model, edit the JSON file. Zero Rust changes required. ──

    #[allow(dead_code)]
    fn _removed_hardcoded_blocks() {
        // This marker exists so git blame shows when the migration happened.
        // The following providers were migrated:
        // OpenRouter, Anthropic, OpenAI, Ollama Cloud, Groq, xAI, Mistral,
        // Google Gemini, Google Antigravity, OpenAI Codex
        unreachable!();
    }

    // NOTE: ~500 lines of hardcoded ModelInfo structs were here.
    // They have been replaced by the registry-driven loop above.

    /// Alias for `cloud_only()`.
    pub fn new() -> Self {
        Self::cloud_only()
    }

    /// Get all models, flattened and optionally filtered by provider.
    pub fn all_models(&self) -> Vec<&ModelInfo> {
        self.providers
            .values()
            .flat_map(|models| models.iter())
            .collect()
    }

    /// Filter models by search term (name or description).
    pub fn search(&self, query: &str) -> Vec<&ModelInfo> {
        let q = query.to_lowercase();
        self.all_models()
            .into_iter()
            .filter(|m| {
                m.name.to_lowercase().contains(&q)
                    || m.id.to_lowercase().contains(&q)
                    || m.description.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Filter by capability.
    pub fn by_capability(&self, cap: Capability) -> Vec<&ModelInfo> {
        self.all_models()
            .into_iter()
            .filter(|m| m.capabilities.contains(&cap))
            .collect()
    }

    /// Filter by provider name.
    pub fn by_provider(&self, provider: &str) -> Vec<&ModelInfo> {
        self.providers
            .get(provider)
            .map(|models| models.iter().collect())
            .unwrap_or_default()
    }

    /// Group all catalog routes by conceptual model identity.
    pub fn by_conceptual_model(&self) -> BTreeMap<String, Vec<&ModelInfo>> {
        let mut grouped: BTreeMap<String, Vec<&ModelInfo>> = BTreeMap::new();
        for model in self.all_models() {
            let key = model
                .conceptual_model_id
                .as_deref()
                .unwrap_or(model.id.as_str())
                .to_string();
            grouped.entry(key).or_default().push(model);
        }
        grouped
    }

    /// Get models available for immediate use (authenticated, installed).
    pub fn available(&self) -> Vec<&ModelInfo> {
        self.all_models()
            .into_iter()
            .filter(|m| m.available)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_projects_discovered_offerings_over_static_registry() {
        use crate::inference_discovery::{DiscoveredModel, DiscoveredModels, DiscoveryCache};
        let mut cache = DiscoveryCache::default();
        cache.record(DiscoveredModels {
            endpoint_id: "github-copilot".into(),
            models: vec![
                DiscoveredModel {
                    id: "claude-sonnet-4.6".into(),
                    ..Default::default()
                },
                DiscoveredModel {
                    id: "gpt-5.6-sol".into(),
                    context_input: Some(400_000),
                    ..Default::default()
                },
                DiscoveredModel {
                    id: "text-embedding-3-small-inference".into(),
                    non_chat: true,
                    ..Default::default()
                },
                DiscoveredModel {
                    id: "trajectory-compaction".into(),
                    non_chat: true,
                    ..Default::default()
                },
            ],
            fetched_at: crate::inference_discovery::unix_now(),
            ttl_secs: 3600,
            cached: false,
        });
        let cat = ModelCatalog::project_with_gate(&cache, |p| p == "github-copilot");
        let copilot = cat
            .providers
            .get("GitHub Copilot")
            .expect("copilot section present");
        let ids: Vec<&str> = copilot.iter().map(|m| m.id.as_str()).collect();
        assert!(
            ids.contains(&"github-copilot:gpt-5.6-sol"),
            "uncurated live model must be selectable: {ids:?}"
        );
        assert!(
            ids.contains(&"github-copilot:claude-sonnet-4.6"),
            "curated live model present: {ids:?}"
        );
        assert!(
            !ids.iter().any(|id| id.contains("embedding")),
            "embedding models filtered from chat selection: {ids:?}"
        );
        assert!(
            !ids.iter().any(|id| id.contains("trajectory-compaction")),
            "internal aux models filtered from chat selection: {ids:?}"
        );
        // Registry Copilot ids absent from live enumeration (e.g. gpt-5.4)
        // are excluded from the selection surface.
        assert!(
            !ids.contains(&"github-copilot:gpt-5.4"),
            "absent-from-live registry id must not be selectable: {ids:?}"
        );
        // Curated metadata survives the merge.
        let sonnet = copilot
            .iter()
            .find(|m| m.id == "github-copilot:claude-sonnet-4.6")
            .unwrap();
        assert_eq!(sonnet.context_input, 128_000);
        assert_eq!(
            sonnet.conceptual_model_id.as_deref(),
            Some("claude-sonnet-4.6")
        );
        // Freshness is reported for the enumerated provider.
        assert!(
            cat.freshness
                .get("GitHub Copilot")
                .is_some_and(|s| s.starts_with("confirmed")),
            "freshness: {:?}",
            cat.freshness
        );
    }

    #[test]
    fn catalog_projection_without_cache_matches_bootstrap_and_is_network_free() {
        use crate::inference_discovery::DiscoveryCache;
        let cache = DiscoveryCache::default();
        let cat = ModelCatalog::project_with_gate(&cache, |p| p == "github-copilot");
        let copilot = cat
            .providers
            .get("GitHub Copilot")
            .expect("bootstrap copilot section");
        assert!(
            copilot.iter().any(|m| m.id == "github-copilot:gpt-5.4"),
            "registry bootstrap serves the selector before any discovery"
        );
        assert!(cat.freshness.is_empty());
    }

    #[test]
    fn catalog_has_cloud_providers() {
        // `new()` / `cloud_only()` returns cloud providers gated by key availability.
        // Ollama is not present here — it only appears via `discover()` at runtime.
        let cat = ModelCatalog::new();
        // At minimum the catalog is buildable and the map is populated (may be empty
        // in CI where no API keys are configured — that is correct behavior).
        let _ = cat.providers; // structural smoke test
    }

    #[test]
    fn discover_smoke() {
        // discover() runs `ollama list`; may return empty in CI — just must not panic.
        let cat = ModelCatalog::discover();
        let _ = cat.providers;
    }

    fn fixture_catalog() -> ModelCatalog {
        let mut providers = std::collections::BTreeMap::new();
        providers.insert(
            "OpenRouter".to_string(),
            vec![ModelInfo {
                id: "openrouter:qwen/qwen-qwq-32b".to_string(),
                name: "Qwen QwQ 32B".to_string(),
                provider: "OpenRouter".to_string(),
                context_input: 32768,
                context_output: 8192,
                capabilities: vec![Capability::Reasoning, Capability::Coding],
                description: "Qwen reasoning model".to_string(),
                available: true,
                conceptual_model_id: Some("qwen/qwen-qwq-32b".to_string()),
                producer: Some("qwen".to_string()),
                execution_class: Some("broker-cloud".to_string()),
            }],
        );
        providers.insert(
            "Anthropic".to_string(),
            vec![ModelInfo {
                id: "anthropic:claude-sonnet-4-6".to_string(),
                name: "Claude Sonnet 4.6".to_string(),
                provider: "Anthropic".to_string(),
                context_input: 1_000_000,
                context_output: 65536,
                capabilities: vec![
                    Capability::Reasoning,
                    Capability::Coding,
                    Capability::Vision,
                ],
                description: "Claude Sonnet 4.6".to_string(),
                available: true,
                conceptual_model_id: Some("claude-sonnet-4.6".to_string()),
                producer: Some("anthropic".to_string()),
                execution_class: Some("api-cloud".to_string()),
            }],
        );
        ModelCatalog {
            providers,
            freshness: BTreeMap::new(),
        }
    }

    #[test]
    fn search_finds_qwen() {
        let cat = fixture_catalog();
        let results = cat.search("qwen");
        assert!(!results.is_empty());
        assert!(results.iter().any(|m| m.name.contains("Qwen")));
    }

    #[test]
    fn by_capability_finds_reasoning() {
        let cat = fixture_catalog();
        let results = cat.by_capability(Capability::Reasoning);
        assert!(!results.is_empty());
    }

    #[test]
    fn groups_routes_by_conceptual_model_identity() {
        let mut cat = fixture_catalog();
        cat.providers
            .entry("GitHub Copilot".to_string())
            .or_default()
            .push(ModelInfo {
                id: "github-copilot:claude-sonnet-4.6".to_string(),
                name: "Claude Sonnet 4.6 (GitHub Copilot)".to_string(),
                provider: "GitHub Copilot".to_string(),
                context_input: 128_000,
                context_output: 32_768,
                capabilities: vec![Capability::Reasoning, Capability::Coding],
                description: "Claude Sonnet via Copilot".to_string(),
                available: true,
                conceptual_model_id: Some("claude-sonnet-4.6".to_string()),
                producer: Some("anthropic".to_string()),
                execution_class: Some("subscription-cloud".to_string()),
            });

        let grouped = cat.by_conceptual_model();
        let sonnet_routes = grouped
            .get("claude-sonnet-4.6")
            .expect("sonnet conceptual group");
        assert_eq!(sonnet_routes.len(), 2);
        assert!(
            sonnet_routes
                .iter()
                .any(|route| route.id == "anthropic:claude-sonnet-4-6")
        );
        assert!(
            sonnet_routes
                .iter()
                .any(|route| route.id == "github-copilot:claude-sonnet-4.6")
        );
    }

    #[test]
    fn context_str_formats_correctly() {
        let model = ModelInfo {
            id: "test:model".to_string(),
            name: "Test".to_string(),
            provider: "Test".to_string(),
            context_input: 128000,
            context_output: 8192,
            capabilities: vec![],
            description: "test".to_string(),
            available: true,
            conceptual_model_id: None,
            producer: None,
            execution_class: None,
        };
        assert_eq!(model.context_str(), "128k in / 8k out");
    }

    #[test]
    fn find_by_id_returns_model() {
        let cat = fixture_catalog();
        let model = cat.find_by_id("anthropic:claude-sonnet-4-6");
        assert!(model.is_some());
    }
}
