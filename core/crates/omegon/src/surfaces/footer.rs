//! Shared footer/status semantic projection types.
//!
//! These structs describe operational telemetry without binding it to Ratatui
//! footer cards, the slim status line, ACP, or web transport details.

#[derive(Debug, Clone, PartialEq)]
pub struct FooterProjection {
    pub engine: EngineProjection,
    pub context: ContextProjection,
    pub memory: MemoryProjection,
    pub session: SessionProjection,
    pub workspace: WorkspaceProjection,
    pub usage_slices: Vec<UsageSliceProjection>,
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
    /// Web-search provider gauge: (provider, configured). Empty when the
    /// readiness snapshot is unavailable. All-false means the DDG scrape
    /// floor is the only search path — a degraded state.
    pub web_search_providers: Vec<(String, bool)>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSliceProjection {
    pub model_id: String,
    pub provider: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

pub trait ProjectFooterSurface {
    fn project_footer_surface(&self) -> FooterProjection;
}
