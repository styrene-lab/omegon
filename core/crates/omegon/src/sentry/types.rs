use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SentryTask {
    pub id: String,
    pub name: String,
    pub priority: u8,
    pub triggers: Vec<Trigger>,
    pub last_run: Option<DateTime<Utc>>,
    pub run_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    pub prompt: String,
    pub model: Option<String>,
    pub skill: Option<String>,
    pub max_turns: Option<u32>,
    pub timeout_secs: Option<u64>,
    pub token_budget: Option<u64>,
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub execution_mode: Option<String>,
    #[serde(default)]
    pub design_node_id: Option<String>,
    #[serde(default)]
    pub openspec_change: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub exit_code: i32,
    pub summary: String,
    pub tokens_used: u64,
    pub duration_secs: u64,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskError {
    pub message: String,
    pub retriable: bool,
    pub attempt: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    Cron {
        schedule: String,
    },
    Webhook {
        name: String,
    },
    FileWatch {
        paths: Vec<PathBuf>,
        debounce_secs: u64,
    },
    GitEvent {
        events: Vec<GitEventKind>,
        poll_interval_secs: u64,
    },
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GitEventKind {
    NewCommit,
    NewTag,
    NewBranch,
    PullRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: String,
    pub task_id: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: RunStatus,
    pub exit_code: Option<i32>,
    pub summary: Option<String>,
    pub tokens_used: u64,
    pub duration_secs: u64,
    pub session_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

impl std::str::FromStr for RunStatus {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            other => anyhow::bail!("unknown run status: {other}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentry_task_roundtrip() {
        let task = SentryTask {
            id: "pr-review".into(),
            name: "PR Review".into(),
            priority: 2,
            triggers: vec![
                Trigger::Cron {
                    schedule: "0 */4 * * *".into(),
                },
                Trigger::Webhook {
                    name: "github-pr".into(),
                },
            ],
            last_run: None,
            run_count: 0,
        };
        let json = serde_json::to_string(&task).unwrap();
        let parsed: SentryTask = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, task);
    }

    #[test]
    fn task_spec_roundtrip() {
        let spec = TaskSpec {
            prompt: "Review open PRs".into(),
            model: Some("anthropic:claude-sonnet-4-6".into()),
            skill: None,
            max_turns: Some(20),
            timeout_secs: Some(300),
            token_budget: Some(500_000),
            cwd: None,
            env: HashMap::new(),
            execution_mode: None,
            design_node_id: None,
            openspec_change: None,
        };
        let json = serde_json::to_string(&spec).unwrap();
        let parsed: TaskSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.prompt, "Review open PRs");
        assert_eq!(parsed.max_turns, Some(20));
    }

    #[test]
    fn task_result_roundtrip() {
        let result = TaskResult {
            exit_code: 0,
            summary: "All PRs reviewed".into(),
            tokens_used: 12345,
            duration_secs: 60,
            session_id: "sess-abc".into(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: TaskResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.exit_code, 0);
        assert_eq!(parsed.tokens_used, 12345);
    }

    #[test]
    fn trigger_variants_roundtrip() {
        let triggers = vec![
            Trigger::Cron {
                schedule: "0 9 * * 1-5".into(),
            },
            Trigger::Webhook {
                name: "deploy".into(),
            },
            Trigger::FileWatch {
                paths: vec!["src/".into()],
                debounce_secs: 30,
            },
            Trigger::GitEvent {
                events: vec![GitEventKind::NewCommit, GitEventKind::PullRequest],
                poll_interval_secs: 60,
            },
            Trigger::Manual,
        ];
        let json = serde_json::to_string(&triggers).unwrap();
        let parsed: Vec<Trigger> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, triggers);
    }

    #[test]
    fn run_status_display_parse() {
        for status in [RunStatus::Running, RunStatus::Completed, RunStatus::Failed] {
            let s = status.to_string();
            let parsed: RunStatus = s.parse().unwrap();
            assert_eq!(parsed, status);
        }
    }
}
