//! Project context discovery for cleave task file enrichment.
//!
//! Extracts dependency versions, test conventions, submodule info,
//! and file signatures from the project to give children the context
//! they need to write correct code on the first attempt.

use std::path::{Path, PathBuf};

/// Discovered project context for a child's scope.
#[derive(Debug, Default)]
pub struct ChildContext {
    /// Submodule paths detected in the repo.
    pub submodules: Vec<String>,
    /// Dependency version snippets relevant to the child's scope.
    pub dependency_snippets: Vec<String>,
    /// Example test code from the same crate/package.
    pub test_example: Option<String>,
    /// Finalization instructions (submodule-aware if applicable).
    pub finalization: String,
}

/// Build context for a child given its scope and the repo path.
pub fn discover_child_context(repo_path: &Path, scope: &[String]) -> ChildContext {
    let submodules = detect_submodule_names(repo_path);
    let dependency_snippets = extract_dependency_versions(repo_path, scope);
    let test_example = sample_test_convention(repo_path, scope);
    let finalization = build_finalization_section(&submodules);

    ChildContext {
        submodules,
        dependency_snippets,
        test_example,
        finalization,
    }
}

/// Format the child context as markdown sections for the task file.
pub fn format_context_sections(ctx: &ChildContext) -> String {
    let mut sections = String::new();

    if !ctx.submodules.is_empty() {
        sections.push_str("## Repository Structure\n\n");
        sections
            .push_str("This repo uses git submodules. Your scope files live inside a submodule.\n");
        sections.push_str("Submodules: ");
        sections.push_str(&ctx.submodules.join(", "));
        sections.push_str("\n\n");
        sections
            .push_str("**Important**: When you modify files inside a submodule, you must commit\n");
        sections
            .push_str("inside the submodule first, then update the pointer in the parent repo.\n");
        sections.push_str("See the Finalization section below for exact steps.\n\n");
    }

    if !ctx.dependency_snippets.is_empty() {
        sections.push_str("## Dependency Versions\n\n");
        sections.push_str(
            "Use these exact versions — do not rely on training data for API shapes:\n\n",
        );
        for snippet in &ctx.dependency_snippets {
            sections.push_str("```toml\n");
            sections.push_str(snippet);
            sections.push_str("\n```\n\n");
        }
    }

    if let Some(ref example) = ctx.test_example {
        sections.push_str("## Test Convention\n\n");
        sections.push_str("Follow this pattern from an existing test in the same crate:\n\n");
        sections.push_str("```rust\n");
        sections.push_str(example);
        sections.push_str("\n```\n\n");
    }

    // Note: finalization is NOT included here — the orchestrator's
    // task file template places it after the Contract section.
    sections
}

/// Structured testing directives extracted from enriched task content.
#[derive(Debug, Default, Clone)]
pub struct TestingDirectives {
    /// Spec scenarios that must have passing tests.
    pub spec_scenarios: Vec<String>,
    /// Edge cases that must each have at least one test.
    pub edge_cases: Vec<String>,
}

impl TestingDirectives {
    /// Whether any testing directives are present.
    pub fn is_empty(&self) -> bool {
        self.spec_scenarios.is_empty() && self.edge_cases.is_empty()
    }
}

/// Extract testing directives from enriched task file content
/// (e.g., from an OpenSpec-generated tasks.md).
///
/// Looks for `### Testing Requirements` sections with two tiers:
/// - `**Spec Scenarios (must pass):**` followed by `- item` lines
/// - `**Edge Cases (must have tests):**` followed by `- item` lines
pub fn extract_testing_directives(task_content: &str) -> TestingDirectives {
    let mut directives = TestingDirectives::default();

    // Find the Testing Requirements section
    let Some(testing_start) = task_content.find("### Testing Requirements") else {
        return directives;
    };
    let section = &task_content[testing_start..];

    // Find end of section (next ## heading or end of content)
    let section_end = section[1..]
        .find("\n## ")
        .map(|i| i + 1)
        .unwrap_or(section.len());
    let section = &section[..section_end];

    let mut current_tier: Option<&str> = None;

    for line in section.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("**Spec Scenarios") {
            current_tier = Some("scenarios");
            continue;
        }
        if trimmed.starts_with("**Edge Cases") {
            current_tier = Some("edge_cases");
            continue;
        }
        // Any other bold heading or section heading resets
        if trimmed.starts_with("**") || trimmed.starts_with("##") {
            current_tier = None;
            continue;
        }

        if let Some(tier) = current_tier
            && let Some(item) = trimmed.strip_prefix("- ")
        {
            // Skip checkbox items (task lines like "- [ ] 1.3 Tests for...")
            if !item.is_empty()
                && !item.starts_with("[ ]")
                && !item.starts_with("[x]")
                && !item.starts_with("[X]")
            {
                match tier {
                    "scenarios" => directives.spec_scenarios.push(item.to_string()),
                    "edge_cases" => directives.edge_cases.push(item.to_string()),
                    _ => {}
                }
            }
        }
    }

    directives
}

/// Format testing directives as a markdown section for the task file.
/// Returns empty string when no directives are present.
pub fn format_testing_section(directives: &TestingDirectives, test_convention: &str) -> String {
    if directives.is_empty() {
        return String::new();
    }

    let mut section = String::from("## Testing Requirements\n\n");

    if !directives.spec_scenarios.is_empty() {
        section.push_str("### Spec Scenarios (must pass)\n\n");
        section
            .push_str("These scenarios from the spec MUST have corresponding passing tests:\n\n");
        for s in &directives.spec_scenarios {
            section.push_str(&format!("- {s}\n"));
        }
        section.push('\n');
    }

    if !directives.edge_cases.is_empty() {
        section.push_str("### Edge Cases (must have tests)\n\n");
        section.push_str("Each of these must have at least one test:\n\n");
        for ec in &directives.edge_cases {
            section.push_str(&format!("- {ec}\n"));
        }
        section.push('\n');
    }

    section.push_str(&format!("### Test Convention\n\n{test_convention}\n\n"));

    section
}

/// Detect submodule names from `git submodule status`.
/// Delegates to worktree::detect_submodules to avoid duplication.
fn detect_submodule_names(repo_path: &Path) -> Vec<String> {
    super::worktree::detect_submodules(repo_path)
        .into_iter()
        .map(|(name, _path)| name)
        .collect()
}

/// Extract dependency version sections from Cargo.toml files relevant to scope.
fn extract_dependency_versions(repo_path: &Path, scope: &[String]) -> Vec<String> {
    let mut snippets = Vec::new();

    // Find unique Cargo.toml paths from scope entries
    let mut cargo_paths: Vec<PathBuf> = Vec::new();
    for s in scope {
        let full = repo_path.join(s);
        // Walk up from scope path to find nearest Cargo.toml
        let mut dir = if full.is_file() || !full.exists() {
            full.parent().map(|p| p.to_path_buf())
        } else {
            Some(full)
        };
        while let Some(d) = dir {
            let cargo = d.join("Cargo.toml");
            if cargo.exists() && !cargo_paths.contains(&cargo) {
                cargo_paths.push(cargo);
                break;
            }
            if d == repo_path {
                break;
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }

    for cargo_path in &cargo_paths {
        if let Ok(content) = std::fs::read_to_string(cargo_path) {
            let relative = cargo_path.strip_prefix(repo_path).unwrap_or(cargo_path);
            let mut snippet = format!("# {}\n", relative.display());
            let mut in_deps = false;
            let mut lines_added = 0;

            for line in content.lines() {
                if line.starts_with("[dependencies]")
                    || line.starts_with("[dev-dependencies]")
                    || line.starts_with("[build-dependencies]")
                {
                    if lines_added > 0 {
                        snippet.push('\n');
                    }
                    snippet.push_str(line);
                    snippet.push('\n');
                    in_deps = true;
                    lines_added += 1;
                } else if line.starts_with('[') {
                    in_deps = false;
                } else if in_deps && !line.trim().is_empty() {
                    snippet.push_str(line);
                    snippet.push('\n');
                    lines_added += 1;
                }
            }

            if lines_added > 1 {
                // More than just the section header
                snippets.push(snippet);
            }
        }
    }

    // Also check package.json
    for s in scope {
        let full = repo_path.join(s);
        let mut dir = if full.is_file() || !full.exists() {
            full.parent().map(|p| p.to_path_buf())
        } else {
            Some(full)
        };
        while let Some(d) = dir {
            let pkg = d.join("package.json");
            if pkg.exists() {
                if let Ok(content) = std::fs::read_to_string(&pkg)
                    && let Ok(v) = serde_json::from_str::<serde_json::Value>(&content)
                {
                    let relative = pkg.strip_prefix(repo_path).unwrap_or(&pkg);
                    let mut parts = vec![format!("# {}", relative.display())];
                    if let Some(deps) = v.get("dependencies").and_then(|d| d.as_object()) {
                        parts.push("[dependencies]".to_string());
                        for (k, ver) in deps {
                            parts.push(format!("{k} = {ver}"));
                        }
                    }
                    if let Some(deps) = v.get("devDependencies").and_then(|d| d.as_object()) {
                        parts.push("[devDependencies]".to_string());
                        for (k, ver) in deps {
                            parts.push(format!("{k} = {ver}"));
                        }
                    }
                    if parts.len() > 1 {
                        snippets.push(parts.join("\n"));
                    }
                }
                break;
            }
            if d == repo_path {
                break;
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }

    snippets
}

/// Sample one existing test from the same crate to show the child the convention.
fn sample_test_convention(repo_path: &Path, scope: &[String]) -> Option<String> {
    // Find the crate/package root from scope
    for s in scope {
        let full = repo_path.join(s);
        let mut dir = if full.is_file() || !full.exists() {
            full.parent().map(|p| p.to_path_buf())
        } else {
            Some(full)
        };

        // Walk up to find src/ directory
        while let Some(d) = dir {
            let src_dir = if d.ends_with("src") {
                d.clone()
            } else if d.join("src").is_dir() {
                d.join("src")
            } else {
                dir = d.parent().map(|p| p.to_path_buf());
                if d == repo_path {
                    break;
                }
                continue;
            };

            // Find a .rs file with #[test] or #[cfg(test)]
            if let Some(test_sample) = find_test_sample(&src_dir) {
                return Some(test_sample);
            }
            break;
        }
    }
    None
}

/// Find a single test function from a directory of Rust source files.
/// Searches recursively up to 3 levels deep.
fn find_test_sample(src_dir: &Path) -> Option<String> {
    find_test_sample_recursive(src_dir, src_dir, 0)
}

fn find_test_sample_recursive(src_dir: &Path, root: &Path, depth: usize) -> Option<String> {
    if depth > 3 {
        return None;
    }
    let entries = std::fs::read_dir(src_dir).ok()?;

    // First pass: check .rs files in this directory
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
        } else if path.extension().is_some_and(|e| e == "rs")
            && let Ok(content) = std::fs::read_to_string(&path)
            && let Some(sample) = extract_first_test(&content)
        {
            let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
            return Some(format!("// From {relative}\n{sample}"));
        }
    }

    // Second pass: recurse into subdirectories
    for subdir in subdirs {
        if let Some(sample) = find_test_sample_recursive(&subdir, root, depth + 1) {
            return Some(sample);
        }
    }
    None
}

/// Extract the first #[test] or #[tokio::test] function from source code.
/// Returns at most 30 lines to stay within token budget.
///
/// Uses brace counting on code outside string literals and comments
/// to find the function boundary.
fn extract_first_test(source: &str) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut start = None;
    let mut brace_depth: i32 = 0;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        if start.is_none() && (trimmed == "#[test]" || trimmed == "#[tokio::test]") {
            start = Some(i);
            brace_depth = 0;
            continue;
        }

        if let Some(s) = start {
            // Count braces outside string literals and comments
            brace_depth += count_braces_outside_strings(trimmed);

            if brace_depth <= 0 && i > s {
                let end = (i + 1).min(s + 30); // cap at 30 lines
                let extracted: Vec<&str> = lines[s..end].to_vec();
                return Some(extracted.join("\n"));
            }
        }
    }
    None
}

/// Count net braces ({/}) in a line, ignoring those inside string literals
/// and after line comments.
fn count_braces_outside_strings(line: &str) -> i32 {
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut in_raw_string = false;
    let mut prev = '\0';

    for ch in line.chars() {
        // Skip line comments
        if !in_string && !in_raw_string && prev == '/' && ch == '/' {
            // Undo the '/' we might have counted (we didn't — it's not a brace)
            break;
        }

        if !in_raw_string && ch == '"' && prev != '\\' {
            in_string = !in_string;
        }

        // Rough raw string detection: r#" ... "#
        if !in_string && prev == '#' && ch == '"' {
            in_raw_string = true;
        } else if in_raw_string && prev == '"' && ch == '#' {
            in_raw_string = false;
        }

        if !in_string && !in_raw_string {
            match ch {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        prev = ch;
    }
    depth
}

/// Build finalization section with submodule-aware instructions.
fn build_finalization_section(submodules: &[String]) -> String {
    let mut section = String::from("## Finalization (REQUIRED before completion)\n\n");

    section.push_str("You MUST complete these steps before finishing:\n\n");
    section.push_str("1. Run all guardrail checks listed above and fix failures\n");
    section.push_str("2. Commit your in-scope work with a clean git state when you are done\n");

    if !submodules.is_empty() {
        section.push_str("3. **Submodule note**: if your scope crosses a submodule boundary, commit your edits normally. The orchestrator will handle submodule pointer updates during harvest/merge.\n");
        section.push_str("4. Verify clean state for the files you changed: `git status` should show nothing left to commit in your worktree\n");
    } else {
        section.push_str(
            "3. Commit with a clear message: `git commit -m \"feat(<label>): <summary>\"`\n",
        );
        section.push_str("4. Verify clean state: `git status` should show nothing to commit\n");
    }

    section.push_str("\nDo NOT edit `.cleave-prompt.md` or any task/result metadata files. Those are orchestrator-owned and may be ignored by git.\n");
    section.push_str(
        "Return your completion summary in your normal final response instead of modifying the prompt file.\n",
    );
    section.push_str("\n> ⚠️ Uncommitted work will be lost. The orchestrator merges from your branch's commits.\n");

    section
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_first_test_finds_sync_test() {
        let source = r#"
fn helper() -> u32 { 42 }

#[test]
fn test_helper() {
    assert_eq!(helper(), 42);
}

#[test]
fn test_other() {
    assert!(true);
}
"#;
        let result = extract_first_test(source).unwrap();
        assert!(result.contains("#[test]"));
        assert!(result.contains("test_helper"));
        assert!(!result.contains("test_other")); // Only first test
    }

    #[test]
    fn extract_first_test_finds_async_test() {
        let source = r#"
#[tokio::test]
async fn test_async() {
    let x = tokio::time::sleep(Duration::from_millis(1)).await;
    assert!(true);
}
"#;
        let result = extract_first_test(source).unwrap();
        assert!(result.contains("#[tokio::test]"));
        assert!(result.contains("test_async"));
    }

    #[test]
    fn extract_first_test_caps_at_30_lines() {
        let mut source = String::from("#[test]\nfn long_test() {\n");
        for i in 0..50 {
            source.push_str(&format!("    let x{i} = {i};\n"));
        }
        source.push_str("}\n");
        let result = extract_first_test(&source).unwrap();
        assert!(result.lines().count() <= 30);
    }

    #[test]
    fn extract_first_test_none_when_no_tests() {
        let source = "fn main() { println!(\"hello\"); }";
        assert!(extract_first_test(source).is_none());
    }

    #[test]
    fn finalization_without_submodules() {
        let section = build_finalization_section(&[]);
        assert!(section.contains("Finalization"));
        assert!(!section.contains("Submodule commits"));
        assert!(section.contains("git commit"));
        assert!(section.contains("Do NOT edit `.cleave-prompt.md`"));
        assert!(!section.contains("Update the Result section below"));
    }

    #[test]
    fn finalization_with_submodules() {
        let section = build_finalization_section(&["core".to_string()]);
        assert!(section.contains("Submodule note"));
        assert!(section.contains("orchestrator will handle submodule pointer updates"));
        assert!(section.contains("Do NOT edit `.cleave-prompt.md`"));
    }

    #[test]
    fn extract_testing_directives_from_enriched_content() {
        let content = r#"
## 1. JWT token validation

- [ ] 1.1 Valid token accepted
- [ ] 1.2 Expired token rejected

### Testing Requirements

**Spec Scenarios (must pass):**
- Valid token accepted
- Expired token rejected

**Edge Cases (must have tests):**
- Empty token string → 401 with descriptive error
- Token with invalid signature → 401, not 500

- [ ] 1.3 Tests for JWT token validation (see Testing Requirements above)

## 2. Refresh tokens
"#;
        let directives = extract_testing_directives(content);
        assert_eq!(directives.spec_scenarios.len(), 2);
        assert_eq!(directives.edge_cases.len(), 2);
        assert_eq!(directives.spec_scenarios[0], "Valid token accepted");
        assert!(directives.edge_cases[0].contains("Empty token"));
    }

    #[test]
    fn extract_testing_directives_empty_when_no_section() {
        let content = "## Task\n\n- [ ] Do something\n\n## Contract\n\n1. Be good\n";
        let directives = extract_testing_directives(content);
        assert!(directives.is_empty());
    }

    #[test]
    fn extract_testing_directives_handles_scenarios_only() {
        let content = "### Testing Requirements\n\n**Spec Scenarios (must pass):**\n- Scenario A\n- Scenario B\n";
        let directives = extract_testing_directives(content);
        assert_eq!(directives.spec_scenarios.len(), 2);
        assert_eq!(directives.edge_cases.len(), 0);
    }

    #[test]
    fn format_testing_section_produces_markdown() {
        let directives = TestingDirectives {
            spec_scenarios: vec!["Read returns data".into(), "Write succeeds".into()],
            edge_cases: vec!["Empty path → error".into()],
        };
        let section = format_testing_section(&directives, "Write #[test] functions");
        assert!(section.contains("## Testing Requirements"));
        assert!(section.contains("### Spec Scenarios (must pass)"));
        assert!(section.contains("Read returns data"));
        assert!(section.contains("### Edge Cases (must have tests)"));
        assert!(section.contains("Empty path → error"));
        assert!(section.contains("### Test Convention"));
        assert!(section.contains("#[test]"));
    }

    #[test]
    fn format_testing_section_empty_when_no_directives() {
        let directives = TestingDirectives::default();
        let section = format_testing_section(&directives, "Write tests");
        assert!(section.is_empty());
    }

    #[test]
    fn dependency_extraction_from_cargo_toml() {
        let dir = tempfile::tempdir().unwrap();
        let cargo = dir.path().join("Cargo.toml");
        std::fs::write(
            &cargo,
            r#"
[package]
name = "test-crate"
version = "0.1.0"

[dependencies]
serde = "1"
reqwest = { version = "0.12", features = ["json"] }

[dev-dependencies]
mockito = "1"
tokio-test = "0.4"

[build-dependencies]
cc = "1"
"#,
        )
        .unwrap();

        let snippets = extract_dependency_versions(dir.path(), &["src/lib.rs".to_string()]);
        assert!(!snippets.is_empty());
        let snippet = &snippets[0];
        assert!(snippet.contains("serde"));
        assert!(snippet.contains("mockito"));
        assert!(snippet.contains("[dev-dependencies]"));
    }
}
