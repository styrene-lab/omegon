//! Cleave run state — persisted to state.json during execution.

use super::plan::CleaveChildRuntimeProfile;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Overall cleave run state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleaveState {
    pub run_id: String,
    pub directive: String,
    pub repo_path: String,
    pub workspace_path: String,
    pub supervisor_token: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_activity_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adoption_worktree_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adoption_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supervisor_token: Option<String>,
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RunningChildReconciliation {
    pub seen: usize,
    pub still_running: usize,
    pub requeued: usize,
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn process_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: kill(pid, 0) probes for process existence without sending a signal.
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

fn canonical_display(path: &str) -> Option<String> {
    std::fs::canonicalize(path)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

fn local_supervisor_token() -> String {
    format!(
        "supv-{}-{}",
        crate::cleave::orchestrator::nanoid(8),
        crate::cleave::orchestrator::nanoid(6)
    )
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

    /// Reconcile children that were marked running by a previous orchestrator.
    ///
    /// If the persisted PID is still alive, keep the child in `running` so a daemon
    /// supervisor can truthfully report in-flight work. Otherwise demote it back to
    /// `pending` because the previous parent is gone and the child is no longer
    /// manageable.
    pub fn reconcile_running_children(&mut self) -> RunningChildReconciliation {
        let mut reconciliation = RunningChildReconciliation::default();
        for child in &mut self.children {
            if child.status != ChildStatus::Running {
                continue;
            }
            reconciliation.seen += 1;
            let worktree_matches = child
                .worktree_path
                .as_deref()
                .and_then(canonical_display)
                .zip(child.adoption_worktree_path.as_ref())
                .map(|(current, expected)| current == *expected)
                .unwrap_or(false);
            let model_matches = child
                .execute_model
                .as_ref()
                .zip(child.adoption_model.as_ref())
                .map(|(current, expected)| current == expected)
                .unwrap_or(false);
            let supervisor_matches = child
                .supervisor_token
                .as_ref()
                .zip(Some(&self.supervisor_token))
                .map(|(child_token, state_token)| child_token == state_token)
                .unwrap_or(false);
            if let Some(pid) = child.pid
                && process_is_alive(pid)
                && worktree_matches
                && model_matches
                && supervisor_matches
            {
                reconciliation.still_running += 1;
                continue;
            }

            child.status = ChildStatus::Pending;
            child.error = None;
            child.duration_secs = None;
            child.stdout = None;
            child.pid = None;
            child.started_at_unix_ms = None;
            child.last_activity_unix_ms = None;
            child.adoption_worktree_path = None;
            child.adoption_model = None;
            child.supervisor_token = None;
            reconciliation.requeued += 1;
        }
        reconciliation
    }

    pub fn mark_child_spawned(&mut self, child_idx: usize, pid: u32) {
        if let Some(child) = self.children.get_mut(child_idx) {
            let now = unix_time_ms();
            child.status = ChildStatus::Running;
            child.pid = Some(pid);
            child.started_at_unix_ms = Some(now);
            child.last_activity_unix_ms = Some(now);
            child.adoption_worktree_path = child.worktree_path.as_deref().and_then(canonical_display);
            child.adoption_model = child.execute_model.clone();
            child.supervisor_token = Some(self.supervisor_token.clone());
        }
    }

    pub fn mark_child_activity(&mut self, child_idx: usize) {
        if let Some(child) = self.children.get_mut(child_idx) {
            child.last_activity_unix_ms = Some(unix_time_ms());
        }
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
                    pid: None,
                    started_at_unix_ms: None,
                    last_activity_unix_ms: None,
                    adoption_worktree_path: None,
                    adoption_model: None,
                    supervisor_token: None,
                }
            })
            .collect();

        Self {
            run_id: run_id.to_string(),
            directive: directive.to_string(),
            repo_path: repo_path.to_string_lossy().to_string(),
            workspace_path: workspace_path.to_string_lossy().to_string(),
            supervisor_token: local_supervisor_token(),
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
        assert!(!loaded.supervisor_token.is_empty());
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

        let reconciliation = state.reconcile_running_children();

        assert_eq!(reconciliation.requeued, 1);
        assert_eq!(reconciliation.still_running, 0);
        assert_eq!(state.children[0].status, ChildStatus::Pending);
        assert!(state.children[0].error.is_none());
        assert!(state.children[0].duration_secs.is_none());
        assert!(state.children[0].stdout.is_none());
        assert!(state.children[0].pid.is_none());
        assert!(state.children[0].started_at_unix_ms.is_none());
        assert!(state.children[0].last_activity_unix_ms.is_none());
        assert!(state.children[0].adoption_worktree_path.is_none());
        assert!(state.children[0].adoption_model.is_none());
        assert_eq!(state.children[1].status, ChildStatus::Completed);
        assert_eq!(state.children[1].duration_secs, Some(42.5));
    }

    #[test]
    fn reconcile_running_children_preserves_live_pid() {
        let plan = sample_plan();
        let mut state = CleaveState::from_plan(
            "run-1",
            "fix bugs",
            Path::new("/repo"),
            Path::new("/ws"),
            &plan,
            "model",
        );
        let pid = std::process::id();
        let worktree = tempfile::tempdir().unwrap();
        state.children[0].status = ChildStatus::Running;
        state.children[0].pid = Some(pid);
        state.children[0].worktree_path = Some(worktree.path().to_string_lossy().to_string());
        state.children[0].adoption_worktree_path = Some(
            std::fs::canonicalize(worktree.path())
                .unwrap()
                .to_string_lossy()
                .to_string(),
        );
        state.children[0].execute_model = Some("model".into());
        state.children[0].adoption_model = Some("model".into());
        state.children[0].supervisor_token = Some(state.supervisor_token.clone());
        state.children[0].started_at_unix_ms = Some(1);
        state.children[0].last_activity_unix_ms = Some(2);

        let reconciliation = state.reconcile_running_children();

        assert_eq!(reconciliation.seen, 1);
        assert_eq!(reconciliation.still_running, 1);
        assert_eq!(reconciliation.requeued, 0);
        assert_eq!(state.children[0].status, ChildStatus::Running);
        assert_eq!(state.children[0].pid, Some(pid));
    }

    #[test]
    fn mark_child_spawned_sets_pid_and_timestamps() {
        let plan = sample_plan();
        let mut state = CleaveState::from_plan(
            "run-1",
            "fix bugs",
            Path::new("/repo"),
            Path::new("/ws"),
            &plan,
            "model",
        );

        state.mark_child_spawned(0, 4242);
        let child = &state.children[0];

        assert_eq!(child.status, ChildStatus::Running);
        assert_eq!(child.pid, Some(4242));
        assert!(child.started_at_unix_ms.is_some());
        assert!(child.last_activity_unix_ms.is_some());
        assert!(child.adoption_worktree_path.is_none());
        assert_eq!(child.adoption_model.as_deref(), Some("model"));
        assert_eq!(child.supervisor_token.as_deref(), Some(state.supervisor_token.as_str()));
    }

    #[test]
    fn reconcile_running_children_requeues_mismatched_supervisor_token() {
        let plan = sample_plan();
        let mut state = CleaveState::from_plan(
            "run-1",
            "fix bugs",
            Path::new("/repo"),
            Path::new("/ws"),
            &plan,
            "model",
        );
        let pid = std::process::id();
        let worktree = tempfile::tempdir().unwrap();
        state.children[0].status = ChildStatus::Running;
        state.children[0].pid = Some(pid);
        state.children[0].worktree_path = Some(worktree.path().to_string_lossy().to_string());
        state.children[0].adoption_worktree_path = Some(
            std::fs::canonicalize(worktree.path())
                .unwrap()
                .to_string_lossy()
                .to_string(),
        );
        state.children[0].execute_model = Some("model".into());
        state.children[0].adoption_model = Some("model".into());
        state.children[0].supervisor_token = Some("different-supervisor".into());

        let reconciliation = state.reconcile_running_children();

        assert_eq!(reconciliation.seen, 1);
        assert_eq!(reconciliation.still_running, 0);
        assert_eq!(reconciliation.requeued, 1);
        assert_eq!(state.children[0].status, ChildStatus::Pending);
        assert!(state.children[0].pid.is_none());
    }

    #[test]
    fn reconcile_running_children_requeues_mismatched_adoption_fingerprint() {
        let plan = sample_plan();
        let mut state = CleaveState::from_plan(
            "run-1",
            "fix bugs",
            Path::new("/repo"),
            Path::new("/ws"),
            &plan,
            "model",
        );
        let pid = std::process::id();
        let worktree = tempfile::tempdir().unwrap();
        state.children[0].status = ChildStatus::Running;
        state.children[0].pid = Some(pid);
        state.children[0].worktree_path = Some(worktree.path().to_string_lossy().to_string());
        state.children[0].adoption_worktree_path = Some("/tmp/other-worktree".into());
        state.children[0].execute_model = Some("model".into());
        state.children[0].adoption_model = Some("different-model".into());

        let reconciliation = state.reconcile_running_children();

        assert_eq!(reconciliation.seen, 1);
        assert_eq!(reconciliation.still_running, 0);
        assert_eq!(reconciliation.requeued, 1);
        assert_eq!(state.children[0].status, ChildStatus::Pending);
        assert!(state.children[0].pid.is_none());
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
