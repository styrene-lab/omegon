//! Core tools — the agent's primary capabilities.
//!
//! Phase 0: primitive tools (bash, read, write, edit).
//! Phase 0+: higher-level tools (understand, change, execute, remember, speculate)
//!           that compose the primitives.

pub mod bash;
pub mod change;
pub mod chronos;
pub mod edit;
pub mod local_inference;
pub mod read;
pub mod render;
pub mod speculate;
pub mod validate;
pub mod view;
pub mod web_search;
pub mod whoami;
pub mod write;

// Phase 0+ stubs:
// pub mod understand;  // tree-sitter + scope graph
// pub mod execute;     // bash with progressive disclosure
// pub mod remember;    // session scratchpad

use async_trait::async_trait;
use omegon_traits::{ToolDefinition, ToolProvider, ToolResult};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

/// Core tool provider — registers the primitive tools.
pub struct CoreTools {
    cwd: PathBuf,
    /// Repository model — tracks branch, dirty files, submodules.
    /// None if not inside a git repo.
    repo_model: Option<std::sync::Arc<omegon_git::RepoModel>>,
}

impl CoreTools {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            repo_model: None,
        }
    }

    /// Create with a RepoModel for git-aware operations.
    pub fn with_repo_model(cwd: PathBuf, repo_model: std::sync::Arc<omegon_git::RepoModel>) -> Self {
        Self {
            cwd,
            repo_model: Some(repo_model),
        }
    }

    /// Resolve a user-provided path against cwd and verify it doesn't escape
    /// the workspace via `../` traversal. Returns the canonical path on success.
    fn resolve_path(&self, path_str: &str) -> anyhow::Result<PathBuf> {
        let joined = self.cwd.join(path_str);

        // Canonicalize to resolve symlinks and `..` — but the file may not
        // exist yet (write/edit creating new files). In that case, canonicalize
        // the parent directory and append the filename.
        let canonical = if joined.exists() {
            joined.canonicalize()?
        } else if let Some(parent) = joined.parent() {
            // Create parent dirs if needed (write tool does this), then canonicalize
            if parent.exists() {
                parent.canonicalize()?.join(joined.file_name().unwrap_or_default())
            } else {
                // Parent doesn't exist — resolve what we can. The write tool
                // will create parents. For now, use lexical normalization.
                lexical_normalize(&joined)
            }
        } else {
            joined.clone()
        };

        let cwd_canonical = self.cwd.canonicalize().unwrap_or_else(|_| self.cwd.clone());

        if !canonical.starts_with(&cwd_canonical) {
            anyhow::bail!(
                "Path '{}' resolves to '{}' which is outside the workspace '{}'",
                path_str,
                canonical.display(),
                cwd_canonical.display()
            );
        }

        Ok(joined)
    }
}

/// Lexical path normalization — resolve `.` and `..` without filesystem access.
/// Used as a fallback when the path doesn't exist yet.
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                // Only pop if there's a normal component to pop
                if components.last().is_some_and(|c| {
                    matches!(c, std::path::Component::Normal(_))
                }) {
                    components.pop();
                } else {
                    components.push(component);
                }
            }
            std::path::Component::CurDir => {} // skip
            _ => components.push(component),
        }
    }
    components.iter().collect()
}

#[async_trait]
impl ToolProvider for CoreTools {
    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "bash".into(),
                label: "bash".into(),
                description: "Execute a bash command in the current working directory. \
                    Returns stdout and stderr. Output is truncated to last 2000 lines \
                    or 50KB. Optionally provide a timeout in seconds."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "Bash command to execute"
                        },
                        "timeout": {
                            "type": "number",
                            "description": "Timeout in seconds (optional)"
                        }
                    },
                    "required": ["command"]
                }),
            },
            ToolDefinition {
                name: "read".into(),
                label: "read".into(),
                description: "Read the contents of a file. Supports text files and \
                    images. Output is truncated to 2000 lines or 50KB. Use offset/limit \
                    for large files."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file to read"
                        },
                        "offset": {
                            "type": "number",
                            "description": "Line number to start from (1-indexed)"
                        },
                        "limit": {
                            "type": "number",
                            "description": "Maximum number of lines to read"
                        }
                    },
                    "required": ["path"]
                }),
            },
            ToolDefinition {
                name: "write".into(),
                label: "write".into(),
                description: "Write content to a file. Creates the file if it doesn't \
                    exist, overwrites if it does. Automatically creates parent directories."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file to write"
                        },
                        "content": {
                            "type": "string",
                            "description": "Content to write"
                        }
                    },
                    "required": ["path", "content"]
                }),
            },
            ToolDefinition {
                name: "edit".into(),
                label: "edit".into(),
                description: "Edit a file by replacing exact text. The oldText must match \
                    exactly (including whitespace). Use this for precise, surgical edits."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file to edit"
                        },
                        "oldText": {
                            "type": "string",
                            "description": "Exact text to find and replace"
                        },
                        "newText": {
                            "type": "string",
                            "description": "New text to replace the old text with"
                        }
                    },
                    "required": ["path", "oldText", "newText"]
                }),
            },
            ToolDefinition {
                name: "change".into(),
                label: "change".into(),
                description: "Atomic multi-file edit with automatic validation. Accepts an array \
                    of edits, applies all atomically (rollback on any failure), then runs type \
                    checking. One tool call replaces multiple edits + validation."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "edits": {
                            "type": "array",
                            "description": "Array of edits to apply atomically",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "file": { "type": "string", "description": "File path" },
                                    "oldText": { "type": "string", "description": "Exact text to find" },
                                    "newText": { "type": "string", "description": "Replacement text" }
                                },
                                "required": ["file", "oldText", "newText"]
                            }
                        },
                        "validate": {
                            "type": "string",
                            "description": "Validation mode: none, quick, standard (default), full (includes tests)",
                            "default": "standard"
                        }
                    },
                    "required": ["edits"]
                }),
            },
            ToolDefinition {
                name: "speculate_start".into(),
                label: "speculate".into(),
                description: "Create a git checkpoint for exploratory changes. Make changes freely, \
                    then use speculate_commit to keep them or speculate_rollback to undo everything."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "label": {
                            "type": "string",
                            "description": "Name for this speculation (e.g. 'try-approach-a')"
                        }
                    },
                    "required": ["label"]
                }),
            },
            ToolDefinition {
                name: "speculate_check".into(),
                label: "speculate".into(),
                description: "Check the current speculation state — shows modified files and \
                    runs validation against them."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDefinition {
                name: "speculate_commit".into(),
                label: "speculate".into(),
                description: "Keep all changes made during speculation and discard the checkpoint."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDefinition {
                name: "speculate_rollback".into(),
                label: "speculate".into(),
                description: "Revert all changes made during speculation back to the checkpoint."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDefinition {
                name: "commit".into(),
                label: "commit".into(),
                description: "Commit changes to git. Stages the specified files (or all \
                    dirty files if none specified), creates a commit with the given message. \
                    Handles submodule commits automatically — if edited files are inside a \
                    submodule, the harness commits inside the submodule first and updates \
                    the parent pointer. Use conventional commit format for the message."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "message": {
                            "type": "string",
                            "description": "Commit message (conventional commit format: type(scope): description)"
                        },
                        "paths": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Specific file paths to commit. Omit to commit all dirty files."
                        }
                    },
                    "required": ["message"]
                }),
            },
            ToolDefinition {
                name: "whoami".into(),
                label: "whoami".into(),
                description: "Check authentication status across development tools \
                    (git, GitHub, GitLab, AWS, Kubernetes, OCI registries). Returns \
                    structured status with error diagnosis and refresh commands for \
                    expired or missing sessions.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            ToolDefinition {
                name: "chronos".into(),
                label: "chronos".into(),
                description: "Get authoritative date and time context from the system clock. \
                    Use before any date calculations, weekly/monthly reporting, relative date \
                    references, quarter boundaries, or epoch timestamps. Eliminates AI date \
                    calculation errors.\n\nSubcommands:\n  week (default) — Current/previous \
                    week boundaries (Mon-Fri)\n  month — Current/previous month boundaries\n  \
                    quarter — Calendar quarter, fiscal year (Oct-Sep)\n  relative — Resolve \
                    expression like '3 days ago', 'next Monday'\n  iso — ISO 8601 week number, \
                    year, day-of-year\n  epoch — Unix timestamp (seconds and milliseconds)\n  \
                    tz — Timezone abbreviation and UTC offset\n  range — Calendar and business \
                    days between two dates\n  all — All of the above combined".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "subcommand": {
                            "type": "string",
                            "enum": ["week", "month", "quarter", "relative", "iso", "epoch", "tz", "range", "all"],
                            "description": "Subcommand (default: week)"
                        },
                        "expression": {
                            "type": "string",
                            "description": "For 'relative': date expression (e.g. '3 days ago', 'next Monday')"
                        },
                        "from_date": {
                            "type": "string",
                            "description": "For 'range': start date YYYY-MM-DD"
                        },
                        "to_date": {
                            "type": "string",
                            "description": "For 'range': end date YYYY-MM-DD"
                        }
                    }
                }),
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        match tool_name {
            "bash" => {
                let command = args["command"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'command' argument"))?;
                let timeout = args["timeout"].as_u64();

                // Warn (but don't block) git mutation commands — the agent should
                // use the structured `commit` tool instead. Hard-blocking would
                // break legitimate uses (git stash in speculate, git branch for
                // exploration). The warning nudges the agent toward the right tool.
                if self.repo_model.is_some() {
                    let cmd_lower = command.to_lowercase();
                    if cmd_lower.contains("git commit")
                        || cmd_lower.contains("git add ")
                        || (cmd_lower.contains("git stash") && !cmd_lower.contains("git stash list"))
                    {
                        tracing::warn!(
                            command = command,
                            "git mutation via bash — prefer the structured `commit` tool \
                             for commits (handles submodules, lifecycle batching, working set)"
                        );
                    }
                }

                bash::execute(command, &self.cwd, timeout, cancel).await
            }
            "read" => {
                let path_str = args["path"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'path' argument"))?;
                let path = self.resolve_path(path_str)?;
                let offset = args["offset"].as_u64().map(|n| n as usize);
                let limit = args["limit"].as_u64().map(|n| n as usize);
                read::execute(&path, offset, limit).await
            }
            "write" => {
                let path_str = args["path"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'path' argument"))?;
                let content = args["content"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'content' argument"))?;
                let path = self.resolve_path(path_str)?;
                let result = write::execute(&path, content, &self.cwd).await;
                if result.is_ok() {
                    if let Some(ref model) = self.repo_model {
                        model.record_edit(path_str);
                    }
                }
                result
            }
            "edit" => {
                let path_str = args["path"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'path' argument"))?;
                let old_text = args["oldText"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'oldText' argument"))?;
                let new_text = args["newText"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'newText' argument"))?;
                let path = self.resolve_path(path_str)?;
                let result = edit::execute(&path, old_text, new_text, &self.cwd).await;
                if result.is_ok() {
                    if let Some(ref model) = self.repo_model {
                        model.record_edit(path_str);
                    }
                }
                result
            }
            "change" => {
                let edits_val = args.get("edits")
                    .ok_or_else(|| anyhow::anyhow!("missing 'edits' argument"))?;
                let edits: Vec<change::EditSpec> = serde_json::from_value(edits_val.clone())?;
                let validate_mode = args.get("validate")
                    .and_then(|v| v.as_str())
                    .map(change::ValidationMode::parse)
                    .unwrap_or(change::ValidationMode::Standard);
                let cwd = self.cwd.clone();
                let cwd2 = cwd.clone();
                let result = change::execute(
                    &edits,
                    validate_mode,
                    &cwd,
                    move |p: &str| {
                        let tools = CoreTools::new(cwd2.clone());
                        tools.resolve_path(p)
                    },
                ).await;
                // Track all edited files in the working set
                if result.is_ok() {
                    if let Some(ref model) = self.repo_model {
                        for edit in &edits {
                            model.record_edit(&edit.file);
                        }
                    }
                }
                result
            }
            "commit" => {
                let message = args["message"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'message' argument"))?;
                let paths: Vec<String> = args
                    .get("paths")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                // Gather pending lifecycle files from RepoModel (if available)
                let lifecycle_paths: Vec<String> = self
                    .repo_model
                    .as_ref()
                    .map(|m| m.pending_lifecycle_files().into_iter().collect())
                    .unwrap_or_default();

                // Detect submodules that need commits first
                let sub_paths = self
                    .repo_model
                    .as_ref()
                    .map(|m| m.submodules().into_iter().map(|s| s.path).collect::<Vec<_>>())
                    .unwrap_or_else(|| {
                        omegon_git::submodule::list_submodule_paths(&self.cwd)
                            .unwrap_or_default()
                    });

                let mut submodule_commits = 0;
                for sub_path in &sub_paths {
                    let sub_prefix = format!("{}/", sub_path);
                    let touches_sub = paths.is_empty()
                        || paths.iter().any(|p| p.starts_with(&sub_prefix));
                    if touches_sub {
                        if let Ok(n) = omegon_git::commit::commit_in_submodule(
                            &self.cwd,
                            sub_path,
                            message,
                        ) {
                            submodule_commits += n;
                        }
                    }
                }

                // Create the commit — includes lifecycle files if any are pending
                let include_lifecycle = !lifecycle_paths.is_empty();
                let result = omegon_git::commit::create_commit(
                    &self.cwd,
                    &omegon_git::commit::CommitOptions {
                        message,
                        paths: &paths,
                        include_lifecycle,
                        lifecycle_paths: &lifecycle_paths,
                    },
                )?;

                // Clear working set + lifecycle queue after successful commit
                if let Some(ref model) = self.repo_model {
                    model.clear_working_set();
                    if let Err(e) = model.refresh() {
                        tracing::warn!("failed to refresh repo model after commit: {e}");
                    }
                }

                let mut summary = format!(
                    "Committed {} file(s): {}",
                    result.files_staged, result.sha
                );
                if submodule_commits > 0 {
                    summary.push_str(&format!(
                        "\n({} file(s) committed inside submodule(s) first)",
                        submodule_commits
                    ));
                }
                if include_lifecycle {
                    summary.push_str(&format!(
                        "\n({} lifecycle file(s) included)",
                        lifecycle_paths.len()
                    ));
                }

                Ok(ToolResult {
                    content: vec![omegon_traits::ContentBlock::Text { text: summary }],
                    details: json!({
                        "sha": result.sha,
                        "files_staged": result.files_staged,
                        "submodule_commits": submodule_commits,
                        "lifecycle_files": lifecycle_paths.len(),
                    }),
                })
            }
            "speculate_start" => {
                let label = args["label"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'label' argument"))?;
                speculate::start(label, &self.cwd).await
            }
            "speculate_check" => {
                speculate::check(&self.cwd).await
            }
            "speculate_commit" => {
                speculate::commit(&self.cwd).await
            }
            "speculate_rollback" => {
                speculate::rollback(&self.cwd).await
            }
            "whoami" => {
                whoami::execute().await
            }
            "chronos" => {
                let sub = args["subcommand"].as_str().unwrap_or("week");
                let expr = args["expression"].as_str();
                let from = args["from_date"].as_str();
                let to = args["to_date"].as_str();
                match chronos::execute(sub, expr, from, to) {
                    Ok(text) => Ok(ToolResult {
                        content: vec![omegon_traits::ContentBlock::Text { text }],
                        details: json!({ "subcommand": sub }),
                    }),
                    Err(e) => anyhow::bail!("{e}"),
                }
            }
            _ => anyhow::bail!("Unknown core tool: {tool_name}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_traversal_blocked() {
        let tools = CoreTools::new(PathBuf::from("/tmp/workspace"));
        // Attempting to escape the workspace via ../
        let result = tools.resolve_path("../../../etc/passwd");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("outside the workspace"), "error: {err}");
    }

    #[test]
    fn path_within_workspace_allowed() {
        let dir = tempfile::tempdir().unwrap();
        // Create the subdirectory so canonicalize works
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();

        // Use canonical path to match what main.rs does with fs::canonicalize(&cli.cwd)
        let cwd = dir.path().canonicalize().unwrap();
        let tools = CoreTools::new(cwd.clone());
        let result = tools.resolve_path("src/main.rs");
        assert!(result.is_ok(), "error: {:?}", result.unwrap_err());
        assert!(result.unwrap().starts_with(&cwd));
    }

    #[test]
    fn lexical_normalize_resolves_dotdot() {
        let result = lexical_normalize(Path::new("/a/b/../c"));
        assert_eq!(result, PathBuf::from("/a/c"));
    }

    #[test]
    fn lexical_normalize_resolves_dot() {
        let result = lexical_normalize(Path::new("/a/./b/./c"));
        assert_eq!(result, PathBuf::from("/a/b/c"));
    }
}
