//! Shared footer/status semantic projection types.
//!
//! These structs describe operational telemetry without binding it to Ratatui
//! footer cards or the slim status line. TUI renderers consume this projection;
//! future protocol clients can derive their own DTOs from the same surface.

use super::footer::{FooterData, SessionUsageSlice};

#[derive(Debug, Clone, PartialEq)]
pub struct FooterProjection {
    pub engine: EngineProjection,
    pub context: ContextProjection,
    pub memory: MemoryProjection,
    pub session: SessionProjection,
    pub workspace: WorkspaceProjection,
    pub usage_slices: Vec<SessionUsageSlice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineProjection {
    pub model_id: String,
    pub model_provider: String,
    pub model_short: String,
    pub model_tier: String,
    pub thinking_level: String,
    pub posture: String,
    pub runtime_brand: String,
    pub principal_id: String,
    pub authorization: String,
    pub provider_connected: bool,
    pub update_available: Option<String>,
    pub sandbox: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextProjection {
    pub percent: f32,
    pub window: usize,
    pub class: String,
    pub actual_class: String,
    pub estimated_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryProjection {
    pub total_facts: usize,
    pub injected_facts: usize,
    pub working_memory: usize,
    pub memory_tokens_est: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionProjection {
    pub turn: u32,
    pub tool_calls: u32,
    pub compactions: u32,
    pub session_input_tokens: u64,
    pub session_output_tokens: u64,
    pub last_turn_input_tokens: u64,
    pub last_turn_output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceProjection {
    pub cwd: String,
    pub cwd_basename: String,
    pub git_branch: Option<String>,
    pub is_oauth: bool,
    pub persona: Option<String>,
}

pub trait ProjectFooterSurface {
    fn project_footer_surface(&self) -> FooterProjection;
}

impl ProjectFooterSurface for FooterData {
    fn project_footer_surface(&self) -> FooterProjection {
        FooterProjection {
            engine: EngineProjection {
                model_id: self.model_id.clone(),
                model_provider: self.model_provider.clone(),
                model_short: crate::settings::humanize_model_id(&self.model_id),
                model_tier: self.model_tier.clone(),
                thinking_level: self.thinking_level.clone(),
                posture: self.posture.clone(),
                runtime_brand: self.runtime_brand.clone(),
                principal_id: self.principal_id.clone(),
                authorization: self.authorization.clone(),
                provider_connected: self.provider_connected,
                update_available: self.update_available.clone(),
                sandbox: self.sandbox,
            },
            context: ContextProjection {
                percent: self.context_percent,
                window: self.context_window,
                class: format!("{:?}", self.context_class).to_ascii_lowercase(),
                actual_class: format!("{:?}", self.actual_context_class).to_ascii_lowercase(),
                estimated_tokens: self.estimated_tokens,
            },
            memory: MemoryProjection {
                total_facts: self.total_facts,
                injected_facts: self.injected_facts,
                working_memory: self.working_memory,
                memory_tokens_est: self.memory_tokens_est,
            },
            session: SessionProjection {
                turn: self.turn,
                tool_calls: self.tool_calls,
                compactions: self.compactions,
                session_input_tokens: self.session_input_tokens,
                session_output_tokens: self.session_output_tokens,
                last_turn_input_tokens: self.last_turn_input_tokens,
                last_turn_output_tokens: self.last_turn_output_tokens,
            },
            workspace: WorkspaceProjection {
                cwd: self.cwd.clone(),
                cwd_basename: std::path::Path::new(&self.cwd)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string(),
                git_branch: self.harness.git_branch.clone(),
                is_oauth: self.is_oauth,
                persona: self.harness.active_persona.as_ref().map(|p| p.name.clone()),
            },
            usage_slices: self.session_usage_slices.clone(),
        }
    }
}
