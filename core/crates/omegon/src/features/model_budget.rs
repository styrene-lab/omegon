//! Model budget — tier routing + thinking level control.
//!
//! Provides two orthogonal levers for cost/capability tuning:
//! 1. Model tier: gloriana (deep) → victory (capable) → retribution (fast)
//! 2. Thinking level: off → minimal → low → medium → high
//!
//! Tools: set_model_tier, set_thinking_level
//! Commands: /gloriana, /victory, /retribution, /haiku, /sonnet, /opus

use std::sync::Arc;

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
            Self::Victory => "↯",
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

    /// Resolve tier to a concrete model ID from the model registry.
    /// If the registry has no entry for this tier+provider, falls back
    /// to the provider default.
    fn resolve_model(&self, provider: &str, _current_model: &str) -> String {
        if matches!(self, Self::Local) {
            return "local".to_string();
        }
        let tier_name = match self {
            Self::Gloriana => "gloriana",
            Self::Victory => "victory",
            Self::Retribution => "retribution",
            Self::Local => unreachable!(),
        };
        let reg = crate::model_registry::ModelRegistry::global();
        reg.tier_model(tier_name, provider)
            .or_else(|| reg.default_model(provider))
            .unwrap_or("claude-sonnet-4-6")
            .to_string()
    }
}

pub struct ModelBudget {
    settings: SharedSettings,
    route_controller: Option<Arc<crate::route::RouteController>>,
}

impl ModelBudget {
    pub fn new(settings: SharedSettings) -> Self {
        Self {
            settings,
            route_controller: None,
        }
    }

    pub fn with_route_controller(
        settings: SharedSettings,
        route_controller: Arc<crate::route::RouteController>,
    ) -> Self {
        Self {
            settings,
            route_controller: Some(route_controller),
        }
    }

    fn current_provider(&self) -> String {
        self.settings.lock().unwrap().provider().to_string()
    }

    async fn switch_tier(&self, tier: ModelTier, reason: &str) -> anyhow::Result<String> {
        let (provider, current) = {
            let s = self.settings.lock().unwrap();
            let provider = if matches!(tier, ModelTier::Local) {
                "ollama".to_string()
            } else {
                s.provider().to_string()
            };
            (provider, s.model_short().to_string())
        };
        let model = if matches!(tier, ModelTier::Local) {
            if provider == "ollama" && current != "local" {
                current
            } else {
                "qwen3:30b".to_string()
            }
        } else {
            tier.resolve_model(&provider, &current)
        };
        let target = format!("{provider}:{model}");
        if let Some(controller) = self.route_controller.as_ref() {
            let bridge = crate::providers::auto_detect_bridge(&target).await;
            let snapshot = controller
                .switch_model(target.clone(), &crate::route::CredentialLedger, bridge)
                .await?;
            if snapshot.serving_model() != Some(target.as_str()) {
                anyhow::bail!(snapshot.operator_status());
            }
        }
        {
            let mut s = self.settings.lock().unwrap();
            s.set_model(&target);
            s.provider_connected = crate::auth::provider_connected_for_model(&target);
        }
        Ok(format!(
            "{} {} → {target} ({})\n{reason}",
            tier.icon(),
            tier.as_str(),
            tier.description(),
        ))
    }

    fn switch_tier_legacy(&self, tier: ModelTier, reason: &str) -> String {
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
        let target = format!("{provider}:{model}");
        s.set_model(&target);
        s.provider_connected = crate::auth::provider_connected_for_model(&target);
        drop(s);
        format!(
            "{} {} → {target} ({})\n{reason}",
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
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
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
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
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
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
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
                let msg = self.switch_tier(tier, reason).await?;
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
                let msg = self.switch_tier(ModelTier::Local, reason).await?;
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
                availability: omegon_traits::CommandAvailability::ALL,
                safety: omegon_traits::CommandSafety::STATE_CHANGING,
            },
            CommandDefinition {
                name: "victory".into(),
                description: "Switch to victory tier (capable coding)".into(),
                subcommands: vec![],
                availability: omegon_traits::CommandAvailability::ALL,
                safety: omegon_traits::CommandSafety::STATE_CHANGING,
            },
            CommandDefinition {
                name: "retribution".into(),
                description: "Switch to retribution tier (fast/cheap)".into(),
                subcommands: vec![],
                availability: omegon_traits::CommandAvailability::ALL,
                safety: omegon_traits::CommandSafety::STATE_CHANGING,
            },
            // Aliases for familiarity
            CommandDefinition {
                name: "opus".into(),
                description: "Switch to gloriana/opus tier".into(),
                subcommands: vec![],
                availability: omegon_traits::CommandAvailability::ALL,
                safety: omegon_traits::CommandSafety::STATE_CHANGING,
            },
            CommandDefinition {
                name: "sonnet".into(),
                description: "Switch to victory/sonnet tier".into(),
                subcommands: vec![],
                availability: omegon_traits::CommandAvailability::ALL,
                safety: omegon_traits::CommandSafety::STATE_CHANGING,
            },
            CommandDefinition {
                name: "haiku".into(),
                description: "Switch to retribution/haiku tier".into(),
                subcommands: vec![],
                availability: omegon_traits::CommandAvailability::ALL,
                safety: omegon_traits::CommandSafety::STATE_CHANGING,
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
        let msg = self.switch_tier_legacy(tier, &format!("/{name} command"));
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
        assert_eq!(
            ModelTier::Gloriana.resolve_model("anthropic", ""),
            "claude-fable-5"
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
        assert_eq!(ModelTier::Gloriana.resolve_model("openai", ""), "gpt-5.5");
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
        let msg = budget.switch_tier_legacy(ModelTier::Gloriana, "test");
        assert!(msg.contains("gloriana"), "should mention tier: {msg}");
        assert_eq!(
            settings.lock().unwrap().model,
            "anthropic:claude-fable-5",
            "should switch to highest-tier Anthropic model"
        );
    }

    #[test]
    fn switch_local_tier_moves_to_ollama_instead_of_anthropic() {
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-6");
        let budget = ModelBudget::new(settings.clone());
        let msg = budget.switch_tier_legacy(ModelTier::Local, "offline please");
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

        // If on a different tier, should switch to highest-tier default.
        let model = ModelTier::Gloriana.resolve_model("anthropic", "claude-sonnet-4-6");
        assert_eq!(model, "claude-fable-5");
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
