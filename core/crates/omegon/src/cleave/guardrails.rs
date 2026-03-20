//! Guardrail discovery — scan project files for typecheck/lint/clippy commands.
//!
//! Port of extensions/cleave/guardrails.ts::discoverGuardrails (auto-detect path only).
//! Skill frontmatter parsing is omitted — the Rust binary doesn't have skill paths.

use std::path::Path;

/// A discovered guardrail check.
#[derive(Debug, Clone)]
pub struct GuardrailCheck {
    pub name: String,
    pub cmd: String,
}

/// Discover guardrails from project configuration files.
///
/// Checks (in priority order):
/// 1. package.json scripts (typecheck, lint)
/// 2. Auto-detection (tsconfig.json → tsc, Cargo.toml → clippy, pyproject.toml → mypy)
pub fn discover_guardrails(cwd: &Path) -> Vec<GuardrailCheck> {
    let mut checks: Vec<GuardrailCheck> = Vec::new();

    fn add(checks: &mut Vec<GuardrailCheck>, name: &str, cmd: &str) {
        if checks.iter().any(|c| c.name == name) { return; }
        checks.push(GuardrailCheck { name: name.to_string(), cmd: cmd.to_string() });
    }

    // 1. package.json scripts
    let pkg_path = cwd.join("package.json");
    if pkg_path.exists()
        && let Ok(content) = std::fs::read_to_string(&pkg_path)
            && let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content)
                && let Some(scripts) = pkg.get("scripts").and_then(|s| s.as_object()) {
                    if let Some(tc) = scripts.get("typecheck").and_then(|v| v.as_str()) {
                        add(&mut checks, "typecheck", tc);
                    }
                    if let Some(lint) = scripts.get("lint").and_then(|v| v.as_str()) {
                        add(&mut checks, "lint", lint);
                    }
                }

    // 2. Auto-detection
    if !checks.iter().any(|c| c.name == "typecheck") && cwd.join("tsconfig.json").exists() {
        add(&mut checks, "typecheck", "npx tsc --noEmit");
    }
    if cwd.join("pyproject.toml").exists() {
        add(&mut checks, "typecheck-python", "mypy .");
    }
    if cwd.join("Cargo.toml").exists() {
        add(&mut checks, "clippy", "cargo clippy -- -D warnings");
    }

    checks
}

/// Format guardrails as a markdown section for task files.
pub fn format_guardrail_section(checks: &[GuardrailCheck]) -> String {
    if checks.is_empty() { return String::new(); }

    let mut lines = vec![
        String::new(),
        "## Project Guardrails".to_string(),
        String::new(),
        "Before reporting success, run these deterministic checks and fix any failures:".to_string(),
        String::new(),
    ];

    for (i, check) in checks.iter().enumerate() {
        lines.push(format!("{}. **{}**: `{}`", i + 1, check.name, check.cmd));
    }

    lines.push(String::new());
    lines.push("Include command output in the Verification section. If any check fails, fix the errors before completing your task.".to_string());
    lines.push(String::new());

    lines.join("\n")
}

/// Per-check timeout (seconds). Matches TS guardrails.ts defaults.
const CHECK_TIMEOUT_SECS: u64 = 60;

/// Run guardrails and return a formatted report.
/// Used for post-merge verification.
pub fn run_guardrails(cwd: &Path, checks: &[GuardrailCheck]) -> String {
    let mut lines = Vec::new();
    let mut all_passed = true;

    for check in checks {
        let child = std::process::Command::new("bash")
            .args(["-c", &check.cmd])
            .current_dir(cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();

        let output = match child {
            Ok(c) => {
                // Wait with timeout
                let start = std::time::Instant::now();
                let mut child = c;
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => break child.wait_with_output(),
                        Ok(None) => {
                            if start.elapsed().as_secs() > CHECK_TIMEOUT_SECS {
                                let _ = child.kill();
                                break Err(std::io::Error::new(
                                    std::io::ErrorKind::TimedOut,
                                    format!("timed out after {CHECK_TIMEOUT_SECS}s"),
                                ));
                            }
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                        Err(e) => break Err(e),
                    }
                }
            }
            Err(e) => Err(e),
        };

        match output {
            Ok(out) if out.status.success() => {
                lines.push(format!("✓ **{}**: passed", check.name));
            }
            Ok(out) => {
                all_passed = false;
                let stderr = String::from_utf8_lossy(&out.stderr);
                let stdout = String::from_utf8_lossy(&out.stdout);
                let detail = if !stderr.is_empty() { stderr } else { stdout };
                let truncated = if detail.lines().count() > 20 {
                    format!("{}\n... (truncated)", detail.lines().take(20).collect::<Vec<_>>().join("\n"))
                } else {
                    detail.to_string()
                };
                lines.push(format!("✗ **{}**: failed (exit {})\n```\n{}\n```", check.name, out.status.code().unwrap_or(-1), truncated.trim()));
            }
            Err(e) => {
                lines.push(format!("⚠ **{}**: could not run ({})", check.name, e));
            }
        }
    }

    if all_passed {
        format!("✅ All deterministic checks passed\n{}", lines.join("\n"))
    } else {
        format!("❌ Some checks failed\n{}", lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_from_tsconfig() {
        let dir = std::env::temp_dir().join("omegon-test-guardrails-ts");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("tsconfig.json"), "{}").unwrap();

        let checks = discover_guardrails(&dir);
        assert!(checks.iter().any(|c| c.name == "typecheck" && c.cmd.contains("tsc")),
            "should detect tsc from tsconfig.json: {checks:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_from_cargo() {
        let dir = std::env::temp_dir().join("omegon-test-guardrails-cargo");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();

        let checks = discover_guardrails(&dir);
        assert!(checks.iter().any(|c| c.name == "clippy"),
            "should detect clippy from Cargo.toml: {checks:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_json_scripts_take_priority() {
        let dir = std::env::temp_dir().join("omegon-test-guardrails-pkg");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("tsconfig.json"), "{}").unwrap();
        std::fs::write(dir.join("package.json"), r#"{"scripts":{"typecheck":"npm run tc"}}"#).unwrap();

        let checks = discover_guardrails(&dir);
        let tc = checks.iter().find(|c| c.name == "typecheck").unwrap();
        assert_eq!(tc.cmd, "npm run tc", "package.json script should win over auto-detect");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn format_section_empty() {
        assert!(format_guardrail_section(&[]).is_empty());
    }

    #[test]
    fn format_section_with_checks() {
        let section = format_guardrail_section(&[
            GuardrailCheck { name: "typecheck".into(), cmd: "tsc".into() },
        ]);
        assert!(section.contains("## Project Guardrails"));
        assert!(section.contains("**typecheck**: `tsc`"));
    }
}
