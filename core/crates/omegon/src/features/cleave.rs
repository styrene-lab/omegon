//! Cleave Feature — task decomposition and parallel dispatch.
//!
//! Provides:
//! - Tool: `cleave_assess` — fast-path complexity assessment
//! - Tool: `cleave_run` — execute a cleave plan (spawn children, merge)
//! - Command: `/cleave` — trigger decomposition from TUI
//! - Dashboard state: live child progress during runs
//!
//! The orchestrator runs async in a spawned task. Progress events are
//! collected and surfaced through the dashboard and conversation segments.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};

use omegon_traits::{
    BusEvent, BusRequest, CommandDefinition, CommandResult,
    Feature, ToolDefinition, ToolResult, ContentBlock,
};

use crate::cleave::{self, CleavePlan, state::ChildStatus};

// ═══════════════════════════════════════════════════════════════════════════
// Complexity assessment — pure pattern matching
// ═══════════════════════════════════════════════════════════════════════════

/// Known patterns for fast-path assessment.
struct Pattern {
    id: &'static str,
    label: &'static str,
    keywords: &'static [&'static str],
    systems: u8,
}

const PATTERNS: &[Pattern] = &[
    Pattern { id: "crud-api", label: "CRUD / API Endpoint", keywords: &["endpoint", "api", "handler", "route", "crud", "rest"], systems: 2 },
    Pattern { id: "data-pipeline", label: "Data Pipeline / ETL", keywords: &["pipeline", "etl", "transform", "ingest", "export"], systems: 3 },
    Pattern { id: "ui-feature", label: "UI Feature / Component", keywords: &["component", "widget", "view", "form", "dialog", "panel", "ui"], systems: 2 },
    Pattern { id: "refactor", label: "Refactor / Rename", keywords: &["refactor", "rename", "extract", "inline", "dedup", "consolidat"], systems: 1 },
    Pattern { id: "infra-tooling", label: "Infrastructure & Tooling", keywords: &["ci", "cd", "docker", "deploy", "container", "workflow", "script", "tool", "config", "lint", "format"], systems: 1 },
    Pattern { id: "auth-security", label: "Auth / Security", keywords: &["auth", "login", "permission", "rbac", "oauth", "token", "secret", "encrypt"], systems: 3 },
    Pattern { id: "multi-service", label: "Multi-Service Integration", keywords: &["service", "microservice", "grpc", "queue", "message", "event-driven", "kafka", "nats"], systems: 4 },
    Pattern { id: "migration", label: "Data Migration / Schema Change", keywords: &["migration", "schema", "alter", "migrate", "upgrade", "backward"], systems: 2 },
    Pattern { id: "test-coverage", label: "Test Coverage / Quality", keywords: &["test", "coverage", "spec", "assert", "mock", "fixture"], systems: 1 },
    Pattern { id: "cross-cutting", label: "Cross-Cutting Concern", keywords: &["logging", "tracing", "metrics", "telemetry", "i18n", "l10n", "error-handling"], systems: 3 },
];

/// Modifiers that increase complexity.
const MODIFIERS: &[(&str, &[&str])] = &[
    ("validation", &["validate", "constraint", "schema", "boundary"]),
    ("backward-compat", &["backward", "compatible", "deprecat", "legacy"]),
    ("multi-platform", &["platform", "cross-platform", "os-specific", "arch"]),
    ("performance", &["performance", "benchmark", "optimize", "cache", "latency"]),
    ("concurrent", &["concurrent", "parallel", "async", "thread", "lock", "mutex"]),
];

fn assess_directive(directive: &str, threshold: f64) -> Value {
    let lower = directive.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();

    // Find best matching pattern — use word-boundary matching:
    // a keyword matches if it equals a word OR the word starts with it
    // (catches "refactoring" for "refactor") but not substring matches
    // (avoids "stool" matching "tool", "build" matching "ui").
    let word_matches = |word: &str, kw: &str| -> bool {
        word == kw || word.starts_with(kw) && word.len() <= kw.len() + 4
    };

    let mut best: Option<(&Pattern, f64)> = None;
    for pattern in PATTERNS {
        let matches = pattern.keywords.iter()
            .filter(|kw| words.iter().any(|w| word_matches(w, kw)))
            .count();
        if matches > 0 {
            let confidence = (matches as f64 / pattern.keywords.len() as f64).min(1.0);
            if best.is_none() || confidence > best.unwrap().1 {
                best = Some((pattern, confidence));
            }
        }
    }

    // Count modifiers (same word-boundary matching)
    let active_modifiers: Vec<&str> = MODIFIERS.iter()
        .filter(|(_, kws)| kws.iter().any(|kw| words.iter().any(|w| word_matches(w, kw))))
        .map(|(name, _)| *name)
        .collect();

    let (systems, pattern_label, pattern_id, confidence) = if let Some((p, conf)) = best {
        (p.systems as f64, p.label, p.id, conf)
    } else {
        (1.0, "Unknown", "unknown", 0.0)
    };

    let modifier_count = active_modifiers.len() as f64;
    let complexity = systems * (1.0 + 0.5 * modifier_count);
    let effective = complexity + 1.0; // +1 for validation offset

    let decision = if effective > threshold { "cleave" } else { "execute" };

    json!({
        "decision": decision,
        "complexity": complexity,
        "systems": systems as u8,
        "modifiers": active_modifiers,
        "method": if confidence > 0.0 { "fast-path" } else { "needs_assessment" },
        "pattern": format!("{} ({}%)", pattern_label, (confidence * 100.0) as u8),
        "pattern_id": pattern_id,
        "confidence": confidence,
        "threshold": threshold,
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// Live progress tracking
// ═══════════════════════════════════════════════════════════════════════════

/// Live progress of an active cleave run, for dashboard rendering.
#[derive(Default, Clone)]
pub struct CleaveProgress {
    pub active: bool,
    pub run_id: String,
    pub total_children: usize,
    pub completed: usize,
    pub failed: usize,
    pub children: Vec<ChildProgress>,
}

#[derive(Clone)]
pub struct ChildProgress {
    pub label: String,
    pub status: String, // "pending", "running", "completed", "failed"
    pub duration_secs: Option<f64>,
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature implementation
// ═══════════════════════════════════════════════════════════════════════════

pub struct CleaveFeature {
    repo_path: PathBuf,
    /// Shared progress state — updated by the spawned orchestrator task,
    /// read by the dashboard renderer.
    progress: Arc<Mutex<CleaveProgress>>,
}

impl CleaveFeature {
    pub fn new(repo_path: &std::path::Path) -> Self {
        Self {
            repo_path: repo_path.to_path_buf(),
            progress: Arc::new(Mutex::new(CleaveProgress::default())),
        }
    }

    /// Get a clone of the current progress for dashboard rendering.
    pub fn progress(&self) -> CleaveProgress {
        self.progress.lock().unwrap().clone()
    }

    /// Get a shared handle to the progress for live dashboard updates.
    pub fn shared_progress(&self) -> Arc<Mutex<CleaveProgress>> {
        Arc::clone(&self.progress)
    }

    fn execute_assess(&self, args: &Value) -> anyhow::Result<ToolResult> {
        let directive = args["directive"].as_str()
            .ok_or_else(|| anyhow::anyhow!("directive required"))?;
        let threshold = args["threshold"].as_f64().unwrap_or(2.0);

        let assessment = assess_directive(directive, threshold);
        Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: serde_json::to_string_pretty(&assessment)?,
            }],
            details: assessment,
        })
    }

    async fn execute_run(
        &self,
        args: &Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let directive = args["directive"].as_str()
            .ok_or_else(|| anyhow::anyhow!("directive required"))?;
        let plan_json = args["plan_json"].as_str()
            .ok_or_else(|| anyhow::anyhow!("plan_json required"))?;
        let max_parallel = args["max_parallel"].as_u64().unwrap_or(4) as usize;

        let plan = CleavePlan::from_json(plan_json)?;

        // Create workspace directory
        let workspace = self.repo_path.join(".omegon/cleave-workspace");
        std::fs::create_dir_all(&workspace)?;

        // Resolve agent binary
        let agent_binary = std::env::current_exe()?;

        // Initialize progress tracking
        {
            let mut prog = self.progress.lock().unwrap();
            prog.active = true;
            prog.total_children = plan.children.len();
            prog.completed = 0;
            prog.failed = 0;
            prog.children = plan.children.iter().map(|c| ChildProgress {
                label: c.label.clone(),
                status: "pending".into(),
                duration_secs: None,
            }).collect();
        }

        let config = cleave::orchestrator::CleaveConfig {
            agent_binary,
            bridge_path: PathBuf::new(), // Not used in native mode
            node: String::new(),
            model: std::env::var("OMEGON_MODEL").unwrap_or_else(|_| "anthropic:claude-sonnet-4-6".into()),
            max_parallel,
            timeout_secs: 900,
            idle_timeout_secs: 180,
            max_turns: 50,
        };

        let result = cleave::run_cleave(
            &plan, directive, &self.repo_path, &workspace, &config, cancel,
        ).await?;

        // Update progress to final state
        {
            let mut prog = self.progress.lock().unwrap();
            prog.active = false;
            prog.completed = result.state.children.iter()
                .filter(|c| c.status == ChildStatus::Completed).count();
            prog.failed = result.state.children.iter()
                .filter(|c| c.status == ChildStatus::Failed).count();
            for (i, child) in result.state.children.iter().enumerate() {
                if let Some(p) = prog.children.get_mut(i) {
                    p.status = match child.status {
                        ChildStatus::Completed => "completed".into(),
                        ChildStatus::Failed => "failed".into(),
                        ChildStatus::Running => "running".into(),
                        ChildStatus::Pending => "pending".into(),
                    };
                    p.duration_secs = child.duration_secs;
                }
            }
        }

        // Build report
        let completed = result.state.children.iter()
            .filter(|c| c.status == ChildStatus::Completed).count();
        let failed = result.state.children.iter()
            .filter(|c| c.status == ChildStatus::Failed).count();

        let mut report = format!(
            "## Cleave Report: {}\n**Duration:** {:.0}s\n**Children:** {} completed, {} failed of {}\n\n",
            result.state.run_id, result.duration_secs, completed, failed, result.state.children.len()
        );

        for child in &result.state.children {
            let icon = match child.status {
                ChildStatus::Completed => "✓",
                ChildStatus::Failed => "✗",
                ChildStatus::Running => "⏳",
                ChildStatus::Pending => "○",
            };
            let dur = child.duration_secs.map(|d| format!(" ({:.0}s)", d)).unwrap_or_default();
            report.push_str(&format!("  {} **{}**{}\n", icon, child.label, dur));
            if let Some(err) = &child.error {
                report.push_str(&format!("    Error: {}\n", err));
            }
        }

        report.push_str("\n### Merge Results\n");
        for (label, outcome) in &result.merge_results {
            match outcome {
                cleave::orchestrator::MergeOutcome::Success => {
                    report.push_str(&format!("  ✓ {} merged\n", label));
                }
                cleave::orchestrator::MergeOutcome::Conflict(d) => {
                    report.push_str(&format!("  ✗ {} CONFLICT: {}\n", label, d.lines().next().unwrap_or("")));
                }
                cleave::orchestrator::MergeOutcome::Failed(d) => {
                    report.push_str(&format!("  ✗ {} FAILED: {}\n", label, d.lines().next().unwrap_or("")));
                }
                cleave::orchestrator::MergeOutcome::Skipped(d) => {
                    report.push_str(&format!("  ○ {} skipped: {}\n", label, d));
                }
            }
        }

        Ok(ToolResult {
            content: vec![ContentBlock::Text { text: report }],
            details: json!({
                "run_id": result.state.run_id,
                "completed": completed,
                "failed": failed,
                "total": result.state.children.len(),
                "duration_secs": result.duration_secs,
                "merged": result.merge_results.iter()
                    .filter(|(_, o)| matches!(o, cleave::orchestrator::MergeOutcome::Success))
                    .count(),
            }),
        })
    }
}

#[async_trait]
impl Feature for CleaveFeature {
    fn name(&self) -> &str {
        "cleave"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "cleave_assess".into(),
                label: "cleave_assess".into(),
                description: "Assess the complexity of a task directive to determine if it should be decomposed. Returns complexity score, matched pattern, and decision (execute/cleave).".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "directive": {
                            "type": "string",
                            "description": "The task directive to assess"
                        },
                        "threshold": {
                            "type": "number",
                            "description": "Complexity threshold (default: 2.0)"
                        }
                    },
                    "required": ["directive"]
                }),
            },
            ToolDefinition {
                name: "cleave_run".into(),
                label: "cleave_run".into(),
                description: "Execute a cleave decomposition plan. Creates git worktrees for each child, dispatches child processes, harvests results, and merges branches back.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "directive": {
                            "type": "string",
                            "description": "The original task directive"
                        },
                        "plan_json": {
                            "type": "string",
                            "description": "JSON string of the split plan: {\"children\": [{\"label\": \"...\", \"description\": \"...\", \"scope\": [...], \"depends_on\": [...]}]}"
                        },
                        "max_parallel": {
                            "type": "number",
                            "description": "Maximum parallel children (default: 4)"
                        }
                    },
                    "required": ["directive", "plan_json"]
                }),
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        match tool_name {
            "cleave_assess" => self.execute_assess(&args),
            "cleave_run" => self.execute_run(&args, cancel).await,
            _ => anyhow::bail!("Unknown tool: {tool_name}"),
        }
    }

    fn commands(&self) -> Vec<CommandDefinition> {
        vec![
            CommandDefinition {
                name: "cleave".into(),
                description: "Show cleave status or trigger decomposition".into(),
                subcommands: vec!["status".into()],
            },
        ]
    }

    fn handle_command(&mut self, name: &str, args: &str) -> CommandResult {
        match name {
            "cleave" => {
                let sub = args.trim();
                let prog = self.progress.lock().unwrap();
                if sub == "status" || sub.is_empty() {
                    if !prog.active && prog.total_children == 0 {
                        return CommandResult::Display("No active cleave run.".into());
                    }
                    let mut lines = Vec::new();
                    if prog.active {
                        lines.push(format!("Cleave active: {}/{} children", prog.completed + prog.failed, prog.total_children));
                    } else {
                        lines.push(format!("Last cleave: {} completed, {} failed of {}", prog.completed, prog.failed, prog.total_children));
                    }
                    for child in &prog.children {
                        let icon = match child.status.as_str() {
                            "completed" => "✓",
                            "failed" => "✗",
                            "running" => "⏳",
                            _ => "○",
                        };
                        let dur = child.duration_secs.map(|d| format!(" ({:.0}s)", d)).unwrap_or_default();
                        lines.push(format!("  {} {}{}", icon, child.label, dur));
                    }
                    CommandResult::Display(lines.join("\n"))
                } else {
                    CommandResult::Display("Usage: /cleave [status]".into())
                }
            }
            _ => CommandResult::NotHandled,
        }
    }

    fn on_event(&mut self, _event: &BusEvent) -> Vec<BusRequest> {
        vec![]
    }
}

fn text_result(text: &str) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text { text: text.to_string() }],
        details: json!(null),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assess_simple_directive() {
        let result = assess_directive("Refactor the utils module to extract helpers", 2.0);
        assert_eq!(result["decision"], "execute", "simple refactor should be execute: {result}");
        assert!(result["complexity"].as_f64().unwrap() >= 1.0);
    }

    #[test]
    fn assess_complex_directive() {
        let result = assess_directive(
            "Build a multi-service integration with gRPC, authentication, and backward compatibility for legacy clients with concurrent processing",
            2.0,
        );
        assert_eq!(result["decision"], "cleave");
        assert!(result["complexity"].as_f64().unwrap() >= 3.0);
    }

    #[test]
    fn assess_unknown_pattern() {
        let result = assess_directive("do something vague", 2.0);
        assert_eq!(result["method"], "needs_assessment");
    }

    #[test]
    fn assess_with_modifiers() {
        let result = assess_directive("Deploy a containerized service with performance optimization and backward compatibility", 2.0);
        let mods = result["modifiers"].as_array().unwrap();
        assert!(!mods.is_empty(), "should detect modifiers");
    }

    #[test]
    fn feature_provides_tools() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path());
        let tools = feature.tools();
        assert_eq!(tools.len(), 2);
        assert!(tools.iter().any(|t| t.name == "cleave_assess"));
        assert!(tools.iter().any(|t| t.name == "cleave_run"));
    }

    #[test]
    fn cleave_status_no_active_run() {
        let dir = tempfile::tempdir().unwrap();
        let mut feature = CleaveFeature::new(dir.path());
        let result = feature.handle_command("cleave", "status");
        assert!(matches!(result, CommandResult::Display(ref s) if s.contains("No active")));
    }

    #[test]
    fn progress_default_inactive() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path());
        let prog = feature.progress();
        assert!(!prog.active);
        assert_eq!(prog.total_children, 0);
    }

    #[tokio::test]
    async fn assess_tool_execution() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path());
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature.execute(
            "cleave_assess", "tc1",
            json!({"directive": "Refactor the auth module", "threshold": 2.0}),
            cancel,
        ).await.unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("decision"), "should return assessment: {text}");
    }
}

#[cfg(test)]
mod assessment_tests {
    use super::*;

    #[test]
    fn ui_component_matches() {
        let r = assess_directive("Build a dialog component for settings", 2.0);
        assert_eq!(r["pattern_id"], "ui-feature");
    }

    #[test]
    fn auth_matches() {
        let r = assess_directive("Add OAuth token refresh with encryption", 2.0);
        assert_eq!(r["pattern_id"], "auth-security");
        assert_eq!(r["decision"], "cleave"); // systems=3 + modifier
    }

    #[test]
    fn test_coverage_is_simple() {
        let r = assess_directive("Add unit test fixtures for the parser", 2.0);
        assert_eq!(r["pattern_id"], "test-coverage");
        assert_eq!(r["decision"], "execute"); // systems=1
    }

    #[test]
    fn multi_service_is_complex() {
        let r = assess_directive("Integrate the gRPC service with the message queue", 2.0);
        assert_eq!(r["pattern_id"], "multi-service");
        assert_eq!(r["decision"], "cleave"); // systems=4
    }

    #[test]
    fn no_keywords_returns_needs_assessment() {
        let r = assess_directive("make it better", 2.0);
        assert_eq!(r["method"], "needs_assessment");
        assert_eq!(r["decision"], "execute");
    }

    #[test]
    fn all_modifiers_stack() {
        let r = assess_directive(
            "concurrent performance optimization with backward compatibility for cross-platform validation",
            100.0  // High threshold so we can just check complexity
        );
        let mods = r["modifiers"].as_array().unwrap();
        assert!(mods.len() >= 3, "should detect multiple modifiers: {mods:?}");
        assert!(r["complexity"].as_f64().unwrap() > 1.0);
    }

    #[test]
    fn custom_threshold() {
        let r = assess_directive("simple refactor extract helpers", 100.0);
        assert_eq!(r["decision"], "execute", "high threshold should always execute");

        let r = assess_directive("simple refactor extract helpers", 0.5);
        assert_eq!(r["decision"], "cleave", "low threshold should always cleave");
    }

    #[test]
    fn confidence_between_0_and_1() {
        let r = assess_directive("Deploy a containerized service", 2.0);
        let conf = r["confidence"].as_f64().unwrap();
        assert!(conf > 0.0 && conf <= 1.0, "confidence should be (0,1]: {conf}");
    }
}
