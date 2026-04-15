//! Agent evaluation framework — score agent bundles against test scenarios.
//!
//! Usage: `omegon eval --agent <id> --suite <path>`
//!
//! The harness spawns the agent as a daemon, feeds it test scenarios via
//! the HTTP event API, collects results, and runs scorers to produce a
//! score card.

pub mod scenario;
pub mod scorer;
pub mod report;

use std::path::Path;

use scenario::{EvalSuite, Scenario};
use scorer::ScorerResult;
use report::{ComponentMatrix, ComponentVersion, ScenarioResult, ScoreCard};

/// Run an eval suite against an agent bundle.
/// If `model_override` is set, the component matrix records it instead of
/// the manifest's default model — useful for testing model portability.
pub async fn run_suite(
    agent_id: &str,
    suite_path: &Path,
    model_override: Option<&str>,
) -> anyhow::Result<ScoreCard> {
    let suite = EvalSuite::load(suite_path)?;
    tracing::info!(
        suite = %suite.suite.name,
        scenarios = suite.scenarios.len(),
        "starting eval suite"
    );

    // Build component matrix from agent manifest (if resolvable).
    let mut components = build_component_matrix(agent_id);
    if let Some(model) = model_override {
        components.model = model.to_string();
    }

    let mut results = Vec::new();

    for scenario_ref in &suite.scenarios {
        let scenario_path = suite_path.parent().unwrap_or(Path::new(".")).join(&scenario_ref.path);
        let scenario = Scenario::load(&scenario_path)?;

        tracing::info!(
            scenario = %scenario.scenario.name,
            difficulty = scenario.scenario.difficulty,
            "running scenario"
        );

        let result = run_scenario(agent_id, &scenario).await;
        match result {
            Ok(r) => {
                tracing::info!(
                    scenario = %scenario.scenario.name,
                    score = r.weighted_score,
                    passed = r.passed,
                    turns = r.turns,
                    "scenario complete"
                );
                results.push(r);
            }
            Err(e) => {
                tracing::error!(
                    scenario = %scenario.scenario.name,
                    error = %e,
                    "scenario failed"
                );
                results.push(ScenarioResult {
                    name: scenario.scenario.name.clone(),
                    difficulty: scenario.scenario.difficulty,
                    scores: std::collections::HashMap::new(),
                    weighted_score: 0.0,
                    turns: 0,
                    tokens: 0,
                    duration_secs: 0.0,
                    passed: false,
                    error: Some(e.to_string()),
                    tests_component: Vec::new(),
                });
            }
        }
    }

    Ok(ScoreCard::from_results(agent_id, &suite.suite.name, components, results))
}

/// Build the component matrix from the agent manifest.
fn build_component_matrix(agent_id: &str) -> ComponentMatrix {
    let cwd = std::env::current_dir().unwrap_or_default();
    let omegon_home = crate::paths::omegon_home().unwrap_or_else(|_| cwd.join(".omegon"));

    // Try to load the agent manifest for component info.
    let manifest = crate::catalog::resolve(&omegon_home, agent_id).ok();

    let mut matrix = ComponentMatrix {
        omegon_version: env!("CARGO_PKG_VERSION").to_string(),
        ..Default::default()
    };

    if let Some(ref resolved) = manifest {
        let m = &resolved.manifest;
        matrix.agent_version = m.agent.version.clone();
        matrix.domain = m.agent.domain.clone();

        if let Some(ref s) = m.settings {
            matrix.model = s.model.clone().unwrap_or_default();
            matrix.thinking_level = s.thinking_level.clone().unwrap_or_else(|| "medium".into());
            matrix.context_class = s.context_class.clone().unwrap_or_else(|| "squad".into());
            matrix.max_turns = s.max_turns.unwrap_or(50);
        }

        if m.persona.is_some() {
            matrix.persona = Some(m.agent.name.clone());
        }

        if let Some(ref exts) = m.extensions {
            matrix.extensions = exts
                .iter()
                .map(|e| ComponentVersion {
                    name: e.name.clone(),
                    version: e.version.clone(),
                })
                .collect();
        }

        if let Some(ref triggers) = m.triggers {
            matrix.triggers = triggers.iter().map(|t| t.name.clone()).collect();
        }

        if let Some(ref wf) = m.workflow {
            matrix.workflow = Some(wf.name.clone());
        }

        if let Some(ref persona_cfg) = m.persona {
            if let Some(ref skills) = persona_cfg.activated_skills {
                matrix.skills = skills.clone();
            }
        }
    }

    // Scan installed plugins for additional context.
    let plugin_dir = omegon_home.join("plugins");
    if plugin_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&plugin_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.join("plugin.toml").exists() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    matrix.plugins.push(ComponentVersion {
                        name,
                        version: "installed".into(),
                    });
                }
            }
        }
    }

    // Scan installed extensions.
    let ext_dir = omegon_home.join("extensions");
    if ext_dir.is_dir() && matrix.extensions.is_empty() {
        if let Ok(entries) = std::fs::read_dir(&ext_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.join("manifest.toml").exists() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    matrix.extensions.push(ComponentVersion {
                        name,
                        version: "installed".into(),
                    });
                }
            }
        }
    }

    matrix
}

/// Run a single scenario. In this initial implementation, we run the
/// scenario in-process (no daemon spawn) by evaluating the scoring
/// rules against simulated/provided outputs. Full daemon integration
/// comes in a follow-up.
async fn run_scenario(
    _agent_id: &str,
    scenario: &Scenario,
) -> anyhow::Result<ScenarioResult> {
    let start = std::time::Instant::now();

    // For now, score the scenario structure itself (validation).
    // Full daemon-driven execution will call POST /api/events and
    // poll /api/state — using the same pattern as daemon_serve_blackbox.rs.
    let mut scores = std::collections::HashMap::new();
    let mut total_weight = 0.0;
    let mut weighted_sum = 0.0;

    for (name, rule) in &scenario.scoring {
        let score = scorer::evaluate_offline(rule);
        total_weight += rule.weight();
        weighted_sum += score * rule.weight();
        scores.insert(name.clone(), score);
    }

    let weighted_score = if total_weight > 0.0 {
        weighted_sum / total_weight
    } else {
        0.0
    };

    Ok(ScenarioResult {
        name: scenario.scenario.name.clone(),
        difficulty: scenario.scenario.difficulty,
        scores,
        weighted_score,
        turns: 0,
        tokens: 0,
        duration_secs: start.elapsed().as_secs_f64(),
        passed: weighted_score >= 0.5,
        error: None,
        tests_component: scenario.scenario.tests_component.clone(),
    })
}
