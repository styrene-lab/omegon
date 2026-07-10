//! Model budget — model intent + thinking level control.
//!
//! Provides two orthogonal levers for cost/capability tuning:
//! 1. Model intent: requested provider-neutral capability grade plus routing reason
//! 2. Thinking level: off → minimal → low → medium → high
//!
//! Tools: set_model_intent, set_thinking_level

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use omegon_traits::{
    CommandDefinition, CommandResult, ContentBlock, Feature, ToolDefinition, ToolResult,
};

use crate::settings::{SharedSettings, ThinkingLevel};

/// Provider-neutral model capability grades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelGrade {
    F,
    D,
    C,
    B,
    A,
    S,
}

impl ModelGrade {
    fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "f" => Some(Self::F),
            "d" => Some(Self::D),
            "c" => Some(Self::C),
            "b" => Some(Self::B),
            "a" => Some(Self::A),
            "s" => Some(Self::S),
            _ => None,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::F => "F",
            Self::D => "D",
            Self::C => "C",
            Self::B => "B",
            Self::A => "A",
            Self::S => "S",
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            Self::F | Self::D | Self::C => "💨",
            Self::B | Self::A => "↯",
            Self::S => "🧠",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::F => "Minimal capability — cheapest viable route",
            Self::D => "Fast, cheap — boilerplate and lookups",
            Self::C => "Fast routine assistance",
            Self::B => "Capable — routine coding and execution",
            Self::A => "Strong — complex implementation and review",
            Self::S => "Deep reasoning — architecture and complex debugging",
        }
    }

    /// Resolve grade to a concrete model ID from the model registry.
    /// If the registry has no entry for this grade+provider, falls back
    /// to the provider default.
    fn resolve_model(&self, provider: &str, _current_model: &str) -> String {
        let reg = crate::model_registry::ModelRegistry::global();
        reg.grade_model(self.as_str(), provider)
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

    async fn switch_grade(&self, grade: ModelGrade, reason: &str) -> anyhow::Result<String> {
        let (provider, current) = {
            let s = self.settings.lock().unwrap();
            (s.provider().to_string(), s.model_short().to_string())
        };
        let model = grade.resolve_model(&provider, &current);
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
            grade.icon(),
            grade.as_str(),
            grade.description(),
        ))
    }

    fn switch_grade_legacy(&self, grade: ModelGrade, reason: &str) -> String {
        let mut s = self.settings.lock().unwrap();
        let provider = s.provider().to_string();
        let current = s.model_short().to_string();
        let model = grade.resolve_model(&provider, &current);
        let target = format!("{provider}:{model}");
        s.set_model(&target);
        s.provider_connected = crate::auth::provider_connected_for_model(&target);
        drop(s);
        format!(
            "{} {} → {target} ({})\n{reason}",
            grade.icon(),
            grade.as_str(),
            grade.description(),
        )
    }

    async fn switch_offline(&self, reason: &str) -> anyhow::Result<String> {
        let current = self.settings.lock().unwrap().model_short().to_string();
        let model = if self.current_provider() == "ollama" && current != "local" {
            current
        } else {
            "qwen3:30b".to_string()
        };
        let target = format!("ollama:{model}");
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
            "🤖 provider local → {target} (On-device model via Ollama)
{reason}"
        ))
    }

    fn switch_offline_legacy(&self, reason: &str) -> String {
        let current = self.settings.lock().unwrap().model_short().to_string();
        let model = if self.current_provider() == "ollama" && current != "local" {
            current
        } else {
            "qwen3:30b".to_string()
        };
        let target = format!("ollama:{model}");
        {
            let mut s = self.settings.lock().unwrap();
            s.set_model(&target);
            s.provider_connected = crate::auth::provider_connected_for_model(&target);
        }
        format!(
            "🤖 provider local → {target} (On-device model via Ollama)
{reason}"
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
                name: crate::tool_registry::model_budget::SET_MODEL_INTENT.into(),
                label: "set_model_intent".into(),
                description: "Switch the active model intent by provider-neutral grade. Use D/C for simple work, B/A for routine coding, S for deep reasoning. Use provider=local to request local endpoints; local is not a grade.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "grade": {
                            "type": "string",
                            "enum": ["F", "D", "C", "B", "A", "S"],
                            "description": "Target provider-neutral model capability grade; local is not a grade"
                        },
                        "provider": {
                            "type": "string",
                            "description": "Optional provider selector such as auto, local, upstream, or an endpoint/provider id"
                        },
                        "reason": {
                            "type": "string",
                            "description": "Brief explanation for the grade change"
                        }
                    },
                    "required": ["grade", "reason"]
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
            crate::tool_registry::model_budget::SET_MODEL_INTENT => {
                let grade_str = args["grade"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("grade required"))?;
                let reason = args["reason"].as_str().unwrap_or("No reason given");
                let provider = args["provider"].as_str().unwrap_or("auto");
                let grade = ModelGrade::parse(grade_str)
                    .ok_or_else(|| anyhow::anyhow!("Invalid grade: {grade_str}; expected F, D, C, B, A, or S. Use provider=local for local endpoints."))?;
                let msg = self.switch_grade(grade, reason).await?;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!("{msg}\nIntent: grade {grade_str}, provider {provider}"),
                    }],
                    details: json!({"grade": grade_str, "provider": provider, "model": grade.resolve_model(&self.current_provider(), "")}),
                })
            }
            crate::tool_registry::model_budget::SWITCH_TO_OFFLINE_DRIVER => {
                let reason = args["reason"]
                    .as_str()
                    .unwrap_or("User requested offline mode");
                let preferred = args["preferred_model"].as_str();
                let model = preferred.unwrap_or("auto");
                let msg = self.switch_offline(reason).await?;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!(
                            "{msg}\nModel preference: {model}. Local inference via Ollama."
                        ),
                    }],
                    details: json!({"provider": "local", "preferred_model": model, "reason": reason}),
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
        vec![]
    }

    fn handle_command(&mut self, _name: &str, _args: &str) -> CommandResult {
        CommandResult::NotHandled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grade_parse() {
        assert_eq!(ModelGrade::parse("S"), Some(ModelGrade::S));
        assert_eq!(ModelGrade::parse("B"), Some(ModelGrade::B));
        assert_eq!(ModelGrade::parse("D"), Some(ModelGrade::D));
        assert_eq!(ModelGrade::parse("invalid"), None);
    }

    #[test]
    fn grade_parse_rejects_local() {
        assert_eq!(ModelGrade::parse("S"), Some(ModelGrade::S));
        assert_eq!(ModelGrade::parse("B"), Some(ModelGrade::B));
        assert_eq!(ModelGrade::parse("D"), Some(ModelGrade::D));
        assert_eq!(ModelGrade::parse("local"), None);
        assert_eq!(ModelGrade::parse("victory"), None);
    }

    #[test]
    fn grade_resolve_anthropic() {
        assert_eq!(
            ModelGrade::S.resolve_model("anthropic", ""),
            "claude-fable-5"
        );
        assert!(
            ModelGrade::B
                .resolve_model("anthropic", "")
                .contains("sonnet")
        );
        assert!(
            ModelGrade::D
                .resolve_model("anthropic", "")
                .contains("haiku")
        );
    }

    #[test]
    fn grade_resolve_openai() {
        assert_eq!(ModelGrade::S.resolve_model("openai", ""), "gpt-5.6");
        assert!(ModelGrade::B.resolve_model("openai", "").contains("gpt"));
    }

    #[test]
    fn grade_resolve_openai_codex() {
        assert_eq!(ModelGrade::S.resolve_model("openai-codex", ""), "gpt-5.6");
        assert_eq!(
            ModelGrade::B.resolve_model("openai-codex", ""),
            "gpt-5.6-terra"
        );
        assert_eq!(
            ModelGrade::D.resolve_model("openai-codex", ""),
            "gpt-5.6-luna"
        );
    }

    #[test]
    fn switch_grade_updates_settings() {
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-6");
        let budget = ModelBudget::new(settings.clone());
        let msg = budget.switch_grade_legacy(ModelGrade::S, "test");
        assert!(msg.contains("S"), "should mention grade: {msg}");
        assert_eq!(
            settings.lock().unwrap().model,
            "anthropic:claude-fable-5",
            "should switch to highest-grade Anthropic model"
        );
    }

    #[test]
    fn switch_local_tier_moves_to_ollama_instead_of_anthropic() {
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-6");
        let budget = ModelBudget::new(settings.clone());
        let msg = budget.switch_offline_legacy("offline please");
        assert!(
            msg.contains("ollama:qwen3:30b"),
            "unexpected message: {msg}"
        );
        assert_eq!(settings.lock().unwrap().model, "ollama:qwen3:30b");
    }

    #[test]
    fn resolve_preserves_current_grade_default() {
        // If already on the B-grade Anthropic default, switching to B should keep it.
        let model = ModelGrade::B.resolve_model("anthropic", "claude-sonnet-5");
        assert_eq!(model, "claude-sonnet-5", "should preserve exact default");

        // If on an older B-grade Anthropic model, switching to B should move to the current default.
        let model = ModelGrade::B.resolve_model("anthropic", "claude-sonnet-4-6");
        assert_eq!(model, "claude-sonnet-5");

        // If on a different grade band, should switch to the S-grade default.
        let model = ModelGrade::S.resolve_model("anthropic", "claude-sonnet-4-6");
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
    fn legacy_tier_commands_are_not_handled() {
        let settings = crate::settings::shared("test");
        let mut budget = ModelBudget::new(settings.clone());

        for command in [
            "gloriana",
            "victory",
            "retribution",
            "opus",
            "sonnet",
            "haiku",
        ] {
            let result = budget.handle_command(command, "");
            assert!(
                matches!(result, CommandResult::NotHandled),
                "legacy command /{command} should not be handled: {result:?}"
            );
        }
    }
}
