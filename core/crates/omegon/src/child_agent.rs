use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::Context;
use tokio::process::{Child, Command};

#[derive(Debug, Clone, Default)]
pub struct ChildAgentRuntimeProfile {
    pub context_class: Option<String>,
    pub thinking_level: Option<String>,
    pub enabled_tools: Vec<String>,
    pub disabled_tools: Vec<String>,
    pub skills: Vec<String>,
    pub enabled_extensions: Vec<String>,
    pub disabled_extensions: Vec<String>,
    pub preloaded_files: Vec<String>,
    pub persona: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChildAgentSpawnConfig {
    pub agent_binary: PathBuf,
    pub model: String,
    pub max_turns: u32,
    pub inherited_env: Vec<(String, String)>,
    pub injected_env: Vec<(String, String)>,
    pub runtime: ChildAgentRuntimeProfile,
}

pub fn write_child_prompt_file(
    cwd: &Path,
    file_name: &str,
    prompt: &str,
) -> anyhow::Result<PathBuf> {
    let prompt_file = std::fs::canonicalize(cwd)
        .unwrap_or_else(|_| cwd.to_path_buf())
        .join(file_name);
    std::fs::write(&prompt_file, prompt).with_context(|| {
        format!(
            "Failed to write child prompt file {}",
            prompt_file.display()
        )
    })?;
    Ok(prompt_file)
}

pub fn spawn_headless_child_agent(
    config: &ChildAgentSpawnConfig,
    cwd: &Path,
    prompt_file: &Path,
) -> anyhow::Result<(Child, u32)> {
    if !cwd.exists() {
        anyhow::bail!("Child cwd does not exist: {}", cwd.display());
    }
    let max_turns_str = config.max_turns.to_string();
    let canonical_cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let cwd_arg = canonical_cwd.to_string_lossy().to_string();
    let prompt_arg = prompt_file.to_string_lossy().to_string();
    let mut args = vec![
        "--prompt-file",
        prompt_arg.as_str(),
        "--cwd",
        cwd_arg.as_str(),
        "--model",
        config.model.as_str(),
        "--max-turns",
        &max_turns_str,
    ];
    if let Some(ref context_class) = config.runtime.context_class {
        args.extend(["--context-class", context_class.as_str()]);
    }

    let mut child = Command::new(&config.agent_binary);
    child
        .args(&args)
        .current_dir(cwd)
        .env("OMEGON_CHILD", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (key, value) in &config.inherited_env {
        child.env(key, value);
    }
    for (key, value) in &config.injected_env {
        child.env(key, value);
    }
    if let Some(ref thinking) = config.runtime.thinking_level {
        child.env("OMEGON_CHILD_THINKING_LEVEL", thinking);
    }
    if let Some(ref context_class) = config.runtime.context_class {
        child.env("OMEGON_CHILD_CONTEXT_CLASS", context_class);
    }
    if !config.runtime.enabled_tools.is_empty() {
        child.env(
            "OMEGON_CHILD_ENABLED_TOOLS",
            config.runtime.enabled_tools.join(","),
        );
    }
    if !config.runtime.disabled_tools.is_empty() {
        child.env(
            "OMEGON_CHILD_DISABLED_TOOLS",
            config.runtime.disabled_tools.join(","),
        );
    }
    if !config.runtime.skills.is_empty() {
        child.env("OMEGON_CHILD_SKILLS", config.runtime.skills.join(","));
    }
    if !config.runtime.enabled_extensions.is_empty() {
        child.env(
            "OMEGON_CHILD_ENABLED_EXTENSIONS",
            config.runtime.enabled_extensions.join(","),
        );
    }
    if !config.runtime.disabled_extensions.is_empty() {
        child.env(
            "OMEGON_CHILD_DISABLED_EXTENSIONS",
            config.runtime.disabled_extensions.join(","),
        );
    }
    if !config.runtime.preloaded_files.is_empty() {
        child.env(
            "OMEGON_CHILD_PRELOADED_FILES",
            config.runtime.preloaded_files.join(":"),
        );
    }
    if let Some(ref persona) = config.runtime.persona {
        child.env("OMEGON_CHILD_PERSONA", persona);
    }
    let child = child
        .spawn()
        .context("Failed to spawn headless child agent")?;
    let pid = child.id().unwrap_or(0);
    Ok((child, pid))
}
