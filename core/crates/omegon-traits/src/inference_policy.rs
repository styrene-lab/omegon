//! Additive live projection. This is not a persisted session-v1 fact.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct InferencePolicyProjection {
    pub snapshot_id: Option<String>,
    pub route_key: Option<String>,
    pub estimated_visible_input: Option<usize>,
    pub assembly_budget: Option<usize>,
    pub estimated_percent: Option<f32>,
    pub effective_reasoning: String,
    pub needs_resolution: Option<String>,
    pub details: serde_json::Value,
    pub last_provider_usage: Option<InferenceProviderUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InferenceProviderUsage {
    #[serde(default)]
    pub measured_at: Option<String>,
    pub snapshot_id: String,
    pub route_key: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub semantics: String,
}
