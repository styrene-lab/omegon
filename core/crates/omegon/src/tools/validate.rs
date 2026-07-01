//! Post-mutation validation — run the appropriate checker after file edits.
//!
//! Discovers project configuration (Cargo.toml, tsconfig.json, etc.) and
//! runs the lightest validation command relevant to the edited file.
//! Results are returned through the dedicated `validate` tool surface.

use anyhow::Result;
use omegon_traits::{ContentBlock, ToolResult};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tokio::process::Command;

/// Maximum time to wait for a validation command to complete.
/// cargo check on a large project can take a while; 30s is generous
/// but prevents indefinite hangs from build locks or broken toolchains.
const VALIDATION_TIMEOUT_SECS: u64 = 30;

/// Cached project validators, keyed by the cwd they were discovered from.
/// Re-discovers if cwd changes (Phase 1 multi-project support).
static VALIDATORS: Mutex<Option<(PathBuf, HashMap<ValidatorKind, ValidatorConfig>)>> =
    Mutex::new(None);

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
enum ValidatorKind {
    Rust,
    TypeScript,
    Python,
}

impl ValidatorKind {
    fn spec(self) -> &'static dyn LanguageValidator {
        match self {
            Self::Rust => &RustValidator,
            Self::TypeScript => &TypeScriptValidator,
            Self::Python => &PythonValidator,
        }
    }

    fn label(self) -> &'static str {
        self.spec().label()
    }

    fn expected_config(self) -> &'static str {
        self.spec().expected_config()
    }
}

#[derive(Debug, Clone)]
struct ValidatorConfig {
    command: &'static str,
    args: Vec<&'static str>,
}

trait LanguageValidator: Sync {
    fn kind(&self) -> ValidatorKind;
    fn label(&self) -> &'static str;
    fn expected_config(&self) -> &'static str;
    fn config_name(&self) -> &'static str;
    fn config(&self) -> ValidatorConfig;
    fn extract_error_summary(&self, stdout: &str, stderr: &str) -> String;
}

struct RustValidator;
struct TypeScriptValidator;
struct PythonValidator;

impl LanguageValidator for RustValidator {
    fn kind(&self) -> ValidatorKind {
        ValidatorKind::Rust
    }

    fn label(&self) -> &'static str {
        "Rust"
    }

    fn expected_config(&self) -> &'static str {
        "Cargo.toml"
    }

    fn config_name(&self) -> &'static str {
        "Cargo.toml"
    }

    fn config(&self) -> ValidatorConfig {
        ValidatorConfig {
            command: "cargo",
            args: vec!["check", "--message-format=short"],
        }
    }

    fn extract_error_summary(&self, stdout: &str, stderr: &str) -> String {
        // cargo check --message-format=short outputs "file:line:col: error[E0xxx]: msg"
        format!("{stdout}\n{stderr}")
            .lines()
            .filter(|l| l.contains("error") || l.contains("warning"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl LanguageValidator for TypeScriptValidator {
    fn kind(&self) -> ValidatorKind {
        ValidatorKind::TypeScript
    }

    fn label(&self) -> &'static str {
        "TypeScript"
    }

    fn expected_config(&self) -> &'static str {
        "tsconfig.json"
    }

    fn config_name(&self) -> &'static str {
        "tsconfig.json"
    }

    fn config(&self) -> ValidatorConfig {
        ValidatorConfig {
            command: "npx",
            args: vec!["tsc", "--noEmit", "--pretty"],
        }
    }

    fn extract_error_summary(&self, stdout: &str, stderr: &str) -> String {
        // tsc outputs "file(line,col): error TSxxxx: msg"
        format!("{stdout}\n{stderr}")
            .lines()
            .filter(|l| l.contains("error TS") || l.contains(": error"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl LanguageValidator for PythonValidator {
    fn kind(&self) -> ValidatorKind {
        ValidatorKind::Python
    }

    fn label(&self) -> &'static str {
        "Python"
    }

    fn expected_config(&self) -> &'static str {
        "pyproject.toml"
    }

    fn config_name(&self) -> &'static str {
        "pyproject.toml"
    }

    fn config(&self) -> ValidatorConfig {
        ValidatorConfig {
            command: "ruff",
            args: vec!["check", "--quiet"],
        }
    }

    fn extract_error_summary(&self, stdout: &str, stderr: &str) -> String {
        // ruff outputs "file:line:col: EXXX msg"
        format!("{stdout}\n{stderr}")
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with("Found") && !l.starts_with("All checks"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

const LANGUAGE_VALIDATORS: &[&dyn LanguageValidator] =
    &[&RustValidator, &TypeScriptValidator, &PythonValidator];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationLevel {
    Quick,
    Standard,
    Full,
}

impl ValidationLevel {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "quick" => Self::Quick,
            "full" => Self::Full,
            _ => Self::Standard,
        }
    }
}

/// Run validation for the supplied paths and return a structured tool result.
pub async fn execute(paths: &[PathBuf], level: ValidationLevel, cwd: &Path) -> Result<ToolResult> {
    if paths.is_empty() {
        anyhow::bail!("No paths provided for validation");
    }

    let mut unique_paths = Vec::new();
    for path in paths {
        if !unique_paths.contains(path) {
            unique_paths.push(path.clone());
        }
    }

    let mut validation_results = Vec::new();
    let mut validated_paths = Vec::new();
    let mut unsupported_paths = Vec::new();
    let mut missing_validator_paths = Vec::new();
    for path in &unique_paths {
        let Some(kind) = validator_for_file(path) else {
            unsupported_paths.push(format!(
                "{} (extension: {})",
                path.display(),
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .filter(|ext| !ext.is_empty())
                    .unwrap_or("none")
            ));
            continue;
        };

        let validators = discover_validators(cwd);
        let Some(config) = validators.get(&kind).cloned() else {
            missing_validator_paths.push(format!(
                "{} ({}, no {} found from {})",
                path.display(),
                kind.label(),
                kind.expected_config(),
                cwd.display()
            ));
            continue;
        };

        validation_results.push(validate_file(path, kind, config, cwd).await);
        validated_paths.push(path.display().to_string());
    }

    if validation_results.is_empty() {
        let mut message =
            "Validation skipped: no applicable validator was available for the supplied path set.\n\
Supported built-in source types: Rust, TypeScript, Python."
                .to_string();
        if !unsupported_paths.is_empty() {
            message.push_str("\nUnsupported paths:\n");
            for path in &unsupported_paths {
                message.push_str(&format!("  - {path}\n"));
            }
        }
        if !missing_validator_paths.is_empty() {
            message.push_str("Supported paths without a discovered project validator:\n");
            for path in &missing_validator_paths {
                message.push_str(&format!("  - {path}\n"));
            }
        }
        message.push_str("\nRecommended next step:\n");
        message.push_str(&validation_recommendation(
            &unique_paths,
            !unsupported_paths.is_empty(),
            !missing_validator_paths.is_empty(),
            cwd,
        ));
        return Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: message.trim_end().to_string(),
            }],
            details: serde_json::json!({
                "paths": Vec::<String>::new(),
                "unsupported_paths": unsupported_paths,
                "missing_validator_paths": missing_validator_paths,
                "level": match level {
                    ValidationLevel::Quick => "quick",
                    ValidationLevel::Standard => "standard",
                    ValidationLevel::Full => "full",
                },
                "validators_run": 0,
                "validation_skipped": true,
                "recommendation": "Do not retry validate for this same path set in this session unless a project validator is added; run a project-specific command or validator plugin instead.",
            }),
        });
    }

    let mut output = format!(
        "Validated {} path(s) with {} applicable validator run(s):\n{}",
        unique_paths.len(),
        validation_results.len(),
        validation_results.join("\n")
    );

    if level == ValidationLevel::Full
        && let Some(test_result) =
            run_affected_tests(cwd, &unique_paths.iter().collect::<Vec<_>>()).await
    {
        output.push_str("\n\nTests:\n");
        output.push_str(&test_result);
    }

    Ok(ToolResult {
        content: vec![ContentBlock::Text { text: output }],
        details: serde_json::json!({
            "paths": validated_paths,
            "unsupported_paths": unsupported_paths,
            "missing_validator_paths": missing_validator_paths,
            "level": match level {
                ValidationLevel::Quick => "quick",
                ValidationLevel::Standard => "standard",
                ValidationLevel::Full => "full",
            },
            "validators_run": validation_results.len(),
        }),
    })
}

/// Run validation for a single file path.
async fn validate_file(
    file_path: &Path,
    kind: ValidatorKind,
    config: ValidatorConfig,
    cwd: &Path,
) -> String {
    let child = Command::new("bash")
        .args([
            "-c",
            &format!("{} {}", config.command, config.args.join(" ")),
        ])
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .output();

    let result = tokio::time::timeout(Duration::from_secs(VALIDATION_TIMEOUT_SECS), child).await;

    match result {
        Ok(Ok(output)) => {
            let exit_code = output.status.code().unwrap_or(-1);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);

            if exit_code == 0 {
                format!(
                    "Validation (`{}`) for {}: ✓ passed",
                    config.command,
                    file_path.display()
                )
            } else {
                // Extract just the error lines, not the full output
                let errors = extract_error_summary(&stdout, &stderr, &kind);
                format!(
                    "Validation (`{}`) for {}: ✗ {} error(s)\n{}",
                    config.command,
                    file_path.display(),
                    count_errors(&errors),
                    truncate_validation(&errors, 500),
                )
            }
        }
        Ok(Err(e)) => {
            tracing::debug!("Validation command failed to execute: {e}");
            format!(
                "Validation (`{}`) for {}: failed to execute: {e}",
                config.command,
                file_path.display()
            )
        }
        Err(_) => {
            tracing::warn!(
                "Validation timed out after {}s for `{}`",
                VALIDATION_TIMEOUT_SECS,
                config.command
            );
            format!(
                "Validation (`{}`) for {}: ⏱ timed out after {}s",
                config.command,
                file_path.display(),
                VALIDATION_TIMEOUT_SECS
            )
        }
    }
}

/// Run tests affected by the changed files. Very simple heuristic:
/// look for co-located test files.
pub async fn run_affected_tests(cwd: &Path, files: &[&PathBuf]) -> Option<String> {
    // Find test files co-located with changed files
    let mut test_files = Vec::new();
    for file in files {
        let Some(stem) = file.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(ext) = file.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let Some(parent) = file.parent() else {
            continue;
        };

        let patterns = [
            format!("{stem}.test.{ext}"),
            format!("{stem}_test.{ext}"),
            format!("test_{stem}.{ext}"),
        ];

        for pattern in &patterns {
            let test_path = parent.join(pattern);
            if test_path.exists() {
                test_files.push(test_path);
            }
        }
    }

    if test_files.is_empty() {
        return None;
    }

    let cmd = if find_upward(cwd, "Cargo.toml").is_some() {
        Some("cargo test".to_string())
    } else if find_upward(cwd, "package.json").is_some() {
        Some("npm test".to_string())
    } else if find_upward(cwd, "pyproject.toml").is_some() {
        Some("pytest".to_string())
    } else {
        None
    }?;

    let child = Command::new("bash")
        .args(["-c", &cmd])
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .output();

    let result = tokio::time::timeout(Duration::from_secs(VALIDATION_TIMEOUT_SECS), child).await;
    match result {
        Ok(Ok(output)) if output.status.success() => {
            Some(format!("Affected tests (`{cmd}`): ✓ passed"))
        }
        Ok(Ok(output)) => Some(format!(
            "Affected tests (`{cmd}`): ✗ failed\n{}",
            truncate_validation(&String::from_utf8_lossy(&output.stderr), 500)
        )),
        Ok(Err(e)) => Some(format!("Affected tests (`{cmd}`): failed to execute: {e}")),
        Err(_) => Some(format!(
            "Affected tests (`{cmd}`): ⏱ timed out after {}s",
            VALIDATION_TIMEOUT_SECS
        )),
    }
}

/// Determine which validator applies to a file based on extension.
fn validator_for_file(path: &Path) -> Option<ValidatorKind> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => Some(ValidatorKind::Rust),
        Some("ts" | "tsx" | "js" | "jsx" | "mts" | "cts") => Some(ValidatorKind::TypeScript),
        Some("py") => Some(ValidatorKind::Python),
        _ => None,
    }
}

fn validation_recommendation(
    paths: &[PathBuf],
    has_unsupported_paths: bool,
    has_missing_project_validator: bool,
    cwd: &Path,
) -> String {
    let quoted_paths = paths
        .iter()
        .map(|path| shell_quote_path(path, cwd))
        .collect::<Vec<_>>()
        .join(" ");
    let mut lines = Vec::new();

    if has_unsupported_paths {
        lines.push(format!(
            "  - For this file type, use a project-specific check such as `git diff --check -- {quoted_paths}` or the repo's docs/config validator if one exists."
        ));
        for recommendation in discover_armory_validator_recommendations(paths, cwd) {
            lines.push(format!("  - {recommendation}"));
        }
        if paths.iter().any(|path| is_markdown_path(path)) {
            lines.push(
                "  - For Markdown/docs, prefer the repo's documentation build or linter when present (`just docs`, `mdbook test`, `markdownlint`, etc.).".to_string(),
            );
        }
        lines.push(
            "  - If this file type should be first-class, add a lightweight Omegon Armory validator plugin so agents can call a named validator instead of guessing shell commands.".to_string(),
        );
    }

    if has_missing_project_validator {
        lines.push(
            "  - For supported source files, add the expected project config or run the repo-specific validation command directly once and report it.".to_string(),
        );
    }

    lines.push(
        "  - Do not retry `validate` for the same unsupported path set this session unless the validator surface changes.".to_string(),
    );

    lines.join("\n")
}

fn discover_armory_validator_recommendations(paths: &[PathBuf], cwd: &Path) -> Vec<String> {
    let mut recommendations = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for manifest_path in armory_plugin_manifest_paths(cwd) {
        let Ok(content) = std::fs::read_to_string(&manifest_path) else {
            continue;
        };
        let Ok(manifest) = crate::plugins::armory::ArmoryManifest::parse(&content) else {
            continue;
        };
        for validator in &manifest.validators {
            if !paths.iter().any(|path| validator.matches_path(path)) {
                continue;
            }
            let key = format!("{}:{}", manifest.plugin.id, validator.tool);
            if !seen.insert(key) {
                continue;
            }
            let extensions = if validator.extensions.is_empty() {
                "declared files".to_string()
            } else {
                validator
                    .extensions
                    .iter()
                    .map(|ext| format!(".{}", ext.trim_start_matches('.')))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            recommendations.push(format!(
                "Installed Armory validator `{}` from `{}` handles {extensions}; call that tool with the rejected path set.",
                validator.tool, manifest.plugin.name
            ));
        }
    }

    recommendations
}

fn armory_plugin_manifest_paths(cwd: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(home) = crate::paths::omegon_home() {
        roots.push(home.join("plugins"));
    }
    roots.push(cwd.join(".omegon").join("plugins"));
    if let Ok(dir) = std::env::var("OMEGON_PLUGIN_DIR") {
        roots.push(PathBuf::from(dir));
    }

    let mut manifests = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path().join("plugin.toml");
            if path.exists() {
                manifests.push(path);
            }
        }
    }
    manifests
}

fn shell_quote_path(path: &Path, cwd: &Path) -> String {
    let display_path = path
        .strip_prefix(cwd)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();
    if display_path
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-'))
    {
        display_path
    } else {
        format!("'{}'", display_path.replace('\'', "'\\''"))
    }
}

fn is_markdown_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("md" | "mdx" | "markdown")
    )
}

/// Discover available validators by scanning for project config files.
/// Caches results per-cwd — re-discovers if cwd changes.
fn discover_validators(cwd: &Path) -> HashMap<ValidatorKind, ValidatorConfig> {
    let mut guard = VALIDATORS.lock().unwrap_or_else(|e| e.into_inner());

    // Return cached if cwd matches
    if let Some((ref cached_cwd, ref validators)) = *guard
        && cached_cwd == cwd
    {
        return validators.clone();
    }

    // Discover fresh
    let mut validators = HashMap::new();

    for validator in LANGUAGE_VALIDATORS {
        if find_upward(cwd, validator.config_name()).is_some() {
            validators.insert(validator.kind(), validator.config());
        }
    }

    *guard = Some((cwd.to_path_buf(), validators.clone()));
    validators
}

/// Walk up from `start` looking for a file named `name`.
fn find_upward(start: &Path, name: &str) -> Option<std::path::PathBuf> {
    let mut dir = start;
    loop {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
        dir = dir.parent()?;
    }
}

/// Extract error-relevant lines from validator output.
fn extract_error_summary(stdout: &str, stderr: &str, kind: &ValidatorKind) -> String {
    kind.spec().extract_error_summary(stdout, stderr)
}

/// Count approximate number of errors from summary text.
fn count_errors(summary: &str) -> usize {
    summary.lines().filter(|l| !l.is_empty()).count()
}

/// Truncate validation output to stay within a byte budget.
/// Safe for multi-byte UTF-8 — finds the last char boundary before the limit.
fn truncate_validation(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    // Find the last valid char boundary at or before max_bytes
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = &text[..end];
    if let Some(last_nl) = truncated.rfind('\n') {
        format!("{}\n... (truncated)", &truncated[..last_nl])
    } else {
        format!("{}... (truncated)", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validator_for_known_extensions() {
        assert_eq!(
            validator_for_file(Path::new("foo.rs")),
            Some(ValidatorKind::Rust)
        );
        assert_eq!(
            validator_for_file(Path::new("bar.ts")),
            Some(ValidatorKind::TypeScript)
        );
        assert_eq!(
            validator_for_file(Path::new("baz.py")),
            Some(ValidatorKind::Python)
        );
        assert!(validator_for_file(Path::new("readme.md")).is_none());
        assert!(validator_for_file(Path::new("config.json")).is_none());
    }

    #[test]
    fn truncation_at_line_boundary() {
        let text = "line one\nline two\nline three\nline four";
        let truncated = truncate_validation(text, 20);
        assert!(truncated.contains("truncated"));
        assert!(!truncated.contains("line three"));
    }

    #[test]
    fn truncation_safe_for_multibyte_utf8() {
        // "café" has a 2-byte é (0xC3 0xA9) — cutting at byte 4 would
        // split the multi-byte character. This must not panic.
        let text = "café\nbar\nbaz";
        let truncated = truncate_validation(text, 4);
        assert!(truncated.contains("truncated"));
        // Should have backed up to byte 3 ("caf") rather than panicking
        assert!(!truncated.contains('é'));
    }

    #[test]
    fn error_count() {
        assert_eq!(count_errors("error 1\nerror 2\n"), 2);
        assert_eq!(count_errors(""), 0);
        assert_eq!(count_errors("one\n\ntwo"), 2);
    }

    #[tokio::test]
    async fn validate_rejects_empty_path_set() {
        let dir = tempfile::tempdir().unwrap();
        let err = execute(&[], ValidationLevel::Standard, dir.path())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("No paths provided"));
    }

    #[tokio::test]
    async fn validate_reports_unsupported_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("README.md");
        std::fs::write(&path, "# docs").unwrap();

        let result = execute(
            std::slice::from_ref(&path),
            ValidationLevel::Standard,
            dir.path(),
        )
        .await
        .unwrap();
        let ContentBlock::Text { text: message } = &result.content[0] else {
            panic!("expected text result");
        };
        assert!(message.contains("Validation skipped"));
        assert!(message.contains("Unsupported paths"));
        assert!(message.contains("README.md"));
        assert!(message.contains("extension: md"));
        assert!(message.contains("Recommended next step"));
        assert!(message.contains("git diff --check"));
        assert!(message.contains("Armory validator plugin"));
        assert!(message.contains("Do not retry `validate`"));
        assert_eq!(result.details["validators_run"], 0);
        assert_eq!(result.details["validation_skipped"], true);
    }

    #[tokio::test]
    async fn validate_recommends_installed_armory_validator() {
        let dir = tempfile::tempdir().unwrap();
        let plugin_dir = dir.path().join(".omegon/plugins/docs-validator");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("plugin.toml"),
            r#"
            [plugin]
            type = "extension"
            id = "dev.example.docs-validator"
            name = "Docs Validator"
            version = "1.0.0"
            description = "Validate Markdown docs"

            [[tools]]
            name = "validate_docs"
            description = "Validate docs"
            runner = "bash"
            script = "tools/validate-docs.sh"

            [[validators]]
            name = "markdown"
            tool = "validate_docs"
            extensions = ["md"]
        "#,
        )
        .unwrap();
        let path = dir.path().join("README.md");
        std::fs::write(&path, "# docs").unwrap();

        let result = execute(
            std::slice::from_ref(&path),
            ValidationLevel::Standard,
            dir.path(),
        )
        .await
        .unwrap();
        let ContentBlock::Text { text: message } = &result.content[0] else {
            panic!("expected text result");
        };
        assert!(
            message.contains("Installed Armory validator `validate_docs` from `Docs Validator`"),
            "{message}"
        );
        assert!(message.contains("handles .md"), "{message}");
    }

    #[tokio::test]
    async fn validate_reports_supported_paths_without_project_validator() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.rs");
        std::fs::write(&path, "fn main() {}\n").unwrap();

        let result = execute(
            std::slice::from_ref(&path),
            ValidationLevel::Standard,
            dir.path(),
        )
        .await
        .unwrap();
        let ContentBlock::Text { text: message } = &result.content[0] else {
            panic!("expected text result");
        };
        assert!(message.contains("Validation skipped"));
        assert!(message.contains("Supported paths without a discovered project validator"));
        assert!(message.contains("main.rs"));
        assert!(message.contains("Cargo.toml"));
        assert!(message.contains("repo-specific validation command"));
        assert_eq!(result.details["validators_run"], 0);
        assert_eq!(result.details["validation_skipped"], true);
    }
}
