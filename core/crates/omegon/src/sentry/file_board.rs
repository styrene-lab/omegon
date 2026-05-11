use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::board::TaskBoard;
use super::state_db::StateDb;
use super::types::{SentryTask, TaskError, TaskResult, TaskSpec, Trigger};

pub struct FileTaskBoard {
    config: super::SentryConfig,
    state_db: Arc<StateDb>,
    instance_id: String,
    config_dir: PathBuf,
}

impl FileTaskBoard {
    pub fn new(
        config: super::SentryConfig,
        state_db: Arc<StateDb>,
        instance_id: String,
        config_dir: PathBuf,
    ) -> Self {
        Self { config, state_db, instance_id, config_dir }
    }

    fn find_task_config(&self, task_id: &str) -> Option<&super::SentryTaskConfig> {
        self.config.tasks.iter().find(|t| t.name == task_id)
    }

    fn resolve_prompt(&self, tc: &super::SentryTaskConfig) -> anyhow::Result<String> {
        if let Some(ref prompt) = tc.prompt {
            return Ok(prompt.clone());
        }
        if let Some(ref prompt_file) = tc.prompt_file {
            let path = if Path::new(prompt_file).is_absolute() {
                PathBuf::from(prompt_file)
            } else {
                self.config_dir.join(prompt_file)
            };
            return std::fs::read_to_string(&path).map_err(|e| {
                anyhow::anyhow!("failed to read prompt file {}: {e}", path.display())
            });
        }
        anyhow::bail!("task '{}' has neither prompt nor prompt_file", tc.name)
    }
}

impl TaskBoard for FileTaskBoard {
    fn list_actionable(&self) -> anyhow::Result<Vec<SentryTask>> {
        let mut tasks = Vec::with_capacity(self.config.tasks.len());
        for tc in &self.config.tasks {
            let (last_run, run_count) = self.state_db.last_run(&tc.name)?
                .map(|(dt, c)| (Some(dt), c))
                .unwrap_or((None, 0));

            let mut triggers = Vec::new();
            if let Some(ref trig) = tc.trigger {
                if let Some(ref cron) = trig.cron {
                    triggers.push(Trigger::Cron { schedule: cron.schedule.clone() });
                }
                if let Some(ref wh) = trig.webhook {
                    triggers.push(Trigger::Webhook { name: wh.name.clone() });
                }
            }
            if triggers.is_empty() {
                triggers.push(Trigger::Manual);
            }

            tasks.push(SentryTask {
                id: tc.name.clone(),
                name: tc.name.clone(),
                priority: tc.priority.unwrap_or(1),
                triggers,
                last_run,
                run_count,
            });
        }
        Ok(tasks)
    }

    fn claim(&self, task_id: &str) -> anyhow::Result<bool> {
        self.state_db.claim_task(task_id, &self.instance_id)
    }

    fn release(&self, task_id: &str) -> anyhow::Result<()> {
        self.state_db.release_task(task_id)
    }

    fn complete(&self, task_id: &str, result: &TaskResult) -> anyhow::Result<()> {
        let run_id = format!("{task_id}-{}", uuid_v4());
        self.state_db.record_run_start(&run_id, task_id)?;
        self.state_db.record_run_complete(&run_id, result)?;
        self.state_db.release_task(task_id)?;
        Ok(())
    }

    fn fail(&self, task_id: &str, error: &TaskError) -> anyhow::Result<()> {
        let run_id = format!("{task_id}-{}", uuid_v4());
        self.state_db.record_run_start(&run_id, task_id)?;
        self.state_db.record_run_failure(&run_id, error)?;
        self.state_db.release_task(task_id)?;
        Ok(())
    }

    fn task_spec(&self, task_id: &str) -> anyhow::Result<TaskSpec> {
        let tc = self.find_task_config(task_id)
            .ok_or_else(|| anyhow::anyhow!("task '{task_id}' not found in config"))?;

        let prompt = self.resolve_prompt(tc)?;

        Ok(TaskSpec {
            prompt,
            model: tc.model.clone(),
            skill: tc.skill.clone(),
            max_turns: tc.max_turns,
            timeout_secs: tc.timeout_secs,
            token_budget: tc.token_budget,
            cwd: tc.cwd.as_ref().map(PathBuf::from),
            env: tc.env.clone().unwrap_or_default(),
            design_node_id: None,
            openspec_change: None,
        })
    }
}

pub(super) fn uuid_v4() -> String {
    use getrandom::fill;
    let mut bytes = [0u8; 16];
    let _ = fill(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11],
        bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{SentryConfig, SentryGlobal, SentryTaskConfig, CronTrigger, TriggerConfig};

    fn test_config() -> SentryConfig {
        SentryConfig {
            sentry: SentryGlobal {
                max_concurrent: 1,
                log_retention_days: 7,
                routing: None,
            },
            tasks: vec![
                SentryTaskConfig {
                    name: "test-task".into(),
                    prompt: Some("do the thing".into()),
                    prompt_file: None,
                    model: Some("anthropic:claude-sonnet-4-6".into()),
                    skill: None,
                    max_turns: Some(10),
                    timeout_secs: Some(120),
                    token_budget: None,
                    cwd: None,
                    env: None,
                    trigger: Some(TriggerConfig {
                        cron: Some(CronTrigger { schedule: "0 * * * * *".into() }),
                        webhook: None,
                    }),
                    budget: None,
                    priority: None,
                },
            ],
        }
    }

    #[test]
    fn list_actionable_returns_tasks() {
        let db = Arc::new(StateDb::in_memory().unwrap());
        let board = FileTaskBoard::new(test_config(), db, "test".into(), PathBuf::from("."));
        let tasks = board.list_actionable().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "test-task");
        assert_eq!(tasks[0].run_count, 0);
    }

    #[test]
    fn claim_and_release() {
        let db = Arc::new(StateDb::in_memory().unwrap());
        let board = FileTaskBoard::new(test_config(), db, "test".into(), PathBuf::from("."));
        assert!(board.claim("test-task").unwrap());
        assert!(!board.claim("test-task").unwrap());
        board.release("test-task").unwrap();
        assert!(board.claim("test-task").unwrap());
    }

    #[test]
    fn complete_records_and_releases() {
        let db = Arc::new(StateDb::in_memory().unwrap());
        let board = FileTaskBoard::new(test_config(), db, "test".into(), PathBuf::from("."));
        board.claim("test-task").unwrap();

        let result = TaskResult {
            exit_code: 0,
            summary: "ok".into(),
            tokens_used: 500,
            duration_secs: 10,
            session_id: "s1".into(),
        };
        board.complete("test-task", &result).unwrap();

        assert!(board.claim("test-task").unwrap());

        let tasks = board.list_actionable().unwrap();
        assert_eq!(tasks[0].run_count, 1);
        assert!(tasks[0].last_run.is_some());
    }

    #[test]
    fn task_spec_returns_prompt() {
        let db = Arc::new(StateDb::in_memory().unwrap());
        let board = FileTaskBoard::new(test_config(), db, "test".into(), PathBuf::from("."));
        let spec = board.task_spec("test-task").unwrap();
        assert_eq!(spec.prompt, "do the thing");
        assert_eq!(spec.model.as_deref(), Some("anthropic:claude-sonnet-4-6"));
        assert_eq!(spec.max_turns, Some(10));
    }

    #[test]
    fn task_spec_not_found() {
        let db = Arc::new(StateDb::in_memory().unwrap());
        let board = FileTaskBoard::new(test_config(), db, "test".into(), PathBuf::from("."));
        assert!(board.task_spec("nonexistent").is_err());
    }

    #[test]
    fn fail_records_and_releases() {
        let db = Arc::new(StateDb::in_memory().unwrap());
        let board = FileTaskBoard::new(test_config(), db, "test".into(), PathBuf::from("."));
        board.claim("test-task").unwrap();

        let error = TaskError {
            message: "boom".into(),
            retriable: true,
            attempt: 1,
        };
        board.fail("test-task", &error).unwrap();

        assert!(board.claim("test-task").unwrap());
    }
}
