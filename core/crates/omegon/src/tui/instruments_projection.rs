//! Shared instrument panel semantic projection types.
//!
//! These structs describe inference/tool telemetry without binding it to Ratatui
//! gauges, colors, or panel layout.

#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentProjection {
    pub inference: InferenceProjection,
    pub tools: ToolActivityProjection,
    pub workers: WorkerActivityProjection,
    pub preferred_height: u16,
    pub focus_mode: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InferenceProjection {
    pub context_fill: f64,
    pub memory_fill: f64,
    pub thinking_level_pct: f64,
    pub thinking_active: bool,
    pub thinking_intensity: f64,
    pub external_wait: f64,
    pub context_window: usize,
    pub last_input_tokens: u32,
    pub last_output_tokens: u32,
    pub last_cache_read_tokens: u32,
    pub session_stores: u32,
    pub session_recalls: u32,
    pub heat: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolActivityProjection {
    pub tools: Vec<ToolProjection>,
    pub heat: f64,
    pub has_ever_fired: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolProjection {
    pub name: String,
    pub last_called_s: f64,
    pub is_error: bool,
    pub running: bool,
    pub last_duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerActivityProjection {
    pub cleave_active: bool,
    pub delegate_active: bool,
}

pub trait ProjectInstrumentSurface {
    fn project_instrument_surface(&self) -> InstrumentProjection;
}
