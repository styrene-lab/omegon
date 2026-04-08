//! Cleave run state — persisted to state.json during execution.

use super::plan::CleaveChildRuntimeProfile;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Instant;

/// Overall cleave run state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleaveState {
    pub run_id: String,
    pub directive: String,
    pub repo_path: String,
    pub workspace_path: String,
    pub children: Vec<ChildState>,
    pub plan: serde_json::Value,
    #[serde(skip)]
    pub started_at: Option<Instant>,
}

/// Per-child state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildState {
    pub child_id: usize,
    pub label: String,
    pub description: String,
    pub scope: Vec<String>,
    pub depends_on: Vec<String>,
    pub status: ChildStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execute_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<CleaveChildRuntimeProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChildStatus {
    Pending,
    Running,
    Completed,
    Failed,
    /// Provider upstream exhausted (rate-limit after all retries). May be retried
    /// by the orchestrator using a cross-provider fallback.
    UpstreamExhausted,
}

impl CleaveState {
    /// Save state to disk.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load state from disk.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }

    /// Requeue children that were marked running by an interrupted parent process.
    ///
    /// Persisted `running` means only that the previous orchestrator had dispatched
    /// the child before it died or was interrupted. It does not prove the child is
    /// still alive or manageable, so resumed cleave runs must treat that state as
    /// stale and requeue the child.
    pub fn requeue_interrupted_children(&mut self) -> usize {
        let mut requeued = 0;
        for child in &mut self.children {
            if child.status == ChildStatus::Running {
                child.status = ChildStatus::Pending;
                child.error = None;
                child.duration_secs = None;
                child.stdout = None;
                requeued += 1;
            }
        }
        requeued
    }

    /// Build initial state from a plan.
    pub fn from_plan(
        run_id: &str,
        directive: &str,
        repo_path: &Path,
        workspace_path: &Path,
        plan: &super::plan::CleavePlan,
        model: &str,
    ) -> Self {
        let children = plan
            .children
            .iter()
            .enumerate()
            .map(|(i, c)| {
                // Model priority: child explicit > plan default > None (triggers routing).
                // When None the orchestrator applies scope-based cost-aware routing.
                // We only fall back to the parent model if neither is set, so that the
                // orchestrator routing condition (is_none_or(m == config.model)) still fires.
                let execute_model = c
                    .model
                    .clone()
                    .or_else(|| plan.default_model.clone())
                    .or_else(|| Some(model.to_string()));
                ChildState {
                    child_id: i,
                    label: c.label.clone(),
                    description: c.description.clone(),
                    scope: c.scope.clone(),
                    depends_on: c.depends_on.clone(),
                    status: ChildStatus::Pending,
                    error: None,
                    branch: Some(format!("cleave/{}-{}", i, c.label)),
                    worktree_path: None,
                    backend: "native".to_string(),
                    execute_model,
                    provider_id: None,
                    duration_secs: None,
                    stdout: None,
                    runtime: c.runtime.clone(),
                }
            })
            .collect();

        Self {
            run_id: run_id.to_string(),
            directive: directive.to_string(),
            repo_path: repo_path.to_string_lossy().to_string(),
            workspace_path: workspace_path.to_string_lossy().to_string(),
            children,
            plan: serde_json::to_value(plan).unwrap_or_default(),
            started_at: Some(Instant::now()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plan() -> super::super::plan::CleavePlan {
        serde_json::from_str(r#"{
            "children": [
                {"label": "alpha", "description": "do alpha", "scope": ["src/"], "depends_on": []},
                {"label": "beta", "description": "do beta", "scope": ["tests/"], "depends_on": ["alpha"]}
            ],
            "rationale": "test plan"
        }"#).unwrap()
    }

    #[test]
    fn from_plan_creates_correct_children() {
        let plan = sample_plan();
        let state = CleaveState::from_plan(
            "run-1",
            "fix bugs",
            Path::new("/repo"),
            Path::new("/ws"),
            &plan,
            "anthropic:sonnet",
        );
        assert_eq!(state.children.len(), 2);
        assert_eq!(state.children[0].label, "alpha");
        assert_eq!(state.children[0].branch.as_deref(), Some("cleave/0-alpha"));
        assert_eq!(state.children[0].status, ChildStatus::Pending);
        assert_eq!(state.children[1].depends_on, vec!["alpha"]);
        assert_eq!(
            state.children[1].execute_model.as_deref(),
            Some("anthropic:sonnet")
        );
    }

    #[test]
    fn state_save_load_round_trip() {
        let plan = sample_plan();
        let mut state = CleaveState::from_plan(
            "run-1",
            "fix bugs",
            Path::new("/repo"),
            Path::new("/ws"),
            &plan,
            "model",
        );
        state.children[0].status = ChildStatus::Completed;
        state.children[0].duration_secs = Some(42.5);

        let tmp = std::env::temp_dir().join("omegon-test-state.json");
        state.save(&tmp).unwrap();

        let loaded = CleaveState::load(&tmp).unwrap();
        assert_eq!(loaded.run_id, "run-1");
        assert_eq!(loaded.children[0].status, ChildStatus::Completed);
        assert_eq!(loaded.children[0].duration_secs, Some(42.5));
        assert_eq!(loaded.children[1].status, ChildStatus::Pending);
        assert!(loaded.children[0].runtime.is_none());

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn requeue_interrupted_children_demotes_running_to_pending() {
        let plan = sample_plan();
        let mut state = CleaveState::from_plan(
            "run-1",
            "fix bugs",
            Path::new("/repo"),
            Path::new("/ws"),
            &plan,
            "model",
        );
        state.children[0].status = ChildStatus::Running;
        state.children[0].error = Some("stale failure".into());
        state.children[0].duration_secs = Some(12.0);
        state.children[0].stdout = Some("stale stdout".into());
        state.children[1].status = ChildStatus::Completed;
        state.children[1].duration_secs = Some(42.5);

        let requeued = state.requeue_interrupted_children();

        assert_eq!(requeued, 1);
        assert_eq!(state.children[0].status, ChildStatus::Pending);
        assert!(state.children[0].error.is_none());
        assert!(state.children[0].duration_secs.is_none());
        assert!(state.children[0].stdout.is_none());
        assert_eq!(state.children[1].status, ChildStatus::Completed);
        assert_eq!(state.children[1].duration_secs, Some(42.5));
    }

    #[test]
    fn state_serializes_camel_case() {
        let plan = sample_plan();
        let state = CleaveState::from_plan(
            "run-1",
            "test",
            Path::new("/r"),
            Path::new("/w"),
            &plan,
            "m",
        );
        let json = serde_json::to_string(&state).unwrap();
        // camelCase field names
        assert!(json.contains("runId"), "should use camelCase: {json}");
        assert!(json.contains("childId"));
        assert!(json.contains("dependsOn"));
        assert!(json.contains("repoPath"));
        // snake_case status values
        assert!(json.contains("\"pending\""));
    }

    #[test]
    fn child_status_deserializes_from_snake_case() {
        let _json = r#"{"child_id":0,"label":"a","description":"d","scope":[],"depends_on":[],"status":"completed","backend":"native"}"#;
        // camelCase version
        let json_camel = r#"{"childId":0,"label":"a","description":"d","scope":[],"dependsOn":[],"status":"completed","backend":"native"}"#;
        let child: ChildState = serde_json::from_str(json_camel).unwrap();
        assert_eq!(child.status, ChildStatus::Completed);
    }
}
