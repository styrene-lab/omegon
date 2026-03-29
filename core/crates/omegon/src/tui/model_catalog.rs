//! Unified model catalog — cloud and local inference models with metadata.
//!
//! This is the single source of truth for all available models across providers.
//! It supports:
//! - Dynamic discovery (Ollama, OpenRouter live queries)
//! - Static fallback (hardcoded model lists for known providers)
//! - Symmetric representation: cloud and local models are peers
//! - Context limits, capability tags, cost tier, hardware requirements

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A model's availability tier and cost characteristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CostTier {
    /// Free tier (no charge, rate-limited)
    Free,
    /// Pay-per-token, low cost (<$1/M input tokens)
    CheapAPI,
    /// Pay-per-token, standard cost ($1-10/M)
    StandardAPI,
    /// Premium models ($10+/M)
    Premium,
    /// Local inference (no API cost)
    Local,
}

impl CostTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::CheapAPI => "cheap",
            Self::StandardAPI => "standard",
            Self::Premium => "premium",
            Self::Local => "local",
        }
    }
}

/// Capability tags for a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    Reasoning,     // Extended thinking, CoT-optimized
    Coding,        // Good at code generation
    Fast,          // Low latency responses
    Vision,        // Can process images
    Instruction,   // Instruction-following optimized
    Multilingual,  // Strong across languages
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
    /// Cost tier
    pub cost_tier: CostTier,
    /// Capability tags
    pub capabilities: Vec<Capability>,
    /// Brief description
    pub description: String,
    /// Whether it's available (authenticated, installed, etc.)
    pub available: bool,
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
}

impl ModelCatalog {
    /// Build the canonical model catalog.
    pub fn new() -> Self {
        let mut providers = BTreeMap::new();

        // ─── Local Inference ──────────────────────────────────────────
        providers.insert("Ollama".to_string(), vec![
            // Common Ollama models (user may have different ones installed)
            ModelInfo {
                id: "ollama:llama2".to_string(),
                name: "Llama 2 (7B)".to_string(),
                provider: "Ollama".to_string(),
                context_input: 4096,
                context_output: 2048,
                cost_tier: CostTier::Local,
                capabilities: vec![Capability::Instruction],
                description: "Meta's Llama 2, 7B params — fastest local option".to_string(),
                available: true, // Show in selector; runtime discovery will update
            },
            ModelInfo {
                id: "ollama:mistral".to_string(),
                name: "Mistral (7B)".to_string(),
                provider: "Ollama".to_string(),
                context_input: 8192,
                context_output: 4096,
                cost_tier: CostTier::Local,
                capabilities: vec![Capability::Instruction, Capability::Coding],
                description: "Mistral 7B — excellent instruction-following".to_string(),
                available: true,
            },
            ModelInfo {
                id: "ollama:neural-chat".to_string(),
                name: "Neural Chat (7B)".to_string(),
                provider: "Ollama".to_string(),
                context_input: 8192,
                context_output: 4096,
                cost_tier: CostTier::Local,
                capabilities: vec![Capability::Instruction, Capability::Multilingual],
                description: "Intel's Neural Chat — multilingual, conversation-optimized".to_string(),
                available: true,
            },
            ModelInfo {
                id: "ollama:dolphin-mixtral".to_string(),
                name: "Dolphin Mixtral (8x7B)".to_string(),
                provider: "Ollama".to_string(),
                context_input: 32768,
                context_output: 16384,
                cost_tier: CostTier::Local,
                capabilities: vec![Capability::Reasoning, Capability::Coding],
                description: "Mixtral MoE with extended context — best local reasoning".to_string(),
                available: true,
            },
        ]);

        // ─── Cloud Providers ──────────────────────────────────────────

        // OpenRouter (largest catalog)
        providers.insert("OpenRouter".to_string(), vec![
            // Qwen models
            ModelInfo {
                id: "openrouter:qwen/qwen-qwq-32b".to_string(),
                name: "Qwen QwQ 32B".to_string(),
                provider: "OpenRouter".to_string(),
                context_input: 32768,
                context_output: 8192,
                cost_tier: CostTier::CheapAPI,
                capabilities: vec![Capability::Reasoning, Capability::Coding],
                description: "Qwen's reasoning model — fast, cheap, excellent for problem-solving".to_string(),
                available: true,
            },
            ModelInfo {
                id: "openrouter:qwen/qwen-2.5-72b-instruct".to_string(),
                name: "Qwen 2.5 72B".to_string(),
                provider: "OpenRouter".to_string(),
                context_input: 131072,
                context_output: 8192,
                cost_tier: CostTier::CheapAPI,
                capabilities: vec![Capability::Instruction, Capability::Coding, Capability::Multilingual],
                description: "Qwen's latest instruct model — long context, multilingual".to_string(),
                available: true,
            },
            ModelInfo {
                id: "openrouter:minimax/minimax-m2.7".to_string(),
                name: "MiniMax M 2.7".to_string(),
                provider: "OpenRouter".to_string(),
                context_input: 8192,
                context_output: 4096,
                cost_tier: CostTier::CheapAPI,
                capabilities: vec![Capability::Instruction, Capability::Coding],
                description: "MiniMax M 2.7 — fast, low-cost inference".to_string(),
                available: true,
            },
            // DeepSeek
            ModelInfo {
                id: "openrouter:deepseek/deepseek-chat".to_string(),
                name: "DeepSeek Chat".to_string(),
                provider: "OpenRouter".to_string(),
                context_input: 128000,
                context_output: 8192,
                cost_tier: CostTier::CheapAPI,
                capabilities: vec![Capability::Instruction, Capability::Coding],
                description: "DeepSeek Chat — long context, very affordable".to_string(),
                available: true,
            },
            // Llama via OpenRouter
            ModelInfo {
                id: "openrouter:meta-llama/llama-2-70b-chat".to_string(),
                name: "Llama 2 70B Chat".to_string(),
                provider: "OpenRouter".to_string(),
                context_input: 4096,
                context_output: 2048,
                cost_tier: CostTier::CheapAPI,
                capabilities: vec![Capability::Instruction],
                description: "Meta's Llama 2 via OpenRouter — solid baseline".to_string(),
                available: true,
            },
        ]);

        // Anthropic
        providers.insert("Anthropic".to_string(), vec![
            ModelInfo {
                id: "anthropic:claude-opus-4-1".to_string(),
                name: "Claude Opus 4.1".to_string(),
                provider: "Anthropic".to_string(),
                context_input: 200000,
                context_output: 4096,
                cost_tier: CostTier::Premium,
                capabilities: vec![Capability::Reasoning, Capability::Coding, Capability::Vision],
                description: "Claude 3.5 Opus — state-of-the-art reasoning and vision".to_string(),
                available: true,
            },
            ModelInfo {
                id: "anthropic:claude-sonnet-4-6".to_string(),
                name: "Claude Sonnet 4.6".to_string(),
                provider: "Anthropic".to_string(),
                context_input: 200000,
                context_output: 4096,
                cost_tier: CostTier::StandardAPI,
                capabilities: vec![Capability::Reasoning, Capability::Coding, Capability::Vision],
                description: "Claude 3.5 Sonnet — balanced performance and cost".to_string(),
                available: true,
            },
            ModelInfo {
                id: "anthropic:claude-haiku-4-5".to_string(),
                name: "Claude Haiku 4.5".to_string(),
                provider: "Anthropic".to_string(),
                context_input: 200000,
                context_output: 1024,
                cost_tier: CostTier::CheapAPI,
                capabilities: vec![Capability::Fast, Capability::Instruction],
                description: "Claude 3.5 Haiku — fastest, cheapest Claude".to_string(),
                available: true,
            },
        ]);

        // OpenAI
        providers.insert("OpenAI".to_string(), vec![
            ModelInfo {
                id: "openai:gpt-4o".to_string(),
                name: "GPT-4o".to_string(),
                provider: "OpenAI".to_string(),
                context_input: 128000,
                context_output: 4096,
                cost_tier: CostTier::StandardAPI,
                capabilities: vec![Capability::Reasoning, Capability::Vision, Capability::Coding],
                description: "OpenAI's latest GPT-4 Omni — vision + reasoning".to_string(),
                available: true,
            },
            ModelInfo {
                id: "openai:gpt-4-turbo".to_string(),
                name: "GPT-4 Turbo".to_string(),
                provider: "OpenAI".to_string(),
                context_input: 128000,
                context_output: 4096,
                cost_tier: CostTier::StandardAPI,
                capabilities: vec![Capability::Reasoning, Capability::Vision],
                description: "GPT-4 Turbo — reliable reasoning engine".to_string(),
                available: true,
            },
            ModelInfo {
                id: "openai:gpt-4-mini".to_string(),
                name: "GPT-4 Mini".to_string(),
                provider: "OpenAI".to_string(),
                context_input: 128000,
                context_output: 4096,
                cost_tier: CostTier::CheapAPI,
                capabilities: vec![Capability::Instruction, Capability::Vision],
                description: "GPT-4 Mini — cost-effective GPT-4".to_string(),
                available: true,
            },
        ]);

        // Groq
        providers.insert("Groq".to_string(), vec![
            ModelInfo {
                id: "groq:mixtral-8x7b-32768".to_string(),
                name: "Mixtral 8x7B".to_string(),
                provider: "Groq".to_string(),
                context_input: 32768,
                context_output: 8192,
                cost_tier: CostTier::Free,
                capabilities: vec![Capability::Fast, Capability::Coding],
                description: "Mixtral MoE on Groq hardware — ultra-fast, free tier available".to_string(),
                available: true,
            },
            ModelInfo {
                id: "groq:llama-3.1-70b-versatile".to_string(),
                name: "Llama 3.1 70B".to_string(),
                provider: "Groq".to_string(),
                context_input: 131072,
                context_output: 8192,
                cost_tier: CostTier::Free,
                capabilities: vec![Capability::Instruction, Capability::Fast],
                description: "Llama 3.1 on Groq — long context, instant latency".to_string(),
                available: true,
            },
        ]);

        // xAI (Grok)
        providers.insert("xAI".to_string(), vec![
            ModelInfo {
                id: "xai:grok-2".to_string(),
                name: "Grok 2".to_string(),
                provider: "xAI".to_string(),
                context_input: 131072,
                context_output: 8192,
                cost_tier: CostTier::StandardAPI,
                capabilities: vec![Capability::Reasoning, Capability::Fast],
                description: "Grok 2 — long context reasoning model".to_string(),
                available: true,
            },
        ]);

        // Mistral
        providers.insert("Mistral".to_string(), vec![
            ModelInfo {
                id: "mistral:large".to_string(),
                name: "Mistral Large".to_string(),
                provider: "Mistral".to_string(),
                context_input: 32768,
                context_output: 4096,
                cost_tier: CostTier::StandardAPI,
                capabilities: vec![Capability::Reasoning, Capability::Coding],
                description: "Mistral Large — flagship model".to_string(),
                available: true,
            },
            ModelInfo {
                id: "mistral:small".to_string(),
                name: "Mistral Small".to_string(),
                provider: "Mistral".to_string(),
                context_input: 8192,
                context_output: 2048,
                cost_tier: CostTier::CheapAPI,
                capabilities: vec![Capability::Fast, Capability::Instruction],
                description: "Mistral Small — lightweight, fast".to_string(),
                available: true,
            },
        ]);

        ModelCatalog { providers }
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
    fn catalog_has_local_and_cloud() {
        let cat = ModelCatalog::new();
        assert!(cat.providers.contains_key("Ollama"));
        assert!(cat.providers.contains_key("OpenRouter"));
        assert!(cat.providers.contains_key("Anthropic"));
    }

    #[test]
    fn search_finds_qwen() {
        let cat = ModelCatalog::new();
        let results = cat.search("qwen");
        assert!(!results.is_empty());
        assert!(results.iter().any(|m| m.name.contains("Qwen")));
    }

    #[test]
    fn by_capability_finds_reasoning() {
        let cat = ModelCatalog::new();
        let results = cat.by_capability(Capability::Reasoning);
        assert!(!results.is_empty());
    }

    #[test]
    fn context_str_formats_correctly() {
        let model = ModelInfo {
            id: "test:model".to_string(),
            name: "Test".to_string(),
            provider: "Test".to_string(),
            context_input: 128000,
            context_output: 8192,
            cost_tier: CostTier::Local,
            capabilities: vec![],
            description: "test".to_string(),
            available: true,
        };
        assert_eq!(model.context_str(), "128k in / 8k out");
    }
}
