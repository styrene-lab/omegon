use std::path::PathBuf;
use std::sync::Arc;

use super::board::TaskBoard;
use super::state_db::StateDb;
use super::types::{SentryTask, TaskError, TaskResult, TaskSpec, Trigger};
use crate::task_tree;

pub struct TaskTreeBoard {
    cwd: PathBuf,
    state_db: Arc<StateDb>,
    instance_id: String,
}

impl TaskTreeBoard {
    pub fn new(cwd: PathBuf, state_db: Arc<StateDb>, instance_id: String) -> Self {
        Self { cwd, state_db, instance_id }
    }
}

impl TaskBoard for TaskTreeBoard {
    fn list_actionable(&self) -> anyhow::Result<Vec<SentryTask>> {
        let tasks = task_tree::actionable_tasks(&self.cwd)?;
        let mut out = Vec::with_capacity(tasks.len());

        for t in &tasks {
            let (last_run, run_count) = self.state_db.last_run(&t.meta.id)?
                .map(|(dt, c)| (Some(dt), c))
                .unwrap_or((None, 0));

            let mut triggers = Vec::new();
            if let Some(ref exec) = t.meta.execution {
                if let Some(ref cron) = exec.cron {
                    triggers.push(Trigger::Cron { schedule: cron.clone() });
                }
                if let Some(ref wh) = exec.webhook {
                    triggers.push(Trigger::Webhook { name: wh.clone() });
                }
            }
            if triggers.is_empty() {
                triggers.push(Trigger::Manual);
            }

            out.push(SentryTask {
                id: t.meta.id.clone(),
                name: t.meta.title.clone(),
                priority: t.meta.priority.as_u8(),
                triggers,
                last_run,
                run_count,
            });
        }

        Ok(out)
    }

    fn claim(&self, task_id: &str) -> anyhow::Result<bool> {
        self.state_db.claim_task(task_id, &self.instance_id)
    }

    fn release(&self, task_id: &str) -> anyhow::Result<()> {
        self.state_db.release_task(task_id)
    }

    fn complete(&self, task_id: &str, _result: &TaskResult) -> anyhow::Result<()> {
        if let Err(e) = task_tree::update_status(&self.cwd, task_id, task_tree::TaskStatus::Done) {
            tracing::warn!(task = %task_id, error = %e, "failed to update task file — releasing claim anyway");
        }
        self.state_db.release_task(task_id)?;
        Ok(())
    }

    fn fail(&self, task_id: &str, _error: &TaskError) -> anyhow::Result<()> {
        if let Err(e) = task_tree::update_status(&self.cwd, task_id, task_tree::TaskStatus::Failed) {
            tracing::warn!(task = %task_id, error = %e, "failed to update task file — releasing claim anyway");
        }
        self.state_db.release_task(task_id)?;
        Ok(())
    }

    fn task_spec(&self, task_id: &str) -> anyhow::Result<TaskSpec> {
        let task = task_tree::get_task(&self.cwd, task_id)?;

        let prompt = if task.body.trim().is_empty() {
            task.meta.title.clone()
        } else {
            task.body.clone()
        };

        let (model, skill, max_turns, timeout_secs, token_budget) =
            if let Some(ref exec) = task.meta.execution {
                (
                    exec.model.clone(),
                    exec.skill.clone(),
                    exec.max_turns,
                    exec.timeout_secs,
                    exec.token_budget,
                )
            } else {
                (None, None, None, None, None)
            };

        Ok(TaskSpec {
            prompt,
            model,
            skill,
            max_turns,
            timeout_secs,
            token_budget,
            cwd: Some(self.cwd.clone()),
            env: Default::default(),
            design_node_id: task.meta.design_node_id.clone(),
            openspec_change: task.meta.openspec_change.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tree_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(StateDb::in_memory().unwrap());
        let board = TaskTreeBoard::new(dir.path().to_path_buf(), db, "test".into());
        let tasks = board.list_actionable().unwrap();
        assert!(tasks.is_empty());
    }

    #[test]
    fn tasks_appear_as_actionable() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        task_tree::create_task(cwd, "Alpha task", "Do alpha").unwrap();
        task_tree::create_task(cwd, "Beta task", "Do beta").unwrap();

        let db = Arc::new(StateDb::in_memory().unwrap());
        let board = TaskTreeBoard::new(cwd.to_path_buf(), db, "test".into());
        let tasks = board.list_actionable().unwrap();
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn done_tasks_not_actionable() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        task_tree::create_task(cwd, "Active", "Go").unwrap();
        task_tree::create_task(cwd, "Finished", "Done").unwrap();
        task_tree::update_status(cwd, "finished", task_tree::TaskStatus::Done).unwrap();

        let db = Arc::new(StateDb::in_memory().unwrap());
        let board = TaskTreeBoard::new(cwd.to_path_buf(), db, "test".into());
        let tasks = board.list_actionable().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "active");
    }

    #[test]
    fn complete_marks_done_in_tree() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        task_tree::create_task(cwd, "Work item", "Do it").unwrap();

        let db = Arc::new(StateDb::in_memory().unwrap());
        let board = TaskTreeBoard::new(cwd.to_path_buf(), db, "test".into());
        board.claim("work-item").unwrap();

        let result = TaskResult {
            exit_code: 0,
            summary: "done".into(),
            tokens_used: 100,
            duration_secs: 5,
            session_id: "s1".into(),
        };
        board.complete("work-item", &result).unwrap();

        let task = task_tree::get_task(cwd, "work-item").unwrap();
        assert_eq!(task.meta.status, task_tree::TaskStatus::Done);
    }

    #[test]
    fn fail_marks_failed_in_tree() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        task_tree::create_task(cwd, "Fragile", "Might break").unwrap();

        let db = Arc::new(StateDb::in_memory().unwrap());
        let board = TaskTreeBoard::new(cwd.to_path_buf(), db, "test".into());
        board.claim("fragile").unwrap();

        let error = TaskError {
            message: "boom".into(),
            retriable: false,
            attempt: 3,
        };
        board.fail("fragile", &error).unwrap();

        let task = task_tree::get_task(cwd, "fragile").unwrap();
        assert_eq!(task.meta.status, task_tree::TaskStatus::Failed);
    }

    #[test]
    fn task_spec_reads_body_as_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        task_tree::create_task(cwd, "Prompt test", "Review all open PRs and leave comments").unwrap();

        let db = Arc::new(StateDb::in_memory().unwrap());
        let board = TaskTreeBoard::new(cwd.to_path_buf(), db, "test".into());
        let spec = board.task_spec("prompt-test").unwrap();
        assert!(spec.prompt.contains("Review all open PRs"));
    }

    #[test]
    fn task_spec_falls_back_to_title() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        task_tree::create_task(cwd, "Fix the auth bug", "").unwrap();

        let db = Arc::new(StateDb::in_memory().unwrap());
        let board = TaskTreeBoard::new(cwd.to_path_buf(), db, "test".into());
        let spec = board.task_spec("fix-the-auth-bug").unwrap();
        assert_eq!(spec.prompt, "Fix the auth bug");
    }

    #[test]
    fn task_spec_includes_execution_params() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        let mut task = task_tree::create_task(cwd, "Configured", "Do it").unwrap();
        task.meta.execution = Some(task_tree::ExecutionSpec {
            model: Some("anthropic:claude-opus-4-6".into()),
            max_turns: Some(50),
            timeout_secs: Some(900),
            ..Default::default()
        });
        task.meta.design_node_id = Some("node-abc".into());
        task_tree::save_task(cwd, &task).unwrap();

        let db = Arc::new(StateDb::in_memory().unwrap());
        let board = TaskTreeBoard::new(cwd.to_path_buf(), db, "test".into());
        let spec = board.task_spec("configured").unwrap();
        assert_eq!(spec.model.as_deref(), Some("anthropic:claude-opus-4-6"));
        assert_eq!(spec.max_turns, Some(50));
        assert_eq!(spec.timeout_secs, Some(900));
        assert_eq!(spec.design_node_id.as_deref(), Some("node-abc"));
    }

    #[test]
    fn dependency_gating_through_board() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();

        task_tree::create_task(cwd, "First step", "A").unwrap();
        let mut second = task_tree::create_task(cwd, "Second step", "B").unwrap();
        second.meta.depends_on = vec!["first-step".into()];
        task_tree::save_task(cwd, &second).unwrap();

        let db = Arc::new(StateDb::in_memory().unwrap());
        let board = TaskTreeBoard::new(cwd.to_path_buf(), db, "test".into());

        let tasks = board.list_actionable().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "first-step");

        // Complete first step via board
        board.claim("first-step").unwrap();
        let result = TaskResult {
            exit_code: 0, summary: "ok".into(),
            tokens_used: 50, duration_secs: 2, session_id: "s".into(),
        };
        board.complete("first-step", &result).unwrap();

        // Now second step is actionable
        let tasks = board.list_actionable().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "second-step");
    }

    #[test]
    fn triggers_from_execution_spec() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        let mut task = task_tree::create_task(cwd, "Scheduled", "Run it").unwrap();
        task.meta.execution = Some(task_tree::ExecutionSpec {
            cron: Some("0 */4 * * *".into()),
            webhook: Some("deploy".into()),
            ..Default::default()
        });
        task_tree::save_task(cwd, &task).unwrap();

        let db = Arc::new(StateDb::in_memory().unwrap());
        let board = TaskTreeBoard::new(cwd.to_path_buf(), db, "test".into());
        let tasks = board.list_actionable().unwrap();
        assert_eq!(tasks[0].triggers.len(), 2);
    }
}
