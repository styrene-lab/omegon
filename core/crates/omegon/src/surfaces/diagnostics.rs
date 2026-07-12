//! Semantic projections for operator-facing `/status` and `/stats` diagnostics.
//!
//! Projection builders own fact selection and unknown-vs-zero semantics. Surface
//! adapters may format these values differently, but must not probe runtime state.

use serde::{Deserialize, Serialize};

use crate::status::HarnessStatus;

pub const DIAGNOSTIC_PROJECTION_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessStatusProjection {
    pub version: u16,
    pub harness: HarnessStatus,
    pub runtime_generation: u64,
    pub session_id: String,
    pub instance_id: String,
    pub automation_level: String,
    pub automation_summary: String,
}

impl HarnessStatusProjection {
    pub fn new(
        harness: HarnessStatus,
        runtime_generation: u64,
        session_id: impl Into<String>,
        instance_id: impl Into<String>,
        automation_level: impl Into<String>,
        automation_summary: impl Into<String>,
    ) -> Self {
        Self {
            version: DIAGNOSTIC_PROJECTION_VERSION,
            harness,
            runtime_generation,
            session_id: session_id.into(),
            instance_id: instance_id.into(),
            automation_level: automation_level.into(),
            automation_summary: automation_summary.into(),
        }
    }

    pub fn render_markdown(&self) -> String {
        format!(
            "{}\nRuntime\n  Generation:   {}\n  Session:      {}\n  Instance:     {}\nAutomation\n  Level:        {} ({})",
            crate::tui::bootstrap::render_bootstrap(&self.harness, false),
            self.runtime_generation,
            self.session_id,
            self.instance_id,
            self.automation_level,
            self.automation_summary,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionStatsProjection {
    pub version: u16,
    pub turns: u32,
    /// `None` means this surface has no authoritative tool-call observation.
    pub tool_calls: Option<u32>,
    pub model: String,
    pub thinking: String,
    pub posture: String,
    pub estimated_context_tokens: usize,
    pub context_window: usize,
    pub max_turns: u32,
    pub persona: Option<String>,
    pub tone: Option<String>,
    pub authenticated_providers: Option<usize>,
    pub provider_count: Option<usize>,
    pub mcp_servers: Option<usize>,
    pub memory_available: Option<bool>,
    pub cleave_available: Option<bool>,
}

impl SessionStatsProjection {
    pub fn context_usage_percent(&self) -> Option<f64> {
        (self.context_window > 0)
            .then(|| (self.estimated_context_tokens as f64 / self.context_window as f64) * 100.0)
    }

    pub fn render_markdown(&self) -> String {
        let tool_calls = self
            .tool_calls
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let context = self
            .context_usage_percent()
            .map(|percent| {
                format!(
                    "{} tokens ({percent:.0}% of {})",
                    self.estimated_context_tokens, self.context_window
                )
            })
            .unwrap_or_else(|| {
                format!("{} tokens (window unknown)", self.estimated_context_tokens)
            });

        let mut output = format!(
            "Session Overview\n\nActivity\n  Turns:            {}\n  Tool calls:       {}\n  Model:            {}\n  Thinking:         {}\n  Posture:          {}\n\nContext\n  Usage:            {}\n  Max turns:        {}",
            self.turns,
            tool_calls,
            self.model,
            self.thinking,
            self.posture,
            context,
            self.max_turns,
        );

        if self.persona.is_some()
            || self.tone.is_some()
            || self.provider_count.is_some()
            || self.mcp_servers.is_some()
        {
            output.push_str("\n\nHarness");
            if let Some(persona) = &self.persona {
                output.push_str(&format!("\n  Persona:          {persona}"));
            }
            if let Some(tone) = &self.tone {
                output.push_str(&format!("\n  Tone:             {tone}"));
            }
            if let (Some(authenticated), Some(total)) =
                (self.authenticated_providers, self.provider_count)
            {
                output.push_str(&format!(
                    "\n  Providers:        {authenticated}/{total} authenticated"
                ));
            }
            if let Some(servers) = self.mcp_servers {
                output.push_str(&format!("\n  MCP servers:      {servers}"));
            }
        }

        if self.memory_available.is_some() || self.cleave_available.is_some() {
            output.push_str("\n\nCapabilities");
            if let Some(available) = self.memory_available {
                output.push_str(&format!(
                    "\n  Memory:           {}",
                    if available {
                        "available"
                    } else {
                        "UNAVAILABLE"
                    }
                ));
            }
            if let Some(available) = self.cleave_available {
                output.push_str(&format!(
                    "\n  Cleave:           {}",
                    if available {
                        "available"
                    } else {
                        "UNAVAILABLE"
                    }
                ));
            }
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_tool_telemetry_is_not_rendered_as_zero() {
        let projection = SessionStatsProjection {
            version: DIAGNOSTIC_PROJECTION_VERSION,
            turns: 2,
            tool_calls: None,
            model: "test:model".into(),
            thinking: "minimal".into(),
            posture: "architect".into(),
            estimated_context_tokens: 12,
            context_window: 0,
            max_turns: 20,
            persona: None,
            tone: None,
            authenticated_providers: None,
            provider_count: None,
            mcp_servers: None,
            memory_available: None,
            cleave_available: None,
        };

        let rendered = projection.render_markdown();
        assert!(rendered.contains("Tool calls:       unknown"));
        assert!(rendered.contains("12 tokens (window unknown)"));
        assert!(!rendered.contains("NaN"));
        assert!(!rendered.contains("inf"));
    }

    #[test]
    fn projection_serialization_contains_no_secret_values() {
        let projection = HarnessStatusProjection::new(
            HarnessStatus::default(),
            1,
            "session",
            "instance",
            "guarded",
            "confirm mutations",
        );
        let json = serde_json::to_string(&projection).unwrap();
        assert!(!json.contains("secret_value"));
        assert!(!json.contains("api_key"));
    }
}
