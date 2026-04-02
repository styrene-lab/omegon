//! Model budget — tier routing + thinking level control.
//!
//! Provides two orthogonal levers for cost/capability tuning:
//! 1. Model tier: gloriana (deep) → victory (capable) → retribution (fast)
//! 2. Thinking level: off → minimal → low → medium → high
//!
//! Tools: set_model_tier, set_thinking_level
//! Commands: /gloriana, /victory, /retribution, /haiku, /sonnet, /opus

use async_trait::async_trait;
use serde_json::{Value, json};

use omegon_traits::{
    CommandDefinition, CommandResult, ContentBlock, Feature, ToolDefinition, ToolResult,
};

use crate::settings::{SharedSettings, ThinkingLevel};

/// Tier definitions with resolution priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    Local,
    Retribution,
    Victory,
    Gloriana,
}

impl ModelTier {
    fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "local" => Some(Self::Local),
            "retribution" => Some(Self::Retribution),
            "victory" => Some(Self::Victory),
            "gloriana" => Some(Self::Gloriana),
            _ => None,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Retribution => "retribution",
            Self::Victory => "victory",
            Self::Gloriana => "gloriana",
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            Self::Local => "🤖",
            Self::Retribution => "💨",
            Self::Victory => "⚡",
            Self::Gloriana => "🧠",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::Local => "On-device model via Ollama",
            Self::Retribution => "Fast, cheap — boilerplate and lookups",
            Self::Victory => "Capable — routine coding and execution",
            Self::Gloriana => "Deep reasoning — architecture and complex debugging",
        }
    }

    /// Model ID prefix for this tier — used for prefix-matching against
    /// the current model to detect which tier is active, and for resolving
    /// a tier to a concrete model ID.
    fn prefix(&self, provider: &str) -> &'static str {
        match (self, provider) {
            (Self::Gloriana, "anthropic") => "claude-opus",
            (Self::Victory, "anthropic") => "claude-sonnet",
            (Self::Retribution, "anthropic") => "claude-haiku",
            (Self::Gloriana, "openai") => "gpt-5.4",
            (Self::Victory, "openai") => "gpt-5",
            (Self::Retribution, "openai") => "gpt-5-mini",
            (Self::Gloriana, "openai-codex") => "gpt-5",
            (Self::Victory, "openai-codex") => "gpt-5",
            (Self::Retribution, "openai-codex") => "codex-mini",
            (Self::Local, _) => "local",
            (Self::Gloriana, _) => "claude-opus",
            (Self::Victory, _) => "claude-sonnet",
            (Self::Retribution, _) => "claude-haiku",
        }
    }

    /// Resolve tier to a concrete model ID.
    /// If the current model already matches this tier's prefix, keep it
    /// (preserves the exact version). Otherwise use the short alias.
    ///
    /// The short alias format (e.g., `claude-sonnet-4-6`) is a stable
    /// pointer that Anthropic/OpenAI resolve to the latest version. This
    /// avoids hardcoding dated model IDs that rot when new versions ship.
    fn resolve_model(&self, provider: &str, current_model: &str) -> String {
        let prefix = self.prefix(provider);
        // If the current model already matches the target tier's prefix, keep it
        if current_model.starts_with(prefix) {
            return current_model.to_string();
        }
        // Use short alias — these are stable pointers, not dated versions
        match (self, provider) {
            (Self::Gloriana, "anthropic") => "claude-opus-4-6",
            (Self::Victory, "anthropic") => "claude-sonnet-4-6",
            (Self::Retribution, "anthropic") => "claude-haiku-4-5-20251001",
            (Self::Gloriana, "openai") => "gpt-5.4",
            (Self::Victory, "openai") => "gpt-5",
            (Self::Retribution, "openai") => "gpt-5-mini",
            (Self::Gloriana, "openai-codex") => "gpt-5.4",
            (Self::Victory, "openai-codex") => "gpt-5.4",
            (Self::Retribution, "openai-codex") => "gpt-5.4-mini",
            (Self::Local, _) => "local",
            _ => "claude-sonnet-4-6",
        }
        .to_string()
    }
}

pub struct ModelBudget {
    settings: SharedSettings,
}

impl ModelBudget {
    pub fn new(settings: SharedSettings) -> Self {
        Self { settings }
    }

    fn current_provider(&self) -> String {
        self.settings.lock().unwrap().provider().to_string()
    }

    fn switch_tier(&self, tier: ModelTier, reason: &str) -> String {
        let mut s = self.settings.lock().unwrap();
        let provider = if matches!(tier, ModelTier::Local) {
            "ollama".to_string()
        } else {
            s.provider().to_string()
        };
        let current = s.model_short().to_string();
        let model = if matches!(tier, ModelTier::Local) {
            if s.provider() == "ollama" && current != "local" {
                current
            } else {
                "qwen3:30b".to_string()
            }
        } else {
            tier.resolve_model(&provider, &current)
        };
        s.model = format!("{provider}:{model}");
        s.context_window = crate::settings::Settings::new(&s.model).context_window;
        drop(s);
        format!(
            "{} {} → {provider}:{model} ({})\n{reason}",
            tier.icon(),
            tier.as_str(),
            tier.description(),
        )
    }

    fn switch_thinking(&self, level: ThinkingLevel, reason: &str) -> String {
        self.settings.lock().unwrap().thinking = level;
        format!(
            "{} Thinking → {} ({})",
            level.icon(),
            level.as_str(),
            reason
        )
    }
}

#[async_trait]
impl Feature for ModelBudget {
    fn name(&self) -> &str {
        "model-budget"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: crate::tool_registry::model_budget::SET_MODEL_TIER.into(),
                label: "set_model_tier".into(),
                description: "Switch the active model tier. Use 'retribution' for simple tasks, 'victory' for routine coding, 'gloriana' for deep reasoning.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "tier": {
                            "type": "string",
                            "enum": ["local", "retribution", "victory", "gloriana"],
                            "description": "Target model tier"
                        },
                        "reason": {
                            "type": "string",
                            "description": "Brief explanation for the tier change"
                        }
                    },
                    "required": ["tier", "reason"]
                }),
            },
            ToolDefinition {
                name: crate::tool_registry::model_budget::SWITCH_TO_OFFLINE_DRIVER.into(),
                label: "switch_to_offline_driver".into(),
                description: "Switch from cloud to a local offline model (Ollama). Use when detecting connectivity issues, API errors, or when offline mode is requested.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "reason": {
                            "type": "string",
                            "description": "Why switching to offline mode"
                        },
                        "preferred_model": {
                            "type": "string",
                            "description": "Optional specific model ID. Omit to auto-select."
                        }
                    },
                    "required": ["reason"]
                }),
            },
            ToolDefinition {
                name: crate::tool_registry::model_budget::SET_THINKING_LEVEL.into(),
                label: "set_thinking_level".into(),
                description: "Adjust the extended thinking budget. Higher = more reasoning tokens, slower. Use 'high' for complex problems, 'low' for speed.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "level": {
                            "type": "string",
                            "enum": ["off", "minimal", "low", "medium", "high"],
                            "description": "Thinking level"
                        },
                        "reason": {
                            "type": "string",
                            "description": "Brief explanation for the thinking level change"
                        }
                    },
                    "required": ["level", "reason"]
                }),
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        match tool_name {
            crate::tool_registry::model_budget::SET_MODEL_TIER => {
                let tier_str = args["tier"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("tier required"))?;
                let reason = args["reason"].as_str().unwrap_or("No reason given");
                let tier = ModelTier::parse(tier_str)
                    .ok_or_else(|| anyhow::anyhow!("Invalid tier: {tier_str}"))?;
                let msg = self.switch_tier(tier, reason);
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: msg }],
                    details: json!({"tier": tier_str, "model": tier.resolve_model(&self.current_provider(), "")}),
                })
            }
            crate::tool_registry::model_budget::SWITCH_TO_OFFLINE_DRIVER => {
                let reason = args["reason"]
                    .as_str()
                    .unwrap_or("User requested offline mode");
                let preferred = args["preferred_model"].as_str();
                let model = preferred.unwrap_or("auto");
                let msg = self.switch_tier(ModelTier::Local, reason);
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!(
                            "{msg}\nModel preference: {model}. Local inference via Ollama."
                        ),
                    }],
                    details: json!({"tier": "local", "preferred_model": model, "reason": reason}),
                })
            }
            crate::tool_registry::model_budget::SET_THINKING_LEVEL => {
                let level_str = args["level"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("level required"))?;
                let reason = args["reason"].as_str().unwrap_or("No reason given");
                let level = ThinkingLevel::parse(level_str)
                    .ok_or_else(|| anyhow::anyhow!("Invalid level: {level_str}"))?;
                let msg = self.switch_thinking(level, reason);
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: msg }],
                    details: json!({"level": level_str}),
                })
            }
            _ => anyhow::bail!("Unknown tool: {tool_name}"),
        }
    }

    fn commands(&self) -> Vec<CommandDefinition> {
        vec![
            CommandDefinition {
                name: "gloriana".into(),
                description: "Switch to gloriana tier (deep reasoning)".into(),
                subcommands: vec![],
            },
            CommandDefinition {
                name: "victory".into(),
                description: "Switch to victory tier (capable coding)".into(),
                subcommands: vec![],
            },
            CommandDefinition {
                name: "retribution".into(),
                description: "Switch to retribution tier (fast/cheap)".into(),
                subcommands: vec![],
            },
            // Aliases for familiarity
            CommandDefinition {
                name: "opus".into(),
                description: "Switch to gloriana/opus tier".into(),
                subcommands: vec![],
            },
            CommandDefinition {
                name: "sonnet".into(),
                description: "Switch to victory/sonnet tier".into(),
                subcommands: vec![],
            },
            CommandDefinition {
                name: "haiku".into(),
                description: "Switch to retribution/haiku tier".into(),
                subcommands: vec![],
            },
        ]
    }

    fn handle_command(&mut self, name: &str, _args: &str) -> CommandResult {
        let tier = match name {
            "gloriana" | "opus" => ModelTier::Gloriana,
            "victory" | "sonnet" => ModelTier::Victory,
            "retribution" | "haiku" => ModelTier::Retribution,
            _ => return CommandResult::NotHandled,
        };
        let msg = self.switch_tier(tier, &format!("/{name} command"));
        CommandResult::Display(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_parse() {
        assert_eq!(ModelTier::parse("gloriana"), Some(ModelTier::Gloriana));
        assert_eq!(ModelTier::parse("victory"), Some(ModelTier::Victory));
        assert_eq!(
            ModelTier::parse("retribution"),
            Some(ModelTier::Retribution)
        );
        assert_eq!(ModelTier::parse("local"), Some(ModelTier::Local));
        assert_eq!(ModelTier::parse("GLORIANA"), Some(ModelTier::Gloriana));
        assert_eq!(ModelTier::parse("invalid"), None);
    }

    #[test]
    fn tier_resolve_anthropic() {
        assert!(
            ModelTier::Gloriana
                .resolve_model("anthropic", "")
                .contains("opus")
        );
        assert!(
            ModelTier::Victory
                .resolve_model("anthropic", "")
                .contains("sonnet")
        );
        assert!(
            ModelTier::Retribution
                .resolve_model("anthropic", "")
                .contains("haiku")
        );
    }

    #[test]
    fn tier_resolve_openai() {
        assert_eq!(ModelTier::Gloriana.resolve_model("openai", ""), "gpt-5.4");
        assert!(
            ModelTier::Victory
                .resolve_model("openai", "")
                .contains("gpt")
        );
    }

    #[test]
    fn tier_resolve_openai_codex() {
        assert_eq!(
            ModelTier::Victory.resolve_model("openai-codex", ""),
            "gpt-5.4"
        );
        assert_eq!(
            ModelTier::Retribution.resolve_model("openai-codex", ""),
            "gpt-5.4-mini"
        );
    }

    #[test]
    fn switch_tier_updates_settings() {
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-6");
        let budget = ModelBudget::new(settings.clone());
        let msg = budget.switch_tier(ModelTier::Gloriana, "test");
        assert!(msg.contains("gloriana"), "should mention tier: {msg}");
        assert!(
            settings.lock().unwrap().model.contains("opus"),
            "should switch to opus"
        );
    }

    #[test]
    fn switch_local_tier_moves_to_ollama_instead_of_anthropic() {
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-6");
        let budget = ModelBudget::new(settings.clone());
        let msg = budget.switch_tier(ModelTier::Local, "offline please");
        assert!(
            msg.contains("ollama:qwen3:30b"),
            "unexpected message: {msg}"
        );
        assert_eq!(settings.lock().unwrap().model, "ollama:qwen3:30b");
    }

    #[test]
    fn resolve_preserves_current_model_version() {
        // If already on a sonnet variant, switching to victory should keep it
        let model = ModelTier::Victory.resolve_model("anthropic", "claude-sonnet-4-6");
        assert_eq!(model, "claude-sonnet-4-6", "should preserve exact version");

        // If on a different tier, should switch to default
        let model = ModelTier::Gloriana.resolve_model("anthropic", "claude-sonnet-4-6");
        assert!(model.contains("opus"), "should switch to opus: {model}");
    }

    #[test]
    fn switch_thinking_updates_settings() {
        let settings = crate::settings::shared("test");
        let budget = ModelBudget::new(settings.clone());
        let msg = budget.switch_thinking(ThinkingLevel::High, "complex task");
        assert!(msg.contains("high"));
        assert_eq!(settings.lock().unwrap().thinking, ThinkingLevel::High);
    }

    #[test]
    fn command_aliases() {
        let settings = crate::settings::shared("test");
        let mut budget = ModelBudget::new(settings.clone());

        let result = budget.handle_command("opus", "");
        assert!(matches!(result, CommandResult::Display(ref s) if s.contains("gloriana")));

        let result = budget.handle_command("sonnet", "");
        assert!(matches!(result, CommandResult::Display(ref s) if s.contains("victory")));

        let result = budget.handle_command("haiku", "");
        assert!(matches!(result, CommandResult::Display(ref s) if s.contains("retribution")));
    }
}
