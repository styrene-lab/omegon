//! Test-architect: synthetic wave-0 child that produces test plans
//! before implementation children run.
//!
//! Only injected when `openspec_change_path` is set. Reads specs + design
//! to produce per-child test plan files that guide test writing.
//! Runs at cheap model tier — pure analysis, no code generation.

use std::path::Path;

/// A test plan for a single implementation child.
#[derive(Debug, Clone)]
pub struct TestPlan {
    /// Child label this plan targets.
    pub child_label: String,
    /// Required test functions (name + description).
    pub required_tests: Vec<TestDescription>,
    /// Edge case descriptions to test.
    pub edge_cases: Vec<String>,
    /// Expected minimum test count.
    pub expected_test_count: usize,
}

/// A single required test.
#[derive(Debug, Clone)]
pub struct TestDescription {
    /// Suggested function name (e.g., `test_empty_path_returns_error`).
    pub name: String,
    /// What the test should verify.
    pub description: String,
}

/// Build the test-architect child's description/prompt.
///
/// The test-architect reads specs and design, then writes one
/// `<child-label>-tests.md` file per implementation child.
pub fn build_test_architect_prompt(
    spec_content: &str,
    design_content: &str,
    children: &[(String, String, Vec<String>)], // (label, description, scope)
) -> String {
    let mut prompt = String::from(
        "You are a test architect. Your job is to design test plans for implementation children.\n\
         Do NOT write code. Write test plan markdown files.\n\n",
    );

    prompt.push_str("## Specifications\n\n");
    prompt.push_str(spec_content);
    prompt.push_str("\n\n## Design\n\n");
    prompt.push_str(design_content);
    prompt.push_str("\n\n## Implementation Children\n\n");

    for (label, description, scope) in children {
        prompt.push_str(&format!(
            "### {label}\n\n{description}\n\nScope: {}\n\n",
            scope.join(", ")
        ));
    }

    prompt.push_str(
        "## Instructions\n\n\
         For each child above, write a file named `<child-label>-tests.md` containing:\n\n\
         1. **Required Tests** — function name + description + key assertions\n\
         2. **Edge Cases** — one-liner edge cases that must each have a test\n\
         3. **Expected Test Count** — minimum number of test functions\n\n\
         Focus on:\n\
         - Every spec scenario must have at least one corresponding test\n\
         - Invert each scenario: empty inputs, error responses, timeouts, boundary values\n\
         - Note any mock/fixture setup requirements\n\
         - Keep it concise — 2-4 sentences per test description\n",
    );

    prompt
}

/// Parse a test plan markdown file into a structured TestPlan.
///
/// Expected format:
/// ```markdown
/// # Test Plan: <child-label>
///
/// ## Required Tests
///
/// ### test_function_name
/// Description of what to test.
///
/// ## Edge Cases
/// - condition → expected behavior
///
/// ## Expected Test Count: N
/// ```
pub fn parse_test_plan(child_label: &str, content: &str) -> TestPlan {
    let mut plan = TestPlan {
        child_label: child_label.to_string(),
        required_tests: Vec::new(),
        edge_cases: Vec::new(),
        expected_test_count: 0,
    };

    let mut in_required = false;
    let mut in_edge_cases = false;
    let mut current_test_name: Option<String> = None;
    let mut current_test_desc = String::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Section detection
        if trimmed.starts_with("## Required Tests") {
            flush_test(&mut plan, &mut current_test_name, &mut current_test_desc);
            in_required = true;
            in_edge_cases = false;
            continue;
        }
        if trimmed.starts_with("## Edge Cases") {
            flush_test(&mut plan, &mut current_test_name, &mut current_test_desc);
            in_required = false;
            in_edge_cases = true;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## Expected Test Count") {
            flush_test(&mut plan, &mut current_test_name, &mut current_test_desc);
            in_required = false;
            in_edge_cases = false;
            // Parse count from "## Expected Test Count: N" or "## Expected Test Count\n\nN"
            if let Some(n) = rest.strip_prefix(':').and_then(|s| s.trim().parse::<usize>().ok()) {
                plan.expected_test_count = n;
            }
            continue;
        }
        if trimmed.starts_with("## ") || trimmed.starts_with("# ") {
            flush_test(&mut plan, &mut current_test_name, &mut current_test_desc);
            in_required = false;
            in_edge_cases = false;
            continue;
        }

        // Content parsing
        if in_required {
            if let Some(name) = trimmed.strip_prefix("### ") {
                flush_test(&mut plan, &mut current_test_name, &mut current_test_desc);
                current_test_name = Some(name.to_string());
            } else if current_test_name.is_some() && !trimmed.is_empty() {
                if !current_test_desc.is_empty() {
                    current_test_desc.push(' ');
                }
                current_test_desc.push_str(trimmed);
            }
        }

        if in_edge_cases && trimmed.starts_with("- ") {
            let item = &trimmed[2..];
            if !item.is_empty() {
                plan.edge_cases.push(item.to_string());
            }
        }
    }

    flush_test(&mut plan, &mut current_test_name, &mut current_test_desc);

    // Default expected count if not specified
    if plan.expected_test_count == 0 {
        plan.expected_test_count = plan.required_tests.len() + plan.edge_cases.len();
    }

    plan
}

fn flush_test(plan: &mut TestPlan, name: &mut Option<String>, desc: &mut String) {
    if let Some(n) = name.take() {
        plan.required_tests.push(TestDescription {
            name: n,
            description: std::mem::take(desc),
        });
    }
    desc.clear();
}

/// Scan a workspace directory for test plan files (<label>-tests.md).
pub fn find_test_plans(workspace: &Path) -> Vec<(String, String)> {
    let mut plans = Vec::new();

    if let Ok(entries) = std::fs::read_dir(workspace) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with("-tests.md") {
                let label = name.trim_end_matches("-tests.md").to_string();
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    plans.push((label, content));
                }
            }
        }
    }

    plans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_includes_all_children() {
        let children = vec![
            ("vault-client".into(), "Implement Vault HTTP client".into(), vec!["src/vault.rs".into()]),
            ("tui-command".into(), "Add /vault command".into(), vec!["src/tui/mod.rs".into()]),
        ];
        let prompt = build_test_architect_prompt("spec content here", "design content here", &children);
        assert!(prompt.contains("vault-client"));
        assert!(prompt.contains("tui-command"));
        assert!(prompt.contains("spec content here"));
        assert!(prompt.contains("design content here"));
        assert!(prompt.contains("<child-label>-tests.md"));
    }

    #[test]
    fn parse_test_plan_extracts_structure() {
        let content = r#"# Test Plan: vault-client

## Required Tests

### test_read_secret_happy_path
Read a secret from Vault when path is allowed and server is healthy.
Should return the secret value.

### test_read_empty_path
Verify that an empty path string returns an error, not a panic.

## Edge Cases
- Network timeout mid-response → clean error
- KV v2 missing data.data → descriptive error
- Token expired → 403 with re-auth hint

## Expected Test Count: 8
"#;
        let plan = parse_test_plan("vault-client", content);
        assert_eq!(plan.child_label, "vault-client");
        assert_eq!(plan.required_tests.len(), 2);
        assert_eq!(plan.required_tests[0].name, "test_read_secret_happy_path");
        assert!(plan.required_tests[0].description.contains("secret value"));
        assert_eq!(plan.edge_cases.len(), 3);
        assert!(plan.edge_cases[0].contains("Network timeout"));
        assert_eq!(plan.expected_test_count, 8);
    }

    #[test]
    fn parse_test_plan_defaults_count() {
        let content = "## Required Tests\n\n### test_a\nDescription A\n\n## Edge Cases\n- case 1\n";
        let plan = parse_test_plan("child", content);
        assert_eq!(plan.expected_test_count, 2); // 1 required + 1 edge case
    }

    #[test]
    fn parse_test_plan_handles_empty_content() {
        let plan = parse_test_plan("empty", "");
        assert!(plan.required_tests.is_empty());
        assert!(plan.edge_cases.is_empty());
        assert_eq!(plan.expected_test_count, 0);
    }

    #[test]
    fn find_test_plans_scans_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("vault-client-tests.md"), "plan A").unwrap();
        std::fs::write(dir.path().join("tui-command-tests.md"), "plan B").unwrap();
        std::fs::write(dir.path().join("not-a-plan.txt"), "ignored").unwrap();

        let plans = find_test_plans(dir.path());
        assert_eq!(plans.len(), 2);
        let labels: Vec<_> = plans.iter().map(|(l, _)| l.as_str()).collect();
        assert!(labels.contains(&"vault-client"));
        assert!(labels.contains(&"tui-command"));
    }
}
