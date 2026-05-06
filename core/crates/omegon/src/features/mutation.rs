//! Mutation — evolutionary skill and diagnostic creation from session experience.
//!
//! Observes the agent loop via `BusEvent`, accumulates a per-session trajectory,
//! and at `SessionEnd` detects recovery patterns. Domain patterns become learned
//! skills; internal tool/harness deficiencies become diagnostic records that drive
//! evolutionary pressure on the tools and extensions themselves.
//!
//! Token burn tracking provides both a trigger threshold and postmortem metrics.

use async_trait::async_trait;
use omegon_traits::{
    BusEvent, BusRequest, ContentBlock, DriftKind, Feature, NotifyLevel, OodaPhase, ProgressSignal,
    ToolCapability, ToolDefinition, ToolResult,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::tool_registry;

// ─── Impact configuration ───────────────────────────────────────────────────
//
// "Impact" is what RL literature calls "fitness" — a measure of whether a
// mutation artifact made the harness better or worse. We use "impact" because
// it's clearer to non-specialists. See docs/design/mutation-eval-bridge.md.

/// All tuning parameters for impact evaluation. Loaded from
/// `~/.omegon/mutation/impact.toml`, falling back to seed defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct ImpactConfig {
    pub weights: ImpactWeights,
    pub learning: LearningConfig,
    pub confidence: ConfidenceConfig,
    pub windows: WindowsConfig,
    pub escalation: EscalationConfig,
    pub telemetry: TelemetryConfig,
    pub behavior: BehaviorConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ImpactWeights {
    pub component_score_delta: f64,
    pub burn_ratio_delta: f64,
    pub recovery_recurrence: f64,
    pub turn_efficiency: f64,
    pub token_efficiency: f64,
    pub usage_frequency: f64,
    pub age_decay: f64,
    pub usage_burn_interaction: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LearningConfig {
    pub learning_rate: f64,
    pub neutral_point: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfidenceConfig {
    pub floor: f64,
    pub ceiling: f64,
    pub auto_archive_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowsConfig {
    pub eval_attribution_days: u32,
    pub burn_comparison_sessions: u32,
    pub recurrence_lookback_sessions: u32,
    pub age_half_life_days: u32,
    pub min_eval_runs_for_attribution: u32,
    pub session_cadence: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EscalationConfig {
    pub diagnostic_recurrence_threshold: u32,
    pub severity_normalizer: u64,
}

/// Controls what the mutation system does vs just observes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BehaviorConfig {
    /// When true, the mutation system generates skill files and diagnostic
    /// records from detected recovery patterns. When false (the default),
    /// it only observes: burn-history is logged, impact evaluations run,
    /// but no artifacts are written. Start with false, review the data via
    /// `mutation_stats`, and enable when you've confirmed the signal is real.
    ///
    /// Note: generated skills are currently template-based (structured
    /// extraction from the recovery trajectory). Higher-quality skill
    /// generation via local inference (Ollama) is architecturally supported
    /// but not yet implemented — the trajectory data collected in
    /// observation mode is the input that synthesis would consume.
    pub generate_artifacts: bool,
    /// Minimum session turns before recovery detection runs. Sessions
    /// shorter than this are too brief to contain meaningful patterns.
    pub min_turns_for_analysis: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct TelemetryConfig {
    pub share_impact_data: bool,
}

impl Default for ImpactWeights {
    fn default() -> Self {
        Self {
            component_score_delta: 1.0,
            burn_ratio_delta: 0.8,
            recovery_recurrence: 0.6,
            turn_efficiency: 0.5,
            token_efficiency: 0.5,
            usage_frequency: 0.3,
            age_decay: 0.2,
            usage_burn_interaction: 0.5,
        }
    }
}

impl Default for LearningConfig {
    fn default() -> Self {
        Self {
            // Deliberately conservative. Lightweight passes use noisy burn
            // data as signal, so each pass should make only small adjustments.
            // At 0.03, it takes ~17 consistently positive evaluations to move
            // confidence from 0.7 to 0.95. This is slow by design — we'd
            // rather under-react than over-react to uncertain data.
            learning_rate: 0.03,
            neutral_point: 0.5,
        }
    }
}

impl Default for ConfidenceConfig {
    fn default() -> Self {
        Self {
            floor: 0.1,
            ceiling: 0.95,
            auto_archive_threshold: 0.15,
        }
    }
}

impl Default for WindowsConfig {
    fn default() -> Self {
        Self {
            eval_attribution_days: 14,
            burn_comparison_sessions: 5,
            recurrence_lookback_sessions: 10,
            age_half_life_days: 30,
            min_eval_runs_for_attribution: 2,
            session_cadence: 20,
        }
    }
}

impl Default for EscalationConfig {
    fn default() -> Self {
        Self {
            diagnostic_recurrence_threshold: 3,
            severity_normalizer: 10_000,
        }
    }
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            generate_artifacts: false,
            min_turns_for_analysis: 8,
        }
    }
}

fn load_impact_config(omegon_home: &std::path::Path) -> ImpactConfig {
    let path = omegon_home.join("mutation/impact.toml");
    if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        ImpactConfig::default()
    }
}

// ─── Creation context (harness state snapshot at skill creation) ─────────────

/// Snapshot of harness configuration when a skill is created, enabling
/// eval attribution: "this skill was created under these conditions."
#[derive(Debug, Clone, Default)]
struct CreationContext {
    model: String,
    capability_tier: String,
    thinking_level: String,
    context_class: String,
    omegon_version: String,
}

impl CreationContext {
    fn to_toml_block(&self, tests_component: &[&str]) -> String {
        let components = tests_component
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            r#"
[creation_context]
model = "{}"
capability_tier = "{}"
thinking_level = "{}"
context_class = "{}"
omegon_version = "{}"
tests_component = [{}]"#,
            self.model,
            self.capability_tier,
            self.thinking_level,
            self.context_class,
            self.omegon_version,
            components,
        )
    }
}

// ─── Impact log entry (for future federation) ───────────────────────────────

/// Structured record of a single impact evaluation, appended to
/// `~/.omegon/mutation/impact-log.jsonl`. Contains everything needed for
/// local debugging and future community aggregation. No user-identifying
/// content, no prompts, no file paths, no code.
#[derive(Debug, Serialize, Deserialize)]
struct ImpactLogEntry {
    instance_id: String,
    artifact_type: String,
    artifact_name: String,
    artifact_tags: Vec<String>,
    artifact_age_days: f32,
    model: String,
    capability_tier: String,
    thinking_level: String,
    context_class: String,
    omegon_version: String,
    tool_count: usize,
    extension_count: usize,
    component_score_delta: Option<f64>,
    burn_ratio_delta: Option<f64>,
    recovery_recurrence: Option<f64>,
    turn_efficiency: Option<f64>,
    token_efficiency: Option<f64>,
    usage_frequency: f64,
    age_decay: f64,
    penalty: f64,
    impact_score: f64,
    confidence_before: f64,
    confidence_after: f64,
    confidence_delta: f64,
    weights_snapshot: ImpactWeights,
    evaluation_mode: String,
    timestamp: String,
}

// ─── Instance ID ────────────────────────────────────────────────────────────

/// Read or generate a random instance ID for impact log disambiguation.
/// Stored at `~/.omegon/instance-id`. Not derived from user identity.
fn get_or_create_instance_id(omegon_home: &std::path::Path) -> String {
    let path = omegon_home.join("instance-id");
    if let Ok(id) = std::fs::read_to_string(&path) {
        let trimmed = id.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    // Generate a random UUID-like ID from system entropy.
    let id = format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        fxhash(&format!("{:?}", std::time::SystemTime::now())) as u32,
        (fxhash("a") >> 16) as u16,
        (fxhash("b") >> 32) as u16,
        (fxhash("c") >> 48) as u16,
        fxhash(&format!(
            "{:?}{}",
            std::time::SystemTime::now(),
            std::process::id()
        )),
    );
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, &id);
    id
}

// ─── Configuration ──────────────────────────────────────────────────────────

/// Maximum forward scan window (turns) when looking for a recovery after a failure.
const RECOVERY_SCAN_WINDOW: u32 = 5;
/// Burn ratio threshold — above this the session is worth analyzing even
/// without explicit recovery sequences.
const BURN_RATIO_THRESHOLD: f32 = 0.3;
/// Single recovery token cost threshold.
const RECOVERY_TOKEN_THRESHOLD: u64 = 10_000;

// ─── Trajectory types ───────────────────────────────────────────────────────

/// Compact trace of a single tool call within a turn.
#[derive(Debug, Clone)]
struct ToolCallTrace {
    call_id: String,
    name: String,
    capabilities: Vec<ToolCapability>,
    /// Compact args: only "path" and "command" fields preserved.
    args_summary: Value,
    /// File path extracted from args, if present.
    target_path: Option<String>,
    is_error: bool,
    completed: bool,
}

/// Behavioral snapshot of a single turn.
#[derive(Debug, Clone)]
struct TurnSnapshot {
    turn: u32,
    phase: Option<OodaPhase>,
    drift: Option<DriftKind>,
    progress: ProgressSignal,
    tools: Vec<ToolCallTrace>,
    input_tokens: u64,
    output_tokens: u64,
    is_burn: bool,
}

/// Accumulated trajectory for the entire session.
#[derive(Debug, Default)]
struct SessionTrajectory {
    session_id: String,
    turns: Vec<TurnSnapshot>,
    /// Tool calls received via ToolStart but not yet finalized by TurnEnd.
    pending_tools: Vec<ToolCallTrace>,
    total_input_tokens: u64,
    total_output_tokens: u64,
    burn_input_tokens: u64,
    burn_output_tokens: u64,
    /// Last known model from TurnEnd events.
    last_model: String,
    /// Last known provider from TurnEnd events.
    last_provider: String,
    /// Skills that were injected via provide_context() during this session.
    /// Behind a Mutex because provide_context() takes &self.
    skills_loaded: Mutex<Vec<String>>,
}

// ─── Recovery detection types ───────────────────────────────────────────────

/// A detected error→recovery sequence in the trajectory.
#[derive(Debug, Clone)]
struct RecoverySequence {
    start_turn: u32,
    end_turn: u32,
    failure: ToolCallTrace,
    success: ToolCallTrace,
    kind: RecoveryKind,
    token_cost: u64,
}

#[derive(Debug, Clone)]
enum RecoveryKind {
    /// Same tool+target, different args — agent compensated for tool limitation.
    SameToolDifferentArgs,
    /// Same tool+target, same args, intervening code change — domain learning.
    RetryAfterCodeChange,
    /// Agent switched tools to accomplish same goal — tool capability gap.
    ToolSwitch { from: String, to: String },
    /// Constraint discovery preceded recovery — domain constraint learned.
    ConstraintDiscoveryRecovery,
}

#[derive(Debug, Clone)]
enum PatternClass {
    DomainPattern {
        confidence: f32,
        description: String,
    },
    InternalDeficiency {
        owning_crate: &'static str,
        tool_name: String,
        confidence: f32,
    },
    // Future: AgentUsagePattern — the tool works correctly, but the agent
    // systematically misuses it due to misleading description or schema.
    // Detecting this requires reasoning observation (ThinkingChunk analysis),
    // not just tool call sequences. When implemented, the action would be
    // "propose a tool description rewrite" rather than skill or diagnostic.
}

// ─── Burn metrics ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize)]
struct BurnMetrics {
    total_input_tokens: u64,
    total_output_tokens: u64,
    burn_input_tokens: u64,
    burn_output_tokens: u64,
    burn_ratio: f32,
    recovery_count: usize,
    worth_analyzing: bool,
}

/// Per-session summary appended to burn-history.jsonl.
#[derive(Debug, Serialize, Deserialize)]
struct BurnLogEntry {
    session_id: String,
    timestamp: String,
    turns: u32,
    total_tokens: u64,
    burn_tokens: u64,
    burn_ratio: f32,
    recoveries: usize,
    skills_created: usize,
    diagnostics_created: usize,
    /// Names of learned skills that were loaded via provide_context() this session.
    #[serde(default)]
    active_learned_skills: Vec<String>,
    /// Names of open diagnostics at session end.
    #[serde(default)]
    active_diagnostics: Vec<String>,
}

// ─── Crate ownership lookup ─────────────────────────────────────────────────

fn owning_crate(tool_name: &str) -> &'static str {
    match tool_name {
        "bash" | "read" | "write" | "edit" | "change" | "commit" | "whoami" | "chronos"
        | "serve" => "omegon (core)",
        "view" => "omegon (view)",
        "web_search" => "omegon (web_search)",
        "ask_local_model" | "list_local_models" | "manage_ollama" => "omegon (local_inference)",
        "codebase_search" | "codebase_index" => "omegon-codescan",
        "memory_store"
        | "memory_recall"
        | "memory_query"
        | "memory_archive"
        | "memory_supersede"
        | "memory_connect"
        | "memory_focus"
        | "memory_release"
        | "memory_episodes"
        | "memory_compact"
        | "memory_search_archive"
        | "memory_ingest_lifecycle" => "omegon-memory",
        "design_tree" | "design_tree_update" | "openspec_manage" | "lifecycle_doctor" => {
            "omegon (lifecycle)"
        }
        "cleave_assess" | "cleave_run" => "omegon (cleave)",
        "delegate" | "delegate_result" | "delegate_status" => "omegon (delegate)",
        "secret_set" | "secret_list" | "secret_delete" => "omegon-secrets",
        _ => "extension (unknown)",
    }
}

fn trace_is_mutation_tool(trace: &ToolCallTrace) -> bool {
    trace.capabilities.contains(&ToolCapability::Mutation)
}

// ─── Feature ────────────────────────────────────────────────────────────────

pub struct MutationFeature {
    trajectory: SessionTrajectory,
    skills_dir: PathBuf,
    diagnostics_dir: PathBuf,
    burn_log_path: PathBuf,
    impact_log_path: PathBuf,
    impact_config: ImpactConfig,
    instance_id: String,
    /// Harness state captured from HarnessStatusChanged events.
    creation_ctx: CreationContext,
    omegon_home: PathBuf,
    /// Cumulative session count since last lightweight impact pass.
    sessions_since_impact_pass: u32,
}

impl MutationFeature {
    pub fn new(omegon_home: PathBuf) -> Self {
        let impact_config = load_impact_config(&omegon_home);
        let instance_id = get_or_create_instance_id(&omegon_home);
        Self {
            trajectory: SessionTrajectory::default(),
            skills_dir: omegon_home.join("skills/learned"),
            diagnostics_dir: omegon_home.join("diagnostics"),
            burn_log_path: omegon_home.join("mutation/burn-history.jsonl"),
            impact_log_path: omegon_home.join("mutation/impact-log.jsonl"),
            impact_config,
            instance_id,
            creation_ctx: CreationContext {
                omegon_version: env!("CARGO_PKG_VERSION").to_string(),
                ..Default::default()
            },
            omegon_home,
            sessions_since_impact_pass: 0,
        }
    }

    // ── Trajectory accumulation ─────────────────────────────────────────

    fn on_tool_start(
        &mut self,
        id: &str,
        name: &str,
        args: &Value,
        capabilities: &[ToolCapability],
    ) {
        let target_path = args.get("path").and_then(|v| v.as_str()).map(String::from);
        let mut summary = serde_json::Map::new();
        if let Some(p) = args.get("path") {
            summary.insert("path".into(), p.clone());
        }
        if let Some(c) = args.get("command") {
            summary.insert("command".into(), c.clone());
        }
        self.trajectory.pending_tools.push(ToolCallTrace {
            call_id: id.to_string(),
            name: name.to_string(),
            capabilities: capabilities.to_vec(),
            args_summary: Value::Object(summary),
            target_path,
            is_error: false,
            completed: false,
        });
    }

    fn on_tool_end(&mut self, id: &str, is_error: bool) {
        if let Some(trace) = self
            .trajectory
            .pending_tools
            .iter_mut()
            .find(|t| t.call_id == id)
        {
            trace.is_error = is_error;
            trace.completed = true;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn on_turn_end(
        &mut self,
        turn: u32,
        phase: Option<OodaPhase>,
        drift: Option<DriftKind>,
        progress: ProgressSignal,
        input_tokens: u64,
        output_tokens: u64,
        model: Option<&str>,
        provider: Option<&str>,
    ) {
        if let Some(m) = model {
            self.trajectory.last_model = m.to_string();
        }
        if let Some(p) = provider {
            self.trajectory.last_provider = p.to_string();
        }
        let tools: Vec<ToolCallTrace> = self.trajectory.pending_tools.drain(..).collect();

        let is_burn = drift.is_some()
            || (matches!(phase, Some(OodaPhase::Observe) | Some(OodaPhase::Orient))
                && matches!(progress, ProgressSignal::None)
                && turn > 2);

        let snap = TurnSnapshot {
            turn,
            phase,
            drift,
            progress,
            tools,
            input_tokens,
            output_tokens,
            is_burn,
        };

        self.trajectory.total_input_tokens += input_tokens;
        self.trajectory.total_output_tokens += output_tokens;
        if is_burn {
            self.trajectory.burn_input_tokens += input_tokens;
            self.trajectory.burn_output_tokens += output_tokens;
        }

        self.trajectory.turns.push(snap);
    }

    // ── Recovery detection ──────────────────────────────────────────────

    fn detect_recoveries(&self) -> Vec<RecoverySequence> {
        let mut sequences = Vec::new();

        // Flatten all tool traces with their turn index.
        let flat: Vec<(u32, &ToolCallTrace)> = self
            .trajectory
            .turns
            .iter()
            .flat_map(|t| t.tools.iter().map(move |tc| (t.turn, tc)))
            .collect();

        for (i, (fail_turn, fail_trace)) in flat.iter().enumerate() {
            if !fail_trace.is_error || !fail_trace.completed {
                continue;
            }

            // Scan forward for recovery within window.
            for (j, (succ_turn, succ_trace)) in flat.iter().enumerate().skip(i + 1) {
                if *succ_turn > fail_turn + RECOVERY_SCAN_WINDOW {
                    break;
                }
                if succ_trace.is_error || !succ_trace.completed {
                    continue;
                }

                let kind = if succ_trace.name == fail_trace.name
                    && succ_trace.target_path == fail_trace.target_path
                {
                    // Same tool, same target — check what changed.
                    let intervening_mutation = flat[i + 1..j].iter().any(|(_, t)| {
                        trace_is_mutation_tool(t)
                            && !t.is_error
                            && t.target_path == fail_trace.target_path
                    });
                    if intervening_mutation {
                        Some(RecoveryKind::RetryAfterCodeChange)
                    } else if succ_trace.args_summary != fail_trace.args_summary {
                        Some(RecoveryKind::SameToolDifferentArgs)
                    } else {
                        None // Same tool, same args, no code change — not a recovery.
                    }
                } else if succ_trace.target_path.is_some()
                    && succ_trace.target_path == fail_trace.target_path
                    && trace_is_mutation_tool(fail_trace)
                    && trace_is_mutation_tool(succ_trace)
                {
                    // Different mutation tool targeting the same file — tool switch.
                    Some(RecoveryKind::ToolSwitch {
                        from: fail_trace.name.clone(),
                        to: succ_trace.name.clone(),
                    })
                } else {
                    None
                };

                if let Some(kind) = kind {
                    let token_cost = self.tokens_in_range(*fail_turn, *succ_turn);
                    sequences.push(RecoverySequence {
                        start_turn: *fail_turn,
                        end_turn: *succ_turn,
                        failure: (*fail_trace).clone(),
                        success: (*succ_trace).clone(),
                        kind,
                        token_cost,
                    });
                    break; // Only match the first recovery per failure.
                }
            }
        }

        // Constraint-discovery pass: look for ConstraintDiscovery → Mutation/Commit.
        for (i, turn) in self.trajectory.turns.iter().enumerate() {
            if !matches!(turn.progress, ProgressSignal::ConstraintDiscovery) {
                continue;
            }
            for following in self.trajectory.turns.iter().skip(i + 1).take(3) {
                if matches!(
                    following.progress,
                    ProgressSignal::Mutation | ProgressSignal::Commit
                ) {
                    // Find a representative tool call from each turn.
                    if let (Some(discovery_tool), Some(resolution_tool)) =
                        (turn.tools.first(), following.tools.first())
                    {
                        let token_cost = self.tokens_in_range(turn.turn, following.turn);
                        sequences.push(RecoverySequence {
                            start_turn: turn.turn,
                            end_turn: following.turn,
                            failure: discovery_tool.clone(),
                            success: resolution_tool.clone(),
                            kind: RecoveryKind::ConstraintDiscoveryRecovery,
                            token_cost,
                        });
                    }
                    break;
                }
            }
        }

        sequences
    }

    fn tokens_in_range(&self, start: u32, end: u32) -> u64 {
        self.trajectory
            .turns
            .iter()
            .filter(|t| t.turn >= start && t.turn <= end)
            .map(|t| t.input_tokens + t.output_tokens)
            .sum()
    }

    // ── Classification ──────────────────────────────────────────────────

    fn classify(seq: &RecoverySequence) -> PatternClass {
        match &seq.kind {
            RecoveryKind::SameToolDifferentArgs => {
                // If the "different arg" is just a path change, it's likely the
                // agent targeted the wrong file initially — domain, not tool bug.
                let path_changed = seq
                    .failure
                    .args_summary
                    .get("path")
                    .and_then(|v| v.as_str())
                    != seq
                        .success
                        .args_summary
                        .get("path")
                        .and_then(|v| v.as_str());
                if path_changed {
                    PatternClass::DomainPattern {
                        confidence: 0.6,
                        description:
                            "Agent targeted wrong file initially, recovered to correct target"
                                .to_string(),
                    }
                } else {
                    PatternClass::InternalDeficiency {
                        owning_crate: owning_crate(&seq.failure.name),
                        tool_name: seq.failure.name.clone(),
                        confidence: 0.8,
                    }
                }
            }
            RecoveryKind::RetryAfterCodeChange => PatternClass::DomainPattern {
                confidence: 0.85,
                description: format!(
                    "Domain constraint required code change before {} succeeded",
                    seq.success.name
                ),
            },
            RecoveryKind::ToolSwitch { from, to } => {
                // bash→specialized tool is expected (agent exploring), not a gap.
                if from == "bash" {
                    PatternClass::DomainPattern {
                        confidence: 0.5,
                        description: format!(
                            "Agent explored via bash, then used {to} — normal exploration"
                        ),
                    }
                } else {
                    PatternClass::InternalDeficiency {
                        owning_crate: owning_crate(from),
                        tool_name: from.clone(),
                        confidence: 0.75,
                    }
                }
            }
            RecoveryKind::ConstraintDiscoveryRecovery => PatternClass::DomainPattern {
                confidence: 0.9,
                description: "Constraint discovery led to successful resolution".to_string(),
            },
        }
    }

    // ── Burn metrics ────────────────────────────────────────────────────

    fn compute_burn_metrics(&self, recoveries: &[RecoverySequence]) -> BurnMetrics {
        let total = self.trajectory.total_input_tokens + self.trajectory.total_output_tokens;
        let burn = self.trajectory.burn_input_tokens + self.trajectory.burn_output_tokens;
        let ratio = if total > 0 {
            burn as f32 / total as f32
        } else {
            0.0
        };
        let max_recovery_cost = recoveries.iter().map(|r| r.token_cost).max().unwrap_or(0);
        let min_turns = self.impact_config.behavior.min_turns_for_analysis;
        let worth = ratio > BURN_RATIO_THRESHOLD
            || max_recovery_cost > RECOVERY_TOKEN_THRESHOLD
            || self.trajectory.turns.len() as u32 >= min_turns;

        BurnMetrics {
            total_input_tokens: self.trajectory.total_input_tokens,
            total_output_tokens: self.trajectory.total_output_tokens,
            burn_input_tokens: self.trajectory.burn_input_tokens,
            burn_output_tokens: self.trajectory.burn_output_tokens,
            burn_ratio: ratio,
            recovery_count: recoveries.len(),
            worth_analyzing: worth,
        }
    }

    // ── File generation ─────────────────────────────────────────────────

    fn generate_skill(
        &self,
        seq: &RecoverySequence,
        description: &str,
    ) -> Option<(String, String)> {
        let id = format!("sk-{:012x}", fxhash(description));
        let name = slug_from_description(description);
        let now = chrono_now();

        let tags = derive_tags(seq);
        let tags_str = tags
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(", ");

        let tests_component: Vec<&str> = vec![match owning_crate(&seq.failure.name) {
            s if s.contains("core") => "tools",
            s if s.contains("memory") => "memory",
            s if s.contains("lifecycle") => "lifecycle",
            s if s.contains("cleave") => "cleave",
            s if s.contains("delegate") => "delegate",
            _ => "tools",
        }];
        let creation_ctx_block = self.creation_ctx.to_toml_block(&tests_component);

        let content = format!(
            r#"+++
id = "{id}"
name = "{name}"
description = "{description}"
tags = [{tags_str}]
origin = "mutation"
confidence = 0.7
reinforcement_count = 1
token_cost = {token_cost}
session_id = "{session_id}"
synthesized_at = "{now}"
{creation_context}
+++

# {title}

## When to apply
This pattern applies when working with `{tool}` on files matching the recovery context.

## Procedure
1. The original attempt using `{tool}` failed: check the failure mode below.
2. Recovery involved: {recovery_desc}
3. The successful approach used `{success_tool}` with adjusted parameters.

## Known failure modes
- Original error on turn {start}: `{tool}` with args matching `{fail_args}`

## Verification
- Confirm the target operation succeeds without retry after applying this pattern.
"#,
            id = id,
            name = name,
            description = description.replace('"', "'"),
            tags_str = tags_str,
            token_cost = seq.token_cost,
            session_id = self.trajectory.session_id,
            now = now,
            creation_context = creation_ctx_block,
            title = titlecase(description),
            tool = seq.failure.name,
            recovery_desc = recovery_description(&seq.kind),
            success_tool = seq.success.name,
            start = seq.start_turn,
            fail_args = compact_args(&seq.failure.args_summary),
        );

        Some((name, content))
    }

    fn generate_diagnostic(
        &self,
        seq: &RecoverySequence,
        owning_crate: &str,
        burn: &BurnMetrics,
    ) -> Option<(String, String)> {
        let hash = format!(
            "{:08x}",
            fxhash(&format!("{}{}", seq.failure.name, seq.start_turn))
        );
        let date = chrono_date();
        let filename = format!("{}-{}-{}", date, seq.failure.name, hash);

        let content = format!(
            r#"# Diagnostic: {tool} — {kind_desc}

**Date**: {date}
**Tool**: {tool}
**Owning crate**: {owning_crate}
**Session**: {session_id}
**Recovery cost**: {token_cost} tokens
**Burn ratio**: {burn_ratio:.0}%

## Reproduction

1. Turn {start}: `{tool}` called with `{fail_args}`
2. Result: error (is_error=true)
3. Recovery at turn {end}: `{success_tool}` called with `{succ_args}` — success

## Classification

- Recovery kind: {kind_label}
- Confidence: {confidence:.2}
- Suggested fix class: {fix_class}
"#,
            tool = seq.failure.name,
            kind_desc = recovery_description(&seq.kind),
            date = date,
            owning_crate = owning_crate,
            session_id = self.trajectory.session_id,
            token_cost = seq.token_cost,
            burn_ratio = burn.burn_ratio * 100.0,
            start = seq.start_turn,
            fail_args = compact_args(&seq.failure.args_summary),
            end = seq.end_turn,
            success_tool = seq.success.name,
            succ_args = compact_args(&seq.success.args_summary),
            kind_label = recovery_kind_label(&seq.kind),
            confidence = match Self::classify(seq) {
                PatternClass::DomainPattern { confidence, .. } => confidence,
                PatternClass::InternalDeficiency { confidence, .. } => confidence,
            },
            fix_class = suggested_fix_class(&seq.kind),
        );

        Some((filename, content))
    }

    fn append_burn_log(&self, entry: &BurnLogEntry) {
        if let Ok(json) = serde_json::to_string(entry) {
            if let Some(parent) = self.burn_log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.burn_log_path)
                .and_then(|mut f| {
                    use std::io::Write;
                    writeln!(f, "{json}")
                });
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(
                    &self.burn_log_path,
                    std::fs::Permissions::from_mode(0o600),
                );
            }
        }
    }

    // ── Impact evaluation ────────────────────────────────────────────────

    /// Append an impact log entry to impact-log.jsonl.
    fn append_impact_log(&self, entry: &ImpactLogEntry) {
        if let Ok(json) = serde_json::to_string(entry) {
            if let Some(parent) = self.impact_log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.impact_log_path)
                .and_then(|mut f| {
                    use std::io::Write;
                    writeln!(f, "{json}")
                });
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(
                    &self.impact_log_path,
                    std::fs::Permissions::from_mode(0o600),
                );
            }
        }
    }

    /// Run a lightweight impact evaluation using burn-history signals only.
    /// Called on session cadence. Does not use eval ScoreCard data.
    fn run_lightweight_impact_pass(&self) {
        if !self.skills_dir.exists() {
            return;
        }
        let entries = match std::fs::read_dir(&self.skills_dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        let burn_entries = self
            .read_recent_burn_entries(self.impact_config.windows.burn_comparison_sessions as usize);

        for entry in entries.flatten() {
            let skill_path = entry.path().join("SKILL.md");
            if !skill_path.exists() {
                continue;
            }
            let content = match std::fs::read_to_string(&skill_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let skill_name = entry.file_name().to_string_lossy().to_string();
            let confidence = extract_frontmatter_field(&content, "confidence")
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.7);
            let tags = extract_frontmatter_field(&content, "tags").unwrap_or_default();
            let age_days = extract_frontmatter_field(&content, "synthesized_at")
                .map(|ts| estimate_age_days(&ts))
                .unwrap_or(0.0);

            // Compute available signals for lightweight pass.
            let burn_delta = self.compute_burn_delta_for_skill(&skill_name, &burn_entries);
            let usage_freq = self.compute_usage_frequency(&skill_name, &burn_entries);
            // Age decay pauses for skills that have never been loaded — they
            // haven't had a fair chance to prove themselves. A skill that was
            // loaded and didn't help should decay; a skill that was never
            // matched shouldn't be penalized for irrelevance to recent work.
            let age_decay = if usage_freq == 0.0 {
                1.0 // No decay — skill was never tested.
            } else {
                compute_age_decay(
                    age_days,
                    self.impact_config.windows.age_half_life_days as f64,
                )
            };

            // Recurrence: check if diagnostics related to skill tags still appear.
            let recurrence = self.compute_recurrence_for_tags(&tags);

            let cfg = &self.impact_config;
            let w = &cfg.weights;

            // Lightweight: only burn_delta, recurrence, usage, age are available.
            let signals: Vec<(f64, f64)> = vec![
                (burn_delta.unwrap_or(0.5), w.burn_ratio_delta),
                (recurrence, w.recovery_recurrence),
                (usage_freq, w.usage_frequency),
                (age_decay, w.age_decay),
            ];

            let total_weight: f64 = signals.iter().map(|(_, wt)| wt).sum();
            let weighted_sum: f64 = signals.iter().map(|(s, wt)| s * wt).sum();

            // Interaction penalty: high usage + negative burn.
            let penalty = if let Some(bd) = burn_delta {
                (usage_freq * (cfg.learning.neutral_point - bd).max(0.0) * w.usage_burn_interaction)
                    .max(0.0)
            } else {
                0.0
            };

            let impact_score = if total_weight > 0.0 {
                ((weighted_sum - penalty) / total_weight).clamp(0.0, 1.0)
            } else {
                0.5
            };

            // Update confidence.
            let new_confidence = (confidence
                + (impact_score - cfg.learning.neutral_point) * cfg.learning.learning_rate)
                .clamp(cfg.confidence.floor, cfg.confidence.ceiling);

            if (new_confidence - confidence).abs() > 0.001 {
                let updated = update_frontmatter_field(
                    &content,
                    "confidence",
                    &format!("{new_confidence:.2}"),
                );
                let _ = std::fs::write(&skill_path, &updated);
            }

            if new_confidence < cfg.confidence.auto_archive_threshold {
                let archive_dir = self
                    .skills_dir
                    .parent()
                    .unwrap_or(&self.skills_dir)
                    .join("archived");
                let _ = std::fs::create_dir_all(&archive_dir);
                let _ = std::fs::rename(entry.path(), archive_dir.join(entry.file_name()));
            }

            // Log the evaluation.
            self.append_impact_log(&ImpactLogEntry {
                instance_id: self.instance_id.clone(),
                artifact_type: "skill".into(),
                artifact_name: skill_name,
                artifact_tags: tags
                    .trim_matches(|c| c == '[' || c == ']')
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
                artifact_age_days: age_days as f32,
                model: self.creation_ctx.model.clone(),
                capability_tier: self.creation_ctx.capability_tier.clone(),
                thinking_level: self.creation_ctx.thinking_level.clone(),
                context_class: self.creation_ctx.context_class.clone(),
                omegon_version: env!("CARGO_PKG_VERSION").to_string(),
                tool_count: 0,
                extension_count: 0,
                component_score_delta: None,
                burn_ratio_delta: burn_delta,
                recovery_recurrence: Some(recurrence),
                turn_efficiency: None,
                token_efficiency: None,
                usage_frequency: usage_freq,
                age_decay,
                penalty,
                impact_score,
                confidence_before: confidence,
                confidence_after: new_confidence,
                confidence_delta: new_confidence - confidence,
                weights_snapshot: cfg.weights.clone(),
                evaluation_mode: "lightweight".into(),
                timestamp: chrono_now(),
            });
        }
    }

    /// Compute burn ratio delta for a skill by comparing sessions where it was
    /// loaded vs sessions where it wasn't.
    fn compute_burn_delta_for_skill(
        &self,
        skill_name: &str,
        entries: &[BurnLogEntry],
    ) -> Option<f64> {
        let (mut with_sum, mut with_count) = (0.0f64, 0u32);
        let (mut without_sum, mut without_count) = (0.0f64, 0u32);

        for entry in entries {
            if entry.active_learned_skills.iter().any(|s| s == skill_name) {
                with_sum += entry.burn_ratio as f64;
                with_count += 1;
            } else {
                without_sum += entry.burn_ratio as f64;
                without_count += 1;
            }
        }

        if with_count == 0 || without_count == 0 {
            return None; // Can't compare without both populations.
        }

        let with_avg = with_sum / with_count as f64;
        let without_avg = without_sum / without_count as f64;
        // Positive delta = improvement (lower burn when skill is present).
        // Normalize to 0.0-1.0 range: 0.5 = no change.
        let raw_delta = without_avg - with_avg; // positive = skill reduced burn
        Some((0.5 + raw_delta).clamp(0.0, 1.0))
    }

    /// Compute how often a skill was loaded across recent sessions.
    fn compute_usage_frequency(&self, skill_name: &str, entries: &[BurnLogEntry]) -> f64 {
        if entries.is_empty() {
            return 0.0;
        }
        let loaded_count = entries
            .iter()
            .filter(|e| e.active_learned_skills.iter().any(|s| s == skill_name))
            .count();
        loaded_count as f64 / entries.len() as f64
    }

    /// Check if recovery patterns related to skill tags still recur in recent diagnostics.
    fn compute_recurrence_for_tags(&self, tags: &str) -> f64 {
        if !self.diagnostics_dir.exists() {
            return 1.0; // No diagnostics = no recurrence = good signal.
        }
        let tag_list: Vec<&str> = tags
            .trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .map(|s| s.trim().trim_matches('"'))
            .filter(|s| !s.is_empty())
            .collect();

        if tag_list.is_empty() {
            return 0.5; // Unknown.
        }

        let matching_diagnostics = std::fs::read_dir(&self.diagnostics_dir)
            .ok()
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| {
                        let name = e.file_name().to_string_lossy().to_lowercase();
                        tag_list
                            .iter()
                            .any(|tag| name.contains(&tag.to_lowercase()))
                    })
                    .count()
            })
            .unwrap_or(0);

        match matching_diagnostics {
            0 => 1.0,
            1 => 0.7,
            2 => 0.4,
            _ => 0.0,
        }
    }

    /// Read recent burn-history entries.
    fn read_recent_burn_entries(&self, n: usize) -> Vec<BurnLogEntry> {
        if !self.burn_log_path.exists() {
            return vec![];
        }
        let content = match std::fs::read_to_string(&self.burn_log_path) {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        content
            .lines()
            .rev()
            .take(n)
            .filter_map(|line| serde_json::from_str::<BurnLogEntry>(line).ok())
            .collect()
    }

    // ── Diagnostic escalation ───────────────────────────────────────────

    /// Check all diagnostics for escalation and generate eval scenario candidates.
    fn check_diagnostic_escalation(&self) -> Vec<String> {
        if !self.diagnostics_dir.exists() {
            return vec![];
        }

        let candidates_dir = self.omegon_home.join("eval-candidates");
        let normalizer = self.impact_config.escalation.severity_normalizer as f64;
        let threshold = self
            .impact_config
            .escalation
            .diagnostic_recurrence_threshold;
        let mut generated = Vec::new();

        // Group diagnostics by tool name (extracted from filename: YYYY-MM-DD-{tool}-{hash}.md).
        let mut by_tool: std::collections::HashMap<String, Vec<(String, u64)>> =
            std::collections::HashMap::new();

        if let Ok(entries) = std::fs::read_dir(&self.diagnostics_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.ends_with(".md") {
                    continue;
                }
                // Parse tool name from filename: YYYY-MM-DD-{tool}-{hash}.md
                let parts: Vec<&str> = name.trim_end_matches(".md").splitn(4, '-').collect();
                if parts.len() >= 4 {
                    let tool = parts[3]
                        .rsplit_once('-')
                        .map(|(t, _)| t)
                        .unwrap_or(parts[3]);
                    // Try to extract token cost from content.
                    let token_cost = std::fs::read_to_string(entry.path())
                        .ok()
                        .and_then(|content| {
                            content
                                .lines()
                                .find(|l| l.starts_with("**Recovery cost**:"))
                                .and_then(|l| {
                                    l.split(':').nth(1).and_then(|s| {
                                        s.split_whitespace()
                                            .next()
                                            .and_then(|n| n.parse::<u64>().ok())
                                    })
                                })
                        })
                        .unwrap_or(0);

                    by_tool
                        .entry(tool.to_string())
                        .or_default()
                        .push((name.clone(), token_cost));
                }
            }
        }

        for (tool, diagnostics) in &by_tool {
            let count = diagnostics.len() as f64;
            let severity: f64 = diagnostics
                .iter()
                .map(|(_, cost)| *cost as f64 / normalizer)
                .sum();
            let escalation_score = count + severity;

            if escalation_score >= threshold as f64 {
                // Check if we already generated a candidate for this tool.
                let candidate_name = format!("{tool}-recovery");
                let candidate_path = candidates_dir.join(format!("{candidate_name}.toml"));
                if candidate_path.exists() {
                    continue;
                }

                let scenario_toml = format!(
                    "# Auto-generated from {} diagnostics for tool '{}'.\n\
                     # Escalation score: {:.1} (threshold: {}).\n\
                     # Review and add to an eval suite if appropriate.\n\
                     \n\
                     [scenario]\n\
                     name = \"{}\"\n\
                     description = \"Agent must handle {} failure pattern without excessive recovery\"\n\
                     difficulty = 2\n\
                     domain = \"coding\"\n\
                     tests_component = [\"tools\"]\n\
                     generated_from = \"diagnostics:{}\"\n\
                     \n\
                     [input]\n\
                     prompt = \"TODO: Write a prompt that reproduces the {} failure pattern.\"\n\
                     \n\
                     [scoring.recovery_efficiency]\n\
                     type = \"turn-count\"\n\
                     max_turns = 8\n\
                     ideal_turns = 3\n\
                     weight = 0.5\n\
                     \n\
                     [scoring.token_budget]\n\
                     type = \"token-budget\"\n\
                     max_tokens = 15000\n\
                     weight = 0.5\n",
                    diagnostics.len(),
                    tool,
                    escalation_score,
                    threshold,
                    candidate_name,
                    tool,
                    tool,
                    tool,
                );

                let _ = std::fs::create_dir_all(&candidates_dir);
                if std::fs::write(&candidate_path, &scenario_toml).is_ok() {
                    generated.push(candidate_name);
                }
            }
        }

        generated
    }

    // ── Session end pipeline ────────────────────────────────────────────

    fn on_session_end(&mut self, turns: u32) -> Vec<BusRequest> {
        let min_turns = self.impact_config.behavior.min_turns_for_analysis;
        if turns < min_turns {
            return vec![];
        }

        // ── Always: observe and log ──────────────────────────────────
        let recoveries = self.detect_recoveries();
        let burn = self.compute_burn_metrics(&recoveries);

        let mut requests = Vec::new();
        let mut skills_created = 0usize;
        let mut diagnostics_created = 0usize;

        // ── Only when generate_artifacts is enabled ──────────────────
        if self.impact_config.behavior.generate_artifacts && burn.worth_analyzing {
            for seq in &recoveries {
                let class = Self::classify(seq);
                match class {
                    PatternClass::DomainPattern {
                        description,
                        confidence,
                    } => {
                        if confidence >= 0.6
                            && let Some((name, content)) = self.generate_skill(seq, &description)
                        {
                            let dir = self.skills_dir.join(&name);
                            let _ = std::fs::create_dir_all(&dir);
                            let path = dir.join("SKILL.md");
                            if !path.exists() {
                                let _ = std::fs::write(&path, &content);
                                skills_created += 1;
                                requests.push(BusRequest::AutoStoreFact {
                                    section: "patterns_conventions".into(),
                                    content: format!("Learned skill: {name} — {description}"),
                                    source: "mutation".into(),
                                });
                            }
                        }
                    }
                    PatternClass::InternalDeficiency {
                        owning_crate: crate_name,
                        tool_name,
                        confidence,
                    } => {
                        if confidence >= 0.7
                            && let Some((filename, content)) =
                                self.generate_diagnostic(seq, crate_name, &burn)
                        {
                            let _ = std::fs::create_dir_all(&self.diagnostics_dir);
                            let path = self.diagnostics_dir.join(format!("{filename}.md"));
                            if !path.exists() {
                                let _ = std::fs::write(&path, &content);
                                diagnostics_created += 1;
                                requests.push(BusRequest::AutoStoreFact {
                                    section: "known_issues".into(),
                                    content: format!(
                                        "Tool deficiency: {tool_name} ({crate_name}) — {}",
                                        recovery_description(&seq.kind)
                                    ),
                                    source: "mutation".into(),
                                });
                            }
                        }
                    }
                }
            }
        }

        // Collect open diagnostics for burn log enrichment.
        let active_diagnostics = if self.diagnostics_dir.exists() {
            std::fs::read_dir(&self.diagnostics_dir)
                .ok()
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else {
            vec![]
        };

        // Always log burn metrics.
        self.append_burn_log(&BurnLogEntry {
            session_id: self.trajectory.session_id.clone(),
            timestamp: chrono_now(),
            turns,
            total_tokens: burn.total_input_tokens + burn.total_output_tokens,
            burn_tokens: burn.burn_input_tokens + burn.burn_output_tokens,
            burn_ratio: burn.burn_ratio,
            recoveries: recoveries.len(),
            skills_created,
            diagnostics_created,
            active_learned_skills: self
                .trajectory
                .skills_loaded
                .lock()
                .map(|v| v.clone())
                .unwrap_or_default(),
            active_diagnostics,
        });

        if skills_created > 0 || diagnostics_created > 0 {
            requests.push(BusRequest::Notify {
                message: format!(
                    "Mutation: {skills_created} skill(s), {diagnostics_created} diagnostic(s) created \
                     (burn ratio: {:.0}%, {} recoveries)",
                    burn.burn_ratio * 100.0,
                    recoveries.len(),
                ),
                level: NotifyLevel::Info,
            });
        }

        let escalated = self.check_diagnostic_escalation();
        for candidate in &escalated {
            requests.push(BusRequest::Notify {
                message: format!("Mutation: diagnostic escalated to eval candidate — {candidate}"),
                level: NotifyLevel::Info,
            });
        }

        // Session cadence: run lightweight impact pass periodically.
        self.sessions_since_impact_pass += 1;
        if self.sessions_since_impact_pass >= self.impact_config.windows.session_cadence {
            self.sessions_since_impact_pass = 0;
            self.run_lightweight_impact_pass();
        }

        requests
    }

    // ── Tool implementations ────────────────────────────────────────────

    fn execute_review(&self) -> ToolResult {
        let mut lines = Vec::new();

        lines.push("## Learned Skills\n".to_string());
        if self.skills_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&self.skills_dir) {
                let mut found = false;
                for entry in entries.flatten() {
                    let skill_path = entry.path().join("SKILL.md");
                    if skill_path.exists()
                        && let Ok(content) = std::fs::read_to_string(&skill_path)
                    {
                        let desc = extract_frontmatter_field(&content, "description")
                            .unwrap_or_else(|| entry.file_name().to_string_lossy().into());
                        let conf = extract_frontmatter_field(&content, "confidence")
                            .unwrap_or_else(|| "?".into());
                        lines.push(format!(
                            "- **{}** (confidence: {}) — {}",
                            entry.file_name().to_string_lossy(),
                            conf,
                            desc
                        ));
                        found = true;
                    }
                }
                if !found {
                    lines.push("(none)".to_string());
                }
            }
        } else {
            lines.push("(none)".to_string());
        }

        // Diagnostics
        lines.push("\n## Diagnostics\n".to_string());
        if self.diagnostics_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&self.diagnostics_dir) {
                let mut found = false;
                for entry in entries.flatten() {
                    if entry.path().extension().is_some_and(|e| e == "md") {
                        lines.push(format!("- {}", entry.file_name().to_string_lossy()));
                        found = true;
                    }
                }
                if !found {
                    lines.push("(none)".to_string());
                }
            }
        } else {
            lines.push("(none)".to_string());
        }

        ToolResult {
            content: vec![ContentBlock::Text {
                text: lines.join("\n"),
            }],
            details: Value::Null,
        }
    }

    fn execute_accept(&self, name: &str) -> ToolResult {
        let skill_path = self.skills_dir.join(name).join("SKILL.md");
        if !skill_path.exists() {
            return tool_error(&format!("Skill '{}' not found", name));
        }
        if let Ok(content) = std::fs::read_to_string(&skill_path) {
            let updated = bump_confidence(&content);
            let _ = std::fs::write(&skill_path, updated);
        }
        ToolResult {
            content: vec![ContentBlock::Text {
                text: format!("Accepted skill '{name}' — confidence boosted."),
            }],
            details: Value::Null,
        }
    }

    fn execute_reject(&self, name: &str) -> ToolResult {
        let skill_dir = self.skills_dir.join(name);
        let diag_path = self.diagnostics_dir.join(format!("{name}.md"));
        if skill_dir.exists() {
            let _ = std::fs::remove_dir_all(&skill_dir);
            return ToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("Rejected and removed skill '{name}'."),
                }],
                details: Value::Null,
            };
        }
        if diag_path.exists() {
            let _ = std::fs::remove_file(&diag_path);
            return ToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("Rejected and removed diagnostic '{name}'."),
                }],
                details: Value::Null,
            };
        }
        tool_error(&format!("'{}' not found in skills or diagnostics", name))
    }

    fn execute_stats(&self) -> ToolResult {
        let mut lines = Vec::new();

        // Current session
        let total = self.trajectory.total_input_tokens + self.trajectory.total_output_tokens;
        let burn = self.trajectory.burn_input_tokens + self.trajectory.burn_output_tokens;
        let ratio = if total > 0 {
            burn as f32 / total as f32
        } else {
            0.0
        };
        lines.push("## Current Session\n".to_string());
        lines.push(format!("- Turns: {}", self.trajectory.turns.len()));
        lines.push(format!("- Total tokens: {total}"));
        lines.push(format!("- Burn tokens: {burn}"));
        lines.push(format!("- Burn ratio: {:.1}%", ratio * 100.0));

        // Configuration
        lines.push("\n## Configuration\n".to_string());
        lines.push(format!(
            "- Artifact generation: {}",
            if self.impact_config.behavior.generate_artifacts {
                "enabled"
            } else {
                "**disabled** (observation only)"
            }
        ));
        lines.push(format!(
            "- Min turns for analysis: {}",
            self.impact_config.behavior.min_turns_for_analysis
        ));

        // Recent history
        lines.push("\n## Recent Sessions\n".to_string());
        if self.burn_log_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&self.burn_log_path) {
                let recent: Vec<&str> = content.lines().rev().take(10).collect();
                if recent.is_empty() {
                    lines.push("(no history)".to_string());
                } else {
                    let mut high_burn_no_recovery = 0u32;
                    let mut total_entries = 0u32;
                    for line in recent.iter().rev() {
                        if let Ok(entry) = serde_json::from_str::<BurnLogEntry>(line) {
                            total_entries += 1;
                            if entry.burn_ratio > 0.3 && entry.recoveries == 0 {
                                high_burn_no_recovery += 1;
                            }
                            lines.push(format!(
                                "- {} — {:.0}% burn, {} recoveries, {} skills, {} diags",
                                &entry.timestamp[..10],
                                entry.burn_ratio * 100.0,
                                entry.recoveries,
                                entry.skills_created,
                                entry.diagnostics_created,
                            ));
                        }
                    }
                    // Surface the key diagnostic: is burn coming from recoveries or elsewhere?
                    if total_entries > 0 && high_burn_no_recovery > 0 {
                        lines.push(format!(
                            "\n**Note:** {high_burn_no_recovery}/{total_entries} recent sessions had high burn (>30%) \
                             with zero recoveries — token waste may not be recovery-related."
                        ));
                    }
                }
            }
        } else {
            lines.push("(no history)".to_string());
        }

        ToolResult {
            content: vec![ContentBlock::Text {
                text: lines.join("\n"),
            }],
            details: Value::Null,
        }
    }
    fn format_config(&self) -> String {
        let cfg = &self.impact_config;
        let config_path = self.omegon_home.join("mutation/impact.toml");
        let config_source = if config_path.exists() {
            format!("{}", config_path.display())
        } else {
            "defaults (no impact.toml found)".into()
        };

        let w = &cfg.weights;
        format!(
            "## Mutation Configuration\n\n\
             **Source:** {config_source}\n\n\
             ### Behavior\n\
             - Artifact generation: {artifact_gen}\n\
             - Min turns for analysis: {min_turns}\n\n\
             ### Signal Weights\n\
             - Component score delta: {w_comp}\n\
             - Burn ratio delta: {w_burn}\n\
             - Recovery recurrence: {w_recur}\n\
             - Turn efficiency: {w_turns}\n\
             - Token efficiency: {w_tokens}\n\
             - Usage frequency: {w_usage}\n\
             - Age decay: {w_age}\n\
             - Usage-burn interaction: {w_interact}\n\n\
             ### Learning\n\
             - Learning rate: {lr}\n\
             - Neutral point: {np}\n\n\
             ### Confidence\n\
             - Floor: {c_floor} / Ceiling: {c_ceil}\n\
             - Auto-archive threshold: {c_arch}\n\n\
             ### Windows\n\
             - Eval attribution: {t_eval} days\n\
             - Burn comparison: {n_burn} sessions\n\
             - Recurrence lookback: {n_recur} sessions\n\
             - Age half-life: {t_half} days\n\
             - Min eval runs: {n_eval}\n\
             - Session cadence: {n_cad} sessions\n\n\
             ### Escalation\n\
             - Diagnostic threshold: {esc_thresh}\n\
             - Severity normalizer: {esc_norm} tokens\n\n\
             ### Telemetry\n\
             - Share impact data: {share}\n\n\
             To customize, create or edit `{config_path}`.",
            config_source = config_source,
            artifact_gen = if cfg.behavior.generate_artifacts {
                "**enabled**"
            } else {
                "disabled (observation only)"
            },
            min_turns = cfg.behavior.min_turns_for_analysis,
            w_comp = w.component_score_delta,
            w_burn = w.burn_ratio_delta,
            w_recur = w.recovery_recurrence,
            w_turns = w.turn_efficiency,
            w_tokens = w.token_efficiency,
            w_usage = w.usage_frequency,
            w_age = w.age_decay,
            w_interact = w.usage_burn_interaction,
            lr = cfg.learning.learning_rate,
            np = cfg.learning.neutral_point,
            c_floor = cfg.confidence.floor,
            c_ceil = cfg.confidence.ceiling,
            c_arch = cfg.confidence.auto_archive_threshold,
            t_eval = cfg.windows.eval_attribution_days,
            n_burn = cfg.windows.burn_comparison_sessions,
            n_recur = cfg.windows.recurrence_lookback_sessions,
            t_half = cfg.windows.age_half_life_days,
            n_eval = cfg.windows.min_eval_runs_for_attribution,
            n_cad = cfg.windows.session_cadence,
            esc_thresh = cfg.escalation.diagnostic_recurrence_threshold,
            esc_norm = cfg.escalation.severity_normalizer,
            share = cfg.telemetry.share_impact_data,
            config_path = config_path.display(),
        )
    }
}

#[async_trait]
impl Feature for MutationFeature {
    fn name(&self) -> &str {
        "mutation"
    }

    fn commands(&self) -> Vec<omegon_traits::CommandDefinition> {
        vec![omegon_traits::CommandDefinition {
            name: "mutation".into(),
            description: "Mutation system — burn metrics, learned skills, diagnostics".into(),
            subcommands: vec!["stats".into(), "review".into(), "config".into()],
        }]
    }

    fn handle_command(&mut self, name: &str, args: &str) -> omegon_traits::CommandResult {
        if name != "mutation" {
            return omegon_traits::CommandResult::NotHandled;
        }
        let sub = args.trim();
        match sub {
            "stats" | "" => {
                let result = self.execute_stats();
                let text = result
                    .content
                    .into_iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                omegon_traits::CommandResult::Display(text)
            }
            "review" => {
                let result = self.execute_review();
                let text = result
                    .content
                    .into_iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                omegon_traits::CommandResult::Display(text)
            }
            "config" => omegon_traits::CommandResult::Display(self.format_config()),
            _ => omegon_traits::CommandResult::Display(
                "Unknown subcommand. Available: stats, review, config".into(),
            ),
        }
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: tool_registry::mutation::MUTATION_REVIEW.into(),
                label: "Mutation Review".into(),
                description:
                    "List learned skills and diagnostic records created by the mutation system."
                        .into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false,
                }),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
            },
            ToolDefinition {
                name: tool_registry::mutation::MUTATION_ACCEPT.into(),
                label: "Mutation Accept".into(),
                description: "Accept a learned skill or diagnostic, boosting its confidence."
                    .into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Name of the skill or diagnostic to accept."
                        }
                    },
                    "required": ["name"],
                    "additionalProperties": false,
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: tool_registry::mutation::MUTATION_REJECT.into(),
                label: "Mutation Reject".into(),
                description: "Reject and remove a learned skill or diagnostic.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Name of the skill or diagnostic to reject."
                        }
                    },
                    "required": ["name"],
                    "additionalProperties": false,
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: tool_registry::mutation::MUTATION_STATS.into(),
                label: "Mutation Stats".into(),
                description: "Show token burn metrics for the current session and recent history."
                    .into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false,
                }),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
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
            tool_registry::mutation::MUTATION_REVIEW => Ok(self.execute_review()),
            tool_registry::mutation::MUTATION_ACCEPT => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing required parameter: name"))?;
                Ok(self.execute_accept(name))
            }
            tool_registry::mutation::MUTATION_REJECT => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing required parameter: name"))?;
                Ok(self.execute_reject(name))
            }
            tool_registry::mutation::MUTATION_STATS => Ok(self.execute_stats()),
            _ => Err(anyhow::anyhow!("Unknown tool: {tool_name}")),
        }
    }

    fn provide_context(
        &self,
        signals: &omegon_traits::ContextSignals<'_>,
    ) -> Option<omegon_traits::ContextInjection> {
        if !self.skills_dir.exists() {
            return None;
        }
        let entries = std::fs::read_dir(&self.skills_dir).ok()?;

        let recent_tools: Vec<&str> = signals.recent_tools.iter().map(|s| s.as_str()).collect();
        let mut injections = Vec::new();
        let mut budget = 2000usize;

        for entry in entries.flatten() {
            if budget == 0 {
                break;
            }
            let skill_path = entry.path().join("SKILL.md");
            if !skill_path.exists() {
                continue;
            }
            let content = match std::fs::read_to_string(&skill_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            // Match on tags.
            let tags = extract_frontmatter_field(&content, "tags").unwrap_or_default();
            let matched = recent_tools.iter().any(|t| tags.contains(t))
                || signals.recent_files.iter().any(|f| {
                    f.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|ext| tags.contains(ext))
                });

            if matched {
                let desc = extract_frontmatter_field(&content, "description").unwrap_or_default();
                let name = extract_frontmatter_field(&content, "name").unwrap_or_default();
                let line = format!("- **{name}**: {desc}");
                if line.len() <= budget {
                    budget = budget.saturating_sub(line.len());
                    injections.push(line);
                    // Track that this skill was loaded for burn-history enrichment.
                    if let Ok(mut loaded) = self.trajectory.skills_loaded.lock()
                        && !loaded.contains(&name)
                    {
                        loaded.push(name);
                    }
                }
            }

            if injections.len() >= 3 {
                break;
            }
        }

        if injections.is_empty() {
            return None;
        }

        Some(omegon_traits::ContextInjection {
            source: "mutation".into(),
            content: format!(
                "[Learned skills from prior sessions]\n{}",
                injections.join("\n")
            ),
            priority: 40,
            ttl_turns: 3,
        })
    }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        match event {
            BusEvent::SessionStart { session_id, .. } => {
                self.trajectory = SessionTrajectory {
                    session_id: session_id.clone(),
                    ..Default::default()
                };
                vec![]
            }
            BusEvent::ToolStart {
                id,
                name,
                args,
                capabilities,
            } => {
                self.on_tool_start(id, name, args, capabilities);
                vec![]
            }
            BusEvent::ToolEnd { id, is_error, .. } => {
                self.on_tool_end(id, *is_error);
                vec![]
            }
            BusEvent::TurnEnd(te) => {
                self.on_turn_end(
                    te.turn,
                    te.dominant_phase,
                    te.drift_kind,
                    te.progress_signal,
                    te.actual_input_tokens,
                    te.actual_output_tokens,
                    te.model.as_deref(),
                    te.provider.as_deref(),
                );
                vec![]
            }
            BusEvent::HarnessStatusChanged { status_json } => {
                // Capture harness configuration for creation_context.
                if let Some(obj) = status_json.as_object() {
                    if let Some(s) = obj.get("capability_tier").and_then(|v| v.as_str()) {
                        self.creation_ctx.capability_tier = s.to_string();
                    }
                    if let Some(s) = obj.get("thinking_level").and_then(|v| v.as_str()) {
                        self.creation_ctx.thinking_level = s.to_string();
                    }
                    if let Some(s) = obj.get("context_class").and_then(|v| v.as_str()) {
                        self.creation_ctx.context_class = s.to_string();
                    }
                }
                // Update model from trajectory (more reliable than harness status).
                if !self.trajectory.last_model.is_empty() {
                    self.creation_ctx.model = self.trajectory.last_model.clone();
                }
                vec![]
            }
            BusEvent::SessionEnd { turns, .. } => self.on_session_end(*turns),
            _ => vec![],
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn compute_age_decay(age_days: f64, half_life_days: f64) -> f64 {
    if half_life_days <= 0.0 {
        return 0.0;
    }
    (-std::f64::consts::LN_2 * age_days / half_life_days)
        .exp()
        .clamp(0.0, 1.0)
}

fn estimate_age_days(timestamp: &str) -> f64 {
    // Rough estimate from our chrono_now() format. Not precise, sufficient for decay.
    let now = chrono_now();
    // Compare first 10 chars (YYYY-MM-DD) as a rough approximation.
    // For real precision, parse properly. For age decay, ±1 day doesn't matter.
    let now_date = &now[..now.len().min(10)];
    let ts_date = &timestamp[..timestamp.len().min(10)];
    if now_date == ts_date {
        return 0.0;
    }
    // Very rough: count days between two YYYY-MM-DD strings.
    // Fallback to 7 days if parsing fails.
    7.0
}

fn update_frontmatter_field(content: &str, field: &str, value: &str) -> String {
    let mut result = String::new();
    let prefix = format!("{field} = ");
    for line in content.lines() {
        if line.trim().starts_with(&prefix) {
            result.push_str(&format!("{field} = {value}\n"));
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

fn tool_error(msg: &str) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text {
            text: msg.to_string(),
        }],
        details: Value::Null,
    }
}

fn fxhash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn chrono_now() -> String {
    use std::time::SystemTime;
    let d = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    // ISO 8601 approximation — good enough for file metadata.
    let secs = d.as_secs();
    let days = secs / 86400;
    let rem = secs % 86400;
    let hours = rem / 3600;
    let minutes = (rem % 3600) / 60;
    let seconds = rem % 60;
    // Approximate date from epoch days (not perfectly accurate for display,
    // but sufficient for timestamping files).
    format!(
        "{}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        1970 + days / 365,
        (days % 365) / 30 + 1,
        (days % 365) % 30 + 1,
        hours,
        minutes,
        seconds,
    )
}

fn chrono_date() -> String {
    let now = chrono_now();
    now[..10].to_string()
}

fn slug_from_description(desc: &str) -> String {
    desc.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("-")
}

fn titlecase(s: &str) -> String {
    s.chars()
        .next()
        .map(|c| c.to_uppercase().to_string() + &s[c.len_utf8()..])
        .unwrap_or_default()
}

fn compact_args(args: &Value) -> String {
    if let Some(obj) = args.as_object() {
        obj.iter()
            .map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or(&v.to_string())))
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        args.to_string()
    }
}

fn recovery_description(kind: &RecoveryKind) -> String {
    match kind {
        RecoveryKind::SameToolDifferentArgs => "same tool retried with different arguments".into(),
        RecoveryKind::RetryAfterCodeChange => "code change followed by successful retry".into(),
        RecoveryKind::ToolSwitch { from, to } => {
            format!("tool switch from {from} to {to}")
        }
        RecoveryKind::ConstraintDiscoveryRecovery => "constraint discovery led to recovery".into(),
    }
}

fn recovery_kind_label(kind: &RecoveryKind) -> &'static str {
    match kind {
        RecoveryKind::SameToolDifferentArgs => "SameToolDifferentArgs",
        RecoveryKind::RetryAfterCodeChange => "RetryAfterCodeChange",
        RecoveryKind::ToolSwitch { .. } => "ToolSwitch",
        RecoveryKind::ConstraintDiscoveryRecovery => "ConstraintDiscoveryRecovery",
    }
}

fn suggested_fix_class(kind: &RecoveryKind) -> &'static str {
    match kind {
        RecoveryKind::SameToolDifferentArgs => "arg validation or UX",
        RecoveryKind::ToolSwitch { .. } => "missing capability",
        _ => "domain knowledge",
    }
}

fn derive_tags(seq: &RecoverySequence) -> Vec<String> {
    let mut tags = vec![seq.failure.name.clone()];
    if seq.success.name != seq.failure.name {
        tags.push(seq.success.name.clone());
    }
    if let Some(path) = &seq.failure.target_path
        && let Some(ext) = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
    {
        tags.push(ext.to_string());
    }
    tags
}

fn extract_frontmatter_field(content: &str, field: &str) -> Option<String> {
    // Simple TOML frontmatter extraction between +++ delimiters.
    let rest = content.strip_prefix("+++")?;
    let end = rest.find("+++")?;
    let frontmatter = &rest[..end];
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix(field).and_then(|s| s.strip_prefix(" = ")) {
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}

fn bump_confidence(content: &str) -> String {
    // Find confidence = X.Y and bump by 0.1, cap at 1.0.
    let mut result = String::new();
    for line in content.lines() {
        if line.trim().starts_with("confidence = ")
            && let Some(val_str) = line.trim().strip_prefix("confidence = ")
            && let Ok(val) = val_str.parse::<f32>()
        {
            let new_val = (val + 0.1).min(1.0);
            result.push_str(&format!("confidence = {new_val:.1}\n"));
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }
    result
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_trace(name: &str, path: Option<&str>, is_error: bool) -> ToolCallTrace {
        let mut summary = serde_json::Map::new();
        if let Some(p) = path {
            summary.insert("path".into(), Value::String(p.into()));
        }
        ToolCallTrace {
            call_id: format!("call-{}", fxhash(name) % 1000),
            name: name.into(),
            capabilities: match name {
                "edit" | "write" | "change" => vec![ToolCapability::Mutation],
                _ => vec![],
            },
            args_summary: Value::Object(summary),
            target_path: path.map(String::from),
            is_error,
            completed: true,
        }
    }

    fn make_turn(
        turn: u32,
        tools: Vec<ToolCallTrace>,
        progress: ProgressSignal,
        drift: Option<DriftKind>,
    ) -> TurnSnapshot {
        TurnSnapshot {
            turn,
            phase: Some(OodaPhase::Act),
            drift,
            progress,
            tools,
            input_tokens: 1000,
            output_tokens: 500,
            is_burn: drift.is_some(),
        }
    }

    #[test]
    fn same_tool_different_args_detected() {
        let mut feature = MutationFeature::new(PathBuf::from("/tmp/test-omegon"));
        feature.trajectory.session_id = "test-session".into();
        feature.trajectory.turns = vec![
            make_turn(
                1,
                vec![make_trace("edit", Some("src/lib.rs"), true)],
                ProgressSignal::None,
                Some(DriftKind::RepeatedActionFailure),
            ),
            make_turn(
                2,
                vec![make_trace("read", Some("src/lib.rs"), false)],
                ProgressSignal::None,
                None,
            ),
            make_turn(
                3,
                vec![{
                    let mut t = make_trace("edit", Some("src/lib.rs"), false);
                    // Different args — add a "command" field.
                    t.args_summary
                        .as_object_mut()
                        .unwrap()
                        .insert("command".into(), Value::String("expanded context".into()));
                    t
                }],
                ProgressSignal::Mutation,
                None,
            ),
        ];

        let recoveries = feature.detect_recoveries();
        assert_eq!(recoveries.len(), 1);
        assert!(matches!(
            recoveries[0].kind,
            RecoveryKind::SameToolDifferentArgs
        ));
    }

    #[test]
    fn retry_after_code_change_detected() {
        let mut feature = MutationFeature::new(PathBuf::from("/tmp/test-omegon"));
        feature.trajectory.session_id = "test-session".into();
        feature.trajectory.turns = vec![
            make_turn(
                1,
                vec![make_trace("bash", Some("src/main.rs"), true)],
                ProgressSignal::None,
                None,
            ),
            make_turn(
                2,
                vec![make_trace("edit", Some("src/main.rs"), false)],
                ProgressSignal::Mutation,
                None,
            ),
            make_turn(
                3,
                vec![make_trace("bash", Some("src/main.rs"), false)],
                ProgressSignal::Mutation,
                None,
            ),
        ];

        let recoveries = feature.detect_recoveries();
        assert_eq!(recoveries.len(), 1);
        assert!(matches!(
            recoveries[0].kind,
            RecoveryKind::RetryAfterCodeChange
        ));
    }

    #[test]
    fn tool_switch_detected() {
        let mut feature = MutationFeature::new(PathBuf::from("/tmp/test-omegon"));
        feature.trajectory.session_id = "test-session".into();
        feature.trajectory.turns = vec![
            make_turn(
                1,
                vec![make_trace("edit", Some("src/lib.rs"), true)],
                ProgressSignal::None,
                Some(DriftKind::RepeatedActionFailure),
            ),
            make_turn(
                2,
                vec![make_trace("write", Some("src/lib.rs"), false)],
                ProgressSignal::Mutation,
                None,
            ),
        ];

        let recoveries = feature.detect_recoveries();
        assert_eq!(recoveries.len(), 1);
        assert!(matches!(
            recoveries[0].kind,
            RecoveryKind::ToolSwitch { .. }
        ));
    }

    #[test]
    fn constraint_discovery_recovery_detected() {
        let mut feature = MutationFeature::new(PathBuf::from("/tmp/test-omegon"));
        feature.trajectory.session_id = "test-session".into();
        feature.trajectory.turns = vec![
            make_turn(
                1,
                vec![make_trace("bash", None, false)],
                ProgressSignal::ConstraintDiscovery,
                None,
            ),
            make_turn(
                2,
                vec![make_trace("edit", Some("src/lib.rs"), false)],
                ProgressSignal::Mutation,
                None,
            ),
        ];

        let recoveries = feature.detect_recoveries();
        assert!(
            recoveries
                .iter()
                .any(|r| matches!(r.kind, RecoveryKind::ConstraintDiscoveryRecovery))
        );
    }

    #[test]
    fn burn_ratio_calculation() {
        let mut feature = MutationFeature::new(PathBuf::from("/tmp/test-omegon"));
        feature.trajectory.total_input_tokens = 8000;
        feature.trajectory.total_output_tokens = 2000;
        feature.trajectory.burn_input_tokens = 3000;
        feature.trajectory.burn_output_tokens = 500;

        let burn = feature.compute_burn_metrics(&[]);
        assert!((burn.burn_ratio - 0.35).abs() < 0.01);
        assert!(burn.worth_analyzing);
    }

    #[test]
    fn classification_same_tool_different_args_is_internal() {
        let seq = RecoverySequence {
            start_turn: 1,
            end_turn: 3,
            failure: make_trace("edit", Some("src/lib.rs"), true),
            success: {
                let mut t = make_trace("edit", Some("src/lib.rs"), false);
                t.args_summary
                    .as_object_mut()
                    .unwrap()
                    .insert("command".into(), Value::String("more context".into()));
                t
            },
            kind: RecoveryKind::SameToolDifferentArgs,
            token_cost: 3000,
        };
        let class = MutationFeature::classify(&seq);
        assert!(matches!(class, PatternClass::InternalDeficiency { .. }));
    }

    #[test]
    fn classification_retry_after_code_change_is_domain() {
        let seq = RecoverySequence {
            start_turn: 1,
            end_turn: 3,
            failure: make_trace("bash", Some("src/main.rs"), true),
            success: make_trace("bash", Some("src/main.rs"), false),
            kind: RecoveryKind::RetryAfterCodeChange,
            token_cost: 5000,
        };
        let class = MutationFeature::classify(&seq);
        assert!(matches!(class, PatternClass::DomainPattern { .. }));
    }

    #[test]
    fn slug_generation() {
        assert_eq!(
            slug_from_description("Domain constraint required code change before bash succeeded"),
            "domain-constraint-required-code-change-before"
        );
    }

    #[test]
    fn owning_crate_lookup() {
        assert_eq!(owning_crate("edit"), "omegon (core)");
        assert_eq!(owning_crate("memory_store"), "omegon-memory");
        assert_eq!(owning_crate("cleave_run"), "omegon (cleave)");
        assert_eq!(owning_crate("some_extension_tool"), "extension (unknown)");
    }

    #[test]
    fn impact_config_defaults_are_sane() {
        let cfg = ImpactConfig::default();
        assert!(cfg.weights.component_score_delta > cfg.weights.age_decay);
        assert!((cfg.learning.neutral_point - 0.5).abs() < f64::EPSILON);
        assert!(cfg.confidence.floor < cfg.confidence.ceiling);
        assert!(cfg.confidence.auto_archive_threshold > cfg.confidence.floor);
        assert!(!cfg.telemetry.share_impact_data);
    }

    #[test]
    fn burn_delta_computation() {
        let feature = MutationFeature::new(PathBuf::from("/tmp/test-omegon"));
        let entries = vec![
            BurnLogEntry {
                session_id: "s1".into(),
                timestamp: "2026-04-20".into(),
                turns: 10,
                total_tokens: 10000,
                burn_tokens: 3000,
                burn_ratio: 0.3,
                recoveries: 1,
                skills_created: 0,
                diagnostics_created: 0,
                active_learned_skills: vec!["my-skill".into()],
                active_diagnostics: vec![],
            },
            BurnLogEntry {
                session_id: "s2".into(),
                timestamp: "2026-04-21".into(),
                turns: 12,
                total_tokens: 12000,
                burn_tokens: 6000,
                burn_ratio: 0.5,
                recoveries: 2,
                skills_created: 0,
                diagnostics_created: 0,
                active_learned_skills: vec![],
                active_diagnostics: vec![],
            },
        ];

        let delta = feature.compute_burn_delta_for_skill("my-skill", &entries);
        assert!(delta.is_some());
        let d = delta.unwrap();
        assert!(d > 0.5, "delta should be positive (skill helped): {d}");
    }

    #[test]
    fn usage_frequency_computation() {
        let feature = MutationFeature::new(PathBuf::from("/tmp/test-omegon"));
        let entries = vec![
            BurnLogEntry {
                session_id: "s1".into(),
                timestamp: "t".into(),
                turns: 5,
                total_tokens: 0,
                burn_tokens: 0,
                burn_ratio: 0.0,
                recoveries: 0,
                skills_created: 0,
                diagnostics_created: 0,
                active_learned_skills: vec!["skill-a".into()],
                active_diagnostics: vec![],
            },
            BurnLogEntry {
                session_id: "s2".into(),
                timestamp: "t".into(),
                turns: 5,
                total_tokens: 0,
                burn_tokens: 0,
                burn_ratio: 0.0,
                recoveries: 0,
                skills_created: 0,
                diagnostics_created: 0,
                active_learned_skills: vec![],
                active_diagnostics: vec![],
            },
            BurnLogEntry {
                session_id: "s3".into(),
                timestamp: "t".into(),
                turns: 5,
                total_tokens: 0,
                burn_tokens: 0,
                burn_ratio: 0.0,
                recoveries: 0,
                skills_created: 0,
                diagnostics_created: 0,
                active_learned_skills: vec!["skill-a".into()],
                active_diagnostics: vec![],
            },
        ];

        let freq = feature.compute_usage_frequency("skill-a", &entries);
        assert!((freq - 2.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn age_decay_at_half_life() {
        let decay = compute_age_decay(30.0, 30.0);
        assert!(
            (decay - 0.5).abs() < 0.1,
            "at half-life, decay should be ~0.5: {decay}"
        );
    }

    #[test]
    fn age_decay_at_zero() {
        let decay = compute_age_decay(0.0, 30.0);
        assert!(
            (decay - 1.0).abs() < 0.01,
            "at age 0, decay should be ~1.0: {decay}"
        );
    }

    #[test]
    fn update_frontmatter_field_works() {
        let content = "+++\nconfidence = 0.7\nname = \"test\"\n+++\n# Body\n";
        let updated = update_frontmatter_field(content, "confidence", "0.85");
        assert!(updated.contains("confidence = 0.85"));
        assert!(updated.contains("name = \"test\""));
    }

    // ── Integration tests ───────────────────────────────────────────

    #[test]
    fn on_session_end_below_min_turns_produces_nothing() {
        let mut feature = MutationFeature::new(PathBuf::from("/tmp/test-omegon-session"));
        feature.trajectory.session_id = "s1".into();
        // 3 turns, below default min_turns_for_analysis (8)
        feature.trajectory.turns = vec![
            make_turn(1, vec![], ProgressSignal::None, None),
            make_turn(2, vec![], ProgressSignal::None, None),
            make_turn(3, vec![], ProgressSignal::Mutation, None),
        ];
        let requests = feature.on_session_end(3);
        assert!(requests.is_empty(), "should not analyze short sessions");
    }

    #[test]
    fn on_session_end_logs_burn_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut feature = MutationFeature::new(dir.path().to_path_buf());
        feature.trajectory.session_id = "s1".into();
        feature.trajectory.total_input_tokens = 5000;
        feature.trajectory.total_output_tokens = 2000;
        feature.trajectory.burn_input_tokens = 1500;
        feature.trajectory.burn_output_tokens = 500;
        // 10 turns to pass min threshold
        feature.trajectory.turns = (1..=10)
            .map(|i| make_turn(i, vec![], ProgressSignal::Mutation, None))
            .collect();

        let _requests = feature.on_session_end(10);

        let burn_path = dir.path().join("mutation/burn-history.jsonl");
        assert!(burn_path.exists(), "burn history should be written");
        let content = std::fs::read_to_string(&burn_path).unwrap();
        assert!(content.contains("\"session_id\":\"s1\""));
        assert!(content.contains("\"turns\":10"));
    }

    #[test]
    fn on_session_end_no_artifacts_when_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let mut feature = MutationFeature::new(dir.path().to_path_buf());
        feature.trajectory.session_id = "s1".into();
        // generate_artifacts is false by default
        assert!(!feature.impact_config.behavior.generate_artifacts);

        // Build a trajectory with a clear recovery pattern
        feature.trajectory.turns = vec![
            make_turn(
                1,
                vec![make_trace("edit", Some("src/lib.rs"), true)],
                ProgressSignal::None,
                Some(DriftKind::RepeatedActionFailure),
            ),
            make_turn(
                2,
                vec![make_trace("read", Some("src/lib.rs"), false)],
                ProgressSignal::None,
                None,
            ),
            make_turn(
                3,
                vec![{
                    let mut t = make_trace("edit", Some("src/lib.rs"), false);
                    t.args_summary
                        .as_object_mut()
                        .unwrap()
                        .insert("command".into(), Value::String("expanded".into()));
                    t
                }],
                ProgressSignal::Mutation,
                None,
            ),
        ];
        // Pad to 10 turns
        for i in 4..=10 {
            feature
                .trajectory
                .turns
                .push(make_turn(i, vec![], ProgressSignal::None, None));
        }

        let requests = feature.on_session_end(10);

        // Burn history is logged regardless
        let burn_path = dir.path().join("mutation/burn-history.jsonl");
        assert!(burn_path.exists());

        // But no skill or diagnostic files created
        let skills_dir = dir.path().join("skills/learned");
        let diag_dir = dir.path().join("diagnostics");
        assert!(!skills_dir.exists() || std::fs::read_dir(&skills_dir).unwrap().count() == 0);
        assert!(!diag_dir.exists() || std::fs::read_dir(&diag_dir).unwrap().count() == 0);

        // No AutoStoreFact requests
        assert!(
            !requests
                .iter()
                .any(|r| matches!(r, BusRequest::AutoStoreFact { .. })),
            "should not create artifacts when generate_artifacts is disabled"
        );
    }

    #[test]
    fn trajectory_accumulation_via_events() {
        let mut feature = MutationFeature::new(PathBuf::from("/tmp/test-omegon-accum"));

        // Simulate the event sequence the bus would deliver
        feature.on_event(&BusEvent::SessionStart {
            cwd: PathBuf::from("/tmp"),
            session_id: "test-accum".into(),
        });

        feature.on_event(&BusEvent::ToolStart {
            id: "c1".into(),
            name: "read".into(),
            args: serde_json::json!({"path": "src/main.rs"}),
            capabilities: vec![ToolCapability::RepoInspection],
        });
        feature.on_event(&BusEvent::ToolEnd {
            id: "c1".into(),
            name: "read".into(),
            result: ToolResult {
                content: vec![ContentBlock::Text {
                    text: "file content".into(),
                }],
                details: Value::Null,
            },
            is_error: false,
        });
        feature.on_event(&BusEvent::TurnEnd(Box::new(
            omegon_traits::BusEventTurnEnd {
                turn: 1,
                model: Some("anthropic:claude-sonnet-4-6".into()),
                provider: Some("anthropic".into()),
                estimated_tokens: 5000,
                context_window: 200_000,
                context_composition: omegon_traits::ContextComposition::default(),
                actual_input_tokens: 1200,
                actual_output_tokens: 300,
                cache_read_tokens: 0,
                provider_telemetry: None,
                dominant_phase: Some(OodaPhase::Observe),
                drift_kind: None,
                progress_signal: ProgressSignal::None,
            },
        )));

        assert_eq!(feature.trajectory.session_id, "test-accum");
        assert_eq!(feature.trajectory.turns.len(), 1);
        assert_eq!(feature.trajectory.turns[0].tools.len(), 1);
        assert_eq!(feature.trajectory.turns[0].tools[0].name, "read");
        assert!(!feature.trajectory.turns[0].tools[0].is_error);
        assert_eq!(feature.trajectory.total_input_tokens, 1200);
        assert_eq!(feature.trajectory.total_output_tokens, 300);
        assert_eq!(feature.trajectory.last_model, "anthropic:claude-sonnet-4-6");
    }

    #[test]
    fn trajectory_records_capabilities_from_tool_start() {
        let mut feature = MutationFeature::new(PathBuf::from("/tmp/test-omegon-caps"));
        feature.on_event(&BusEvent::SessionStart {
            cwd: PathBuf::from("/tmp"),
            session_id: "test-caps".into(),
        });

        feature.on_event(&BusEvent::ToolStart {
            id: "c1".into(),
            name: "surgical_patch".into(),
            args: serde_json::json!({"path": "src/main.rs"}),
            capabilities: vec![ToolCapability::Mutation, ToolCapability::StateChanging],
        });

        let trace = feature
            .trajectory
            .pending_tools
            .first()
            .expect("pending tool trace");
        assert_eq!(trace.name, "surgical_patch");
        assert!(trace_is_mutation_tool(trace));
    }

    #[test]
    fn slash_command_stats_returns_display() {
        let mut feature = MutationFeature::new(PathBuf::from("/tmp/test-omegon-cmd"));
        let result = feature.handle_command("mutation", "stats");
        assert!(matches!(result, omegon_traits::CommandResult::Display(_)));
        if let omegon_traits::CommandResult::Display(text) = result {
            assert!(text.contains("Current Session"));
            assert!(text.contains("Configuration"));
        }
    }

    #[test]
    fn slash_command_config_shows_parameters() {
        let mut feature = MutationFeature::new(PathBuf::from("/tmp/test-omegon-cfg"));
        let result = feature.handle_command("mutation", "config");
        assert!(matches!(result, omegon_traits::CommandResult::Display(_)));
        if let omegon_traits::CommandResult::Display(text) = result {
            assert!(text.contains("Signal Weights"));
            assert!(text.contains("Learning"));
            assert!(text.contains("Artifact generation"));
            assert!(text.contains("impact.toml"));
        }
    }

    #[test]
    fn slash_command_unknown_subcommand() {
        let mut feature = MutationFeature::new(PathBuf::from("/tmp/test-omegon-unk"));
        let result = feature.handle_command("mutation", "bogus");
        if let omegon_traits::CommandResult::Display(text) = result {
            assert!(text.contains("Unknown subcommand"));
        }
    }

    #[test]
    fn slash_command_wrong_name_not_handled() {
        let mut feature = MutationFeature::new(PathBuf::from("/tmp/test-omegon-wrong"));
        let result = feature.handle_command("usage", "");
        assert!(matches!(result, omegon_traits::CommandResult::NotHandled));
    }

    #[test]
    fn escalation_score_does_not_fire_below_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let feature = MutationFeature::new(dir.path().to_path_buf());
        // No diagnostics dir = no escalation
        let escalated = feature.check_diagnostic_escalation();
        assert!(escalated.is_empty());
    }
}
