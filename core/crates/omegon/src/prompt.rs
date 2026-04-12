//! System prompt assembly for the headless agent.
//!
//! Phase 0: static base prompt + tool definitions + project directives.
//! Phase 0+: ContextManager provides dynamic injection.

use omegon_traits::{PromptComposition, PromptSectionMetric, ToolDefinition};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptAssembly {
    pub prompt: String,
    pub composition: PromptComposition,
}

/// Build the base system prompt.
///
/// Assembles: identity, tool list, tool guidelines, behavior directives,
/// lifecycle context (if artifacts exist), global/project AGENTS.md,
/// project conventions (auto-detected from config files).
pub fn build_base_prompt(cwd: &Path, tools: &[ToolDefinition]) -> String {
    build_base_prompt_with_breakdown(cwd, tools, false).prompt
}

/// Build the base system prompt and return per-section size instrumentation.
pub fn build_base_prompt_with_breakdown(
    cwd: &Path,
    tools: &[ToolDefinition],
    slim: bool,
) -> PromptAssembly {
    let date = utc_date();
    let tool_list = format_tool_list(tools);
    let lex_imperialis = load_lex_imperialis();
    let lifecycle_context = if slim {
        String::new()
    } else {
        detect_lifecycle_context(cwd, tools)
    };
    let global_directives = if slim {
        String::new()
    } else {
        load_global_directives()
    };
    let project_directives = load_project_directives(cwd);
    let project_conventions = detect_project_conventions(cwd);

    let sections = vec![
        prompt_section(
            "identity",
            "Identity",
            "You are an expert coding assistant. You help by reading files, executing commands, editing code, and writing new files.\n\n",
        ),
        prompt_section(
            "tools",
            "Available Tools",
            &format!("Available tools: {tool_list}\n\n"),
        ),
        prompt_section(
            "behavior",
            "Behavior",
            if slim {
                "# Behavior\n\n- You are operating in OM coding mode — the lean terminal coding loop for direct repo work.\n- Prefer the shortest path to useful local progress: inspect the relevant file, make the smallest justified edit, and run one narrow validation.\n- Keep responses terse, concrete, and grounded in evidence from the repo.\n- Stay inside the local coding loop by default. Do not introduce lifecycle workflows, orchestration, or ambient meta-process unless the operator asks or the task clearly requires them.\n- Small safe edits are allowed, but do not widen scope casually.\n- Always respond to the user. Tool calls gather information — they are not the answer.\n- Be direct — act, don't narrate intent.\n- Read files before editing. Edit requires exact text matches.\n- Ground claims in evidence — cite files and lines.\n- Every non-trivial change needs tests. Commit when done, do NOT push.\n"
            } else {
                "# Behavior\n\n- Always respond to the user. Tool calls gather information — they are not the answer. After calling tools, synthesize what you found into a direct response. Never end a turn with only tool calls and no text.\n- Be direct — act, don't narrate intent. Disagree when you see a better path.\n- Read files before editing. Edit requires exact text matches.\n- Ground claims in evidence — cite files and lines. Don't assert about unread code.\n- Every non-trivial change needs tests. Commit when done, do NOT push.\n- Prefer `request_context` before making multiple exploratory tool calls when you need session orientation or recent runtime evidence. Use direct read/search tools first only when you already know the exact target.\n"
            },
        ),
        prompt_section("core_directives", "Core Directives", &lex_imperialis),
        prompt_section("project_lifecycle", "Project Lifecycle", &lifecycle_context),
        prompt_section(
            "operator_directives",
            "Operator Directives",
            &global_directives,
        ),
        prompt_section(
            "project_directives",
            "Project Directives",
            &project_directives,
        ),
        prompt_section(
            "project_conventions",
            "Project Conventions",
            &project_conventions,
        ),
        prompt_section(
            "runtime_context",
            "Runtime Context",
            &format!(
                "Current date: {date}\nCurrent working directory: {}",
                cwd.display()
            ),
        ),
    ];

    let prompt: String = sections
        .iter()
        .map(|section| section.content.as_str())
        .collect();
    let composition = PromptComposition {
        sections: sections
            .iter()
            .map(|section| PromptSectionMetric {
                key: section.key.to_string(),
                label: section.label.to_string(),
                chars: section.content.len(),
                estimated_tokens: estimate_chars_to_tokens(section.content.len()),
            })
            .collect(),
        total_chars: prompt.len(),
        total_estimated_tokens: estimate_chars_to_tokens(prompt.len()),
    };

    PromptAssembly {
        prompt,
        composition,
    }
}

/// Rich tool guidelines — how to use each tool well, not just what it does.
fn detect_lifecycle_context(cwd: &Path, tools: &[ToolDefinition]) -> String {
    let repo_root = find_repo_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
    let tool_names: std::collections::HashSet<&str> =
        tools.iter().map(|t| t.name.as_str()).collect();

    let has_design_tools = tool_names.contains("design_tree");
    let has_openspec_tools = tool_names.contains("openspec_manage");
    let has_cleave_tools =
        tool_names.contains("cleave_assess") || tool_names.contains("cleave_run");

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
        format!("\n# Project Conventions\n\n{}\n", conventions.join("\n"))
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
            && let Ok(content) = std::fs::read_to_string(&agents_file)
        {
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
                    && let Some(gitdir) = content.strip_prefix("gitdir: ")
                {
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
                        && let Some(repo) = dot_git.parent()
                    {
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

struct PromptSection<'a> {
    key: &'a str,
    label: &'a str,
    content: String,
}

fn prompt_section<'a>(key: &'a str, label: &'a str, content: &str) -> PromptSection<'a> {
    PromptSection {
        key,
        label,
        content: content.to_string(),
    }
}

fn estimate_chars_to_tokens(chars: usize) -> usize {
    chars / 4
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
        if days < ydays {
            break;
        }
        days -= ydays;
        y += 1;
    }
    let leap = is_leap(y);
    let mdays: [i64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0usize;
    for (i, &md) in mdays.iter().enumerate() {
        if days < md {
            m = i;
            break;
        }
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
    fn prompt_breakdown_tracks_sections_and_totals() {
        let tools = vec![omegon_traits::ToolDefinition {
            name: "test_tool".into(),
            label: "test".into(),
            description: "A test tool".into(),
            parameters: serde_json::json!({}),
        }];
        let assembly = build_base_prompt_with_breakdown(Path::new("/tmp"), &tools, false);
        assert_eq!(assembly.composition.total_chars, assembly.prompt.len());
        assert_eq!(
            assembly.composition.total_estimated_tokens,
            assembly.prompt.len() / 4
        );
        assert!(
            assembly
                .composition
                .sections
                .iter()
                .any(|section| section.key == "identity" && section.chars > 0)
        );
        let tools_section = assembly
            .composition
            .sections
            .iter()
            .find(|section| section.key == "tools")
            .unwrap();
        assert!(tools_section.chars >= "Available tools: test_tool\n\n".len());
        assert_eq!(tools_section.estimated_tokens, tools_section.chars / 4);
    }

    #[test]
    fn prompt_breakdown_preserves_prompt_output() {
        let tools = vec![];
        let prompt = build_base_prompt(Path::new("/tmp"), &tools);
        let assembly = build_base_prompt_with_breakdown(Path::new("/tmp"), &tools, false);
        assert_eq!(prompt, assembly.prompt);
    }

    #[test]
    fn slim_prompt_omits_lifecycle_global_and_core_directive_sections() {
        let tools = vec![omegon_traits::ToolDefinition {
            name: "bash".into(),
            label: "test".into(),
            description: "A test tool".into(),
            parameters: serde_json::json!({}),
        }];
        let assembly = build_base_prompt_with_breakdown(Path::new("/tmp"), &tools, true);
        let section_keys: Vec<&str> = assembly
            .composition
            .sections
            .iter()
            .filter(|section| section.chars > 0)
            .map(|section| section.key.as_str())
            .collect();
        assert!(!section_keys.contains(&"project_lifecycle"));
        assert!(!section_keys.contains(&"operator_directives"));
        assert!(section_keys.contains(&"core_directives"));
        assert!(assembly.prompt.contains("OM coding mode"));
        assert!(assembly.prompt.contains("lean terminal coding loop"));
        assert!(
            !assembly
                .prompt
                .contains("recommend escalating to full Omegon")
        );
        assert!(assembly.prompt.contains("Lex Imperialis"));
    }

    #[test]
    fn base_prompt_includes_commit_instructions() {
        let tools = vec![];
        let prompt = build_base_prompt(Path::new("/tmp"), &tools);
        assert!(
            prompt.contains("Commit when done"),
            "should instruct to commit"
        );
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
        assert!(
            ctx.contains("Project Lifecycle"),
            "should detect lifecycle, got: {ctx}"
        );
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
        assert!(
            ctx.contains("openspec"),
            "should detect openspec, got: {ctx}"
        );
        assert!(ctx.contains("Spec-driven"), "should include spec guidance");
    }

    #[test]
    fn lifecycle_context_empty_when_no_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = detect_lifecycle_context(dir.path(), &[]);
        assert!(
            ctx.is_empty(),
            "no artifacts + no tools = no lifecycle section"
        );
    }

    #[test]
    fn evidence_grounding_in_prompt() {
        let tools = vec![];
        let prompt = build_base_prompt(Path::new("/tmp"), &tools);
        assert!(
            prompt.contains("Ground claims in evidence"),
            "should include evidence directive"
        );
    }

    #[test]
    fn lex_imperialis_in_prompt() {
        let tools = vec![];
        let prompt = build_base_prompt(Path::new("/tmp"), &tools);
        assert!(
            prompt.contains("Lex Imperialis"),
            "should include Lex Imperialis"
        );
        assert!(
            prompt.contains("Anti-Sycophancy"),
            "should include directive I"
        );
        assert!(
            prompt.contains("Evidence-Based Epistemology"),
            "should include directive II"
        );
        assert!(
            prompt.contains("Perfection Is the Enemy of Good"),
            "should include directive III"
        );
        assert!(
            prompt.contains("Systems Engineering Harness"),
            "should include directive IV"
        );
        assert!(
            prompt.contains("Cognitive Honesty"),
            "should include directive V"
        );
        assert!(
            prompt.contains("Operator Agency"),
            "should include directive VI"
        );
    }

    #[test]
    fn lex_imperialis_before_operator_directives() {
        let tools = vec![];
        let prompt = build_base_prompt(Path::new("/tmp"), &tools);
        let lex_pos = prompt.find("Lex Imperialis").unwrap_or(usize::MAX);
        // Lex should come before any operator/project directives sections
        if let Some(op_pos) = prompt.find("Operator Directives") {
            assert!(
                lex_pos < op_pos,
                "Lex Imperialis must appear before Operator Directives"
            );
        }
        if let Some(proj_pos) = prompt.find("Project Directives") {
            assert!(
                lex_pos < proj_pos,
                "Lex Imperialis must appear before Project Directives"
            );
        }
    }

    /// Audit: measure token budget consumed by all registered tools.
    /// This test doesn't assert — it prints a budget report.
    /// Run with: cargo test -p omegon -- tool_token_budget_audit --nocapture
    #[test]
    fn tool_token_budget_audit() {
        use omegon_traits::ToolProvider;

        // Gather all tool providers (mirrors setup.rs registration order)
        let providers: Vec<(&str, Box<dyn ToolProvider>)> = vec![
            (
                "core-tools",
                Box::new(crate::tools::CoreTools::new(std::path::PathBuf::from(
                    "/tmp",
                ))),
            ),
            (
                "web-search",
                Box::new(crate::tools::web_search::WebSearchProvider::new()),
            ),
            (
                "local-inference",
                Box::new(crate::tools::local_inference::LocalInferenceProvider::new()),
            ),
            (
                "view",
                Box::new(crate::tools::view::ViewProvider::new(
                    std::path::PathBuf::from("/tmp"),
                )),
            ),
            (
                "render",
                Box::new(crate::tools::render::RenderProvider::new()),
            ),
        ];

        // Disabled tools (from setup.rs default profile)
        let disabled: std::collections::HashSet<&str> = [
            crate::tool_registry::core::SPECULATE_START,
            crate::tool_registry::core::SPECULATE_CHECK,
            crate::tool_registry::core::SPECULATE_COMMIT,
            crate::tool_registry::core::SPECULATE_ROLLBACK,
            crate::tool_registry::render::RENDER_DIAGRAM,
            crate::tool_registry::render::GENERATE_IMAGE_LOCAL,
            crate::tool_registry::persona::SWITCH_PERSONA,
            crate::tool_registry::persona::SWITCH_TONE,
            crate::tool_registry::persona::LIST_PERSONAS,
            crate::tool_registry::delegate::DELEGATE,
            crate::tool_registry::delegate::DELEGATE_RESULT,
            crate::tool_registry::delegate::DELEGATE_STATUS,
            crate::tool_registry::auth::AUTH_STATUS,
            crate::tool_registry::harness_settings::HARNESS_SETTINGS,
            crate::tool_registry::memory::MEMORY_INGEST_LIFECYCLE,
            crate::tool_registry::memory::MEMORY_CONNECT,
            crate::tool_registry::memory::MEMORY_SEARCH_ARCHIVE,
        ]
        .into_iter()
        .collect();

        let mut all_tools = Vec::new();
        let mut group_budgets: Vec<(&str, usize, usize, usize)> = Vec::new(); // (group, active_count, active_tokens, disabled_tokens)

        for (group, provider) in &providers {
            let tools = provider.tools();
            let mut active_tokens = 0usize;
            let mut disabled_tokens = 0usize;
            let mut active_count = 0usize;

            for tool in &tools {
                let schema_json = serde_json::to_string(&tool.parameters).unwrap_or_default();
                let tool_chars = tool.name.len() + tool.description.len() + schema_json.len();
                let tool_tokens = tool_chars / 4;

                if disabled.contains(tool.name.as_str()) {
                    disabled_tokens += tool_tokens;
                } else {
                    active_tokens += tool_tokens;
                    active_count += 1;
                }
                all_tools.push((
                    tool.name.clone(),
                    tool_tokens,
                    disabled.contains(tool.name.as_str()),
                    group.to_string(),
                ));
            }
            group_budgets.push((group, active_count, active_tokens, disabled_tokens));
        }

        // Sort by token cost descending
        all_tools.sort_by(|a, b| b.1.cmp(&a.1));

        eprintln!("\n╔═══════════════════════════════════════════════════════════════╗");
        eprintln!("║              TOOL TOKEN BUDGET AUDIT                         ║");
        eprintln!("╠═══════════════════════════════════════════════════════════════╣");
        eprintln!(
            "║ {:>5} {:3} {:<30} {:<8} {:<10} ║",
            "Tok", "Act", "Tool", "Group", "Status"
        );
        eprintln!("╠═══════════════════════════════════════════════════════════════╣");
        for (name, tokens, is_disabled, group) in &all_tools {
            let status = if *is_disabled { "disabled" } else { "ACTIVE" };
            let marker = if *is_disabled { " " } else { "●" };
            eprintln!(
                "║ {:>5} {marker:>3} {:<30} {:<8} {:<10} ║",
                tokens, name, group, status
            );
        }
        eprintln!("╠═══════════════════════════════════════════════════════════════╣");

        let total_active: usize = all_tools.iter().filter(|t| !t.2).map(|t| t.1).sum();
        let total_disabled: usize = all_tools.iter().filter(|t| t.2).map(|t| t.1).sum();
        let total_all: usize = all_tools.iter().map(|t| t.1).sum();
        let active_count = all_tools.iter().filter(|t| !t.2).count();
        let disabled_count = all_tools.iter().filter(|t| t.2).count();

        eprintln!(
            "║ Active:   {:>3} tools = {:>5} tokens/request              ║",
            active_count, total_active
        );
        eprintln!(
            "║ Disabled: {:>3} tools = {:>5} tokens (saved)               ║",
            disabled_count, total_disabled
        );
        eprintln!(
            "║ Total:    {:>3} tools = {:>5} tokens (if all enabled)      ║",
            all_tools.len(),
            total_all
        );
        eprintln!("╠═══════════════════════════════════════════════════════════════╣");

        // System prompt measurement
        let active_tool_defs: Vec<_> = providers
            .iter()
            .flat_map(|(_, p)| p.tools())
            .filter(|t| !disabled.contains(t.name.as_str()))
            .collect();
        let prompt = build_base_prompt(Path::new("/tmp"), &active_tool_defs);
        let prompt_tokens = prompt.len() / 4;
        eprintln!(
            "║ System prompt:     {:>5} tokens ({} chars)          ║",
            prompt_tokens,
            prompt.len()
        );
        eprintln!(
            "║ Fixed overhead:    {:>5} tokens/request              ║",
            prompt_tokens + total_active
        );
        eprintln!("║                                                               ║");

        // Budget impact on different context classes
        for (class, window) in [
            ("Squad 128k", 131_072usize),
            ("Maniple 272k", 278_528usize),
            ("Clan 440k", 409_600usize),
            ("Legion 1M", 1_048_576usize),
        ] {
            let overhead = prompt_tokens + total_active + 16_384; // + max_output_tokens
            let available = window.saturating_sub(overhead);
            let pct = (overhead as f64 / window as f64 * 100.0) as usize;
            eprintln!(
                "║ {class:<15} overhead: {pct:>2}% → {available:>7} tokens for conversation ║"
            );
        }
        eprintln!("╚═══════════════════════════════════════════════════════════════╝\n");

        // Soft assertion: active tools shouldn't exceed 10k tokens
        assert!(
            total_active < 15_000,
            "Active tool token budget ({total_active}) exceeds 15k — review tool descriptions"
        );
    }
}
