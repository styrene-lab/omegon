//! Registry-native loop job command surface.
//!
//! This is the first daemon-prerequisite slice for recurring prompt work: a
//! durable job definition store, `/loop` command metadata, agent tool surfaces,
//! and shared helpers consumed by the daemon loop scheduler.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use omegon_traits::{
    CommandDefinition, CommandResult, CommandSafety, ContentBlock, Feature, ToolDefinition,
    ToolResult,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoopJob {
    pub id: String,
    pub prompt: String,
    pub trigger: LoopTrigger,
    pub stop: LoopStop,
    pub concurrency: LoopConcurrencyPolicy,
    pub enabled: bool,
    pub prompt_path: String,
    pub prompt_sha256: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum LoopTrigger {
    Now,
    Every { duration: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum LoopStop {
    OperatorStop,
    MaxRuns { max_runs: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LoopConcurrencyPolicy {
    SkipIfRunning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoopRunRecord {
    pub job_id: String,
    pub fired_at: String,
    pub outcome: String,
    pub message: String,
}

pub fn jobs_path(project_root: &Path) -> PathBuf {
    project_root.join(".omegon").join("loops").join("jobs.json")
}

pub fn runs_path(project_root: &Path) -> PathBuf {
    project_root
        .join(".omegon")
        .join("loops")
        .join("runs.jsonl")
}

pub fn load_jobs_from_project(project_root: &Path) -> anyhow::Result<Vec<LoopJob>> {
    load_jobs_at(&jobs_path(project_root))
}

pub fn save_jobs_to_project(project_root: &Path, jobs: &[LoopJob]) -> anyhow::Result<()> {
    save_jobs_at(&jobs_path(project_root), jobs)
}

pub fn append_run_record(project_root: &Path, record: &LoopRunRecord) -> anyhow::Result<()> {
    let path = runs_path(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{}", serde_json::to_string(record)?)?;
    Ok(())
}

pub fn last_run_at(project_root: &Path, job_id: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let content = std::fs::read_to_string(runs_path(project_root)).ok()?;
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<LoopRunRecord>(line).ok())
        .filter(|record| record.job_id == job_id && record.outcome == "dispatched")
        .filter_map(|record| chrono::DateTime::parse_from_rfc3339(&record.fired_at).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .max()
}

pub fn last_run_record(project_root: &Path, job_id: &str) -> Option<LoopRunRecord> {
    let content = std::fs::read_to_string(runs_path(project_root)).ok()?;
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<LoopRunRecord>(line).ok())
        .filter(|record| record.job_id == job_id)
        .filter_map(|record| {
            let ts = chrono::DateTime::parse_from_rfc3339(&record.fired_at).ok()?;
            Some((ts, record))
        })
        .max_by_key(|(ts, _)| *ts)
        .map(|(_, record)| record)
}

pub fn next_due_at(project_root: &Path, job: &LoopJob) -> Option<chrono::DateTime<chrono::Utc>> {
    let LoopTrigger::Every { duration } = &job.trigger else {
        return None;
    };
    let interval = parse_loop_duration(duration)?;
    Some(last_run_at(project_root, &job.id).map_or_else(chrono::Utc::now, |last| last + interval))
}

pub fn run_count(project_root: &Path, job_id: &str) -> usize {
    let Ok(content) = std::fs::read_to_string(runs_path(project_root)) else {
        return 0;
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<LoopRunRecord>(line).ok())
        .filter(|record| record.job_id == job_id && record.outcome == "dispatched")
        .count()
}

pub fn parse_loop_duration(value: &str) -> Option<chrono::Duration> {
    let value = value.trim();
    let (num, unit) = value.split_at(value.len().saturating_sub(1));
    let n = num.parse::<i64>().ok()?;
    if n <= 0 {
        return None;
    }
    match unit {
        "s" => Some(chrono::Duration::seconds(n)),
        "m" => Some(chrono::Duration::minutes(n)),
        "h" => Some(chrono::Duration::hours(n)),
        "d" => Some(chrono::Duration::days(n)),
        _ => None,
    }
}

fn load_jobs_at(path: &Path) -> anyhow::Result<Vec<LoopJob>> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(serde_json::from_str(&content)?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err.into()),
    }
}

fn save_jobs_at(path: &Path, jobs: &[LoopJob]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(jobs)?)?;
    Ok(())
}

pub struct LoopFeature {
    project_root: PathBuf,
    store_path: PathBuf,
}

impl LoopFeature {
    pub fn new(project_root: &Path) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            store_path: jobs_path(project_root),
        }
    }

    fn help() -> String {
        Self::menu().render_markdown()
    }

    fn menu() -> crate::surfaces::palette::PaletteProjection {
        use crate::surfaces::palette::{
            PaletteGroupProjection, PaletteProjection, PaletteRowProjection,
        };

        PaletteProjection::new("Loop jobs")
            .with_summary("Create recurring prompt jobs without remembering cron syntax. Agents can use loop_* tools; operators can use these slash-command recipes.")
            .with_group(
                PaletteGroupProjection::new("Common schedules")
                    .with_description("Pick a human interval first; cron/file/git triggers stay daemon-internal until the scheduler slice is wired.")
                    .with_row(PaletteRowProjection::action("loop.every.15m", "Every 15 minutes", "Good for polling short-lived work").with_command("/loop start <prompt> --every 15m"))
                    .with_row(PaletteRowProjection::action("loop.every.hour", "Hourly", "Good for inbox, board, or PR review").with_command("/loop start <prompt> --every 1h"))
                    .with_row(PaletteRowProjection::action("loop.every.day", "Daily", "Good for morning/evening summaries").with_command("/loop start <prompt> --every 24h"))
                    .with_row(PaletteRowProjection::action("loop.max.runs", "Bounded run", "Stop automatically after a fixed number of runs").with_command("/loop start <prompt> --every 1h --max-runs 5")),
            )
            .with_group(
                PaletteGroupProjection::new("Manage jobs")
                    .with_row(PaletteRowProjection::action("loop.list", "List jobs", "Show durable loop definitions").with_command("/loop list"))
                    .with_row(PaletteRowProjection::action("loop.status", "Inspect job", "Show one job as JSON, including prompt hash").with_command("/loop status <id>"))
                    .with_row(PaletteRowProjection::action("loop.stop", "Stop job", "Disable a loop job without deleting its record").with_command("/loop stop <id>")),
            )
            .with_footer("First slice: definitions only. Jobs bind to prompt path + SHA-256; daemon execution lands next.")
    }

    fn load_jobs(&self) -> anyhow::Result<Vec<LoopJob>> {
        load_jobs_at(&self.store_path)
    }

    fn save_jobs(&self, jobs: &[LoopJob]) -> anyhow::Result<()> {
        save_jobs_at(&self.store_path, jobs)
    }

    fn list(&self) -> anyhow::Result<String> {
        let jobs = self.load_jobs()?;
        if jobs.is_empty() {
            return Ok("No loop jobs registered.".into());
        }
        let mut out = String::from("## Loop jobs\n");
        for job in jobs {
            let trigger = match &job.trigger {
                LoopTrigger::Now => "now".to_string(),
                LoopTrigger::Every { duration } => format!("every {duration}"),
            };
            let state = if job.enabled { "enabled" } else { "stopped" };
            let runs = run_count(&self.project_root, &job.id);
            let last = last_run_record(&self.project_root, &job.id)
                .map(|record| format!("last:{}@{}", record.outcome, record.fired_at))
                .unwrap_or_else(|| "last:never".into());
            let next = if job.enabled {
                next_due_at(&self.project_root, &job)
                    .map(|due| format!("next:{}", due.to_rfc3339()))
                    .unwrap_or_else(|| "next:unknown".into())
            } else {
                "next:disabled".into()
            };
            out.push_str(&format!(
                "- `{}` — {} · prompt `{}` · {} · runs {} · {} · {} · hash {}\n",
                job.id,
                state,
                job.prompt,
                trigger,
                runs,
                last,
                next,
                &job.prompt_sha256[..12.min(job.prompt_sha256.len())]
            ));
        }
        Ok(out)
    }

    fn status(&self, id: &str) -> anyhow::Result<String> {
        let jobs = self.load_jobs()?;
        let Some(job) = jobs.into_iter().find(|job| job.id == id) else {
            anyhow::bail!("unknown loop job '{id}'");
        };
        Ok(serde_json::to_string_pretty(&job)?)
    }

    fn stop(&self, id: &str) -> anyhow::Result<String> {
        let mut jobs = self.load_jobs()?;
        let Some(job) = jobs.iter_mut().find(|job| job.id == id) else {
            anyhow::bail!("unknown loop job '{id}'");
        };
        job.enabled = false;
        self.save_jobs(&jobs)?;
        Ok(format!("Stopped loop job `{id}`."))
    }

    fn start(&self, args: &str) -> anyhow::Result<String> {
        let parts: Vec<&str> = args.split_whitespace().collect();
        let Some(prompt) = parts.first().copied() else {
            anyhow::bail!("/loop start requires a prompt name");
        };
        let every = option_value(&parts, "--every")
            .ok_or_else(|| anyhow::anyhow!("/loop start currently requires --every <duration>"))?;
        let max_runs = option_value(&parts, "--max-runs")
            .map(str::parse::<u32>)
            .transpose()?;
        self.create_job(prompt, every, max_runs)
    }

    fn create_job(
        &self,
        prompt: &str,
        every: &str,
        max_runs: Option<u32>,
    ) -> anyhow::Result<String> {
        let (_manifest, body, path) = crate::prompts::get_prompt(prompt)?;
        let safety = crate::prompts::safety_verdict(&body);
        if safety.is_blocked() {
            anyhow::bail!("prompt is blocked by safety verdict: {safety:?}");
        }

        let mut hasher = Sha256::new();
        hasher.update(body.as_bytes());
        let prompt_sha256 = format!("{:x}", hasher.finalize());
        let id = format!("loop-{}", uuid::Uuid::new_v4().simple());
        let job = LoopJob {
            id: id.clone(),
            prompt: prompt.to_string(),
            trigger: LoopTrigger::Every {
                duration: every.to_string(),
            },
            stop: max_runs
                .map(|max_runs| LoopStop::MaxRuns { max_runs })
                .unwrap_or(LoopStop::OperatorStop),
            concurrency: LoopConcurrencyPolicy::SkipIfRunning,
            enabled: true,
            prompt_path: path.display().to_string(),
            prompt_sha256,
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        let mut jobs = self.load_jobs()?;
        jobs.push(job);
        self.save_jobs(&jobs)?;
        Ok(format!(
            "Registered loop job `{id}` for prompt `{prompt}` every {every}. Execution is pending daemon scheduler wiring."
        ))
    }
}

fn option_value<'a>(parts: &'a [&str], flag: &str) -> Option<&'a str> {
    parts
        .windows(2)
        .find_map(|window| (window[0] == flag).then_some(window[1]))
}

#[async_trait]
impl Feature for LoopFeature {
    fn name(&self) -> &str {
        "loop"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: crate::tool_registry::loop_jobs::LOOP_LIST.into(),
                label: "loop_list".into(),
                description: "List durable loop job definitions.".into(),
                parameters: json!({"type": "object", "properties": {}}),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
            },
            ToolDefinition {
                name: crate::tool_registry::loop_jobs::LOOP_CREATE.into(),
                label: "loop_create".into(),
                description: "Create a durable recurring prompt job using a human interval such as 15m, 1h, or 24h. Jobs are definitions only until daemon scheduler execution is wired.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "prompt": {"type": "string", "description": "Prompt id to run"},
                        "every": {"type": "string", "description": "Human duration: 15m, 1h, 24h"},
                        "max_runs": {"type": "integer", "minimum": 1, "description": "Optional automatic stop after N runs"}
                    },
                    "required": ["prompt", "every"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: crate::tool_registry::loop_jobs::LOOP_STATUS.into(),
                label: "loop_status".into(),
                description: "Inspect one durable loop job definition by id.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {"id": {"type": "string"}},
                    "required": ["id"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
            },
            ToolDefinition {
                name: crate::tool_registry::loop_jobs::LOOP_STOP.into(),
                label: "loop_stop".into(),
                description: "Disable a durable loop job definition by id.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {"id": {"type": "string"}},
                    "required": ["id"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let text = match tool_name {
            crate::tool_registry::loop_jobs::LOOP_LIST => self.list()?,
            crate::tool_registry::loop_jobs::LOOP_CREATE => {
                let prompt = args["prompt"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("loop_create requires prompt"))?;
                let every = args["every"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("loop_create requires every"))?;
                let max_runs = args["max_runs"].as_u64().map(|n| n as u32);
                self.create_job(prompt, every, max_runs)?
            }
            crate::tool_registry::loop_jobs::LOOP_STATUS => {
                let id = args["id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("loop_status requires id"))?;
                self.status(id)?
            }
            crate::tool_registry::loop_jobs::LOOP_STOP => {
                let id = args["id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("loop_stop requires id"))?;
                self.stop(id)?
            }
            _ => anyhow::bail!("Unknown tool: {tool_name}"),
        };
        Ok(ToolResult {
            content: vec![ContentBlock::Text { text }],
            details: json!({}),
        })
    }

    fn commands(&self) -> Vec<CommandDefinition> {
        vec![CommandDefinition {
            name: "loop".into(),
            description: "Register and inspect durable recurring prompt jobs".into(),
            subcommands: vec![
                "menu".into(),
                "list".into(),
                "start".into(),
                "stop".into(),
                "status".into(),
            ],
            availability: omegon_traits::CommandAvailability::ALL,
            safety: CommandSafety::QUEUE_MUTATION,
        }]
    }

    fn handle_command(&mut self, name: &str, args: &str) -> CommandResult {
        if name != "loop" {
            return CommandResult::NotHandled;
        }
        let args = args.trim();
        let result = if args.is_empty() || args == "help" {
            Ok(Self::help())
        } else {
            let (subcommand, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
            let rest = rest.trim();
            match subcommand {
                "menu" => Ok(Self::menu().render_markdown()),
                "list" => self.list(),
                "start" => self.start(rest),
                "stop" if !rest.is_empty() => self.stop(rest),
                "status" if !rest.is_empty() => self.status(rest),
                "stop" | "status" => Err(anyhow::anyhow!("/loop {subcommand} requires a job id")),
                other => Err(anyhow::anyhow!("unknown /loop subcommand '{other}'")),
            }
        };
        match result {
            Ok(output) => CommandResult::Display(output),
            Err(err) => CommandResult::Display(format!("/loop failed: {err}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omegon_traits::{CommandSafetyClass, Feature};

    #[test]
    fn loop_command_declares_registry_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let feature = LoopFeature::new(dir.path());
        let command = feature.commands().into_iter().next().unwrap();
        assert_eq!(command.name, "loop");
        assert!(command.availability.tui);
        assert!(command.availability.cli);
        assert!(command.availability.acp);
        assert_eq!(command.safety.class, CommandSafetyClass::QueueMutation);
        assert!(command.safety.prompt_injection_sensitive);
    }

    #[test]
    fn empty_loop_list_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let feature = LoopFeature::new(dir.path());
        assert_eq!(feature.list().unwrap(), "No loop jobs registered.");
    }

    #[test]
    fn loop_duration_parses_human_intervals() {
        assert_eq!(
            parse_loop_duration("15m"),
            Some(chrono::Duration::minutes(15))
        );
        assert_eq!(parse_loop_duration("1h"), Some(chrono::Duration::hours(1)));
        assert_eq!(
            parse_loop_duration("24h"),
            Some(chrono::Duration::hours(24))
        );
        assert_eq!(parse_loop_duration("daily"), None);
    }
}
