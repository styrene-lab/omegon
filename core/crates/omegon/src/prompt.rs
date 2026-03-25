//! System prompt assembly for the headless agent.
//!
//! Phase 0: static base prompt + tool definitions + project directives.
//! Phase 0+: ContextManager provides dynamic injection.

use omegon_traits::ToolDefinition;
use std::path::{Path, PathBuf};

/// Build the base system prompt.
///
/// Assembles: identity, tool list, tool guidelines, behavior directives,
/// lifecycle context (if artifacts exist), global/project AGENTS.md,
/// project conventions (auto-detected from config files).
pub fn build_base_prompt(cwd: &Path, tools: &[ToolDefinition]) -> String {
    let date = utc_date();
    let tool_list = format_tool_list(tools);
    let lex_imperialis = load_lex_imperialis();
    let lifecycle_context = detect_lifecycle_context(cwd, tools);
    let global_directives = load_global_directives();
    let project_directives = load_project_directives(cwd);
    let project_conventions = detect_project_conventions(cwd);

    format!(
        r#"You are an expert coding assistant. You help by reading files, executing commands, editing code, and writing new files.

Available tools: {tool_list}

# Behavior

- Always respond to the user. Tool calls gather information — they are not the answer. After calling tools, synthesize what you found into a direct response. Never end a turn with only tool calls and no text.
- Be direct — act, don't narrate intent. Disagree when you see a better path.
- Ground claims in evidence — cite files and lines. Don't assert about unread code.
- Every non-trivial change needs tests. Commit when done, do NOT push.

# Core Tools

## bash
- Use for: ls, grep, find, rg, git, running tests, installing deps, any shell command.
- Output is truncated to 2000 lines / 50KB. For large output, pipe through head/tail/grep.
- Long-running commands: set a timeout. Don't let builds hang forever.
- Never use cat to read files — use the read tool. Never use sed for edits — use the edit tool.

## read
- Always read a file before editing it. You cannot guess file contents accurately.
- Use offset/limit for large files — read in chunks, don't load 10K lines at once.
- Supports images (jpg/png/gif/webp) — they're sent as attachments.

## edit
- The oldText must match the file EXACTLY — whitespace, newlines, everything.
- Read the file first. Copy the exact text you want to replace.
- Use for surgical changes. For complete rewrites, use write instead.
- If an edit fails with "text not found", re-read the file — it may have changed.

## write
- Creates parent directories automatically. Overwrites if the file exists.
- Use for new files or when replacing >50% of a file's content.
- Don't use write for small changes to existing files — use edit.

## web_search
- Modes: quick (single provider, fast), deep (more results), compare (fan out to all providers).
- Use compare mode for research requiring cross-source verification.
- Available when search API keys are configured (Brave, Tavily, Serper).
{lex_imperialis}{lifecycle_context}{global_directives}{project_directives}{project_conventions}
Current date: {date}
Current working directory: {cwd}"#,
        cwd = cwd.display()
    )
}

/// Rich tool guidelines — how to use each tool well, not just what it does.
fn detect_lifecycle_context(cwd: &Path, tools: &[ToolDefinition]) -> String {
    let repo_root = find_repo_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
    let tool_names: std::collections::HashSet<&str> =
        tools.iter().map(|t| t.name.as_str()).collect();

    let has_design_tools = tool_names.contains("design_tree");
    let has_openspec_tools = tool_names.contains("openspec_manage");
    let has_cleave_tools = tool_names.contains("cleave_assess") || tool_names.contains("cleave_run");

    let docs_dir = repo_root.join("docs");
    let openspec_dir = repo_root.join("openspec");
    // Count design docs in a single pass
    let design_doc_count = if docs_dir.is_dir() {
        std::fs::read_dir(&docs_dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
                    .count()
            })
            .unwrap_or(0)
    } else {
        0
    };
    let has_design_docs = design_doc_count > 0;
    let has_openspec = openspec_dir.is_dir();

    if !has_design_docs && !has_openspec && !has_design_tools {
        return String::new();
    }

    let mut sections: Vec<String> = Vec::new();

    sections.push(
        "This project uses structured lifecycle management. \
         Design exploration, specification, and implementation are tracked as artifacts."
            .into(),
    );

    if has_design_docs && has_design_tools {
        let doc_count = design_doc_count;

        sections.push(format!(
            "design-tree: {doc_count} design doc(s) in docs/. Use design_tree to query nodes, \
             track decisions, and manage open questions. Use design_tree_update to \
             record decisions, add research, and transition node status \
             (seed → exploring → resolved → decided). \
             When exploring a design node, actively surface assumptions as \
             [assumption]-tagged open questions (e.g. '[assumption] The operator has git installed'). \
             Assumptions are unknowns we're treating as true but haven't validated. \
             A node's readiness = decisions / (decisions + questions + assumptions). \
             Resolve all unknowns before deciding."
        ));
    }

    if has_openspec && has_openspec_tools {
        sections.push(
            "openspec: Spec-driven implementation lifecycle. The full cycle is: \
             design_tree_update(implement) → spec → fast_forward → /cleave → \
             /assess spec → archive. Specs define what must be true BEFORE code is written."
                .into(),
        );
    }

    if has_cleave_tools {
        sections.push(
            "cleave: Task decomposition into parallel children. Use cleave_assess \
             to check complexity (threshold 2.0). The loop auto-batches edit calls \
             atomically — you don't need to worry about partial state."
                .into(),
        );
    }

    if sections.len() <= 1 {
        return String::new();
    }

    format!("\n# Project Lifecycle\n\n{}\n", sections.join("\n\n"))
}

/// Load the Lex Imperialis — non-overridable core directives.
///
/// These are constitutional axioms that define what Omegon *is*.
/// They are always injected, always first in the directive stack,
/// and cannot be disabled by personas, tones, or operator config.
pub fn load_lex_imperialis() -> String {
    // Embedded at compile time from the armory source
    static LEX: &str = include_str!("../../../../data/lex-imperialis.md");
    static TOOL_LIMITS: &str = include_str!("../../../../data/tool-limitations.md");
    format!("\n# Core Directives\n\n{LEX}\n\n{TOOL_LIMITS}\n")
}

/// Load global operator directives from ~/.omegon/AGENTS.md
fn load_global_directives() -> String {
    let home = dirs::home_dir().unwrap_or_default();
    let global_agents = home.join(".omegon/AGENTS.md");

    if let Ok(content) = std::fs::read_to_string(&global_agents) {
        let trimmed = truncate_directive(&content, 3000);
        format!("\n# Operator Directives\n\n{trimmed}\n")
    } else {
        String::new()
    }
}

/// Detect project conventions by scanning for config files.
fn detect_project_conventions(cwd: &Path) -> String {
    let mut conventions = Vec::new();
    let repo_root = find_repo_root(cwd).unwrap_or_else(|| cwd.to_path_buf());

    // Rust
    if repo_root.join("Cargo.toml").exists() {
        conventions.push("- Rust project: use `cargo check` for type checking, `cargo clippy` for lints, `cargo test` for tests");
        if repo_root.join("Cargo.lock").exists() {
            conventions.push("- Cargo.lock is committed — this is an application, not a library");
        }
    }

    // TypeScript / JavaScript
    if repo_root.join("tsconfig.json").exists() {
        conventions.push("- TypeScript project: use `npx tsc --noEmit` for type checking");
    }
    if repo_root.join("package.json").exists() {
        // Check for test runner
        if repo_root.join("vitest.config.ts").exists()
            || repo_root.join("vitest.config.js").exists()
        {
            conventions.push("- Vitest for testing: `npx vitest run`");
        } else if repo_root.join("jest.config.ts").exists()
            || repo_root.join("jest.config.js").exists()
        {
            conventions.push("- Jest for testing: `npx jest`");
        }
    }

    // Python
    if repo_root.join("pyproject.toml").exists() {
        conventions.push("- Python project: use `ruff check` for linting, `pytest` for tests");
    }

    // Go
    if repo_root.join("go.mod").exists() {
        conventions.push("- Go project: use `go vet` for checking, `go test ./...` for tests");
    }

    // Git conventions
    if repo_root.join(".gitignore").exists() {
        conventions.push("- .gitignore present — respect it when creating files");
    }

    if conventions.is_empty() {
        String::new()
    } else {
        format!(
            "\n# Project Conventions\n\n{}\n",
            conventions.join("\n")
        )
    }
}

/// Truncate a directive string to a byte budget, breaking at a line boundary.
fn truncate_directive(content: &str, max_width: usize) -> String {
    crate::util::truncate(content, max_width)
}

/// Load project directives from AGENTS.md files.
///
/// Checks (in order):
/// 1. `<cwd>/AGENTS.md` — project-level directives
/// 2. Walks up to repo root looking for AGENTS.md
///
/// Returns a formatted section or empty string if no directives found.
fn load_project_directives(cwd: &Path) -> String {
    // Resolve the repo root — handles both normal repos and worktrees.
    // In a worktree, .git is a file containing "gitdir: /path/to/main/.git/worktrees/name".
    // We need to find the main repo root where AGENTS.md lives.
    let repo_root = find_repo_root(cwd);

    // Search order: cwd, then walk up to repo root (if different)
    let search_dirs: Vec<&Path> = if let Some(ref root) = repo_root {
        if root != cwd {
            vec![cwd, root.as_path()]
        } else {
            vec![cwd]
        }
    } else {
        vec![cwd]
    };

    for dir in search_dirs {
        let agents_file = dir.join("AGENTS.md");
        if agents_file.exists()
            && let Ok(content) = std::fs::read_to_string(&agents_file) {
                let trimmed = if content.len() > 4000 {
                    let mut end = 4000;
                    while end > 0 && !content.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}...\n[truncated at ~4000 bytes]", &content[..end])
                } else {
                    content
                };
                return format!(
                    "\n# Project Directives\n\nFrom `{}`:\n\n{trimmed}\n",
                    agents_file.display()
                );
            }
    }
    String::new()
}

/// Find the git repo root, handling worktrees.
/// In a worktree, `.git` is a file containing `gitdir: <path>`.
/// We follow that to find the main repo's `.git` directory.
fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let git_path = dir.join(".git");
        if git_path.exists() {
            if git_path.is_file() {
                // Worktree: .git is a file like "gitdir: /main/repo/.git/worktrees/name"
                if let Ok(content) = std::fs::read_to_string(&git_path)
                    && let Some(gitdir) = content.strip_prefix("gitdir: ") {
                        let gitdir = gitdir.trim();
                        // gitdir points to .git/worktrees/<name>, go up to .git, then up to repo root
                        let gitdir_path = if Path::new(gitdir).is_absolute() {
                            PathBuf::from(gitdir)
                        } else {
                            dir.join(gitdir)
                        };
                        // .git/worktrees/<name> → .git → repo root
                        // .git/worktrees/<name> → .git → repo root
                        if let Some(dot_git) = gitdir_path.parent().and_then(|p| p.parent())
                            && let Some(repo) = dot_git.parent() {
                                return Some(repo.to_path_buf());
                            }
                    }
                // Fallback: treat as repo root
                return Some(dir);
            } else {
                // Normal repo: .git is a directory
                return Some(dir);
            }
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

fn format_tool_list(tools: &[ToolDefinition]) -> String {
    // Just list names — full descriptions are in the tool definitions
    // sent separately in the API request. No need to duplicate.
    tools
        .iter()
        .map(|t| t.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// UTC date as YYYY-MM-DD from the system clock.
/// Hand-rolled to avoid pulling in chrono/time crates for one function.
fn utc_date() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    epoch_to_ymd(secs)
}

fn epoch_to_ymd(epoch_secs: u64) -> String {
    let mut days = (epoch_secs / 86400) as i64;
    let mut y = 1970i64;
    loop {
        let ydays = if is_leap(y) { 366 } else { 365 };
        if days < ydays { break; }
        days -= ydays;
        y += 1;
    }
    let leap = is_leap(y);
    let mdays: [i64; 12] = [31, if leap {29} else {28}, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0usize;
    for (i, &md) in mdays.iter().enumerate() {
        if days < md { m = i; break; }
        days -= md;
    }
    format!("{y}-{:02}-{:02}", m + 1, days + 1)
}

fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_format() {
        let date = utc_date();
        assert!(date.len() == 10, "date should be YYYY-MM-DD: {date}");
        assert!(date.starts_with("202"), "date should be in 202x: {date}");
    }

    #[test]
    fn base_prompt_includes_tools() {
        let tools = vec![omegon_traits::ToolDefinition {
            name: "test_tool".into(),
            label: "test".into(),
            description: "A test tool".into(),
            parameters: serde_json::json!({}),
        }];
        let prompt = build_base_prompt(Path::new("/tmp"), &tools);
        // Tool list is comma-separated names (descriptions are in API tool defs)
        assert!(prompt.contains("test_tool"));
        assert!(prompt.contains("/tmp"));
    }

    #[test]
    fn base_prompt_includes_commit_instructions() {
        let tools = vec![];
        let prompt = build_base_prompt(Path::new("/tmp"), &tools);
        assert!(prompt.contains("Commit when done"), "should instruct to commit");
        assert!(prompt.contains("NOT push"), "should instruct not to push");
    }

    #[test]
    fn load_directives_returns_empty_for_missing() {
        let directives = load_project_directives(Path::new("/tmp/nonexistent"));
        assert!(directives.is_empty());
    }

    #[test]
    fn lifecycle_context_detected_when_docs_exist() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();

        // Create a git repo with docs/
        std::fs::create_dir_all(cwd.join(".git")).unwrap();
        std::fs::create_dir_all(cwd.join("docs")).unwrap();
        std::fs::write(cwd.join("docs/some-design.md"), "# Design").unwrap();

        // With design_tree tools registered
        let tools = vec![
            ToolDefinition {
                name: "design_tree".into(),
                label: "dt".into(),
                description: "query".into(),
                parameters: serde_json::json!({}),
            },
            ToolDefinition {
                name: "design_tree_update".into(),
                label: "dtu".into(),
                description: "mutate".into(),
                parameters: serde_json::json!({}),
            },
        ];

        let ctx = detect_lifecycle_context(cwd, &tools);
        assert!(ctx.contains("Project Lifecycle"), "should detect lifecycle, got: {ctx}");
        assert!(ctx.contains("design-tree"), "should mention design-tree");
        assert!(ctx.contains("1 design doc"), "should count docs");
    }

    #[test]
    fn lifecycle_context_openspec_only() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        std::fs::create_dir_all(cwd.join(".git")).unwrap();
        std::fs::create_dir_all(cwd.join("openspec")).unwrap();

        let tools = vec![ToolDefinition {
            name: "openspec_manage".into(),
            label: "os".into(),
            description: "manage".into(),
            parameters: serde_json::json!({}),
        }];

        let ctx = detect_lifecycle_context(cwd, &tools);
        assert!(ctx.contains("openspec"), "should detect openspec, got: {ctx}");
        assert!(ctx.contains("Spec-driven"), "should include spec guidance");
    }

    #[test]
    fn lifecycle_context_empty_when_no_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = detect_lifecycle_context(dir.path(), &[]);
        assert!(ctx.is_empty(), "no artifacts + no tools = no lifecycle section");
    }

    #[test]
    fn evidence_grounding_in_prompt() {
        let tools = vec![];
        let prompt = build_base_prompt(Path::new("/tmp"), &tools);
        assert!(prompt.contains("Ground claims in evidence"), "should include evidence directive");
    }

    #[test]
    fn lex_imperialis_in_prompt() {
        let tools = vec![];
        let prompt = build_base_prompt(Path::new("/tmp"), &tools);
        assert!(prompt.contains("Lex Imperialis"), "should include Lex Imperialis");
        assert!(prompt.contains("Anti-Sycophancy"), "should include directive I");
        assert!(prompt.contains("Evidence-Based Epistemology"), "should include directive II");
        assert!(prompt.contains("Perfection Is the Enemy of Good"), "should include directive III");
        assert!(prompt.contains("Systems Engineering Harness"), "should include directive IV");
        assert!(prompt.contains("Cognitive Honesty"), "should include directive V");
        assert!(prompt.contains("Operator Agency"), "should include directive VI");
    }

    #[test]
    fn lex_imperialis_before_operator_directives() {
        let tools = vec![];
        let prompt = build_base_prompt(Path::new("/tmp"), &tools);
        let lex_pos = prompt.find("Lex Imperialis").unwrap_or(usize::MAX);
        // Lex should come before any operator/project directives sections
        if let Some(op_pos) = prompt.find("Operator Directives") {
            assert!(lex_pos < op_pos, "Lex Imperialis must appear before Operator Directives");
        }
        if let Some(proj_pos) = prompt.find("Project Directives") {
            assert!(lex_pos < proj_pos, "Lex Imperialis must appear before Project Directives");
        }
    }

    #[test]
    fn core_tool_guidelines_present() {
        let tools = vec![];
        let prompt = build_base_prompt(Path::new("/tmp"), &tools);
        // Each core tool should have a dedicated section
        assert!(prompt.contains("## bash"), "should have bash guidelines");
        assert!(prompt.contains("## read"), "should have read guidelines");
        assert!(prompt.contains("## edit"), "should have edit guidelines");
        assert!(prompt.contains("## write"), "should have write guidelines");
        assert!(prompt.contains("## web_search"), "should have web_search guidelines");
    }

    #[test]
    fn core_tool_guidelines_have_behavioral_advice() {
        let tools = vec![];
        let prompt = build_base_prompt(Path::new("/tmp"), &tools);
        // Not just names — actual guidance
        assert!(prompt.contains("read a file before editing"), "bash should advise read-before-edit");
        assert!(prompt.contains("oldText must match"), "edit should warn about exact matching");
        assert!(prompt.contains("Never use cat"), "bash should redirect to read tool");
        assert!(prompt.contains("surgical changes"), "edit should describe its use case");
        assert!(prompt.contains("compare mode"), "web_search should describe compare mode");
    }
}
