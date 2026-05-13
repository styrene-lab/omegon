//! Container-aware child agent spawning.
//!
//! Parallels `spawn_headless_child_agent()` in child_agent.rs but runs
//! the child inside an OCI container defined by a NexProfile.

use std::path::Path;

use anyhow::{Context, Result};
use tokio::process::Child;

use super::container::materialize_container;
use super::profile::NexProfile;
use crate::child_agent::ChildAgentSpawnConfig;

/// Spawn a child agent inside an OCI container.
///
/// The container is configured from the `NexProfile`:
/// - Workspace mounted at `/work`
/// - Resource limits (memory, CPU, PIDs, read-only rootfs)
/// - Network isolation per profile capabilities
/// - Environment variables from inherited/injected env + passthrough list
/// - Tool allow/deny lists from both the runtime config AND the profile capabilities
///
/// Returns the child process handle and its PID.
pub fn spawn_containerized_child_agent(
    config: &ChildAgentSpawnConfig,
    profile: &NexProfile,
    cwd: &Path,
    prompt_file: &Path,
) -> Result<(Child, u32)> {
    let runtime = detect_container_runtime()
        .context("nex profile requires a container runtime (podman or docker)")?;

    // Resolve prompt file path relative to /work inside the container
    let canonical_cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let canonical_prompt =
        std::fs::canonicalize(prompt_file).unwrap_or_else(|_| prompt_file.to_path_buf());

    let prompt_path_in_container = if canonical_prompt.starts_with(&canonical_cwd) {
        let relative = canonical_prompt
            .strip_prefix(&canonical_cwd)
            .unwrap_or(canonical_prompt.as_ref());
        format!("/work/{}", relative.display())
    } else {
        "/prompt".to_string()
    };

    // Build agent CLI args — match child_agent.rs:67-76 format (separate key/value args)
    let max_turns_str = config.max_turns.to_string();
    let agent_args: Vec<String> = vec![
        "--prompt-file".to_string(),
        prompt_path_in_container,
        "--cwd".to_string(),
        "/work".to_string(),
        "--model".to_string(),
        config.model.clone(),
        "--max-turns".to_string(),
        max_turns_str,
    ];

    // Collect env vars — inherited + injected + runtime config env vars
    // (mirrors child_agent.rs:90-128)
    let mut env: Vec<(String, String)> = Vec::new();
    env.extend(config.inherited_env.iter().cloned());
    env.extend(config.injected_env.iter().cloned());

    if let Some(ref thinking) = config.runtime.thinking_level {
        env.push(("OMEGON_CHILD_THINKING_LEVEL".into(), thinking.clone()));
    }
    if let Some(ref ctx) = config.runtime.context_class {
        env.push(("OMEGON_CHILD_CONTEXT_CLASS".into(), ctx.clone()));
    }
    if config.runtime.slim {
        env.push(("OMEGON_CHILD_SLIM".into(), "1".into()));
    }
    if let Some(ref persona) = config.runtime.persona {
        env.push(("OMEGON_CHILD_PERSONA".into(), persona.clone()));
    }

    // Tool allow/deny — merge runtime config + profile capabilities (M5 fix)
    let mut enabled = config.runtime.enabled_tools.clone();
    if !profile.capabilities.allowed_tools.is_empty() {
        // Profile allowlist intersects with runtime enabled list
        // (if runtime has no allowlist, profile's list becomes the allowlist)
        if enabled.is_empty() {
            enabled = profile.capabilities.allowed_tools.clone();
        }
    }
    let mut disabled = config.runtime.disabled_tools.clone();
    for denied in &profile.capabilities.denied_tools {
        if !disabled.contains(denied) {
            disabled.push(denied.clone());
        }
    }
    if !enabled.is_empty() {
        env.push(("OMEGON_CHILD_ENABLED_TOOLS".into(), enabled.join(",")));
    }
    if !disabled.is_empty() {
        env.push(("OMEGON_CHILD_DISABLED_TOOLS".into(), disabled.join(",")));
    }

    if !config.runtime.skills.is_empty() {
        env.push((
            "OMEGON_CHILD_SKILLS".into(),
            config.runtime.skills.join(","),
        ));
    }

    let std_cmd = materialize_container(profile, &runtime, cwd, prompt_file, &agent_args, &env);

    // Convert to tokio async command
    let mut cmd = tokio::process::Command::from(std_cmd);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.kill_on_drop(true);

    let child = cmd.spawn().with_context(|| {
        format!(
            "failed to spawn containerized child agent (runtime={}, profile={})",
            runtime, profile.name
        )
    })?;

    let pid = child.id().unwrap_or_else(|| {
        tracing::warn!("containerized child agent has no PID — process may not have started");
        0
    });
    tracing::info!(
        profile = %profile.name,
        image = profile.image_ref.as_deref().unwrap_or("default"),
        runtime = %runtime,
        pid = pid,
        "spawned containerized child agent"
    );

    Ok((child, pid))
}

/// Public accessor for TUI `/sandbox` command — checks runtime availability.
pub fn detect_container_runtime_public() -> Option<String> {
    detect_container_runtime()
}

/// Detect available container runtime. Prefers podman (rootless, daemonless).
fn detect_container_runtime() -> Option<String> {
    // Check env override first
    if let Ok(runtime) = std::env::var("OMEGON_CONTAINER_RUNTIME") {
        let r = runtime.to_lowercase();
        if matches!(r.as_str(), "podman" | "docker" | "nerdctl") {
            return Some(r);
        }
    }

    // Probe for podman first (preferred — rootless, no daemon)
    for candidate in &["podman", "docker", "nerdctl"] {
        if std::process::Command::new(candidate)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
        {
            return Some((*candidate).to_string());
        }
    }

    None
}
