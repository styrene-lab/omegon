//! Registry-native loop job command surface.
//!
//! This is the first daemon-prerequisite slice for recurring prompt work: a
//! durable job definition store and `/loop` command metadata. It deliberately
//! does not execute jobs yet; daemon execution can consume this stable on-disk
//! model in the next slice.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use omegon_traits::{CommandDefinition, CommandResult, CommandSafety, Feature};
use serde::{Deserialize, Serialize};
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

pub struct LoopFeature {
    store_path: PathBuf,
}

impl LoopFeature {
    pub fn new(project_root: &Path) -> Self {
        Self {
            store_path: project_root.join(".omegon").join("loops").join("jobs.json"),
        }
    }

    fn help() -> String {
        [
            "Loop jobs",
            "",
            "Usage:",
            "  /loop list",
            "  /loop start <prompt> --every <duration> [--max-runs <n>]",
            "  /loop stop <id>",
            "  /loop status <id>",
            "",
            "First slice: durable loop definitions only. Daemon execution is a follow-up slice.",
            "Loop jobs bind to the prompt file path and SHA-256 hash at creation time.",
        ]
        .join("\n")
    }

    fn load_jobs(&self) -> anyhow::Result<Vec<LoopJob>> {
        match std::fs::read_to_string(&self.store_path) {
            Ok(content) => Ok(serde_json::from_str(&content)?),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(err) => Err(err.into()),
        }
    }

    fn save_jobs(&self, jobs: &[LoopJob]) -> anyhow::Result<()> {
        if let Some(parent) = self.store_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.store_path, serde_json::to_string_pretty(jobs)?)?;
        Ok(())
    }

    fn list(&self) -> anyhow::Result<String> {
        let jobs = self.load_jobs()?;
        if jobs.is_empty() {
            return Ok("No loop jobs registered.".into());
        }
        let mut out = String::from("## Loop jobs\n");
        for job in jobs {
            let trigger = match job.trigger {
                LoopTrigger::Now => "now".to_string(),
                LoopTrigger::Every { duration } => format!("every {duration}"),
            };
            let state = if job.enabled { "enabled" } else { "stopped" };
            out.push_str(&format!(
                "- `{}` — {} · prompt `{}` · {} · hash {}\n",
                job.id,
                state,
                job.prompt,
                trigger,
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

    fn commands(&self) -> Vec<CommandDefinition> {
        vec![CommandDefinition {
            name: "loop".into(),
            description: "Register and inspect durable recurring prompt jobs".into(),
            subcommands: vec![
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
}
