use super::types::{SentryTask, TaskError, TaskResult, TaskSpec};

pub trait TaskBoard: Send + Sync {
    fn list_actionable(&self) -> anyhow::Result<Vec<SentryTask>>;

    fn claim(&self, task_id: &str) -> anyhow::Result<bool>;

    fn release(&self, task_id: &str) -> anyhow::Result<()>;

    fn complete(&self, task_id: &str, result: &TaskResult) -> anyhow::Result<()>;

    fn fail(&self, task_id: &str, error: &TaskError) -> anyhow::Result<()>;

    fn task_spec(&self, task_id: &str) -> anyhow::Result<TaskSpec>;
}
