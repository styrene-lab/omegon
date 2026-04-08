//! Usage advisory feature — exposes /usage from existing provider telemetry.
//!
//! Uses only the already-captured `ProviderTelemetrySnapshot` carried on
//! `BusEvent::TurnEnd`. No new upstream calls are made.

use async_trait::async_trait;
use omegon_traits::{
    BusEvent, BusRequest, CommandDefinition, CommandResult, Feature, ProviderTelemetrySnapshot,
};

use crate::usage::{authoritative_links, derive_headroom_state, derive_rationale, format_raw_telemetry_lines};

#[derive(Debug, Clone, Default)]
struct LatestUsageSnapshot {
    model: Option<String>,
    provider: Option<String>,
    telemetry: Option<ProviderTelemetrySnapshot>,
}

pub struct UsageFeature {
    latest: LatestUsageSnapshot,
}

impl UsageFeature {
    pub fn new() -> Self {
        Self {
            latest: LatestUsageSnapshot::default(),
        }
    }

    fn format_usage_report(&self) -> String {
        let provider = self.latest.provider.as_deref().unwrap_or("unknown");
        let model = self.latest.model.as_deref().unwrap_or("unknown");
        let telemetry = self.latest.telemetry.as_ref();
        let headroom = derive_headroom_state(telemetry);
        let rationale = derive_rationale(telemetry, &headroom);
        let authority = authoritative_links(provider);

        let mut lines = vec![
            "Usage".to_string(),
            String::new(),
            "Current route".to_string(),
            format!("- provider: {provider}"),
            format!("- model: {model}"),
            String::new(),
            "Raw upstream telemetry".to_string(),
        ];

        match telemetry {
            Some(t) => {
                let raw_lines = format_raw_telemetry_lines(t);
                if raw_lines.is_empty() {
                    lines.push("- none exposed in current session".to_string());
                } else {
                    lines.extend(raw_lines.into_iter().map(|line| format!("- {line}")));
                }
            }
            None => lines.push("- none captured yet in this session".to_string()),
        }

        lines.push(String::new());
        lines.push("Derived advisory".to_string());
        lines.push(format!("- headroom: {}", headroom.as_str()));
        lines.push(format!("- rationale: {rationale}"));

        if !authority.is_empty() {
            lines.push(String::new());
            lines.push("Authority".to_string());
            for link in authority {
                lines.push(format!("- {}: {}", link.label, link.url));
            }
        }

        lines.join("\n")
    }
}

#[async_trait]
impl Feature for UsageFeature {
    fn name(&self) -> &str {
        "usage"
    }

    fn commands(&self) -> Vec<CommandDefinition> {
        vec![CommandDefinition {
            name: "usage".into(),
            description: "Show current provider usage telemetry and advisory".into(),
            subcommands: vec![],
        }]
    }

    fn handle_command(&mut self, name: &str, _args: &str) -> CommandResult {
        if name != "usage" {
            return CommandResult::NotHandled;
        }
        CommandResult::Display(self.format_usage_report())
    }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        if let BusEvent::TurnEnd {
            model,
            provider,
            provider_telemetry,
            ..
        } = event
        {
            self.latest.model = model.clone();
            self.latest.provider = provider.clone();
            self.latest.telemetry = provider_telemetry.clone();
        }
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_command_formats_anthropic_with_authority_link() {
        let mut feature = UsageFeature::new();
        feature.on_event(&BusEvent::TurnEnd {
            turn: 1,
            model: Some("anthropic:claude-sonnet-4-6".into()),
            provider: Some("anthropic".into()),
            estimated_tokens: 0,
            context_window: 0,
            context_composition: Default::default(),
            actual_input_tokens: 0,
            actual_output_tokens: 0,
            cache_read_tokens: 0,
            provider_telemetry: Some(ProviderTelemetrySnapshot {
                provider: "anthropic".into(),
                source: "response_headers".into(),
                unified_5h_utilization_pct: Some(42.0),
                unified_7d_utilization_pct: Some(64.0),
                ..Default::default()
            }),
        });

        let CommandResult::Display(text) = feature.handle_command("usage", "") else {
            panic!("expected display result");
        };
        assert!(text.contains("5h utilization: 42%"), "got: {text}");
        assert!(text.contains("7d utilization: 64%"), "got: {text}");
        assert!(text.contains("headroom: healthy"), "got: {text}");
        assert!(
            text.contains("https://platform.claude.com/docs/en/api/rate-limits"),
            "got: {text}"
        );
    }

    #[test]
    fn usage_command_formats_codex_with_help_links() {
        let mut feature = UsageFeature::new();
        feature.on_event(&BusEvent::TurnEnd {
            turn: 1,
            model: Some("openai-codex:gpt-5.4".into()),
            provider: Some("openai-codex".into()),
            estimated_tokens: 0,
            context_window: 0,
            context_composition: Default::default(),
            actual_input_tokens: 0,
            actual_output_tokens: 0,
            cache_read_tokens: 0,
            provider_telemetry: Some(ProviderTelemetrySnapshot {
                provider: "openai-codex".into(),
                source: "response_headers".into(),
                codex_active_limit: Some("codex".into()),
                codex_primary_pct: Some(99),
                codex_primary_reset_secs: Some(13648),
                codex_limit_name: Some("GPT-5.3-Codex-Spark".into()),
                ..Default::default()
            }),
        });

        let CommandResult::Display(text) = feature.handle_command("usage", "") else {
            panic!("expected display result");
        };
        assert!(text.contains("primary utilization: 99%"), "got: {text}");
        assert!(text.contains("headroom: exhausted"), "got: {text}");
        assert!(text.contains("OpenAI help"), "got: {text}");
        assert!(text.contains("developers.openai.com/api/docs/guides/rate-limits"), "got: {text}");
    }

    #[test]
    fn usage_command_handles_missing_telemetry() {
        let feature = UsageFeature::new();
        let CommandResult::Display(text) = ({
            let mut feature = feature;
            feature.handle_command("usage", "")
        }) else {
            panic!("expected display result");
        };
        assert!(text.contains("none captured yet in this session"), "got: {text}");
        assert!(text.contains("headroom: unknown"), "got: {text}");
    }
}
