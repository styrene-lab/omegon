//! Centralized model registry loaded from `data/model-registry.json`.
//!
//! This is the single source of truth for model metadata: IDs, pricing,
//! context windows, tier mappings, and capabilities. Adding a new model
//! means editing the JSON file — zero Rust changes required.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

static REGISTRY_JSON: &str = include_str!("../../../../data/model-registry.json");
static REGISTRY: OnceLock<ModelRegistry> = OnceLock::new();

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryFile {
    defaults: HashMap<String, String>,
    tiers: HashMap<String, HashMap<String, String>>,
    routes: Vec<RouteEntry>,
    models: Vec<ModelEntry>,
    #[serde(default)]
    inference_defaults: InferenceDefaults,
}

/// Defaults for dynamically discovered models not in the registry.
/// These heuristics are intentionally in the JSON so they're easy to
/// adjust via PR without touching Rust code.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceDefaults {
    #[serde(default = "default_context_input")]
    pub context_input: usize,
    #[serde(default = "default_context_output")]
    pub context_output: usize,
    #[serde(default = "default_cost_tier")]
    pub cost_tier: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub supports_reasoning: bool,
    #[serde(default)]
    pub name_patterns: HashMap<String, Vec<String>>,
}

fn default_context_input() -> usize {
    131_072
}
fn default_context_output() -> usize {
    32_768
}
fn default_cost_tier() -> String {
    "free".into()
}

impl Default for InferenceDefaults {
    fn default() -> Self {
        Self {
            context_input: default_context_input(),
            context_output: default_context_output(),
            cost_tier: default_cost_tier(),
            capabilities: vec!["instruction".into(), "coding".into()],
            supports_reasoning: false,
            name_patterns: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteEntry {
    pub provider: String,
    #[serde(rename = "modelIdPattern")]
    pub model_id_pattern: String,
    #[serde(rename = "contextCeiling")]
    pub context_ceiling: usize,
    pub tier: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub provider: String,
    pub name: String,
    #[serde(rename = "contextInput")]
    pub context_input: usize,
    #[serde(rename = "contextOutput")]
    pub context_output: usize,
    #[serde(rename = "costTier")]
    pub cost_tier: String,
    pub pricing: Option<PricingEntry>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "supportsReasoning")]
    pub supports_reasoning: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct PricingEntry {
    pub input: f64,
    pub output: f64,
}

pub struct ModelRegistry {
    defaults: HashMap<String, String>,
    tiers: HashMap<String, HashMap<String, String>>,
    routes: Vec<RouteEntry>,
    /// Keyed by "provider:model_id"
    models: HashMap<String, ModelEntry>,
    inference_defaults: InferenceDefaults,
}

impl ModelRegistry {
    pub fn global() -> &'static ModelRegistry {
        REGISTRY.get_or_init(|| {
            let file: RegistryFile =
                serde_json::from_str(REGISTRY_JSON).expect("model-registry.json parse error");
            let mut models = HashMap::new();
            for entry in file.models {
                let key = format!("{}:{}", entry.provider, entry.id);
                models.insert(key, entry);
            }
            ModelRegistry {
                defaults: file.defaults,
                tiers: file.tiers,
                routes: file.routes,
                models,
                inference_defaults: file.inference_defaults,
            }
        })
    }

    /// Default model for a provider (bare model ID, not qualified).
    pub fn default_model(&self, provider: &str) -> Option<&str> {
        self.defaults.get(provider).map(|s| s.as_str())
    }

    /// Model for a given tier + provider (bare model ID).
    pub fn tier_model(&self, tier: &str, provider: &str) -> Option<&str> {
        self.tiers
            .get(tier)
            .and_then(|m| m.get(provider))
            .map(|s| s.as_str())
    }

    /// Full model entry by qualified ID ("provider:model_id").
    pub fn model_info(&self, qualified_id: &str) -> Option<&ModelEntry> {
        self.models.get(qualified_id)
    }

    /// Pricing for a qualified model ID.
    pub fn pricing(&self, qualified_id: &str) -> Option<PricingEntry> {
        self.models.get(qualified_id).and_then(|m| m.pricing)
    }

    /// All model entries for a given provider.
    pub fn models_for_provider(&self, provider: &str) -> Vec<&ModelEntry> {
        self.models
            .values()
            .filter(|m| m.provider == provider)
            .collect()
    }

    /// All model entries.
    pub fn all_models(&self) -> impl Iterator<Item = &ModelEntry> {
        self.models.values()
    }

    /// Context ceiling from route patterns (glob match, highest specificity wins).
    pub fn context_ceiling(&self, provider: &str, model_id: &str) -> Option<usize> {
        let prov = if provider == "ollama" {
            "local"
        } else {
            provider
        };
        let mut best: Option<(usize, usize)> = None; // (specificity, ceiling)
        for route in &self.routes {
            if route.provider != prov && route.provider != provider {
                continue;
            }
            if glob_match(&route.model_id_pattern, model_id) {
                let specificity = route.model_id_pattern.len();
                if best.is_none_or(|(s, _)| specificity > s) {
                    best = Some((specificity, route.context_ceiling));
                }
            }
        }
        best.map(|(_, c)| c)
    }

    /// Infer capability tier from route patterns.
    pub fn infer_tier(&self, provider: &str, model_id: &str) -> Option<&str> {
        let prov = if provider == "ollama" {
            "local"
        } else {
            provider
        };
        let mut best: Option<(usize, &str)> = None;
        for route in &self.routes {
            if route.provider != prov && route.provider != provider {
                continue;
            }
            if glob_match(&route.model_id_pattern, model_id) {
                let specificity = route.model_id_pattern.len();
                if best.is_none_or(|(s, _)| specificity > s) {
                    best = Some((specificity, route.tier.as_str()));
                }
            }
        }
        best.map(|(_, t)| t)
    }

    /// Whether this model supports reasoning/thinking parameters.
    pub fn supports_reasoning(&self, qualified_id: &str) -> bool {
        self.models
            .get(qualified_id)
            .is_some_and(|m| m.supports_reasoning)
    }

    /// Route entries (for settings.rs compatibility during migration).
    pub fn routes(&self) -> &[RouteEntry] {
        &self.routes
    }

    /// Synthesize a ModelEntry for a model not in the registry, using
    /// inference defaults from the JSON config. Capabilities are inferred
    /// from name patterns (e.g. "coder" → coding, "qwq" → reasoning).
    ///
    /// These defaults are intentionally in model-registry.json so they
    /// can be adjusted via PR without Rust changes.
    pub fn infer_unknown_model(&self, provider: &str, model_id: &str) -> ModelEntry {
        let lower = model_id.to_ascii_lowercase();
        let d = &self.inference_defaults;

        // Start with default capabilities
        let mut caps: Vec<String> = d.capabilities.clone();

        // Override from name patterns
        for (cap_name, patterns) in &d.name_patterns {
            if cap_name.starts_with('_') {
                continue; // skip _comment keys
            }
            for pat in patterns {
                if lower.contains(pat) && !caps.contains(cap_name) {
                    caps.push(cap_name.clone());
                    break;
                }
            }
        }

        let supports_reasoning = caps.contains(&"reasoning".to_string());

        ModelEntry {
            id: model_id.to_string(),
            provider: provider.to_string(),
            name: model_id.to_string(),
            context_input: d.context_input,
            context_output: d.context_output,
            cost_tier: d.cost_tier.clone(),
            pricing: Some(PricingEntry {
                input: 0.0,
                output: 0.0,
            }),
            capabilities: caps,
            description: format!("Dynamically discovered {provider} model"),
            supports_reasoning,
        }
    }

    /// Get model info, falling back to inference for unknown models.
    pub fn model_info_or_infer(&self, provider: &str, model_id: &str) -> ModelEntry {
        let key = format!("{provider}:{model_id}");
        if let Some(entry) = self.models.get(&key) {
            return entry.clone();
        }
        self.infer_unknown_model(provider, model_id)
    }
}

/// Simple glob matching: `*` at end matches any suffix, otherwise exact.
fn glob_match(pattern: &str, value: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        value.starts_with(prefix)
    } else {
        value == pattern
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_loads_without_panic() {
        let reg = ModelRegistry::global();
        assert!(!reg.defaults.is_empty());
        assert!(!reg.models.is_empty());
        assert!(!reg.routes.is_empty());
    }

    #[test]
    fn default_model_lookup() {
        let reg = ModelRegistry::global();
        assert_eq!(reg.default_model("openai"), Some("gpt-5.5"));
        assert_eq!(reg.default_model("openai-codex"), Some("gpt-5.5"));
        assert_eq!(reg.default_model("anthropic"), Some("claude-sonnet-4-6"));
        assert_eq!(reg.default_model("nonexistent"), None);
    }

    #[test]
    fn tier_model_lookup() {
        let reg = ModelRegistry::global();
        assert_eq!(reg.tier_model("gloriana", "openai"), Some("gpt-5.5"));
        assert_eq!(
            reg.tier_model("gloriana", "anthropic"),
            Some("claude-opus-4-6")
        );
        assert_eq!(
            reg.tier_model("retribution", "anthropic"),
            Some("claude-haiku-4-5-20251001")
        );
        assert_eq!(reg.tier_model("gloriana", "nonexistent"), None);
    }

    #[test]
    fn claude_opus_4_8_is_registered_as_frontier_anthropic_model() {
        let reg = ModelRegistry::global();
        let info = reg.model_info("anthropic:claude-opus-4-8").unwrap();
        assert_eq!(info.name, "Claude Opus 4.8");
        assert_eq!(info.context_input, 1_000_000);
        assert_eq!(info.context_output, 131_072);
        assert_eq!(info.cost_tier, "premium");
        assert_eq!(
            reg.infer_tier("anthropic", "claude-opus-4-8"),
            Some("gloriana")
        );
    }

    #[test]
    fn model_info_lookup() {
        let reg = ModelRegistry::global();
        let info = reg.model_info("openai:gpt-5.5").unwrap();
        assert_eq!(info.name, "GPT-5.5");
        assert_eq!(info.context_input, 1_000_000);
        assert!(info.supports_reasoning);
    }

    #[test]
    fn pricing_lookup() {
        let reg = ModelRegistry::global();
        let p = reg.pricing("anthropic:claude-opus-4-6").unwrap();
        assert!((p.input - 15.0).abs() < 0.01);
        assert!((p.output - 75.0).abs() < 0.01);
    }

    #[test]
    fn context_ceiling_glob() {
        let reg = ModelRegistry::global();
        assert_eq!(
            reg.context_ceiling("anthropic", "claude-opus-4-6"),
            Some(1_000_000)
        );
        assert_eq!(reg.context_ceiling("openai", "gpt-5.5"), Some(1_000_000));
        assert_eq!(reg.context_ceiling("openai", "gpt-5.4"), Some(1_000_000));
        assert_eq!(reg.context_ceiling("openai", "gpt-5.4-mini"), Some(400_000));
    }

    #[test]
    fn infer_tier_from_routes() {
        let reg = ModelRegistry::global();
        assert_eq!(reg.infer_tier("openai", "gpt-5.5"), Some("gloriana"));
        assert_eq!(
            reg.infer_tier("anthropic", "claude-haiku-4-5-20251001"),
            Some("retribution")
        );
        assert_eq!(reg.infer_tier("openai", "gpt-5-mini"), Some("retribution"));
    }

    #[test]
    fn exact_model_entries_agree_with_matching_routes() {
        let reg = ModelRegistry::global();
        for model in reg.all_models() {
            if let Some(route_ceiling) = reg.context_ceiling(&model.provider, &model.id) {
                assert_eq!(
                    route_ceiling, model.context_input,
                    "route/model context mismatch for {}:{}",
                    model.provider, model.id
                );
            }
        }
    }

    #[test]
    fn defaults_and_tier_models_have_context_constraints() {
        let reg = ModelRegistry::global();
        for (provider, model_id) in &reg.defaults {
            let info = reg.model_info_or_infer(provider, model_id);
            assert!(
                info.context_input > 0,
                "default model lacks context constraint: {provider}:{model_id}"
            );
        }
        for (tier, providers) in &reg.tiers {
            for (provider, model_id) in providers {
                let info = reg.model_info_or_infer(provider, model_id);
                assert!(
                    info.context_input > 0,
                    "tier model lacks context constraint: {tier} {provider}:{model_id}"
                );
            }
        }
    }
    #[test]
    fn infer_unknown_model_applies_name_patterns() {
        let reg = ModelRegistry::global();
        let coder = reg.infer_unknown_model("ollama-cloud", "qwen3-coder:480b-cloud");
        assert!(coder.capabilities.contains(&"coding".to_string()));

        let reasoner = reg.infer_unknown_model("ollama-cloud", "deepseek-r1:70b");
        assert!(reasoner.capabilities.contains(&"reasoning".to_string()));
        assert!(reasoner.supports_reasoning);

        let generic = reg.infer_unknown_model("ollama-cloud", "some-model:7b");
        assert!(!generic.supports_reasoning);
        assert_eq!(generic.context_input, 131_072);
    }

    #[test]
    fn model_info_or_infer_falls_back() {
        let reg = ModelRegistry::global();
        // Known model returns real data
        let known = reg.model_info_or_infer("openai", "gpt-5.5");
        assert_eq!(known.name, "GPT-5.5");
        // Unknown model returns inferred data
        let unknown = reg.model_info_or_infer("ollama-cloud", "mystery-model:13b");
        assert_eq!(unknown.provider, "ollama-cloud");
        assert_eq!(unknown.context_input, 131_072);
    }

    #[test]
    fn glob_match_works() {
        assert!(glob_match("gpt-5.5*", "gpt-5.5"));
        assert!(glob_match("gpt-5.5*", "gpt-5.5-turbo"));
        assert!(!glob_match("gpt-5.5*", "gpt-5.4"));
        assert!(glob_match("gpt-5.4", "gpt-5.4"));
        assert!(!glob_match("gpt-5.4", "gpt-5.4-mini"));
    }
}
