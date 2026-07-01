//! Post-mutation validation — run the appropriate checker after file edits.
//!
//! Discovers project configuration (Cargo.toml, tsconfig.json, etc.) and
//! runs the lightest validation command relevant to the edited file.
//! Results are returned through the dedicated `validate` tool surface.

use anyhow::Result;
use omegon_traits::{ContentBlock, ToolResult};
use serde::Deserialize;
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

    fn id(self) -> &'static str {
        self.spec().id()
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
    fn id(&self) -> &'static str;
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

    fn id(&self) -> &'static str {
        "language.rust"
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

    fn id(&self) -> &'static str {
        "language.typescript"
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

    fn id(&self) -> &'static str {
        "language.python"
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
enum EmbeddedValidatorKind {
    Json,
    Toml,
    Yaml,
    Markdown,
}

impl EmbeddedValidatorKind {
    fn id(self) -> &'static str {
        match self {
            Self::Json => "core.json",
            Self::Toml => "core.toml",
            Self::Yaml => "core.yaml",
            Self::Markdown => "core.markdown-basic",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Toml => "TOML",
            Self::Yaml => "YAML",
            Self::Markdown => "Markdown hygiene",
        }
    }

    fn include(self) -> Vec<&'static str> {
        match self {
            Self::Json => vec!["**/*.json"],
            Self::Toml => vec!["**/*.toml"],
            Self::Yaml => vec!["**/*.yaml", "**/*.yml"],
            Self::Markdown => vec!["**/*.md", "**/*.mdx", "**/*.markdown"],
        }
    }

    fn runner_summary(self) -> &'static str {
        match self {
            Self::Json => "embedded serde_json parser",
            Self::Toml => "embedded toml parser",
            Self::Yaml => "embedded serde_yaml parser",
            Self::Markdown => "embedded UTF-8/trailing-whitespace hygiene",
        }
    }
}

const EMBEDDED_VALIDATORS: &[EmbeddedValidatorKind] = &[
    EmbeddedValidatorKind::Json,
    EmbeddedValidatorKind::Toml,
    EmbeddedValidatorKind::Yaml,
    EmbeddedValidatorKind::Markdown,
];

#[derive(Debug, Clone)]
struct BuiltinValidatorInventory {
    id: String,
    label: String,
    source: &'static str,
    enabled: bool,
    mode: &'static str,
    include: Vec<&'static str>,
    levels: Vec<&'static str>,
    runner_summary: String,
    policy_summary: &'static str,
}

impl BuiltinValidatorInventory {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "label": self.label,
            "source": self.source,
            "enabled": self.enabled,
            "mode": self.mode,
            "include": self.include,
            "levels": self.levels,
            "runner_summary": self.runner_summary,
            "policy_summary": self.policy_summary,
        })
    }
}

fn builtin_validator_inventory(cwd: &Path) -> Vec<BuiltinValidatorInventory> {
    let discovered = discover_validators(cwd);
    let mut inventory = EMBEDDED_VALIDATORS
        .iter()
        .map(|validator| BuiltinValidatorInventory {
            id: validator.id().to_string(),
            label: validator.label().to_string(),
            source: "builtin",
            enabled: true,
            mode: "supplement",
            include: validator.include(),
            levels: vec!["quick", "standard", "full"],
            runner_summary: validator.runner_summary().to_string(),
            policy_summary: "embedded, read-only, no-network, no-mutation",
        })
        .collect::<Vec<_>>();

    inventory.extend(LANGUAGE_VALIDATORS.iter().map(|validator| {
        let config = validator.config();
        BuiltinValidatorInventory {
            id: validator.id().to_string(),
            label: validator.label().to_string(),
            source: "builtin-toolchain",
            enabled: discovered.contains_key(&validator.kind()),
            mode: "supplement",
            include: match validator.kind() {
                ValidatorKind::Rust => vec!["**/*.rs"],
                ValidatorKind::TypeScript => vec![
                    "**/*.ts", "**/*.tsx", "**/*.js", "**/*.jsx", "**/*.mts", "**/*.cts",
                ],
                ValidatorKind::Python => vec!["**/*.py"],
            },
            levels: vec!["quick", "standard", "full"],
            runner_summary: format_command(config.command, &config.args),
            policy_summary: "external toolchain, read-only, no-network, no-mutation",
        }
    }));
    inventory
}

fn builtin_validator_inventory_json(cwd: &Path) -> Vec<serde_json::Value> {
    builtin_validator_inventory(cwd)
        .iter()
        .map(BuiltinValidatorInventory::to_json)
        .collect()
}

#[derive(Debug, Clone, Deserialize)]
struct OperatorValidatorConfig {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    validators: Vec<OperatorValidator>,
}

#[derive(Debug, Clone, Deserialize)]
struct OperatorValidator {
    id: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    levels: Vec<String>,
    #[serde(default = "default_operator_validator_mode")]
    mode: OperatorValidatorMode,
    #[serde(default)]
    replaces: Vec<String>,
    #[serde(default)]
    priority: i32,
    runner: OperatorValidatorRunner,
    #[serde(default)]
    policy: OperatorValidatorPolicy,
    #[serde(skip)]
    source: PathBuf,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum OperatorValidatorMode {
    Supplement,
    Replace,
}

fn default_operator_validator_mode() -> OperatorValidatorMode {
    OperatorValidatorMode::Supplement
}

impl Default for OperatorValidatorMode {
    fn default() -> Self {
        default_operator_validator_mode()
    }
}

#[derive(Debug, Clone, Deserialize)]
struct OperatorValidatorRunner {
    kind: OperatorValidatorRunnerKind,
    program: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    path_arg_mode: OperatorPathArgMode,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum OperatorValidatorRunnerKind {
    Process,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum OperatorPathArgMode {
    Append,
    None,
}

impl Default for OperatorPathArgMode {
    fn default() -> Self {
        Self::Append
    }
}

#[derive(Debug, Clone, Deserialize)]
struct OperatorValidatorPolicy {
    #[serde(default = "default_true")]
    read_only: bool,
    #[serde(default)]
    network: bool,
    #[serde(default)]
    mutates: bool,
    #[serde(default = "default_operator_timeout_secs")]
    timeout_secs: u64,
}

impl Default for OperatorValidatorPolicy {
    fn default() -> Self {
        Self {
            read_only: true,
            network: false,
            mutates: false,
            timeout_secs: default_operator_timeout_secs(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_operator_timeout_secs() -> u64 {
    VALIDATION_TIMEOUT_SECS
}

impl OperatorValidator {
    fn matches_level(&self, level: ValidationLevel) -> bool {
        self.levels.is_empty()
            || self
                .levels
                .iter()
                .any(|candidate| ValidationLevel::parse(candidate) == level)
    }

    fn matches_path(&self, path: &Path) -> bool {
        let included = self.include.is_empty()
            || self
                .include
                .iter()
                .any(|pattern| operator_validator_glob_matches(pattern, path));
        let excluded = self
            .exclude
            .iter()
            .any(|pattern| operator_validator_glob_matches(pattern, path));
        included && !excluded
    }

    fn provenance(&self) -> String {
        format!("{} ({})", self.id, self.source.display())
    }

    fn config_errors(&self, index: usize) -> Vec<String> {
        let mut errors = Vec::new();
        let label = if self.id.trim().is_empty() {
            format!("validators[{index}]")
        } else {
            format!("validator `{}`", self.id)
        };
        if self.id.trim().is_empty() {
            errors.push(format!("{label}: id must not be empty"));
        }
        if self.runner.program.trim().is_empty() {
            errors.push(format!("{label}: runner.program must not be empty"));
        }
        if self.policy.timeout_secs == 0 {
            errors.push(format!(
                "{label}: policy.timeout_secs must be greater than 0"
            ));
        }
        if self.mode == OperatorValidatorMode::Replace && self.replaces.is_empty() {
            errors.push(format!(
                "{label}: mode = \"replace\" requires at least one replaces entry"
            ));
        }
        errors
    }

    fn inventory_json(&self) -> serde_json::Value {
        let mode = match self.mode {
            OperatorValidatorMode::Supplement => "supplement",
            OperatorValidatorMode::Replace => "replace",
        };
        let policy = format!(
            "read_only={}, network={}, mutates={}, timeout_secs={}",
            self.policy.read_only,
            self.policy.network,
            self.policy.mutates,
            self.policy.timeout_secs
        );
        serde_json::json!({
            "id": self.id,
            "label": self.description.as_deref().unwrap_or(&self.id),
            "source": "project",
            "source_path": self.source,
            "enabled": true,
            "mode": mode,
            "replaces": self.replaces,
            "include": self.include,
            "exclude": self.exclude,
            "levels": self.levels,
            "runner_summary": format_dynamic_command(&self.runner.program, &self.runner.args),
            "policy_summary": policy,
        })
    }
}

fn operator_validator_inventory_json(validators: &[OperatorValidator]) -> Vec<serde_json::Value> {
    validators
        .iter()
        .map(OperatorValidator::inventory_json)
        .collect()
}

fn discover_operator_validators(
    cwd: &Path,
    paths: &[PathBuf],
    level: ValidationLevel,
) -> (Vec<OperatorValidator>, Vec<String>) {
    let config_path = cwd.join(".omegon").join("validators.toml");
    let Ok(raw) = std::fs::read_to_string(&config_path) else {
        return (Vec::new(), Vec::new());
    };
    let mut config = match toml::from_str::<OperatorValidatorConfig>(&raw) {
        Ok(config) => config,
        Err(error) => {
            return (
                Vec::new(),
                vec![format!(
                    "{}: invalid validator config: {error}",
                    config_path.display()
                )],
            );
        }
    };
    let mut errors = Vec::new();
    if config.version > 1 {
        errors.push(format!(
            "{}: unsupported validator config version {}; expected version 1",
            config_path.display(),
            config.version
        ));
    }
    let mut validators = Vec::new();
    for (index, mut validator) in config.validators.drain(..).enumerate() {
        validator.source = config_path.clone();
        errors.extend(validator.config_errors(index));
        if validator.matches_level(level) && paths.iter().any(|path| validator.matches_path(path)) {
            validators.push(validator);
        }
    }
    validators.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| left.id.cmp(&right.id))
    });
    (validators, errors)
}

fn builtin_replaced_by_operator(
    kind: EmbeddedValidatorKind,
    validators: &[OperatorValidator],
) -> bool {
    validators.iter().any(|validator| {
        validator.mode == OperatorValidatorMode::Replace
            && validator.replaces.iter().any(|id| id == kind.id())
    })
}

fn language_replaced_by_operator(kind: ValidatorKind, validators: &[OperatorValidator]) -> bool {
    validators.iter().any(|validator| {
        validator.mode == OperatorValidatorMode::Replace
            && validator.replaces.iter().any(|id| id == kind.id())
    })
}

fn operator_validator_inventory(validators: &[OperatorValidator]) -> Vec<String> {
    validators
        .iter()
        .map(|validator| {
            let mode = match validator.mode {
                OperatorValidatorMode::Supplement => "supplement",
                OperatorValidatorMode::Replace => "replace",
            };
            let replacement = if validator.replaces.is_empty() {
                String::new()
            } else {
                format!("; replaces {}", validator.replaces.join(", "))
            };
            let description = validator
                .description
                .as_deref()
                .filter(|description| !description.trim().is_empty())
                .map(|description| format!(" — {description}"))
                .unwrap_or_default();
            format!(
                "{} [{mode}{replacement}] via {} {}{}",
                validator.provenance(),
                validator.runner.program,
                validator.runner.args.join(" "),
                description
            )
        })
        .collect()
}

fn operator_validator_paths<'a>(
    validator: &OperatorValidator,
    paths: &'a [PathBuf],
) -> Vec<&'a PathBuf> {
    paths
        .iter()
        .filter(|path| validator.matches_path(path))
        .collect()
}

fn operator_validator_command(validator: &OperatorValidator, paths: &[&PathBuf]) -> Vec<String> {
    let mut args = validator.runner.args.clone();
    if validator.runner.path_arg_mode == OperatorPathArgMode::Append {
        args.extend(paths.iter().map(|path| path.display().to_string()));
    }
    args
}

fn format_dynamic_command(command: &str, args: &[String]) -> String {
    if args.is_empty() {
        command.to_string()
    } else {
        format!("{} {}", command, args.join(" "))
    }
}

async fn run_operator_validator(
    validator: &OperatorValidator,
    paths: &[PathBuf],
    cwd: &Path,
) -> String {
    let matched_paths = operator_validator_paths(validator, paths);
    if matched_paths.is_empty() {
        return format!(
            "Operator validator `{}`: skipped; no matched paths remained",
            validator.id
        );
    }
    if validator.runner.kind != OperatorValidatorRunnerKind::Process {
        return format!(
            "Operator validator `{}`: skipped; unsupported runner kind",
            validator.id
        );
    }
    if validator.policy.network || validator.policy.mutates || !validator.policy.read_only {
        return format!(
            "Operator validator `{}`: skipped; policy requires network/mutation/non-read-only access",
            validator.id
        );
    }

    let args = operator_validator_command(validator, &matched_paths);
    let rendered = format_dynamic_command(&validator.runner.program, &args);
    let child = Command::new(&validator.runner.program)
        .args(&args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .output();
    let timeout_secs = validator.policy.timeout_secs.max(1);
    let result = tokio::time::timeout(Duration::from_secs(timeout_secs), child).await;

    match result {
        Ok(Ok(output)) if output.status.success() => format!(
            "Operator validator `{}` (`{rendered}`): ✓ passed for {} path(s)",
            validator.id,
            matched_paths.len()
        ),
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let combined = format!(
                "{stdout}
{stderr}"
            );
            let summary = truncate_validation(combined.trim(), 700);
            format!(
                "Operator validator `{}` (`{rendered}`): ✗ failed for {} path(s)
{}",
                validator.id,
                matched_paths.len(),
                summary
            )
        }
        Ok(Err(e)) => format!(
            "Operator validator `{}` (`{rendered}`): failed to execute: {e}",
            validator.id
        ),
        Err(_) => format!(
            "Operator validator `{}` (`{rendered}`): ⏱ timed out after {timeout_secs}s",
            validator.id
        ),
    }
}

async fn run_operator_validators(
    validators: &[OperatorValidator],
    paths: &[PathBuf],
    cwd: &Path,
) -> Vec<String> {
    let mut results = Vec::new();
    for validator in validators {
        results.push(run_operator_validator(validator, paths, cwd).await);
    }
    results
}

fn operator_validator_glob_matches(pattern: &str, path: &Path) -> bool {
    let normalized_pattern = pattern.replace('\\', "/");
    let normalized_path = path.to_string_lossy().replace('\\', "/");
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");

    if let Some(suffix) = normalized_pattern.strip_prefix("**/*") {
        return normalized_path.ends_with(suffix);
    }
    if let Some(suffix) = normalized_pattern.strip_prefix("**/") {
        return normalized_path.ends_with(suffix) || file_name == suffix;
    }
    if normalized_pattern.contains('*') {
        let parts = normalized_pattern.split('*').collect::<Vec<_>>();
        let mut remainder = normalized_path.as_str();
        for (idx, part) in parts.iter().enumerate() {
            if part.is_empty() {
                continue;
            }
            if idx == 0 && !remainder.starts_with(part) {
                return false;
            }
            let Some(found) = remainder.find(part) else {
                return false;
            };
            remainder = &remainder[found + part.len()..];
        }
        return normalized_pattern.ends_with('*')
            || parts
                .last()
                .is_some_and(|part| remainder.is_empty() || normalized_path.ends_with(part));
    }
    normalized_path.ends_with(&normalized_pattern) || file_name == normalized_pattern
}

fn embedded_validator_for_file(path: &Path) -> Option<EmbeddedValidatorKind> {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
    {
        Some(ext) if ext == "json" => Some(EmbeddedValidatorKind::Json),
        Some(ext) if ext == "toml" => Some(EmbeddedValidatorKind::Toml),
        Some(ext) if ext == "yaml" || ext == "yml" => Some(EmbeddedValidatorKind::Yaml),
        Some(ext) if ext == "md" || ext == "mdx" || ext == "markdown" => {
            Some(EmbeddedValidatorKind::Markdown)
        }
        _ => None,
    }
}

async fn validate_embedded_file(file_path: &Path, kind: EmbeddedValidatorKind) -> String {
    let result = match std::fs::read_to_string(file_path) {
        Ok(content) => validate_embedded_content(file_path, kind, &content),
        Err(error) => Err(format!("failed to read file: {error}")),
    };
    match result {
        Ok(()) => format!(
            "Embedded validation (`{}`) for {}: ✓ passed",
            kind.runner_summary(),
            file_path.display()
        ),
        Err(error) => format!(
            "Embedded validation (`{}`) for {}: ✗ 1 error(s)\n{}",
            kind.runner_summary(),
            file_path.display(),
            truncate_validation(&error, 500)
        ),
    }
}

fn validate_embedded_content(
    path: &Path,
    kind: EmbeddedValidatorKind,
    content: &str,
) -> std::result::Result<(), String> {
    match kind {
        EmbeddedValidatorKind::Json => serde_json::from_str::<serde_json::Value>(content)
            .map(|_| ())
            .map_err(|error| format!("{}: invalid JSON: {error}", path.display())),
        EmbeddedValidatorKind::Toml => toml::from_str::<toml::Value>(content)
            .map(|_| ())
            .map_err(|error| format!("{}: invalid TOML: {error}", path.display())),
        EmbeddedValidatorKind::Yaml => serde_yaml::from_str::<serde_yaml::Value>(content)
            .map(|_| ())
            .map_err(|error| format!("{}: invalid YAML: {error}", path.display())),
        EmbeddedValidatorKind::Markdown => validate_markdown_hygiene(path, content),
    }
}

fn validate_markdown_hygiene(path: &Path, content: &str) -> std::result::Result<(), String> {
    if content.contains('\0') {
        return Err(format!("{}: contains NUL byte", path.display()));
    }
    for (idx, line) in content.lines().enumerate() {
        if line.ends_with(' ') || line.ends_with('\t') {
            return Err(format!(
                "{}:{}: trailing whitespace",
                path.display(),
                idx + 1
            ));
        }
    }
    Ok(())
}

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

    let (operator_validators, operator_config_errors) =
        discover_operator_validators(cwd, &unique_paths, level);
    if !operator_config_errors.is_empty() {
        let mut message = "Validation configuration error(s):
"
        .to_string();
        for error in &operator_config_errors {
            message.push_str(&format!(
                "  - {error}
"
            ));
        }
        return Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: message.trim_end().to_string(),
            }],
            details: serde_json::json!({
                "paths": Vec::<String>::new(),
                "level": match level {
                    ValidationLevel::Quick => "quick",
                    ValidationLevel::Standard => "standard",
                    ValidationLevel::Full => "full",
                },
                "validators_run": 0,
                "builtin_validators_run": 0,
                "operator_validators_run": 0,
                "operator_config_errors": operator_config_errors,
                "validation_skipped": true,
            }),
        });
    }

    let mut validation_results = Vec::new();
    let mut validated_paths = Vec::new();
    let mut unsupported_paths = Vec::new();
    let mut missing_validator_paths = Vec::new();
    let mut language_plan: HashMap<ValidatorKind, Vec<PathBuf>> = HashMap::new();
    for path in &unique_paths {
        if let Some(kind) = embedded_validator_for_file(path) {
            if !builtin_replaced_by_operator(kind, &operator_validators) {
                validation_results.push(validate_embedded_file(path, kind).await);
                validated_paths.push(path.display().to_string());
            }
            continue;
        }

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

        if language_replaced_by_operator(kind, &operator_validators) {
            continue;
        }

        language_plan.entry(kind).or_default().push(path.clone());
    }

    let validators = discover_validators(cwd);
    for kind in [
        ValidatorKind::Rust,
        ValidatorKind::TypeScript,
        ValidatorKind::Python,
    ] {
        let Some(paths) = language_plan.get(&kind) else {
            continue;
        };
        let Some(config) = validators.get(&kind).cloned() else {
            for path in paths {
                missing_validator_paths.push(format!(
                    "{} ({}, no {} found from {})",
                    path.display(),
                    kind.label(),
                    kind.expected_config(),
                    cwd.display()
                ));
            }
            continue;
        };

        validation_results.push(validate_language_paths(kind, paths, config, cwd).await);
        validated_paths.extend(paths.iter().map(|path| path.display().to_string()));
    }

    let builtin_inventory = builtin_validator_inventory_json(cwd);
    let operator_inventory = operator_validator_inventory_json(&operator_validators);
    let operator_validation_results =
        run_operator_validators(&operator_validators, &unique_paths, cwd).await;
    validation_results.extend(operator_validation_results.iter().cloned());

    if validation_results.is_empty() {
        let mut message =
            "Validation skipped: no applicable validator was available for the supplied path set.\n\
Supported built-in types: JSON, TOML, YAML, Markdown, Rust, TypeScript, Python."
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
        if !operator_validators.is_empty() {
            message.push_str("\nMatched operator validator override(s):\n");
            for validator in operator_validator_inventory(&operator_validators) {
                message.push_str(&format!("  - {validator}\n"));
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
                "builtin_validators_run": 0,
                "operator_validators_run": operator_validation_results.len(),
                "operator_validators": operator_validator_inventory(&operator_validators),
            "validator_inventory": {
                "builtins": builtin_inventory,
                "operators": operator_inventory,
            },
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

    if !operator_validators.is_empty() {
        output.push_str("\n\nMatched operator validator override(s):\n");
        for validator in operator_validator_inventory(&operator_validators) {
            output.push_str(&format!("  - {validator}\n"));
        }
    }

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
            "builtin_validators_run": validation_results.len().saturating_sub(operator_validation_results.len()),
            "operator_validators_run": operator_validation_results.len(),
            "operator_validators": operator_validator_inventory(&operator_validators),
            "validator_inventory": {
                "builtins": builtin_inventory,
                "operators": operator_inventory,
            },
        }),
    })
}

/// Run validation once for all changed paths covered by one language toolchain.
async fn validate_language_paths(
    kind: ValidatorKind,
    paths: &[PathBuf],
    config: ValidatorConfig,
    cwd: &Path,
) -> String {
    let child = Command::new(config.command)
        .args(&config.args)
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
                    "Validation (`{}`) for {} {} path(s): ✓ passed",
                    format_command(config.command, &config.args),
                    paths.len(),
                    kind.label()
                )
            } else {
                // Extract just the error lines, not the full output
                let errors = extract_error_summary(&stdout, &stderr, &kind);
                format!(
                    "Validation (`{}`) for {} {} path(s): ✗ {} error(s)\n{}",
                    format_command(config.command, &config.args),
                    paths.len(),
                    kind.label(),
                    count_errors(&errors),
                    truncate_validation(&errors, 500),
                )
            }
        }
        Ok(Err(e)) => {
            tracing::debug!("Validation command failed to execute: {e}");
            format!(
                "Validation (`{}`) for {} {} path(s): failed to execute: {e}",
                format_command(config.command, &config.args),
                paths.len(),
                kind.label()
            )
        }
        Err(_) => {
            tracing::warn!(
                "Validation timed out after {}s for `{}`",
                VALIDATION_TIMEOUT_SECS,
                format_command(config.command, &config.args)
            );
            format!(
                "Validation (`{}`) for {} {} path(s): ⏱ timed out after {}s",
                format_command(config.command, &config.args),
                paths.len(),
                kind.label(),
                VALIDATION_TIMEOUT_SECS
            )
        }
    }
}

fn format_command(command: &str, args: &[&str]) -> String {
    if args.is_empty() {
        command.to_string()
    } else {
        format!("{} {}", command, args.join(" "))
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

    let test_command = if find_upward(cwd, "Cargo.toml").is_some() {
        Some(("cargo", vec!["test"]))
    } else if find_upward(cwd, "package.json").is_some() {
        Some(("npm", vec!["test"]))
    } else if find_upward(cwd, "pyproject.toml").is_some() {
        Some(("pytest", vec![]))
    } else {
        None
    }?;
    let cmd = format_command(test_command.0, &test_command.1);

    let child = Command::new(test_command.0)
        .args(&test_command.1)
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
        let path = dir.path().join("notes.rst");
        std::fs::write(&path, "docs").unwrap();

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
        assert!(message.contains("notes.rst"));
        assert!(message.contains("extension: rst"));
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
            extensions = ["rst"]
        "#,
        )
        .unwrap();
        let path = dir.path().join("README.rst");
        std::fs::write(&path, "docs").unwrap();

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
        assert!(message.contains("handles .rst"), "{message}");
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
    #[tokio::test]
    async fn validate_reports_matching_operator_override() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".omegon")).unwrap();
        std::fs::write(
            dir.path().join(".omegon/validators.toml"),
            r#"
            version = 1

            [[validators]]
            id = "project.docs"
            description = "Project docs policy"
            include = ["**/*.md"]
            levels = ["standard"]
            mode = "replace"
            replaces = ["core.markdown-basic"]
            priority = 50

            [validators.runner]
            kind = "process"
            program = "markdownlint"
            args = ["--config", ".markdownlint.json"]
            path_arg_mode = "append"
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
            message.contains("Matched operator validator override(s)"),
            "{message}"
        );
        assert!(message.contains("project.docs"), "{message}");
        assert!(
            message.contains("replace; replaces core.markdown-basic"),
            "{message}"
        );
        assert!(
            message.contains("markdownlint --config .markdownlint.json"),
            "{message}"
        );
        assert_eq!(
            result.details["operator_validators"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    #[tokio::test]
    async fn validate_runs_operator_override_and_surfaces_result() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".omegon")).unwrap();
        std::fs::write(
            dir.path().join(".omegon/validators.toml"),
            r#"
            version = 1

            [[validators]]
            id = "project.docs"
            include = ["**/*.md"]
            levels = ["standard"]

            [validators.runner]
            kind = "process"
            program = "/bin/echo"
            args = ["docs-ok"]
            path_arg_mode = "append"

            [validators.policy]
            read_only = true
            network = false
            mutates = false
            timeout_secs = 5
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
            message.contains("Validated 1 path(s) with 2 applicable validator run(s)"),
            "{message}"
        );
        assert!(
            message.contains("Operator validator `project.docs`"),
            "{message}"
        );
        assert!(message.contains("/bin/echo docs-ok"), "{message}");
        assert!(message.contains("✓ passed"), "{message}");
        assert_eq!(result.details["validators_run"], 2);
        assert_eq!(result.details["builtin_validators_run"], 1);
        assert_eq!(result.details["operator_validators_run"], 1);
    }

    #[tokio::test]
    async fn operator_replace_suppresses_named_embedded_builtin() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".omegon")).unwrap();
        std::fs::write(
            dir.path().join(".omegon/validators.toml"),
            r#"
            version = 1

            [[validators]]
            id = "project.docs"
            include = ["**/*.md"]
            levels = ["standard"]
            mode = "replace"
            replaces = ["core.markdown-basic"]

            [validators.runner]
            kind = "process"
            program = "/bin/echo"
            args = ["docs-ok"]
            path_arg_mode = "append"
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
            message.contains("Validated 1 path(s) with 1 applicable validator run(s)"),
            "{message}"
        );
        assert!(
            message.contains("Operator validator `project.docs`"),
            "{message}"
        );
        assert!(
            !message.contains("embedded UTF-8/trailing-whitespace hygiene"),
            "{message}"
        );
        assert_eq!(result.details["validators_run"], 1);
        assert_eq!(result.details["builtin_validators_run"], 0);
        assert_eq!(result.details["operator_validators_run"], 1);
    }

    #[tokio::test]
    async fn embedded_json_validator_reports_parse_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model-registry.json");
        std::fs::write(&path, "{ invalid json").unwrap();

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
            message.contains("Embedded validation (`embedded serde_json parser`)"),
            "{message}"
        );
        assert!(message.contains("invalid JSON"), "{message}");
        assert_eq!(result.details["validators_run"], 1);
        assert_eq!(result.details["builtin_validators_run"], 1);
    }

    #[tokio::test]
    async fn embedded_toml_and_yaml_validators_pass_valid_files() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join("config.toml");
        let yaml_path = dir.path().join("workflow.yaml");
        std::fs::write(&toml_path, "[package]\nname = \"demo\"\n").unwrap();
        std::fs::write(&yaml_path, "name: demo\nsteps:\n  - run: test\n").unwrap();

        let result = execute(
            &[toml_path.clone(), yaml_path.clone()],
            ValidationLevel::Standard,
            dir.path(),
        )
        .await
        .unwrap();
        let ContentBlock::Text { text: message } = &result.content[0] else {
            panic!("expected text result");
        };
        assert!(message.contains("embedded toml parser"), "{message}");
        assert!(message.contains("embedded serde_yaml parser"), "{message}");
        assert!(message.contains("✓ passed"), "{message}");
        assert_eq!(result.details["validators_run"], 2);
        assert_eq!(result.details["builtin_validators_run"], 2);
    }

    #[tokio::test]
    async fn embedded_markdown_validator_reports_trailing_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("README.md");
        std::fs::write(&path, "# docs  \n").unwrap();

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
            message.contains("embedded UTF-8/trailing-whitespace hygiene"),
            "{message}"
        );
        assert!(message.contains("trailing whitespace"), "{message}");
        assert_eq!(result.details["validators_run"], 1);
        assert_eq!(result.details["builtin_validators_run"], 1);
    }

    #[tokio::test]
    async fn malformed_operator_validator_config_blocks_validation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".omegon")).unwrap();
        std::fs::write(
            dir.path().join(".omegon/validators.toml"),
            "[[validators]\n",
        )
        .unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{}").unwrap();

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
            message.contains("Validation configuration error(s):"),
            "{message}"
        );
        assert!(message.contains("invalid validator config"), "{message}");
        assert_eq!(result.details["validators_run"], 0);
        assert_eq!(result.details["validation_skipped"], true);
        assert_eq!(
            result.details["operator_config_errors"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn invalid_operator_validator_fields_block_validation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".omegon")).unwrap();
        std::fs::write(
            dir.path().join(".omegon/validators.toml"),
            r#"
            version = 1

            [[validators]]
            id = ""
            include = ["**/*.json"]
            mode = "replace"

            [validators.runner]
            kind = "process"
            program = ""
            path_arg_mode = "append"

            [validators.policy]
            timeout_secs = 0
        "#,
        )
        .unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{}").unwrap();

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
            message.contains("Validation configuration error(s):"),
            "{message}"
        );
        assert!(message.contains("id must not be empty"), "{message}");
        assert!(
            message.contains("runner.program must not be empty"),
            "{message}"
        );
        assert!(
            message.contains("policy.timeout_secs must be greater than 0"),
            "{message}"
        );
        assert!(
            message.contains("mode = \"replace\" requires at least one replaces entry"),
            "{message}"
        );
        assert_eq!(result.details["validators_run"], 0);
        assert_eq!(
            result.details["operator_config_errors"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
    }

    #[tokio::test]
    async fn language_validator_coalesces_same_kind_paths() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let src_dir = dir.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let path_one = src_dir.join("lib.rs");
        let path_two = src_dir.join("main.rs");
        std::fs::write(&path_one, "pub fn demo() {}\n").unwrap();
        std::fs::write(&path_two, "fn main() {}\n").unwrap();

        let result = execute(
            &[path_one.clone(), path_two.clone()],
            ValidationLevel::Standard,
            dir.path(),
        )
        .await
        .unwrap();
        let ContentBlock::Text { text: message } = &result.content[0] else {
            panic!("expected text result");
        };
        assert!(
            message.contains("Validated 2 path(s) with 1 applicable validator run(s)"),
            "{message}"
        );
        assert!(message.contains("for 2 Rust path(s)"), "{message}");
        assert_eq!(result.details["validators_run"], 1);
        assert_eq!(result.details["builtin_validators_run"], 1);
    }
}
