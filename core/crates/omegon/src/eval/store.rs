//! Persistent storage for eval score cards.
//!
//! Score cards are stored as JSON files under `$OMEGON_HOME/eval-results/`
//! with the naming convention: `{agent_id}/{suite}-{timestamp}.json`.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::report::ScoreCard;

/// Summary of a stored score card (for listing without loading full data).
#[derive(Debug, Clone, Serialize)]
pub struct ScoreCardEntry {
    pub id: String,
    pub agent_id: String,
    pub suite: String,
    pub timestamp: String,
    pub total_score: f64,
    pub pass_rate: f64,
    pub scenario_count: usize,
    pub model: String,
    pub domain: String,
    pub omegon_version: String,
}

/// Diff between two score cards.
#[derive(Debug, Clone, Serialize)]
pub struct ScoreCardDiff {
    pub card_a: String,
    pub card_b: String,
    pub total_score_delta: f64,
    pub pass_rate_delta: f64,
    pub dimension_deltas: std::collections::HashMap<String, f64>,
    pub component_deltas: std::collections::HashMap<String, f64>,
    pub scenario_diffs: Vec<ScenarioDiff>,
    pub matrix_changes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScenarioDiff {
    pub name: String,
    pub score_a: Option<f64>,
    pub score_b: Option<f64>,
    pub delta: f64,
    pub status_change: Option<String>,
}

/// Ranking entry for an agent in a specific suite.
#[derive(Debug, Clone, Serialize)]
pub struct RankingEntry {
    pub agent_id: String,
    pub suite: String,
    pub latest_score: f64,
    pub latest_pass_rate: f64,
    pub latest_timestamp: String,
    pub run_count: usize,
    pub trend: Trend,
    pub model: String,
    pub domain: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Trend {
    Improving,
    Stable,
    Declining,
    Insufficient,
}

fn eval_results_dir() -> anyhow::Result<PathBuf> {
    let home = crate::paths::omegon_home()?;
    Ok(home.join("eval-results"))
}

fn agent_dir(agent_id: &str) -> anyhow::Result<PathBuf> {
    Ok(eval_results_dir()?.join(agent_id.replace('.', "-")))
}

fn card_filename(suite: &str, timestamp: &str) -> String {
    let ts = timestamp
        .replace(':', "-")
        .replace('T', "_")
        .chars()
        .take(19)
        .collect::<String>();
    format!("{suite}-{ts}.json")
}

fn card_id_from_path(path: &Path, agent_id: &str) -> String {
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    format!("{}/{stem}", agent_id.replace('.', "-"))
}

/// Write a score card to persistent storage. Returns the storage path.
pub fn store(card: &ScoreCard) -> anyhow::Result<PathBuf> {
    let dir = agent_dir(&card.agent_id)?;
    std::fs::create_dir_all(&dir)?;
    let filename = card_filename(&card.suite, &card.timestamp);
    let path = dir.join(&filename);
    let json = serde_json::to_string_pretty(card)?;
    std::fs::write(&path, &json)?;
    Ok(path)
}

/// List all stored score card entries (summaries, not full cards).
pub fn list() -> anyhow::Result<Vec<ScoreCardEntry>> {
    let root = eval_results_dir()?;
    if !root.is_dir() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();

    for agent_entry in std::fs::read_dir(&root)? {
        let agent_entry = agent_entry?;
        let agent_dir = agent_entry.path();
        if !agent_dir.is_dir() {
            continue;
        }

        for card_entry in std::fs::read_dir(&agent_dir)? {
            let card_entry = card_entry?;
            let card_path = card_entry.path();
            if card_path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            match load_card(&card_path) {
                Ok(card) => {
                    entries.push(ScoreCardEntry {
                        id: card_id_from_path(&card_path, &card.agent_id),
                        agent_id: card.agent_id.clone(),
                        suite: card.suite.clone(),
                        timestamp: card.timestamp.clone(),
                        total_score: card.aggregate.total_score,
                        pass_rate: card.aggregate.pass_rate,
                        scenario_count: card.scenarios.len(),
                        model: card.components.model.clone(),
                        domain: card.components.domain.clone(),
                        omegon_version: card.components.omegon_version.clone(),
                    });
                }
                Err(e) => {
                    tracing::warn!(path = %card_path.display(), error = %e, "skipping malformed score card");
                }
            }
        }
    }

    // Sort by timestamp descending (most recent first).
    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(entries)
}

/// Load a full score card by its storage ID (e.g., "styrene-coding-agent/coding-2026-04-15_14-00-00").
pub fn load(id: &str) -> anyhow::Result<ScoreCard> {
    let root = eval_results_dir()?;
    let path = root.join(format!("{id}.json"));
    load_card(&path)
}

fn load_card(path: &Path) -> anyhow::Result<ScoreCard> {
    let content = std::fs::read_to_string(path)?;
    let card: ScoreCard = serde_json::from_str(&content)?;
    Ok(card)
}

/// Compare two score cards and produce a diff.
pub fn compare(id_a: &str, id_b: &str) -> anyhow::Result<ScoreCardDiff> {
    let card_a = load(id_a)?;
    let card_b = load(id_b)?;
    Ok(diff_cards(id_a, id_b, &card_a, &card_b))
}

fn diff_cards(id_a: &str, id_b: &str, a: &ScoreCard, b: &ScoreCard) -> ScoreCardDiff {
    let total_score_delta = b.aggregate.total_score - a.aggregate.total_score;
    let pass_rate_delta = b.aggregate.pass_rate - a.aggregate.pass_rate;

    // Dimension deltas
    let mut dimension_deltas = std::collections::HashMap::new();
    let all_dims: std::collections::HashSet<&String> = a
        .aggregate
        .by_dimension
        .keys()
        .chain(b.aggregate.by_dimension.keys())
        .collect();
    for dim in all_dims {
        let va = a.aggregate.by_dimension.get(dim).copied().unwrap_or(0.0);
        let vb = b.aggregate.by_dimension.get(dim).copied().unwrap_or(0.0);
        dimension_deltas.insert(dim.clone(), vb - va);
    }

    // Component deltas
    let mut component_deltas = std::collections::HashMap::new();
    let all_comps: std::collections::HashSet<&String> = a
        .aggregate
        .by_component
        .keys()
        .chain(b.aggregate.by_component.keys())
        .collect();
    for comp in all_comps {
        let va = a.aggregate.by_component.get(comp).copied().unwrap_or(0.0);
        let vb = b.aggregate.by_component.get(comp).copied().unwrap_or(0.0);
        component_deltas.insert(comp.clone(), vb - va);
    }

    // Scenario diffs
    let a_scenarios: std::collections::HashMap<&str, &super::report::ScenarioResult> =
        a.scenarios.iter().map(|s| (s.name.as_str(), s)).collect();
    let b_scenarios: std::collections::HashMap<&str, &super::report::ScenarioResult> =
        b.scenarios.iter().map(|s| (s.name.as_str(), s)).collect();
    let all_names: std::collections::HashSet<&str> = a_scenarios
        .keys()
        .chain(b_scenarios.keys())
        .copied()
        .collect();

    let mut scenario_diffs = Vec::new();
    for name in all_names {
        let sa = a_scenarios.get(name);
        let sb = b_scenarios.get(name);
        let score_a = sa.map(|s| s.weighted_score);
        let score_b = sb.map(|s| s.weighted_score);
        let delta = score_b.unwrap_or(0.0) - score_a.unwrap_or(0.0);
        let status_change = match (sa.map(|s| s.passed), sb.map(|s| s.passed)) {
            (Some(false), Some(true)) => Some("fixed".into()),
            (Some(true), Some(false)) => Some("regressed".into()),
            (None, Some(_)) => Some("added".into()),
            (Some(_), None) => Some("removed".into()),
            _ => None,
        };
        scenario_diffs.push(ScenarioDiff {
            name: name.to_string(),
            score_a,
            score_b,
            delta,
            status_change,
        });
    }
    scenario_diffs.sort_by(|a, b| a.name.cmp(&b.name));

    // Matrix changes
    let mut matrix_changes = Vec::new();
    if a.components.model != b.components.model {
        matrix_changes.push(format!(
            "model: {} -> {}",
            a.components.model, b.components.model
        ));
    }
    if a.components.domain != b.components.domain {
        matrix_changes.push(format!(
            "domain: {} -> {}",
            a.components.domain, b.components.domain
        ));
    }
    if a.components.thinking_level != b.components.thinking_level {
        matrix_changes.push(format!(
            "thinking: {} -> {}",
            a.components.thinking_level, b.components.thinking_level
        ));
    }
    if a.components.context_class != b.components.context_class {
        matrix_changes.push(format!(
            "context: {} -> {}",
            a.components.context_class, b.components.context_class
        ));
    }
    if a.components.agent_version != b.components.agent_version {
        matrix_changes.push(format!(
            "agent_version: {} -> {}",
            a.components.agent_version, b.components.agent_version
        ));
    }
    if a.components.omegon_version != b.components.omegon_version {
        matrix_changes.push(format!(
            "omegon_version: {} -> {}",
            a.components.omegon_version, b.components.omegon_version
        ));
    }

    ScoreCardDiff {
        card_a: id_a.to_string(),
        card_b: id_b.to_string(),
        total_score_delta,
        pass_rate_delta,
        dimension_deltas,
        component_deltas,
        scenario_diffs,
        matrix_changes,
    }
}

/// Build a ranking table across all stored results.
pub fn rankings() -> anyhow::Result<Vec<RankingEntry>> {
    let entries = list()?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    // Group by (agent_id, suite)
    let mut groups: std::collections::HashMap<(String, String), Vec<&ScoreCardEntry>> =
        std::collections::HashMap::new();
    for entry in &entries {
        groups
            .entry((entry.agent_id.clone(), entry.suite.clone()))
            .or_default()
            .push(entry);
    }

    let mut rankings = Vec::new();
    for ((agent_id, suite), mut runs) in groups {
        runs.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        let latest = runs.last().unwrap();
        let trend = compute_trend(&runs);

        rankings.push(RankingEntry {
            agent_id,
            suite,
            latest_score: latest.total_score,
            latest_pass_rate: latest.pass_rate,
            latest_timestamp: latest.timestamp.clone(),
            run_count: runs.len(),
            trend,
            model: latest.model.clone(),
            domain: latest.domain.clone(),
        });
    }

    // Sort by latest_score descending.
    rankings.sort_by(|a, b| {
        b.latest_score
            .partial_cmp(&a.latest_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(rankings)
}

fn compute_trend(runs: &[&ScoreCardEntry]) -> Trend {
    if runs.len() < 2 {
        return Trend::Insufficient;
    }
    // Compare last two runs.
    let prev = &runs[runs.len() - 2];
    let latest = &runs[runs.len() - 1];
    let delta = latest.total_score - prev.total_score;
    if delta > 0.02 {
        Trend::Improving
    } else if delta < -0.02 {
        Trend::Declining
    } else {
        Trend::Stable
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::report::*;
    use std::collections::HashMap;

    fn sample_card(agent: &str, suite: &str, score: f64, ts: &str) -> ScoreCard {
        ScoreCard {
            agent_id: agent.to_string(),
            suite: suite.to_string(),
            timestamp: ts.to_string(),
            components: ComponentMatrix {
                agent_version: "1.0.0".into(),
                domain: "coding".into(),
                model: "anthropic:claude-sonnet-4-6".into(),
                thinking_level: "medium".into(),
                context_class: "squad".into(),
                max_turns: 50,
                omegon_version: "0.15.24".into(),
                ..Default::default()
            },
            scenarios: vec![ScenarioResult {
                name: "test-scenario".into(),
                difficulty: 1,
                scores: HashMap::from([("correctness".into(), score)]),
                weighted_score: score,
                turns: 3,
                tokens: 1000,
                duration_secs: 10.0,
                passed: score >= 0.5,
                error: None,
                tests_component: vec!["tools".into()],
            }],
            aggregate: AggregateScore {
                total_score: score,
                pass_rate: if score >= 0.5 { 1.0 } else { 0.0 },
                avg_turns: 3.0,
                avg_tokens: 1000.0,
                by_difficulty: HashMap::from([("1".into(), score)]),
                by_dimension: HashMap::from([("correctness".into(), score)]),
                by_component: HashMap::from([("tools".into(), score)]),
            },
        }
    }

    #[test]
    fn store_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let results_dir = dir.path().join("eval-results");

        let card = sample_card(
            "styrene.coding-agent",
            "coding",
            0.85,
            "2026-04-15T14:00:00Z",
        );

        // Store directly using the resolved path
        let agent_dir = results_dir.join("styrene-coding-agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        let filename = card_filename(&card.suite, &card.timestamp);
        let path = agent_dir.join(&filename);
        let json = serde_json::to_string_pretty(&card).unwrap();
        std::fs::write(&path, &json).unwrap();
        assert!(path.exists());

        // Load directly
        let loaded = load_card(&path).unwrap();
        assert_eq!(loaded.agent_id, card.agent_id);
        assert_eq!(loaded.scenarios.len(), 1);
        assert!((loaded.aggregate.total_score - 0.85).abs() < 0.01);
    }

    #[test]
    fn compare_cards() {
        let a = sample_card(
            "styrene.coding-agent",
            "coding",
            0.7,
            "2026-04-14T12:00:00Z",
        );
        let mut b = sample_card(
            "styrene.coding-agent",
            "coding",
            0.9,
            "2026-04-15T14:00:00Z",
        );
        b.components.model = "anthropic:claude-opus-4-6".into();

        let diff = diff_cards("a", "b", &a, &b);
        assert!((diff.total_score_delta - 0.2).abs() < 0.01);
        assert!(diff.matrix_changes.iter().any(|c| c.contains("model")));
        assert_eq!(diff.scenario_diffs.len(), 1);
    }

    #[test]
    fn trend_computation() {
        let e1 = ScoreCardEntry {
            id: "a".into(),
            agent_id: "test".into(),
            suite: "s".into(),
            timestamp: "2026-04-14T00:00:00Z".into(),
            total_score: 0.7,
            pass_rate: 0.8,
            scenario_count: 3,
            model: "m".into(),
            domain: "d".into(),
            omegon_version: "0.15.24".into(),
        };
        let e2 = ScoreCardEntry {
            total_score: 0.9,
            timestamp: "2026-04-15T00:00:00Z".into(),
            ..e1.clone()
        };
        let e3 = ScoreCardEntry {
            total_score: 0.89,
            timestamp: "2026-04-16T00:00:00Z".into(),
            ..e1.clone()
        };
        let e4 = ScoreCardEntry {
            total_score: 0.8,
            timestamp: "2026-04-17T00:00:00Z".into(),
            ..e1.clone()
        };

        assert!(matches!(compute_trend(&[&e1, &e2]), Trend::Improving));
        assert!(matches!(compute_trend(&[&e2, &e3]), Trend::Stable));
        assert!(matches!(compute_trend(&[&e2, &e4]), Trend::Declining));
        assert!(matches!(compute_trend(&[&e1]), Trend::Insufficient));
    }
}
