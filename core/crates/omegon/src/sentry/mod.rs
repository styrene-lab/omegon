pub mod board;
pub mod executor;
pub mod file_board;
pub mod flynt_board;
pub mod routes;
pub mod state_db;
pub mod tree_board;
pub mod types;

#[cfg(test)]
mod integration_tests;

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

pub use board::TaskBoard;
pub use file_board::FileTaskBoard;
pub use flynt_board::{FlyntTaskBoard, is_flynt_vault};

#[derive(Debug, Clone, Deserialize)]
pub struct SentryConfig {
    pub sentry: SentryGlobal,
    #[serde(default, rename = "task")]
    pub tasks: Vec<SentryTaskConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SentryGlobal {
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
    #[serde(default = "default_log_retention_days")]
    pub log_retention_days: u32,
    #[serde(default)]
    pub routing: Option<RoutingConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoutingConfig {
    pub prefilter_model: String,
    pub light_model: String,
    pub heavy_model: String,
}

fn default_max_concurrent() -> usize { 1 }
fn default_log_retention_days() -> u32 { 30 }

#[derive(Debug, Clone, Deserialize)]
pub struct SentryTaskConfig {
    pub name: String,
    pub prompt: Option<String>,
    pub prompt_file: Option<String>,
    pub model: Option<String>,
    pub skill: Option<String>,
    pub max_turns: Option<u32>,
    pub timeout_secs: Option<u64>,
    pub token_budget: Option<u64>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    pub execution_mode: Option<String>,
    pub trigger: Option<TriggerConfig>,
    pub budget: Option<BudgetConfig>,
    pub priority: Option<u8>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TriggerConfig {
    pub cron: Option<CronTrigger>,
    pub webhook: Option<WebhookTrigger>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CronTrigger {
    pub schedule: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebhookTrigger {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BudgetConfig {
    pub max_tokens_per_day: Option<u64>,
    pub max_cost_per_day_usd: Option<f64>,
}

pub fn load_config(path: &Path) -> anyhow::Result<SentryConfig> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        anyhow::anyhow!("failed to read sentry config {}: {e}", path.display())
    })?;
    let config: SentryConfig = toml::from_str(&content).map_err(|e| {
        anyhow::anyhow!("failed to parse sentry config {}: {e}", path.display())
    })?;

    for task in &config.tasks {
        if task.prompt.is_none() && task.prompt_file.is_none() {
            anyhow::bail!(
                "sentry task '{}' must have either 'prompt' or 'prompt_file'",
                task.name
            );
        }
    }

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE_CONFIG: &str = r#"
[sentry]
max_concurrent = 3
log_retention_days = 30

[[task]]
name = "pr-review"
prompt = "Review all open PRs, leave comments on issues found"
model = "anthropic:claude-sonnet-4-6"
max_turns = 20
timeout_secs = 300

[task.trigger.cron]
schedule = "0 */4 * * *"

[task.trigger.webhook]
name = "github-pr"

[task.budget]
max_tokens_per_day = 500000
max_cost_per_day_usd = 5.00

[[task]]
name = "staging-monitor"
prompt = "Check staging health"
model = "anthropic:claude-haiku-4-5-20251001"
max_turns = 10

[task.trigger.cron]
schedule = "*/30 * * * *"
"#;

    #[test]
    fn parse_full_config() {
        let config: SentryConfig = toml::from_str(EXAMPLE_CONFIG).unwrap();
        assert_eq!(config.sentry.max_concurrent, 3);
        assert_eq!(config.sentry.log_retention_days, 30);
        assert_eq!(config.tasks.len(), 2);

        let pr = &config.tasks[0];
        assert_eq!(pr.name, "pr-review");
        assert_eq!(pr.prompt.as_deref(), Some("Review all open PRs, leave comments on issues found"));
        assert_eq!(pr.model.as_deref(), Some("anthropic:claude-sonnet-4-6"));
        assert_eq!(pr.max_turns, Some(20));
        assert_eq!(pr.timeout_secs, Some(300));

        let trig = pr.trigger.as_ref().unwrap();
        assert_eq!(trig.cron.as_ref().unwrap().schedule, "0 */4 * * *");
        assert_eq!(trig.webhook.as_ref().unwrap().name, "github-pr");

        let budget = pr.budget.as_ref().unwrap();
        assert_eq!(budget.max_tokens_per_day, Some(500000));
        assert_eq!(budget.max_cost_per_day_usd, Some(5.0));
    }

    #[test]
    fn parse_minimal_config() {
        let toml_str = r#"
[sentry]

[[task]]
name = "simple"
prompt = "do it"
"#;
        let config: SentryConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.sentry.max_concurrent, 1);
        assert_eq!(config.sentry.log_retention_days, 30);
        assert_eq!(config.tasks.len(), 1);
        assert_eq!(config.tasks[0].name, "simple");
        assert!(config.tasks[0].trigger.is_none());
        assert!(config.tasks[0].budget.is_none());
    }

    #[test]
    fn parse_empty_tasks() {
        let toml_str = r#"
[sentry]
max_concurrent = 2
"#;
        let config: SentryConfig = toml::from_str(toml_str).unwrap();
        assert!(config.tasks.is_empty());
    }

    #[test]
    fn parse_cron_only_trigger() {
        let toml_str = r#"
[sentry]

[[task]]
name = "cron-only"
prompt = "check"

[task.trigger.cron]
schedule = "0 9 * * 1-5"
"#;
        let config: SentryConfig = toml::from_str(toml_str).unwrap();
        let trig = config.tasks[0].trigger.as_ref().unwrap();
        assert!(trig.cron.is_some());
        assert!(trig.webhook.is_none());
    }

    #[test]
    fn parse_webhook_only_trigger() {
        let toml_str = r#"
[sentry]

[[task]]
name = "webhook-only"
prompt = "deploy check"

[task.trigger.webhook]
name = "deploy"
"#;
        let config: SentryConfig = toml::from_str(toml_str).unwrap();
        let trig = config.tasks[0].trigger.as_ref().unwrap();
        assert!(trig.cron.is_none());
        assert!(trig.webhook.is_some());
    }

    #[test]
    fn load_config_validates_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sentry.toml");
        std::fs::write(&path, r#"
[sentry]
[[task]]
name = "bad"
"#).unwrap();
        assert!(load_config(&path).is_err());
    }

    #[test]
    fn parse_task_with_priority() {
        let toml_str = r#"
[sentry]

[[task]]
name = "high-pri"
prompt = "urgent"
priority = 3
"#;
        let config: SentryConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.tasks[0].priority, Some(3));
    }
}
