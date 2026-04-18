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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TokenPricing {
    /// USD per 1M input/prompt tokens.
    pub input_per_million_usd: f64,
    /// USD per 1M output/completion tokens.
    pub output_per_million_usd: f64,
}

impl TokenPricing {
    pub const fn new(input_per_million_usd: f64, output_per_million_usd: f64) -> Self {
        Self {
            input_per_million_usd,
            output_per_million_usd,
        }
    }

    pub fn estimate_cost_usd(&self, input_tokens: u64, output_tokens: u64) -> f64 {
        (input_tokens as f64 / 1_000_000.0) * self.input_per_million_usd
            + (output_tokens as f64 / 1_000_000.0) * self.output_per_million_usd
    }
}

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
    /// Cost tier
    pub cost_tier: CostTier,
    /// Explicit token pricing when known. This is the authoritative source for
    /// footer/session cost calculations; `cost_tier` is only a coarse UX bucket.
    pub pricing: Option<TokenPricing>,
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
    pub fn pricing_for_model(model_id: &str) -> Option<TokenPricing> {
        match model_id {
            // Ollama / local
            id if id.starts_with("ollama:") => Some(TokenPricing::new(0.0, 0.0)),
            // Ollama Cloud
            "ollama-cloud:gpt-oss:120b-cloud" => Some(TokenPricing::new(0.0, 0.0)),
            "ollama-cloud:qwen3-coder:480b-cloud" => Some(TokenPricing::new(0.0, 0.0)),

            // OpenRouter
            "openrouter:qwen/qwen-qwq-32b" => Some(TokenPricing::new(0.20, 0.20)),
            "openrouter:qwen/qwen-2.5-72b-instruct" => Some(TokenPricing::new(0.35, 0.40)),
            "openrouter:minimax/minimax-m2.7" => Some(TokenPricing::new(0.28, 1.10)),
            "openrouter:deepseek/deepseek-chat" => Some(TokenPricing::new(0.27, 1.10)),
            "openrouter:meta-llama/llama-2-70b-chat" => Some(TokenPricing::new(0.90, 0.90)),

            // Anthropic
            "anthropic:claude-opus-4-6" => Some(TokenPricing::new(15.0, 75.0)),
            "anthropic:claude-sonnet-4-6" => Some(TokenPricing::new(3.0, 15.0)),
            "anthropic:claude-haiku-4-5-20251001" => Some(TokenPricing::new(0.8, 4.0)),

            // OpenAI API
            "openai:gpt-5.4" => Some(TokenPricing::new(2.5, 15.0)),
            "openai:gpt-5" => Some(TokenPricing::new(2.5, 15.0)),
            "openai:gpt-5-mini" => Some(TokenPricing::new(0.750, 4.500)),
            "openai:gpt-4.1" => Some(TokenPricing::new(2.0, 8.0)),
            "openai:o4-mini" => Some(TokenPricing::new(1.1, 4.4)),

            // Groq / free
            "groq:llama-3.3-70b-versatile" => Some(TokenPricing::new(0.0, 0.0)),

            // xAI
            "xai:grok-4.20-0309-reasoning" => Some(TokenPricing::new(2.0, 6.0)),
            "xai:grok-4.20-0309-non-reasoning" => Some(TokenPricing::new(2.0, 6.0)),
            "xai:grok-4-1-fast-reasoning" => Some(TokenPricing::new(0.2, 0.5)),
            "xai:grok-4-1-fast-non-reasoning" => Some(TokenPricing::new(0.2, 0.5)),
            "xai:grok-4-0709" => Some(TokenPricing::new(3.0, 15.0)),
            "xai:grok-3" => Some(TokenPricing::new(2.0, 10.0)),

            // Mistral
            "mistral:mistral-large-latest" => Some(TokenPricing::new(2.0, 6.0)),
            "mistral:mistral-small-latest" => Some(TokenPricing::new(0.2, 0.6)),

            // ChatGPT / Codex OAuth
            "openai-codex:gpt-5.4" => Some(TokenPricing::new(2.5, 15.0)),
            "openai-codex:gpt-5.4-mini" => Some(TokenPricing::new(0.750, 4.500)),

            _ => None,
        }
    }

    pub fn find_by_id(&self, model_id: &str) -> Option<&ModelInfo> {
        self.providers
            .values()
            .flat_map(|models| models.iter())
            .find(|model| model.id == model_id)
    }

    /// Discover the live model catalog.
    ///
    /// - Ollama section is populated by running `ollama list` — only models
    ///   actually installed on this machine appear.
    /// - Cloud provider sections are included only when a valid API key or
    ///   OAuth token can be resolved for that provider.
    pub fn discover() -> Self {
        let mut cat = Self::cloud_only();

        // Populate Ollama from live `ollama list`
        let ollama_models = Self::query_ollama();
        if !ollama_models.is_empty() {
            cat.providers.insert("Ollama".to_string(), ollama_models);
        }

        cat
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
                cost_tier: CostTier::Local,
                pricing: Some(TokenPricing::new(0.0, 0.0)),
                capabilities: vec![Capability::Instruction, Capability::Coding],
                description,
                available: true,
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

        // ─── Cloud Providers (auth-gated) ─────────────────────────────

        // OpenRouter
        if has_key("openrouter") {
            providers.insert(
                "OpenRouter".to_string(),
                vec![
                    // Qwen models
                    ModelInfo {
                        id: "openrouter:qwen/qwen-qwq-32b".to_string(),
                        name: "Qwen QwQ 32B".to_string(),
                        provider: "OpenRouter".to_string(),
                        context_input: 32768,
                        context_output: 8192,
                        cost_tier: CostTier::CheapAPI,
                        pricing: Some(TokenPricing::new(0.20, 0.20)),
                        capabilities: vec![Capability::Reasoning, Capability::Coding],
                        description:
                            "Qwen's reasoning model — fast, cheap, excellent for problem-solving"
                                .to_string(),
                        available: true,
                    },
                    ModelInfo {
                        id: "openrouter:qwen/qwen-2.5-72b-instruct".to_string(),
                        name: "Qwen 2.5 72B".to_string(),
                        provider: "OpenRouter".to_string(),
                        context_input: 131072,
                        context_output: 8192,
                        cost_tier: CostTier::CheapAPI,
                        pricing: Some(TokenPricing::new(0.35, 0.40)),
                        capabilities: vec![
                            Capability::Instruction,
                            Capability::Coding,
                            Capability::Multilingual,
                        ],
                        description: "Qwen's latest instruct model — long context, multilingual"
                            .to_string(),
                        available: true,
                    },
                    ModelInfo {
                        id: "openrouter:minimax/minimax-m2.7".to_string(),
                        name: "MiniMax M 2.7".to_string(),
                        provider: "OpenRouter".to_string(),
                        context_input: 8192,
                        context_output: 4096,
                        cost_tier: CostTier::CheapAPI,
                        pricing: Some(TokenPricing::new(0.28, 1.10)),
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
                        pricing: Some(TokenPricing::new(0.27, 1.10)),
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
                        pricing: Some(TokenPricing::new(0.90, 0.90)),
                        capabilities: vec![Capability::Instruction],
                        description: "Meta's Llama 2 via OpenRouter — solid baseline".to_string(),
                        available: true,
                    },
                ],
            ); // end OpenRouter
        } // end if has_key("openrouter")

        // Anthropic
        if has_key("anthropic") {
            providers.insert(
                "Anthropic".to_string(),
                vec![
                    ModelInfo {
                        id: "anthropic:claude-opus-4-6".to_string(),
                        name: "Claude Opus 4.6".to_string(),
                        provider: "Anthropic".to_string(),
                        context_input: 1000000,
                        context_output: 131072,
                        cost_tier: CostTier::Premium,
                        pricing: Some(TokenPricing::new(15.0, 75.0)),
                        capabilities: vec![
                            Capability::Reasoning,
                            Capability::Coding,
                            Capability::Vision,
                        ],
                        description: "Claude Opus 4.6 — frontier reasoning, coding, and vision"
                            .to_string(),
                        available: true,
                    },
                    ModelInfo {
                        id: "anthropic:claude-sonnet-4-6".to_string(),
                        name: "Claude Sonnet 4.6".to_string(),
                        provider: "Anthropic".to_string(),
                        context_input: 1000000,
                        context_output: 65536,
                        cost_tier: CostTier::StandardAPI,
                        pricing: Some(TokenPricing::new(3.0, 15.0)),
                        capabilities: vec![
                            Capability::Reasoning,
                            Capability::Coding,
                            Capability::Vision,
                        ],
                        description: "Claude Sonnet 4.6 — balanced performance and cost"
                            .to_string(),
                        available: true,
                    },
                    ModelInfo {
                        id: "anthropic:claude-haiku-4-5-20251001".to_string(),
                        name: "Claude Haiku 4.5".to_string(),
                        provider: "Anthropic".to_string(),
                        context_input: 200000,
                        context_output: 65536,
                        cost_tier: CostTier::CheapAPI,
                        pricing: Some(TokenPricing::new(0.8, 4.0)),
                        capabilities: vec![Capability::Fast, Capability::Instruction],
                        description: "Claude Haiku 4.5 — fastest, cheapest Claude".to_string(),
                        available: true,
                    },
                ],
            ); // end Anthropic
        } // end if has_key("anthropic")

        // OpenAI
        if has_key("openai") {
            providers.insert(
                "OpenAI".to_string(),
                vec![
                    ModelInfo {
                        id: "openai:gpt-5.4".to_string(),
                        name: "GPT-5.4".to_string(),
                        provider: "OpenAI".to_string(),
                        context_input: 1000000,
                        context_output: 32768,
                        cost_tier: CostTier::Premium,
                        pricing: Some(TokenPricing::new(2.5, 15.0)),
                        capabilities: vec![
                            Capability::Reasoning,
                            Capability::Vision,
                            Capability::Coding,
                        ],
                        description:
                            "GPT-5.4 — OpenAI's latest frontier model, 1M context, tool search"
                                .to_string(),
                        available: true,
                    },
                    ModelInfo {
                        id: "openai:gpt-5".to_string(),
                        name: "GPT-5".to_string(),
                        provider: "OpenAI".to_string(),
                        context_input: 1000000,
                        context_output: 32768,
                        cost_tier: CostTier::Premium,
                        pricing: Some(TokenPricing::new(2.5, 15.0)),
                        capabilities: vec![
                            Capability::Reasoning,
                            Capability::Vision,
                            Capability::Coding,
                        ],
                        description: "GPT-5 — flagship reasoning, replaces GPT-4o / o3 / o4-mini"
                            .to_string(),
                        available: true,
                    },
                    ModelInfo {
                        id: "openai:gpt-5-mini".to_string(),
                        name: "GPT-5 Mini".to_string(),
                        provider: "OpenAI".to_string(),
                        context_input: 1000000,
                        context_output: 32768,
                        cost_tier: CostTier::CheapAPI,
                        pricing: Some(TokenPricing::new(0.750, 4.500)),
                        capabilities: vec![
                            Capability::Fast,
                            Capability::Instruction,
                            Capability::Coding,
                        ],
                        description: "GPT-5 Mini — fast, cost-effective, 1M context".to_string(),
                        available: true,
                    },
                    ModelInfo {
                        id: "openai:gpt-4.1".to_string(),
                        name: "GPT-4.1".to_string(),
                        provider: "OpenAI".to_string(),
                        context_input: 1000000,
                        context_output: 32768,
                        cost_tier: CostTier::StandardAPI,
                        pricing: Some(TokenPricing::new(2.0, 8.0)),
                        capabilities: vec![
                            Capability::Reasoning,
                            Capability::Vision,
                            Capability::Coding,
                        ],
                        description: "GPT-4.1 — 1M context (legacy, superseded by GPT-5)"
                            .to_string(),
                        available: true,
                    },
                    ModelInfo {
                        id: "openai:o4-mini".to_string(),
                        name: "o4-mini".to_string(),
                        provider: "OpenAI".to_string(),
                        context_input: 200000,
                        context_output: 16384,
                        cost_tier: CostTier::StandardAPI,
                        pricing: Some(TokenPricing::new(1.1, 4.4)),
                        capabilities: vec![Capability::Reasoning, Capability::Coding],
                        description:
                            "o4-mini — efficient o-series reasoning (succeeded by GPT-5 mini)"
                                .to_string(),
                        available: true,
                    },
                ],
            ); // end OpenAI
        } // end if has_key("openai")

        // Ollama Cloud
        if has_key("ollama-cloud") {
            providers.insert(
                "Ollama Cloud".to_string(),
                vec![
                    ModelInfo {
                        id: "ollama-cloud:gpt-oss:120b-cloud".to_string(),
                        name: "GPT OSS 120B Cloud".to_string(),
                        provider: "Ollama Cloud".to_string(),
                        context_input: 256000,
                        context_output: 32768,
                        cost_tier: CostTier::Free,
                        pricing: Some(TokenPricing::new(0.0, 0.0)),
                        capabilities: vec![Capability::Reasoning, Capability::Coding],
                        description: "Hosted Ollama model via ollama.com/api".to_string(),
                        available: true,
                    },
                    ModelInfo {
                        id: "ollama-cloud:qwen3-coder:480b-cloud".to_string(),
                        name: "Qwen3 Coder 480B Cloud".to_string(),
                        provider: "Ollama Cloud".to_string(),
                        context_input: 256000,
                        context_output: 32768,
                        cost_tier: CostTier::Free,
                        pricing: Some(TokenPricing::new(0.0, 0.0)),
                        capabilities: vec![Capability::Reasoning, Capability::Coding],
                        description: "Hosted Qwen coder model via Ollama Cloud".to_string(),
                        available: true,
                    },
                ],
            );
        } // end if has_key("ollama-cloud")

        // Groq
        if has_key("groq") {
            providers.insert(
                "Groq".to_string(),
                vec![ModelInfo {
                    id: "groq:llama-3.3-70b-versatile".to_string(),
                    name: "Llama 3.3 70B".to_string(),
                    provider: "Groq".to_string(),
                    context_input: 131072,
                    context_output: 8192,
                    cost_tier: CostTier::Free,
                    pricing: Some(TokenPricing::new(0.0, 0.0)),
                    capabilities: vec![
                        Capability::Fast,
                        Capability::Instruction,
                        Capability::Coding,
                    ],
                    description: "Llama 3.3 70B on Groq — fast inference, free tier".to_string(),
                    available: true,
                }],
            ); // end Groq
        } // end if has_key("groq")

        // xAI (Grok)
        if has_key("xai") {
            providers.insert(
                "xAI".to_string(),
                vec![
                    ModelInfo {
                        id: "xai:grok-4.20-0309-reasoning".to_string(),
                        name: "Grok 4.2".to_string(),
                        provider: "xAI".to_string(),
                        context_input: 2_000_000,
                        context_output: 32768,
                        cost_tier: CostTier::StandardAPI,
                        pricing: Some(TokenPricing::new(2.0, 6.0)),
                        capabilities: vec![
                            Capability::Reasoning,
                            Capability::Coding,
                            Capability::Vision,
                        ],
                        description: "Grok 4.2 — xAI flagship reasoning and coding model".to_string(),
                        available: true,
                    },
                    ModelInfo {
                        id: "xai:grok-4.20-0309-non-reasoning".to_string(),
                        name: "Grok 4.2 non reasoning".to_string(),
                        provider: "xAI".to_string(),
                        context_input: 2_000_000,
                        context_output: 32768,
                        cost_tier: CostTier::StandardAPI,
                        pricing: Some(TokenPricing::new(2.0, 6.0)),
                        capabilities: vec![
                            Capability::Coding,
                            Capability::Vision,
                        ],
                        description: "Grok 4.2 (Non-Reasoning) — xAI flagship non-reasoning and coding model".to_string(),
                        available: true,
                    },
                    ModelInfo {
                        id: "xai:grok-4-1-fast-reasoning".to_string(),
                        name: "Grok 4.1 Fast".to_string(),
                        provider: "xAI".to_string(),
                        context_input: 2_000_000,
                        context_output: 32768,
                        cost_tier: CostTier::CheapAPI,
                        pricing: Some(TokenPricing::new(0.2, 0.5)),
                        capabilities: vec![
                            Capability::Reasoning,
                            Capability::Coding,
                            Capability::Vision,
                        ],
                        description: "Grok 4.1 Fast — A frontier multimodal model optimized specifically for high-performance agentic tool calling.".to_string(),
                        available: true,
                    },
                    ModelInfo {
                        id: "xai:grok-4-1-fast-non-reasoning".to_string(),
                        name: "Grok 4.1 Fast (Non-Reasoning)".to_string(),
                        provider: "xAI".to_string(),
                        context_input: 2_000_000,
                        context_output: 32768,
                        cost_tier: CostTier::CheapAPI,
                        pricing: Some(TokenPricing::new(0.2, 0.5)),
                        capabilities: vec![
                            Capability::Coding,
                            Capability::Vision,
                        ],
                        description: "Grok 4.1 Fast (Non-Reasoning) — A frontier multimodal model optimized specifically for high-performance agentic tool calling.".to_string(),
                        available: true,
                    },
                    ModelInfo {
                        id: "xai:grok-4-0709".to_string(),
                        name: "Grok 4".to_string(),
                        provider: "xAI".to_string(),
                        context_input: 256000,
                        context_output: 32768,
                        cost_tier: CostTier::Premium,
                        pricing: Some(TokenPricing::new(3.0, 15.0)),
                        capabilities: vec![
                            Capability::Reasoning,
                            Capability::Coding,
                            Capability::Vision,
                        ],
                        description: "Grok 4 — xAI flagship reasoning and coding model".to_string(),
                        available: true,
                    },
                    ModelInfo {
                        id: "xai:grok-3".to_string(),
                        name: "Grok 3".to_string(),
                        provider: "xAI".to_string(),
                        context_input: 131072,
                        context_output: 16384,
                        cost_tier: CostTier::StandardAPI,
                        pricing: Some(TokenPricing::new(2.0, 10.0)),
                        capabilities: vec![Capability::Reasoning, Capability::Fast],
                        description: "Grok 3 — fast reasoning, 131k context".to_string(),
                        available: true,
                    },
                ],
            ); // end xAI
        } // end if has_key("xai")

        // Mistral
        if has_key("mistral") {
            providers.insert(
                "Mistral".to_string(),
                vec![
                    ModelInfo {
                        id: "mistral:mistral-large-latest".to_string(),
                        name: "Mistral Large 3".to_string(),
                        provider: "Mistral".to_string(),
                        context_input: 128000,
                        context_output: 16384,
                        cost_tier: CostTier::StandardAPI,
                        pricing: Some(TokenPricing::new(2.0, 6.0)),
                        capabilities: vec![Capability::Reasoning, Capability::Coding],
                        description: "Mistral Large 3 — 128k context, open-weight MoE flagship"
                            .to_string(),
                        available: true,
                    },
                    ModelInfo {
                        id: "mistral:mistral-small-latest".to_string(),
                        name: "Mistral Small".to_string(),
                        provider: "Mistral".to_string(),
                        context_input: 32000,
                        context_output: 8192,
                        cost_tier: CostTier::CheapAPI,
                        pricing: Some(TokenPricing::new(0.2, 0.6)),
                        capabilities: vec![Capability::Fast, Capability::Instruction],
                        description: "Mistral Small — lightweight, cost-effective".to_string(),
                        available: true,
                    },
                ],
            ); // end Mistral
        } // end if has_key("mistral")

        // OpenAI Codex (ChatGPT OAuth — /codex/responses endpoint)
        if has_key("openai-codex") {
            providers.insert("ChatGPT / Codex".to_string(), vec![
            ModelInfo {
                id: "openai-codex:gpt-5.4".to_string(),
                name: "GPT-5.4".to_string(),
                provider: "ChatGPT / Codex".to_string(),
                context_input: 1_000_000,
                context_output: 32_768,
                cost_tier: CostTier::Premium,
                pricing: Some(TokenPricing::new(2.5, 15.0)),
                capabilities: vec![Capability::Reasoning, Capability::Vision, Capability::Coding],
                description: "GPT-5.4 via ChatGPT/Codex OAuth — experimental consumer route, 1M context".to_string(),
                available: true,
            },
            ModelInfo {
                id: "openai-codex:gpt-5.4-mini".to_string(),
                name: "GPT-5.4 mini".to_string(),
                provider: "ChatGPT / Codex".to_string(),
                context_input: 1_000_000,
                context_output: 32_768,
                cost_tier: CostTier::CheapAPI,
                pricing: Some(TokenPricing::new(0.750, 4.500)),
                capabilities: vec![Capability::Coding, Capability::Fast],
                description: "GPT-5.4 mini via ChatGPT/Codex OAuth — experimental consumer route".to_string(),
                available: true,
            },
        ]); // end ChatGPT / Codex
        } // end if has_key("openai-codex")

        ModelCatalog { providers }
    }

    /// Alias for `cloud_only()` — kept for tests and non-interactive code paths.
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
                cost_tier: CostTier::CheapAPI,
                pricing: Some(TokenPricing::new(0.20, 0.20)),
                capabilities: vec![Capability::Reasoning, Capability::Coding],
                description: "Qwen reasoning model".to_string(),
                available: true,
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
                cost_tier: CostTier::StandardAPI,
                pricing: Some(TokenPricing::new(3.0, 15.0)),
                capabilities: vec![
                    Capability::Reasoning,
                    Capability::Coding,
                    Capability::Vision,
                ],
                description: "Claude Sonnet 4.6".to_string(),
                available: true,
            }],
        );
        ModelCatalog { providers }
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
    fn context_str_formats_correctly() {
        let model = ModelInfo {
            id: "test:model".to_string(),
            name: "Test".to_string(),
            provider: "Test".to_string(),
            context_input: 128000,
            context_output: 8192,
            cost_tier: CostTier::Local,
            pricing: Some(TokenPricing::new(0.0, 0.0)),
            capabilities: vec![],
            description: "test".to_string(),
            available: true,
        };
        assert_eq!(model.context_str(), "128k in / 8k out");
    }

    #[test]
    fn pricing_estimates_cost() {
        let pricing = TokenPricing::new(3.0, 15.0);
        let usd = pricing.estimate_cost_usd(100_000, 20_000);
        assert!((usd - 0.6).abs() < 0.000_001, "got {usd}");
    }

    #[test]
    fn find_by_id_returns_model() {
        let cat = fixture_catalog();
        let model = cat.find_by_id("anthropic:claude-sonnet-4-6");
        assert!(model.is_some());
    }

    #[test]
    fn pricing_for_model_is_not_auth_gated() {
        let pricing = ModelCatalog::pricing_for_model("openai:gpt-5.4");
        assert_eq!(pricing, Some(TokenPricing::new(2.5, 15.0)));
    }

    #[test]
    fn pricing_for_ollama_cloud_is_defined() {
        let pricing = ModelCatalog::pricing_for_model("ollama-cloud:gpt-oss:120b-cloud");
        assert_eq!(pricing, Some(TokenPricing::new(0.0, 0.0)));
    }
}
