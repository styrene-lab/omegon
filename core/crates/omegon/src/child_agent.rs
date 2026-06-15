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
    /// Force slim mode on the child (compact schemas, lazy tool injection,
    /// reduced prompt surface). Delegate workers always set this.
    pub slim: bool,

    /// Nex profile name — when set, the child spawns inside an OCI container
    /// with sandbox isolation defined by the named profile.
    pub nex_profile: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChildAgentBoundary {
    pub cwd: PathBuf,
    pub readable_paths: Vec<String>,
    pub writable_paths: Vec<String>,
    pub disabled_tools: Vec<String>,
    pub enabled_tools: Vec<String>,
    pub sandbox_profile: Option<String>,
    pub notes: Vec<String>,
}

impl ChildAgentBoundary {
    pub fn from_runtime(cwd: &Path, runtime: &ChildAgentRuntimeProfile) -> Self {
        let scope = runtime.preloaded_files.clone();
        Self {
            cwd: std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf()),
            readable_paths: scope.clone(),
            writable_paths: scope,
            disabled_tools: runtime.disabled_tools.clone(),
            enabled_tools: runtime.enabled_tools.clone(),
            sandbox_profile: runtime.nex_profile.clone(),
            notes: Vec::new(),
        }
    }

    pub fn to_prompt_section(&self) -> String {
        fn list_or_none(items: &[String]) -> String {
            if items.is_empty() {
                "- none declared\n".to_string()
            } else {
                items.iter().map(|item| format!("- {item}\n")).collect()
            }
        }

        let mut out = String::from("## Execution Boundary\n");
        out.push_str(&format!("- Working directory: {}\n", self.cwd.display()));
        out.push_str("- Treat this boundary as authoritative. If a required read/write/tool is outside it, stop and report the blocker instead of guessing or broadening scope.\n");
        out.push_str("\nReadable paths/scope:\n");
        out.push_str(&list_or_none(&self.readable_paths));
        out.push_str("\nWritable paths/scope:\n");
        out.push_str(&list_or_none(&self.writable_paths));
        out.push_str("\nEnabled tools:\n");
        out.push_str(&list_or_none(&self.enabled_tools));
        out.push_str("\nUnavailable tools/resources:\n");
        out.push_str(&list_or_none(&self.disabled_tools));
        if let Some(profile) = &self.sandbox_profile {
            out.push_str(&format!("\nSandbox profile: {profile}\n"));
        }
        if !self.notes.is_empty() {
            out.push_str("\nBoundary notes:\n");
            out.push_str(&list_or_none(&self.notes));
        }
        out.push('\n');
        out
    }
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
        .env("OMEGON_NO_KEYRING", "1")
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
    if config.runtime.slim {
        child.env("OMEGON_CHILD_SLIM", "1");
    }
    let child = child
        .spawn()
        .context("Failed to spawn headless child agent")?;
    let pid = child.id().unwrap_or(0);
    Ok((child, pid))
}

#[cfg(test)]
mod boundary_tests {
    use super::*;

    #[test]
    fn child_agent_boundary_prompt_section_lists_scope_tools_and_blocker_guidance() {
        let runtime = ChildAgentRuntimeProfile {
            preloaded_files: vec!["src/lib.rs".into()],
            enabled_tools: vec!["read".into(), "bash".into()],
            disabled_tools: vec!["delegate".into(), "cleave_run".into()],
            nex_profile: Some("delegate-sandbox".into()),
            ..Default::default()
        };
        let boundary = ChildAgentBoundary::from_runtime(Path::new("/workspace/project"), &runtime);
        let prompt = boundary.to_prompt_section();

        assert!(prompt.contains("## Execution Boundary"));
        assert!(prompt.contains("src/lib.rs"));
        assert!(prompt.contains("read"));
        assert!(prompt.contains("delegate"));
        assert!(prompt.contains("cleave_run"));
        assert!(prompt.contains("Sandbox profile: delegate-sandbox"));
        assert!(prompt.contains("stop and report the blocker"));
    }
}
