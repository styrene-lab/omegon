use serde::{Deserialize, Serialize};

use super::profiles::AssistantLaunchStatus;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantRunSummary {
    pub run_id: String,
    pub assistant_id: String,
    pub status: AssistantRunStatus,
    pub trigger: AssistantRunTrigger,
    pub readiness_status: AssistantLaunchStatus,
    pub safe_progress: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantRunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantRunTrigger {
    pub source: AssistantRunTriggerSource,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantRunTriggerSource {
    Console,
    Acp,
    Tui,
    System,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantRunStore {
    pub runs: Vec<AssistantRunSummary>,
}

impl AssistantRunStore {
    pub fn empty() -> Self {
        Self { runs: Vec::new() }
    }

    pub fn list(&self) -> Vec<AssistantRunSummary> {
        let mut runs = self.runs.clone();
        runs.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| b.created_at.cmp(&a.created_at))
                .then_with(|| a.run_id.cmp(&b.run_id))
        });
        runs
    }

    pub fn get(&self, run_id: &str) -> Option<AssistantRunSummary> {
        self.runs.iter().find(|run| run.run_id == run_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_store_lists_newest_first_without_payloads() {
        let store = AssistantRunStore {
            runs: vec![
                AssistantRunSummary {
                    run_id: "old".into(),
                    assistant_id: "daily".into(),
                    status: AssistantRunStatus::Succeeded,
                    trigger: AssistantRunTrigger {
                        source: AssistantRunTriggerSource::Console,
                        label: Some("manual".into()),
                    },
                    readiness_status: AssistantLaunchStatus::Ready,
                    safe_progress: Some("completed".into()),
                    created_at: Some("2026-06-11T00:00:00Z".into()),
                    updated_at: Some("2026-06-11T00:01:00Z".into()),
                },
                AssistantRunSummary {
                    run_id: "new".into(),
                    assistant_id: "daily".into(),
                    status: AssistantRunStatus::Running,
                    trigger: AssistantRunTrigger {
                        source: AssistantRunTriggerSource::Console,
                        label: None,
                    },
                    readiness_status: AssistantLaunchStatus::Ready,
                    safe_progress: None,
                    created_at: Some("2026-06-11T00:02:00Z".into()),
                    updated_at: Some("2026-06-11T00:03:00Z".into()),
                },
            ],
        };

        let runs = store.list();
        assert_eq!(runs[0].run_id, "new");
        assert_eq!(
            store.get("old").unwrap().safe_progress.as_deref(),
            Some("completed")
        );
        assert!(store.get("missing").is_none());
    }
}
