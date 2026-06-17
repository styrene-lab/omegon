use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::process::{Child, Command};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChildAgentRuntimeProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enabled_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enabled_extensions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_extensions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preloaded_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
    /// Force slim mode on the child (compact schemas, lazy tool injection,
    /// reduced prompt surface). Delegate workers always set this.
    #[serde(default)]
    pub slim: bool,

    /// Nex profile name — when set, the child spawns inside an OCI container
    /// with sandbox isolation defined by the named profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    pub dangerously_bypass_permissions: bool,
    pub notes: Vec<String>,
}

impl ChildAgentBoundary {
    pub fn from_runtime(cwd: &Path, runtime: &ChildAgentRuntimeProfile) -> Self {
        Self::from_runtime_with_safety(
            cwd,
            runtime,
            std::env::var("OMEGON_BYPASS_PERMISSIONS").is_ok(),
        )
    }

    pub fn from_runtime_with_safety(
        cwd: &Path,
        runtime: &ChildAgentRuntimeProfile,
        dangerously_bypass_permissions: bool,
    ) -> Self {
        let scope = runtime.preloaded_files.clone();
        Self {
            cwd: std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf()),
            readable_paths: scope.clone(),
            writable_paths: scope,
            disabled_tools: runtime.disabled_tools.clone(),
            enabled_tools: runtime.enabled_tools.clone(),
            sandbox_profile: runtime.nex_profile.clone(),
            dangerously_bypass_permissions,
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
        let permission_mode = if self.dangerously_bypass_permissions {
            "dangerously_bypass_permissions inherited from parent"
        } else {
            "normal workspace boundary checks"
        };
        out.push_str(&format!("\nPermission mode: {permission_mode}\n"));
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
    /// Propagate parent --dangerously-bypass-permissions into child Omegon processes.
    pub dangerously_bypass_permissions: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildPromptKind {
    Delegate,
    Cleave,
}

impl ChildPromptKind {
    fn directory(self) -> &'static str {
        match self {
            Self::Delegate => ".omegon/delegate-prompts",
            Self::Cleave => ".omegon/cleave-prompts",
        }
    }
}

pub fn child_prompt_relative_path(kind: ChildPromptKind, child_id: &str) -> anyhow::Result<String> {
    if child_id.is_empty()
        || child_id.contains('/')
        || child_id.contains('\\')
        || child_id == "."
        || child_id == ".."
    {
        anyhow::bail!("child prompt id must be a plain file stem: {child_id}");
    }
    let relative = format!("{}/{}.md", kind.directory(), child_id);
    validate_child_prompt_relative_path(&relative)?;
    Ok(relative)
}

fn validate_child_prompt_relative_path(file_name: &str) -> anyhow::Result<()> {
    let relative = Path::new(file_name);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        anyhow::bail!("child prompt path must stay under cwd: {file_name}");
    }
    Ok(())
}

/// A single task item extracted from a child prompt checklist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildTaskItem {
    pub description: String,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildAgentActivity {
    Tool {
        tool: String,
        target: Option<String>,
    },
    Turn {
        turn: u32,
    },
    Tokens {
        input_tokens: u64,
        output_tokens: u64,
    },
    TaskDone {
        task_index: usize,
    },
}

/// Parse a child stderr line for tool-call, turn-boundary, token, or task markers.
pub fn parse_child_activity(line: &str) -> Option<ChildAgentActivity> {
    let clean = strip_ansi(line);
    let trimmed = clean.trim();

    if let Some(pos) = trimmed.find("TASK_DONE: ") {
        let rest = &trimmed[pos + "TASK_DONE: ".len()..];
        let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(task_index) = num_str.parse::<usize>() {
            return Some(ChildAgentActivity::TaskDone { task_index });
        }
    }

    if let Some(arrow_pos) = trimmed.find("→ ") {
        let rest = &trimmed[arrow_pos + "→ ".len()..];
        if !rest.is_empty() {
            let mut parts = rest.splitn(2, ' ');
            let tool = parts.next()?.to_string();
            let target = parts.next().map(|s| s.to_string());
            return Some(ChildAgentActivity::Tool { tool, target });
        }
    }

    if let Some(turn) = extract_turn_number(trimmed) {
        let (input_tokens, output_tokens) = extract_token_counts(trimmed);
        if input_tokens > 0 || output_tokens > 0 {
            return Some(ChildAgentActivity::Tokens {
                input_tokens,
                output_tokens,
            });
        }
        return Some(ChildAgentActivity::Turn { turn });
    }

    None
}

fn extract_token_counts(s: &str) -> (u64, u64) {
    let input_tokens = s
        .find("in:")
        .and_then(|p| s[p + 3..].split_whitespace().next())
        .and_then(|v| {
            v.trim_end_matches(|c: char| !c.is_ascii_digit())
                .parse()
                .ok()
        })
        .unwrap_or(0);
    let output_tokens = s
        .find("out:")
        .and_then(|p| s[p + 4..].split_whitespace().next())
        .and_then(|v| {
            v.trim_end_matches(|c: char| !c.is_ascii_digit())
                .parse()
                .ok()
        })
        .unwrap_or(0);
    (input_tokens, output_tokens)
}

fn extract_turn_number(s: &str) -> Option<u32> {
    let turn_pos = s.find("Turn ")?;
    let after = &s[turn_pos + "Turn ".len()..];
    let num_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    if num_str.is_empty() {
        return None;
    }
    num_str.parse().ok()
}

pub fn count_task_items(content: &str) -> usize {
    extract_task_items(content).len()
}

/// Extract task checklist items with descriptions from a child prompt or task file.
pub fn extract_task_items(content: &str) -> Vec<ChildTaskItem> {
    let mut checklist_items = Vec::new();
    let mut numbered_items = Vec::new();
    let mut bullet_items = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## Result")
            || trimmed.starts_with("## Contract")
            || trimmed.starts_with("## Finalization")
            || trimmed.starts_with("## Constraints")
            || trimmed.starts_with("## Output")
        {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            checklist_items.push(ChildTaskItem {
                description: rest.to_string(),
                done: false,
            });
        } else if let Some(rest) = trimmed
            .strip_prefix("- [x] ")
            .or_else(|| trimmed.strip_prefix("- [X] "))
        {
            checklist_items.push(ChildTaskItem {
                description: rest.to_string(),
                done: true,
            });
        } else if let Some(rest) = strip_numbered_prefix(trimmed) {
            numbered_items.push(ChildTaskItem {
                description: rest.to_string(),
                done: false,
            });
        } else if let Some(rest) = trimmed.strip_prefix("- ")
            && rest.len() > 3
            && !rest.starts_with("Stay ")
            && !rest.starts_with("Do not ")
        {
            bullet_items.push(ChildTaskItem {
                description: rest.to_string(),
                done: false,
            });
        }
    }

    if !checklist_items.is_empty() {
        checklist_items
    } else if !numbered_items.is_empty() {
        numbered_items
    } else {
        bullet_items
    }
}

fn strip_numbered_prefix(s: &str) -> Option<&str> {
    let digit_end = s.find(|c: char| !c.is_ascii_digit())?;
    if digit_end == 0 {
        return None;
    }
    let rest = &s[digit_end..];
    rest.strip_prefix(". ")
}

fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for c2 in chars.by_ref() {
                if c2.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

pub fn write_child_prompt_file(
    cwd: &Path,
    file_name: &str,
    prompt: &str,
) -> anyhow::Result<PathBuf> {
    validate_child_prompt_relative_path(file_name)?;
    let relative = Path::new(file_name);

    let prompt_file = std::fs::canonicalize(cwd)
        .unwrap_or_else(|_| cwd.to_path_buf())
        .join(relative);
    if let Some(parent) = prompt_file.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create child prompt directory {}",
                parent.display()
            )
        })?;
    }
    std::fs::write(&prompt_file, prompt).with_context(|| {
        format!(
            "Failed to write child prompt file {}",
            prompt_file.display()
        )
    })?;
    Ok(prompt_file)
}

fn resolve_child_sandbox_profile_name(
    preferred_profile: Option<&str>,
    workspace_profile: Option<&str>,
    has_profile: impl Fn(&str) -> bool,
) -> Option<String> {
    if let Some(preferred) = preferred_profile
        && has_profile(preferred)
    {
        return Some(preferred.to_string());
    }
    if let Some(workspace) = workspace_profile
        && has_profile(workspace)
    {
        return Some(workspace.to_string());
    }
    if has_profile("coding") {
        return Some("coding".to_string());
    }
    None
}

pub fn spawn_sandboxed_child_agent(
    config: &ChildAgentSpawnConfig,
    cwd: &Path,
    prompt_file: &Path,
    preferred_profile: Option<&str>,
) -> anyhow::Result<(Child, u32)> {
    let home = dirs::home_dir().unwrap_or_default().join(".omegon");
    let registry = crate::nex::NexRegistry::load(&home, Some(cwd))?;
    let workspace_profile = cwd.file_name().and_then(|name| name.to_str());
    let profile_name =
        resolve_child_sandbox_profile_name(preferred_profile, workspace_profile, |name| {
            registry.resolve(name).is_some()
        })
        .ok_or_else(|| anyhow::anyhow!("no nex profile available for child sandbox"))?;
    let profile = registry
        .resolve(&profile_name)
        .expect("resolved profile exists")
        .clone();

    crate::nex::spawn_containerized_child_agent(config, &profile, cwd, prompt_file)
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
    if config.dangerously_bypass_permissions {
        args.push("--dangerously-bypass-permissions");
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
    if config.dangerously_bypass_permissions {
        child.env("OMEGON_BYPASS_PERMISSIONS", "1");
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
    fn child_sandbox_profile_resolution_prefers_explicit_then_workspace_then_coding() {
        let available = |name: &str| matches!(name, "coding" | "project" | "strict");
        assert_eq!(
            resolve_child_sandbox_profile_name(Some("strict"), Some("project"), available),
            Some("strict".into())
        );
        assert_eq!(
            resolve_child_sandbox_profile_name(Some("missing"), Some("project"), available),
            Some("project".into())
        );
        assert_eq!(
            resolve_child_sandbox_profile_name(Some("missing"), Some("other"), available),
            Some("coding".into())
        );
        assert_eq!(
            resolve_child_sandbox_profile_name(Some("missing"), Some("other"), |_| false),
            None
        );
    }

    #[test]
    fn child_agent_runtime_profile_deserializes_camel_case_contract() {
        let json = r#"{
            "model": "test:model",
            "thinkingLevel": "high",
            "contextClass": "massive",
            "enabledTools": ["read"],
            "disabledTools": ["bash"],
            "skills": ["rust"],
            "enabledExtensions": ["scribe"],
            "disabledExtensions": ["legacy"],
            "preloadedFiles": ["docs/spec.md"],
            "persona": "reviewer",
            "slim": true,
            "nexProfile": "delegate-sandbox"
        }"#;

        let runtime: ChildAgentRuntimeProfile = serde_json::from_str(json).unwrap();
        assert_eq!(runtime.model.as_deref(), Some("test:model"));
        assert_eq!(runtime.thinking_level.as_deref(), Some("high"));
        assert_eq!(runtime.context_class.as_deref(), Some("massive"));
        assert_eq!(runtime.enabled_tools, vec!["read"]);
        assert_eq!(runtime.disabled_tools, vec!["bash"]);
        assert_eq!(runtime.skills, vec!["rust"]);
        assert_eq!(runtime.enabled_extensions, vec!["scribe"]);
        assert_eq!(runtime.disabled_extensions, vec!["legacy"]);
        assert_eq!(runtime.preloaded_files, vec!["docs/spec.md"]);
        assert_eq!(runtime.persona.as_deref(), Some("reviewer"));
        assert!(runtime.slim);
        assert_eq!(runtime.nex_profile.as_deref(), Some("delegate-sandbox"));
    }

    #[test]
    fn child_agent_boundary_prompt_section_lists_scope_tools_and_blocker_guidance() {
        let runtime = ChildAgentRuntimeProfile {
            preloaded_files: vec!["src/lib.rs".into()],
            enabled_tools: vec!["read".into(), "bash".into()],
            disabled_tools: vec!["delegate".into(), "cleave_run".into()],
            nex_profile: Some("delegate-sandbox".into()),
            ..Default::default()
        };
        let boundary = ChildAgentBoundary::from_runtime_with_safety(
            Path::new("/workspace/project"),
            &runtime,
            false,
        );
        let prompt = boundary.to_prompt_section();

        assert!(prompt.contains("## Execution Boundary"));
        assert!(prompt.contains("src/lib.rs"));
        assert!(prompt.contains("read"));
        assert!(prompt.contains("delegate"));
        assert!(prompt.contains("cleave_run"));
        assert!(prompt.contains("Sandbox profile: delegate-sandbox"));
        assert!(prompt.contains("Permission mode: normal workspace boundary checks"));
        assert!(prompt.contains("stop and report the blocker"));
    }

    #[test]
    fn child_agent_boundary_prompt_discloses_inherited_bypass() {
        let runtime = ChildAgentRuntimeProfile::default();
        let boundary = ChildAgentBoundary::from_runtime_with_safety(
            Path::new("/workspace/project"),
            &runtime,
            true,
        );
        let prompt = boundary.to_prompt_section();
        assert!(
            prompt
                .contains("Permission mode: dangerously_bypass_permissions inherited from parent")
        );
    }

    #[test]
    fn child_prompt_relative_path_routes_delegate_and_cleave_prompts() {
        assert_eq!(
            child_prompt_relative_path(ChildPromptKind::Delegate, "delegate_7").unwrap(),
            ".omegon/delegate-prompts/delegate_7.md"
        );
        assert_eq!(
            child_prompt_relative_path(ChildPromptKind::Cleave, "alpha").unwrap(),
            ".omegon/cleave-prompts/alpha.md"
        );
    }

    #[test]
    fn child_prompt_relative_path_rejects_path_like_ids() {
        assert!(child_prompt_relative_path(ChildPromptKind::Delegate, "../escape").is_err());
        assert!(child_prompt_relative_path(ChildPromptKind::Cleave, "feature/ui").is_err());
        assert!(child_prompt_relative_path(ChildPromptKind::Cleave, "").is_err());
    }

    #[test]
    fn child_agent_activity_parses_tool_turn_tokens_and_task_done() {
        assert_eq!(
            parse_child_activity("\u{1b}[32m INFO\u{1b}[0m → bash ls -la"),
            Some(ChildAgentActivity::Tool {
                tool: "bash".into(),
                target: Some("ls -la".into())
            })
        );
        assert_eq!(
            parse_child_activity("2026-03-18T02:22:24Z INFO ── Turn 3 ──"),
            Some(ChildAgentActivity::Turn { turn: 3 })
        );
        assert_eq!(
            parse_child_activity("── Turn 3 complete — in:1234 out:567 ──"),
            Some(ChildAgentActivity::Tokens {
                input_tokens: 1234,
                output_tokens: 567
            })
        );
        assert_eq!(
            parse_child_activity("2026-04-18T02:22:27Z INFO TASK_DONE: 2"),
            Some(ChildAgentActivity::TaskDone { task_index: 2 })
        );
        assert_eq!(parse_child_activity("LLM bridge ready"), None);
    }

    #[test]
    fn child_agent_extract_task_items_prefers_checklists_and_stops_at_contract() {
        let items = extract_task_items(
            "1. ignored when checklist exists\n- [ ] Real task\n- [x] Done task\n## Contract\n- [ ] Hidden",
        );
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].description, "Real task");
        assert!(!items[0].done);
        assert_eq!(items[1].description, "Done task");
        assert!(items[1].done);
    }

    #[test]
    fn write_child_prompt_file_creates_nested_prompt_directory() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = write_child_prompt_file(
            temp.path(),
            ".omegon/delegate-prompts/delegate_7.md",
            "child prompt",
        )
        .unwrap();

        assert!(path.ends_with(".omegon/delegate-prompts/delegate_7.md"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "child prompt");
    }

    #[test]
    fn write_child_prompt_file_rejects_paths_outside_cwd() {
        let temp = tempfile::TempDir::new().unwrap();

        assert!(write_child_prompt_file(temp.path(), "../escape.md", "prompt").is_err());
        assert!(write_child_prompt_file(temp.path(), "/tmp/escape.md", "prompt").is_err());
    }

    #[tokio::test]
    async fn headless_child_spawn_propagates_bypass_flag_and_env() {
        let captured = run_fake_child_with_bypass(true).await;
        assert!(captured.contains("--dangerously-bypass-permissions"));
        assert!(captured.contains("bypass:1"));
    }

    #[tokio::test]
    async fn headless_child_spawn_omits_bypass_flag_and_env_when_disabled() {
        let captured = run_fake_child_with_bypass(false).await;
        assert!(!captured.contains("--dangerously-bypass-permissions"));
        assert!(captured.contains("bypass:\n"));
    }

    async fn run_fake_child_with_bypass(dangerously_bypass_permissions: bool) -> String {
        let temp = tempfile::TempDir::new().unwrap();
        let script = temp.path().join("fake-child.sh");
        let output = temp.path().join("child-output.txt");
        let prompt = temp.path().join("prompt.md");
        std::fs::write(&prompt, "test prompt").unwrap();
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf 'args:%s\\n' \"$*\" > \"$OUTPUT_PATH\"\nprintf 'bypass:%s\\n' \"${OMEGON_BYPASS_PERMISSIONS:-}\" >> \"$OUTPUT_PATH\"\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }

        let config = ChildAgentSpawnConfig {
            agent_binary: script,
            model: "test:model".into(),
            max_turns: 1,
            inherited_env: vec![("OUTPUT_PATH".into(), output.to_string_lossy().to_string())],
            injected_env: Vec::new(),
            runtime: ChildAgentRuntimeProfile::default(),
            dangerously_bypass_permissions,
        };

        let (mut child, _) = spawn_headless_child_agent(&config, temp.path(), &prompt).unwrap();
        let status = child.wait().await.unwrap();
        assert!(status.success());
        std::fs::read_to_string(output).unwrap()
    }
}
