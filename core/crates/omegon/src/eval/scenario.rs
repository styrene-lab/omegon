//! Scenario and suite parsing from TOML files.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use super::scorer::ScoringRule;

// ── Suite ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct EvalSuite {
    pub suite: SuiteMeta,
    #[serde(default)]
    pub scenarios: Vec<ScenarioRef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SuiteMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub domain: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioRef {
    pub path: String,
}

impl EvalSuite {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let suite: Self = toml::from_str(&content)?;
        Ok(suite)
    }
}

// ── Scenario ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct Scenario {
    pub scenario: ScenarioMeta,
    pub input: ScenarioInput,
    #[serde(default)]
    pub setup: Option<ScenarioSetup>,
    #[serde(default)]
    pub scoring: HashMap<String, ScoringRule>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_difficulty")]
    pub difficulty: u8,
    #[serde(default)]
    pub domain: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Which component(s) this scenario primarily tests.
    /// Used for attribution when analyzing score regressions.
    /// Values: "persona", "tools", "extensions", "plugins", "model",
    ///         "workflow", "triggers", "context", "safety"
    #[serde(default)]
    pub tests_component: Vec<String>,
}

fn default_difficulty() -> u8 {
    1
}

fn default_timeout() -> u64 {
    120
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioInput {
    pub prompt: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioSetup {
    pub fixture: Option<String>,
    #[serde(default)]
    pub files: HashMap<String, String>,
}

impl Scenario {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let scenario: Self = toml::from_str(&content)?;
        Ok(scenario)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_suite() {
        let toml_str = r#"
[suite]
name = "coding"
description = "Coding agent evaluation"
domain = "coding"

[[scenarios]]
path = "scenarios/fix-typo.toml"

[[scenarios]]
path = "scenarios/add-test.toml"
"#;
        let suite: EvalSuite = toml::from_str(toml_str).unwrap();
        assert_eq!(suite.suite.name, "coding");
        assert_eq!(suite.scenarios.len(), 2);
    }

    #[test]
    fn parse_scenario() {
        let toml_str = r#"
[scenario]
name = "fix-typo"
description = "Fix a typo in a docstring"
difficulty = 1
timeout_secs = 60

[input]
prompt = "Fix the typo in src/utils.py"

[scoring.correctness]
type = "contains"
expected = "returns"
weight = 0.5

[scoring.efficiency]
type = "turn-count"
max_turns = 10
ideal_turns = 2
weight = 0.3

[scoring.safety]
type = "no-destructive"
weight = 0.2
"#;
        let scenario: Scenario = toml::from_str(toml_str).unwrap();
        assert_eq!(scenario.scenario.name, "fix-typo");
        assert_eq!(scenario.scenario.difficulty, 1);
        assert_eq!(scenario.scoring.len(), 3);
        assert!(scenario.scoring.contains_key("correctness"));
        assert!(scenario.scoring.contains_key("efficiency"));
        assert!(scenario.scoring.contains_key("safety"));
    }
}
