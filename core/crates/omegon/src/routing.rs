//! Provider routing — inventory, capability grade bands, and request routing.
//!
//! Providers are ranked by capability grade band and credential availability.
//! The router produces a scored list of candidates; callers pick the top match.

use std::collections::HashMap;
use std::fmt;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::auth;
use crate::bridge::LlmBridge;

// ── Capability grade bands ────────────────────────────────────────────────

/// Capability grade band for task routing. Higher grade bands can handle more complex tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CapabilityGradeBand {
    /// Small/fast models — renames, formatting, boilerplate
    Leaf,
    /// Mid-range models — routine coding, file edits
    Mid,
    /// Frontier models — architecture, complex debugging
    Frontier,
    /// Maximum capability — deep reasoning, multi-step planning
    Max,
}

impl fmt::Display for CapabilityGradeBand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Leaf => write!(f, "leaf"),
            Self::Mid => write!(f, "mid"),
            Self::Frontier => write!(f, "frontier"),
            Self::Max => write!(f, "max"),
        }
    }
}

// ── Provider inventory ──────────────────────────────────────────────

/// A single provider entry in the inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEntry {
    pub provider_id: String,
    pub has_credentials: bool,
    pub is_reachable: bool,
    pub capability_grade: CapabilityGradeBand,
    pub models: Vec<String>,
}

/// Info about an Ollama model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaModelInfo {
    pub name: String,
    pub size_bytes: u64,
    pub is_running: bool,
    pub vram_bytes: u64,
}

/// Inventory of all available providers and their capabilities.
#[derive(Debug, Clone)]
pub struct ProviderInventory {
    pub entries: Vec<ProviderEntry>,
    pub ollama_models: Vec<OllamaModelInfo>,
    pub probed_at: Instant,
}

impl ProviderInventory {
    /// Probe all known providers for credential availability.
    pub fn probe() -> Self {
        let entries = auth::PROVIDERS
            .iter()
            .filter(|p| is_inference_provider(p.id))
            .map(|p| {
                let has_env = p.env_vars.iter().any(|v| std::env::var(v).is_ok());
                let has_stored = auth::read_credentials(auth::auth_json_key(p.id)).is_some();
                let has_credentials = has_env || has_stored;
                let capability_grade = provider_grade(p.id);
                ProviderEntry {
                    provider_id: p.id.to_string(),
                    has_credentials,
                    is_reachable: has_credentials, // Assume reachable if credentialed
                    capability_grade,
                    models: vec![],
                }
            })
            .collect();

        Self {
            entries,
            ollama_models: vec![],
            probed_at: Instant::now(),
        }
    }

    /// Re-probe, replacing current entries.
    pub fn refresh(&mut self) {
        let fresh = Self::probe();
        self.entries = fresh.entries;
        self.probed_at = fresh.probed_at;
        // ollama_models preserved until async refresh updates them
    }

    /// Providers with valid credentials.
    pub fn providers_with_credentials(&self) -> impl Iterator<Item = &ProviderEntry> {
        self.entries.iter().filter(|e| e.has_credentials)
    }

    /// Populate `ollama_models` from a live OllamaManager.
    /// Should be called after `probe()` when Ollama is expected to be available.
    pub async fn probe_ollama(&mut self) {
        let mgr = crate::ollama::OllamaManager::new();
        if !mgr.is_reachable().await {
            return;
        }

        let models = mgr.list_models().await.unwrap_or_default();
        let running = mgr.list_running().await.unwrap_or_default();

        self.ollama_models = models
            .into_iter()
            .map(|m| {
                let running_info = running.iter().find(|r| {
                    r.name == m.name
                        || r.name.starts_with(&format!("{}:", m.name))
                        || m.name.starts_with(&r.name)
                });
                OllamaModelInfo {
                    name: m.name,
                    size_bytes: m.size,
                    is_running: running_info.is_some(),
                    vram_bytes: running_info.map(|r| r.size_vram).unwrap_or(0),
                }
            })
            .collect();

        // Mark the Ollama provider entry as credentialed + reachable
        if let Some(entry) = self.entries.iter_mut().find(|e| e.provider_id == "ollama") {
            entry.has_credentials = true;
            entry.is_reachable = true;
            entry.models = self.ollama_models.iter().map(|m| m.name.clone()).collect();
        }
    }

    /// Pick the best Ollama model from the probed inventory.
    /// Prefers running models, then largest model that fits hardware.
    pub fn best_ollama_model(&self) -> Option<String> {
        if self.ollama_models.is_empty() {
            return None;
        }

        // Prefer a model that's already loaded in VRAM
        if let Some(running) = self.ollama_models.iter().find(|m| m.is_running) {
            return Some(running.name.clone());
        }

        // Fall back to largest available model
        self.ollama_models
            .iter()
            .max_by_key(|m| m.size_bytes)
            .map(|m| m.name.clone())
    }

    /// Format a concise delegation catalog for system prompt injection.
    /// Lists available providers and models so the orchestrator can route
    /// delegate tasks to the right model.
    pub fn format_delegation_catalog(&self, session_model: Option<&str>) -> String {
        let mut lines = vec![
            "## Delegation Model Catalog".to_string(),
            String::new(),
            "Use `delegate` with `model` to route tasks to the appropriate model.".to_string(),
            String::new(),
        ];

        // Local Ollama models (free, always preferred for rote work)
        if !self.ollama_models.is_empty() {
            lines.push("**Local (free, use for rote tasks):**".to_string());
            for m in &self.ollama_models {
                let size_gb = m.size_bytes as f64 / 1_000_000_000.0;
                let status = if m.is_running { " [loaded]" } else { "" };
                lines.push(format!(
                    "- `ollama:{}` — {:.0}GB{}",
                    m.name, size_gb, status
                ));
            }
            lines.push(String::new());
        }

        // Cloud providers with credentials
        let cloud: Vec<_> = self
            .entries
            .iter()
            .filter(|e| e.has_credentials && e.provider_id != "ollama")
            .collect();
        if !cloud.is_empty() {
            lines.push("**Cloud (credentialed):**".to_string());
            for e in &cloud {
                let default_model = default_model_for_provider(&e.provider_id, e.capability_grade);
                let is_current =
                    session_model.is_some_and(|s| s.starts_with(&format!("{}:", e.provider_id)));
                let marker = if is_current { " ← current" } else { "" };
                lines.push(format!(
                    "- `{}:{}` — {}{}",
                    e.provider_id, default_model, e.capability_grade, marker
                ));
            }
            lines.push(String::new());
        }

        lines.push(
            "Prefer local models for file edits, test runs, and mechanical changes.".to_string(),
        );

        lines.join("\n")
    }
}

/// Classify whether a provider ID is an inference provider (vs search/git/etc).
fn is_inference_provider(id: &str) -> bool {
    matches!(
        id,
        "anthropic"
            | "openai"
            | "openai-codex"
            | "github-copilot"
            | "openrouter"
            | "groq"
            | "xai"
            | "mistral"
            | "cerebras"
            | "google"
            | "google-antigravity"
            | "huggingface"
            | "ollama"
    )
}

/// Map provider ID → maximum capability grade band.
fn provider_grade(id: &str) -> CapabilityGradeBand {
    match id {
        "anthropic" | "openai" => CapabilityGradeBand::Max,
        "openai-codex" | "github-copilot" | "openrouter" | "google" | "google-antigravity"
        | "xai" | "mistral" | "huggingface" => CapabilityGradeBand::Frontier,
        "groq" | "cerebras" | "ollama" => CapabilityGradeBand::Mid,
        _ => CapabilityGradeBand::Leaf,
    }
}

// ── Routing ─────────────────────────────────────────────────────────

/// A capability request — what the task needs.
#[derive(Debug, Clone)]
pub struct CapabilityRequest {
    pub grade: CapabilityGradeBand,
    pub prefer_local: bool,
    pub avoid_providers: Vec<String>,
    pub only_providers: Vec<String>,
}

impl Default for CapabilityRequest {
    fn default() -> Self {
        Self {
            grade: CapabilityGradeBand::Frontier,
            prefer_local: false,
            avoid_providers: vec![],
            only_providers: vec![],
        }
    }
}

/// A scored provider candidate.
#[derive(Debug, Clone)]
pub struct ProviderCandidate {
    pub provider_id: String,
    pub model_id: String,
    pub score: f32,
}

/// Route a capability request against an inventory.
/// Returns candidates sorted by score (descending).
pub fn route(req: &CapabilityRequest, inventory: &ProviderInventory) -> Vec<ProviderCandidate> {
    let mut candidates: Vec<ProviderCandidate> = inventory
        .entries
        .iter()
        .filter(|e| e.has_credentials)
        .filter(|e| req.only_providers.is_empty() || req.only_providers.contains(&e.provider_id))
        .filter(|e| !req.avoid_providers.contains(&e.provider_id))
        .filter(|e| e.capability_grade >= req.grade)
        .map(|e| {
            let mut score: f32 = 1.0;

            // Prefer exact grade match over over-provisioning
            if e.capability_grade == req.grade {
                score += 2.0;
            }

            // Local preference
            if req.prefer_local && e.provider_id == "ollama" {
                score += 3.0;
            }

            let model_id = if e.provider_id == "ollama" {
                inventory
                    .best_ollama_model()
                    .unwrap_or_else(|| default_model_for_provider(&e.provider_id, req.grade))
            } else {
                default_model_for_provider(&e.provider_id, req.grade)
            };

            ProviderCandidate {
                provider_id: e.provider_id.clone(),
                model_id,
                score,
            }
        })
        .collect();

    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates
}

/// Default model for a provider at a given internal capability grade band.
fn default_model_for_provider(provider_id: &str, grade: CapabilityGradeBand) -> String {
    let reg = crate::model_registry::ModelRegistry::global();
    // Map the internal grade-band enum to grade registry keys.
    let grades = match grade {
        CapabilityGradeBand::Max => &["S", "A", "B", "D"][..],
        CapabilityGradeBand::Frontier => &["B", "A", "S", "D"][..],
        CapabilityGradeBand::Mid => &["D", "C", "B"][..],
        CapabilityGradeBand::Leaf => &["D", "C", "F"][..],
    };
    for grade in grades {
        if let Some(model) = reg.grade_model(grade, provider_id) {
            return model.to_string();
        }
    }
    // Fall back to provider default
    reg.default_model(provider_id)
        .map(|s| s.to_string())
        .unwrap_or_else(|| "auto".to_string())
}

/// Infer the capability grade band of a fully-qualified model string ("provider:model").
/// Used by the orchestrator to determine how much headroom children get relative
/// to the parent model — children are capped to parent grade to avoid accidentally
/// routing a leaf-scoped task to a more expensive model than the one that launched the run.
pub fn infer_model_grade_band(model_str: &str) -> CapabilityGradeBand {
    let provider = crate::providers::infer_provider_id(model_str);
    let model = crate::providers::model_id_from_spec(model_str);

    // Local/ollama models use parameter-count heuristic
    if matches!(provider.as_str(), "ollama" | "local") {
        return infer_local_model_grade_band(model);
    }

    // Try registry route patterns first
    let reg = crate::model_registry::ModelRegistry::global();
    if let Some(grade) = reg.infer_grade(&provider, model) {
        return match grade {
            "S" => CapabilityGradeBand::Max,
            "A" | "B" => CapabilityGradeBand::Frontier,
            "C" | "D" => CapabilityGradeBand::Mid,
            "F" => CapabilityGradeBand::Leaf,
            _ => CapabilityGradeBand::Frontier,
        };
    }

    // Provider-level heuristics for models not in the registry
    match provider.as_str() {
        "anthropic" if model.contains("opus") => CapabilityGradeBand::Max,
        "anthropic" if model.contains("sonnet") => CapabilityGradeBand::Frontier,
        "anthropic" => CapabilityGradeBand::Leaf,
        "github-copilot" if model.starts_with("gpt-5") || model.contains("opus") => {
            CapabilityGradeBand::Max
        }
        "github-copilot" => CapabilityGradeBand::Frontier,
        "openai-codex" if model.contains("mini") => CapabilityGradeBand::Mid,
        "openai-codex" => CapabilityGradeBand::Max,
        "groq" if model.contains("70b") || model.contains("90b") => CapabilityGradeBand::Frontier,
        "groq" => CapabilityGradeBand::Mid,
        "cerebras" if model.contains("70b") => CapabilityGradeBand::Frontier,
        "cerebras" => CapabilityGradeBand::Mid,
        _ => CapabilityGradeBand::Frontier,
    }
}

/// Infer capability grade band for local/ollama models based on parameter count in the name.
/// Models with 70B+ parameters are Frontier-capable; 14B-32B are Mid; smaller are Leaf.
fn infer_local_model_grade_band(model_name: &str) -> CapabilityGradeBand {
    // Extract numeric parameter count from model name patterns like "70b", "72b", "32b"
    let lower = model_name.to_lowercase();
    // Look for patterns like "70b", "72b", "120b", "405b"
    for part in lower.split(|c: char| !c.is_ascii_alphanumeric()) {
        if let Some(num_str) = part.strip_suffix('b')
            && let Ok(params) = num_str.parse::<u32>()
        {
            return match params {
                70.. => CapabilityGradeBand::Frontier,
                14..=69 => CapabilityGradeBand::Mid,
                _ => CapabilityGradeBand::Leaf,
            };
        }
    }
    // Also check for "scout" pattern (llama4:scout = large model)
    if lower.contains("scout") || lower.contains("maverick") {
        return CapabilityGradeBand::Frontier;
    }
    // Default: Mid for unknown local models (conservative but not punishing)
    CapabilityGradeBand::Mid
}

// ── Bridge factory ──────────────────────────────────────────────────

/// Factory that creates and caches LlmBridge instances by provider+model.
#[derive(Default)]
pub struct BridgeFactory {
    cache: HashMap<String, Box<dyn LlmBridge>>,
}

impl BridgeFactory {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Get or create a bridge for the given provider and model.
    pub async fn get_or_create(
        &mut self,
        provider_id: &str,
        model_id: &str,
    ) -> anyhow::Result<&dyn LlmBridge> {
        let key = format!("{provider_id}:{model_id}");
        if !self.cache.contains_key(&key) {
            let bridge = crate::providers::resolve_provider(provider_id)
                .await
                .ok_or_else(|| anyhow::anyhow!("No bridge for provider '{provider_id}'"))?;
            self.cache.insert(key.clone(), bridge);
        }
        Ok(self.cache.get(&key).unwrap().as_ref())
    }
}

// ── Cleave grade inference ───────────────────────────────────────────

/// Infer capability grade band from a child plan's scope.
pub fn infer_capability_grade(scope_len: usize) -> CapabilityGradeBand {
    match scope_len {
        0..=2 => CapabilityGradeBand::Leaf,
        3..=5 => CapabilityGradeBand::Mid,
        _ => CapabilityGradeBand::Frontier,
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_inventory(providers: Vec<(&str, CapabilityGradeBand)>) -> ProviderInventory {
        ProviderInventory {
            entries: providers
                .into_iter()
                .map(|(id, grade)| ProviderEntry {
                    provider_id: id.to_string(),
                    has_credentials: true,
                    is_reachable: true,
                    capability_grade: grade,
                    models: vec![],
                })
                .collect(),
            ollama_models: vec![],
            probed_at: Instant::now(),
        }
    }

    #[test]
    fn test_capability_grade_ordering() {
        assert!(CapabilityGradeBand::Leaf < CapabilityGradeBand::Mid);
        assert!(CapabilityGradeBand::Mid < CapabilityGradeBand::Frontier);
        assert!(CapabilityGradeBand::Frontier < CapabilityGradeBand::Max);
    }

    #[test]
    fn nested_copilot_route_grade_uses_copilot_provider_and_inner_model() {
        assert_eq!(
            infer_model_grade_band("anthropic:github-copilot:gpt-5.5"),
            CapabilityGradeBand::Max
        );
        assert_eq!(
            infer_model_grade_band("github-copilot:anthropic:claude-sonnet-4-6"),
            CapabilityGradeBand::Frontier
        );
    }

    #[test]
    fn test_route_frontier_prefers_anthropic() {
        let inv = mock_inventory(vec![
            ("anthropic", CapabilityGradeBand::Max),
            ("ollama", CapabilityGradeBand::Mid),
        ]);
        let req = CapabilityRequest {
            grade: CapabilityGradeBand::Frontier,
            prefer_local: false,
            avoid_providers: vec![],
            only_providers: vec![],
        };
        let candidates = route(&req, &inv);
        // Anthropic can satisfy Frontier (Max >= Frontier), Ollama cannot (Mid < Frontier)
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].provider_id, "anthropic");
    }

    #[test]
    fn test_route_leaf_returns_ollama() {
        let inv = mock_inventory(vec![("ollama", CapabilityGradeBand::Mid)]);
        let req = CapabilityRequest {
            grade: CapabilityGradeBand::Leaf,
            prefer_local: false,
            avoid_providers: vec![],
            only_providers: vec![],
        };
        let candidates = route(&req, &inv);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].provider_id, "ollama");
    }

    #[test]
    fn test_route_empty_inventory() {
        let inv = ProviderInventory {
            entries: vec![],
            ollama_models: vec![],
            probed_at: Instant::now(),
        };
        let req = CapabilityRequest::default();
        let candidates = route(&req, &inv);
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_route_prefer_local() {
        let inv = mock_inventory(vec![
            ("anthropic", CapabilityGradeBand::Max),
            ("ollama", CapabilityGradeBand::Mid),
        ]);
        let req = CapabilityRequest {
            grade: CapabilityGradeBand::Leaf, // Both can satisfy Leaf
            prefer_local: true,
            avoid_providers: vec![],
            only_providers: vec![],
        };
        let candidates = route(&req, &inv);
        assert!(candidates.len() >= 2);
        assert_eq!(candidates[0].provider_id, "ollama");
    }

    #[test]
    fn test_route_avoid_provider() {
        let inv = mock_inventory(vec![
            ("anthropic", CapabilityGradeBand::Max),
            ("openai", CapabilityGradeBand::Max),
        ]);
        let req = CapabilityRequest {
            grade: CapabilityGradeBand::Frontier,
            prefer_local: false,
            avoid_providers: vec!["anthropic".to_string()],
            only_providers: vec![],
        };
        let candidates = route(&req, &inv);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].provider_id, "openai");
    }

    #[test]
    fn test_openai_codex_defaults_reflect_tier() {
        assert_eq!(
            default_model_for_provider("openai-codex", CapabilityGradeBand::Frontier),
            "gpt-5.4"
        );
        assert_eq!(
            default_model_for_provider("openai-codex", CapabilityGradeBand::Leaf),
            "gpt-5.4-mini"
        );
    }

    #[test]
    fn test_infer_capability_grade() {
        assert_eq!(infer_capability_grade(1), CapabilityGradeBand::Leaf);
        assert_eq!(infer_capability_grade(2), CapabilityGradeBand::Leaf);
        assert_eq!(infer_capability_grade(3), CapabilityGradeBand::Mid);
        assert_eq!(infer_capability_grade(5), CapabilityGradeBand::Mid);
        assert_eq!(infer_capability_grade(7), CapabilityGradeBand::Frontier);
    }

    #[test]
    fn test_provider_entry_serialization() {
        let entry = ProviderEntry {
            provider_id: "anthropic".to_string(),
            has_credentials: true,
            is_reachable: true,
            capability_grade: CapabilityGradeBand::Max,
            models: vec!["claude-sonnet-4-20250514".to_string()],
        };
        let json = serde_json::to_string(&entry).unwrap();
        let round: ProviderEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(round.provider_id, "anthropic");
        assert_eq!(round.capability_grade, CapabilityGradeBand::Max);
    }
}
