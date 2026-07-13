//! TUI backing-state adapter for the shared footer/status surface projection.

use super::footer::FooterData;
use crate::settings::ContextClass;
use crate::surfaces::footer::{
    ContextProjection, EngineProjection, FooterProjection, MemoryProjection, ProjectFooterSurface,
    SessionProjection, UsageSliceProjection, WorkspaceProjection,
};

fn project_context_class(class: ContextClass) -> String {
    class.short().to_ascii_lowercase()
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
                web_search_providers: self
                    .web_search_providers
                    .iter()
                    .map(|provider| (provider.provider.to_string(), provider.configured))
                    .collect(),
            },
            context: ContextProjection {
                percent: self.context_percent,
                window: self.context_window,
                class: project_context_class(self.context_class),
                actual_class: project_context_class(self.actual_context_class),
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
            usage_slices: self
                .session_usage_slices
                .iter()
                .map(|slice| UsageSliceProjection {
                    model_id: slice.model_id.clone(),
                    provider: slice.provider.clone(),
                    input_tokens: slice.input_tokens,
                    output_tokens: slice.output_tokens,
                })
                .collect(),
        }
    }
}
