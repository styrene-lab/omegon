//! Post-merge coverage check — deterministic (no LLM).
//!
//! Compares test-architect plans against actual test functions in merged source.
//! Reports which planned tests were implemented, which are missing, and which
//! are new (unplanned).

use std::path::Path;

use super::test_architect::TestPlan;

/// Result of comparing test plans against actual implementation.
#[derive(Debug, Clone)]
pub struct CoverageReport {
    /// Total planned test functions across all children.
    pub total_planned: usize,
    /// Number of planned tests found in source.
    pub found: usize,
    /// Planned tests not found in source.
    pub missing: Vec<MissingTest>,
    /// Test functions in source that weren't in any plan.
    pub unplanned: Vec<String>,
    /// Per-child breakdown.
    pub per_child: Vec<ChildCoverage>,
}

/// A planned test that wasn't found in the merged source.
#[derive(Debug, Clone)]
pub struct MissingTest {
    pub child_label: String,
    pub test_name: String,
    pub description: String,
}

/// Coverage breakdown for a single child.
#[derive(Debug, Clone)]
pub struct ChildCoverage {
    pub child_label: String,
    pub planned: usize,
    pub found: usize,
    pub missing: usize,
}

impl CoverageReport {
    /// Coverage percentage (0.0 to 100.0).
    pub fn coverage_percent(&self) -> f64 {
        if self.total_planned == 0 {
            return 100.0;
        }
        (self.found as f64 / self.total_planned as f64) * 100.0
    }

    /// Format as a concise human-readable summary.
    pub fn summary(&self) -> String {
        let mut lines = vec![format!(
            "Test plan coverage: {}/{} planned tests implemented ({:.0}%)",
            self.found,
            self.total_planned,
            self.coverage_percent()
        )];

        if !self.missing.is_empty() {
            lines.push(format!("Missing: {}", self.missing.iter()
                .map(|m| m.test_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")));
        }

        if !self.unplanned.is_empty() {
            lines.push(format!(
                "Unplanned (bonus): {} additional test(s)",
                self.unplanned.len()
            ));
        }

        lines.join("\n")
    }
}

/// Check test coverage by comparing plans against actual test functions in source.
///
/// Scans source files for test function names and matches them against
/// the test plans. Uses fuzzy matching — a plan entry `test_read_secret`
/// matches any test function containing `read_secret` (case-insensitive).
pub fn check_test_coverage(
    repo_path: &Path,
    plans: &[TestPlan],
    scope_files: &[String],
) -> CoverageReport {
    // Collect all actual test function names from source files
    let actual_tests = find_test_functions(repo_path, scope_files);

    let mut total_planned = 0;
    let mut total_found = 0;
    let mut all_missing = Vec::new();
    let mut matched_actuals = std::collections::HashSet::new();
    let mut per_child = Vec::new();

    for plan in plans {
        let mut child_found = 0;
        let planned = plan.required_tests.len() + plan.edge_cases.len();
        total_planned += planned;

        // Match required tests
        for test_desc in &plan.required_tests {
            let needle = normalize_test_name(&test_desc.name);
            if let Some(actual) = actual_tests.iter().find(|a| fuzzy_match(&needle, a)) {
                child_found += 1;
                matched_actuals.insert(actual.clone());
            } else {
                all_missing.push(MissingTest {
                    child_label: plan.child_label.clone(),
                    test_name: test_desc.name.clone(),
                    description: test_desc.description.clone(),
                });
            }
        }

        // Match edge cases — they might map to test function names
        for ec in &plan.edge_cases {
            let needle = edge_case_to_test_name(ec);
            if let Some(actual) = actual_tests.iter().find(|a| fuzzy_match(&needle, a)) {
                child_found += 1;
                matched_actuals.insert(actual.clone());
            } else {
                all_missing.push(MissingTest {
                    child_label: plan.child_label.clone(),
                    test_name: edge_case_to_test_name(ec),
                    description: ec.clone(),
                });
            }
        }

        total_found += child_found;

        per_child.push(ChildCoverage {
            child_label: plan.child_label.clone(),
            planned,
            found: child_found,
            missing: planned - child_found,
        });
    }

    // Unplanned tests: actual tests not matched by any plan
    let unplanned = actual_tests
        .into_iter()
        .filter(|t| !matched_actuals.contains(t))
        .collect();

    CoverageReport {
        total_planned,
        found: total_found,
        missing: all_missing,
        unplanned,
        per_child,
    }
}

/// Find test function names in source files.
///
/// Supports Rust (#[test], #[tokio::test]), TypeScript (describe/it/test),
/// and Python (def test_*).
fn find_test_functions(repo_path: &Path, scope_files: &[String]) -> Vec<String> {
    let mut test_names = Vec::new();

    for file in scope_files {
        let full_path = repo_path.join(file);
        let content = match std::fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if file.ends_with(".rs") {
            // Rust: #[test] or #[tokio::test] followed by fn name
            let mut prev_is_test_attr = false;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed == "#[test]" || trimmed.starts_with("#[tokio::test") || trimmed.starts_with("#[rstest") {
                    prev_is_test_attr = true;
                    continue;
                }
                if prev_is_test_attr {
                    if let Some(name) = extract_rust_fn_name(trimmed) {
                        test_names.push(name);
                    }
                    prev_is_test_attr = false;
                }
            }
        } else if file.ends_with(".ts") || file.ends_with(".js") {
            // TypeScript/JavaScript: it("name", ...) or test("name", ...)
            for line in content.lines() {
                let trimmed = line.trim();
                if let Some(name) = extract_ts_test_name(trimmed) {
                    test_names.push(name);
                }
            }
        } else if file.ends_with(".py") {
            // Python: def test_name(...)
            for line in content.lines() {
                let trimmed = line.trim();
                if let Some(name) = trimmed.strip_prefix("def test_")
                    .and_then(|n| n.find('(').map(|p| &n[..p]))
                {
                    test_names.push(format!("test_{name}"));
                }
            }
        }
    }

    test_names
}

/// Extract a Rust function name from a line like `fn test_something() {`
fn extract_rust_fn_name(line: &str) -> Option<String> {
    let trimmed = line.trim().strip_prefix("pub ")
        .or_else(|| Some(line.trim()))?;
    let trimmed = trimmed.strip_prefix("async ")
        .unwrap_or(trimmed);
    let rest = trimmed.strip_prefix("fn ")?;
    let name_end = rest.find('(')?;
    Some(rest[..name_end].trim().to_string())
}

/// Extract a TypeScript test name from `it("...", ...)` or `test("...", ...)`
fn extract_ts_test_name(line: &str) -> Option<String> {
    for prefix in &["it(", "test("] {
        if let Some(rest) = line.strip_prefix(prefix) {
            let quote = rest.chars().next()?;
            if quote == '"' || quote == '\'' || quote == '`' {
                let end = rest[1..].find(quote)?;
                return Some(rest[1..1 + end].to_string());
            }
        }
    }
    None
}

/// Normalize a test name for fuzzy matching.
fn normalize_test_name(name: &str) -> String {
    name.to_lowercase()
        .replace(['-', ' '], "_")
        .trim_start_matches("test_")
        .to_string()
}

/// Convert an edge case one-liner to a plausible test function name.
/// "Empty path → error" → "empty_path"
fn edge_case_to_test_name(edge_case: &str) -> String {
    let before_arrow = edge_case.split('→').next().unwrap_or(edge_case);
    before_arrow
        .trim()
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric(), "_")
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

/// Fuzzy match: does a planned test name match an actual test function?
/// Matches if the normalized names share a significant substring.
fn fuzzy_match(planned: &str, actual: &str) -> bool {
    let actual_norm = normalize_test_name(actual);
    // Exact match
    if planned == actual_norm {
        return true;
    }
    // Planned is contained in actual or vice versa
    if actual_norm.contains(planned) || planned.contains(&actual_norm) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_architect::{TestDescription, TestPlan};

    #[test]
    fn coverage_report_all_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("lib.rs"), r#"
#[test]
fn test_read_secret() {
    assert!(true);
}

#[test]
fn test_write_secret() {
    assert!(true);
}
"#).unwrap();

        let plans = vec![TestPlan {
            child_label: "vault".into(),
            required_tests: vec![
                TestDescription { name: "test_read_secret".into(), description: "Read a secret".into() },
                TestDescription { name: "test_write_secret".into(), description: "Write a secret".into() },
            ],
            edge_cases: vec![],
            expected_test_count: 2,
        }];

        let report = check_test_coverage(dir.path(), &plans, &["lib.rs".into()]);
        assert_eq!(report.total_planned, 2);
        assert_eq!(report.found, 2);
        assert!(report.missing.is_empty());
        assert_eq!(report.coverage_percent(), 100.0);
    }

    #[test]
    fn coverage_report_missing_tests() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("lib.rs"), r#"
#[test]
fn test_read_secret() { assert!(true); }
"#).unwrap();

        let plans = vec![TestPlan {
            child_label: "vault".into(),
            required_tests: vec![
                TestDescription { name: "test_read_secret".into(), description: "exists".into() },
                TestDescription { name: "test_write_secret".into(), description: "missing".into() },
            ],
            edge_cases: vec!["Empty path → error".into()],
            expected_test_count: 3,
        }];

        let report = check_test_coverage(dir.path(), &plans, &["lib.rs".into()]);
        assert_eq!(report.total_planned, 3);
        assert_eq!(report.found, 1);
        assert_eq!(report.missing.len(), 2);
        assert!(report.summary().contains("1/3"));
    }

    #[test]
    fn coverage_report_unplanned_tests() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("lib.rs"), r#"
#[test]
fn test_read_secret() { assert!(true); }

#[test]
fn test_bonus_test() { assert!(true); }
"#).unwrap();

        let plans = vec![TestPlan {
            child_label: "vault".into(),
            required_tests: vec![
                TestDescription { name: "test_read_secret".into(), description: "Read".into() },
            ],
            edge_cases: vec![],
            expected_test_count: 1,
        }];

        let report = check_test_coverage(dir.path(), &plans, &["lib.rs".into()]);
        assert_eq!(report.found, 1);
        assert_eq!(report.unplanned.len(), 1);
        assert!(report.unplanned[0].contains("bonus_test"));
    }

    #[test]
    fn fuzzy_match_handles_prefix_stripping() {
        assert!(fuzzy_match("read_secret", "test_read_secret"));
        assert!(fuzzy_match("read_secret", "read_secret_happy_path"));
        assert!(!fuzzy_match("read_secret", "write_secret"));
    }

    #[test]
    fn edge_case_to_test_name_conversion() {
        assert_eq!(edge_case_to_test_name("Empty path → error"), "empty_path");
        assert_eq!(edge_case_to_test_name("Token with invalid signature → 401"), "token_with_invalid_signature");
    }

    #[test]
    fn extract_rust_fn_name_variants() {
        assert_eq!(extract_rust_fn_name("fn test_something() {"), Some("test_something".into()));
        assert_eq!(extract_rust_fn_name("async fn test_async() {"), Some("test_async".into()));
        assert_eq!(extract_rust_fn_name("pub fn test_pub() {"), Some("test_pub".into()));
        assert_eq!(extract_rust_fn_name("let x = 5;"), None);
    }

    #[test]
    fn extract_ts_test_name_variants() {
        assert_eq!(extract_ts_test_name(r#"it("should do something", () => {"#), Some("should do something".into()));
        assert_eq!(extract_ts_test_name(r#"test("my test", () => {"#), Some("my test".into()));
        assert_eq!(extract_ts_test_name("const x = 5;"), None);
    }

    #[test]
    fn coverage_empty_plans() {
        let report = check_test_coverage(Path::new("/nonexistent"), &[], &[]);
        assert_eq!(report.total_planned, 0);
        assert_eq!(report.coverage_percent(), 100.0);
    }
}
