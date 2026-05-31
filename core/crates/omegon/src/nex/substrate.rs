//! Read-only Nex substrate inspection wrapper.
//!
//! Omegon consumes Nex as the source of truth for deterministic substrate
//! discovery and adds an advisory policy overlay for agent/runtime decisions.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tokio::process::Command;

const REPORT_SCHEMA: &str = "io.styrene.omegon.nex-substrate-report.v1";
const NEX_DEVENV_REPORT_SCHEMA: &str = "io.styrene.nex.devenv-import-report.v1";
const NEX_TIMEOUT: Duration = Duration::from_secs(20);
const OUTPUT_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct NexSubstrateReport {
    pub schema: &'static str,
    pub source: &'static str,
    pub nex_available: bool,
    pub path: String,
    pub mode: String,
    pub reports: NexSubstrateReports,
    pub policy: NexSubstratePolicy,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct NexSubstrateReports {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devenv_import: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NexSubstratePolicy {
    pub context: &'static str,
    pub enforcement: &'static str,
    pub summary: NexSubstratePolicySummary,
    pub findings: Vec<NexSubstrateFinding>,
}

impl Default for NexSubstratePolicy {
    fn default() -> Self {
        Self {
            context: "interactive_inspection",
            enforcement: "advisory",
            summary: NexSubstratePolicySummary::default(),
            findings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct NexSubstratePolicySummary {
    pub blockers: usize,
    pub warnings: usize,
    pub review_items: usize,
    pub secret_contracts: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct NexSubstrateFinding {
    pub severity: &'static str,
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub promote_to_blocker_in: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<NexSubstrateFindingSource>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NexSubstrateFindingSource {
    pub report: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

pub async fn inspect_devenv(path: &Path) -> NexSubstrateReport {
    let path_display = path.display().to_string();
    let Some(nex) = find_on_path("nex") else {
        let mut report = unavailable_report(path_display, "devenv".to_string());
        report.diagnostics.push(
            "install or expose `nex` to enable deterministic substrate inspection".to_string(),
        );
        return report;
    };

    let output = match tokio::time::timeout(
        NEX_TIMEOUT,
        Command::new(nex)
            .arg("devenv")
            .arg("inspect")
            .arg(path)
            .arg("--json")
            .kill_on_drop(true)
            .output(),
    )
    .await
    {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            let mut report = unavailable_report(path_display, "devenv".to_string());
            report.diagnostics.push(format!("failed to execute nex: {error}"));
            return report;
        }
        Err(_) => {
            let mut report = unavailable_report(path_display, "devenv".to_string());
            report
                .diagnostics
                .push(format!("nex devenv inspect timed out after {}s", NEX_TIMEOUT.as_secs()));
            return report;
        }
    };

    let mut diagnostics = Vec::new();
    if output.stdout.len() > OUTPUT_LIMIT {
        return execution_error_report(
            path_display,
            format!("nex stdout exceeded {} bytes", OUTPUT_LIMIT),
        );
    }
    if output.stderr.len() > OUTPUT_LIMIT {
        diagnostics.push(format!("nex stderr exceeded {} bytes and was truncated", OUTPUT_LIMIT));
    } else if !output.stderr.is_empty() {
        diagnostics.push(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    if !output.status.success() {
        return execution_error_report(
            path_display,
            format!("nex devenv inspect exited with {}", output.status),
        );
    }

    let report_json: Value = match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(error) => {
            return execution_error_report(path_display, format!("nex emitted invalid JSON: {error}"));
        }
    };

    let policy = derive_policy(&report_json);
    NexSubstrateReport {
        schema: REPORT_SCHEMA,
        source: "nex",
        nex_available: true,
        path: path_display,
        mode: "devenv".to_string(),
        reports: NexSubstrateReports {
            devenv_import: Some(report_json),
        },
        policy,
        diagnostics,
    }
}

pub fn derive_policy(devenv_report: &Value) -> NexSubstratePolicy {
    let mut policy = NexSubstratePolicy::default();

    if devenv_report.get("schema").and_then(Value::as_str) != Some(NEX_DEVENV_REPORT_SCHEMA) {
        policy.findings.push(NexSubstrateFinding {
            severity: "warning",
            code: "schema_unknown",
            message: "Nex returned an unknown or missing devenv report schema".to_string(),
            promote_to_blocker_in: Vec::new(),
            source: Some(NexSubstrateFindingSource {
                report: "devenv_import",
                item_id: None,
                file: None,
                path: None,
            }),
        });
    }

    for item in devenv_report
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        derive_item_findings(item, &mut policy);
    }

    summarize_policy(&mut policy);
    policy
}

fn derive_item_findings(item: &Value, policy: &mut NexSubstratePolicy) {
    let id = item.get("id").and_then(Value::as_str).unwrap_or("unknown");
    let kind = item.get("kind").and_then(Value::as_str).unwrap_or("");
    let bucket = item.get("bucket").and_then(Value::as_str).unwrap_or("");
    let safety = item
        .get("safety")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    let review_required = item
        .get("review")
        .and_then(|review| review.get("required"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let source = item_source(item);

    if review_required {
        policy.findings.push(finding(
            "warning",
            "requires_review",
            format!("{id} requires operator review before deterministic migration or enforcement"),
            vec!["headless", "release"],
            source.clone(),
        ));
    }
    if bucket == "unsupported" {
        policy.findings.push(finding(
            "warning",
            "unsupported_substrate",
            format!("{id} is unsupported by Nex import"),
            vec!["headless", "release"],
            source.clone(),
        ));
    }
    if kind == "secret-contract" || safety.contains(&"secret-contract") {
        policy.findings.push(finding(
            "info",
            "secret_contract",
            format!("{id} declares a secret contract"),
            Vec::new(),
            source.clone(),
        ));
    }
    if safety.contains(&"secret-value-runtime") {
        policy.findings.push(finding(
            "warning",
            "secret_runtime_value",
            format!("{id} may expose runtime secret values and needs scoped grants/redaction"),
            vec!["headless", "release"],
            source.clone(),
        ));
    }
    if safety.contains(&"arbitrary-command") {
        policy.findings.push(finding(
            "warning",
            "arbitrary_command",
            format!("{id} contains arbitrary command surface"),
            vec!["headless", "release"],
            source.clone(),
        ));
    }
    if safety.contains(&"privileged-mutation") || safety.contains(&"system-config-mutation") {
        policy.findings.push(finding(
            "warning",
            "privileged_mutation",
            format!("{id} may require privileged or system mutation"),
            vec!["headless", "release"],
            source.clone(),
        ));
    }
    if safety.contains(&"destructive-disk-operation") || safety.contains(&"hardware-driver-mutation") {
        policy.findings.push(finding(
            "warning",
            "destructive_mutation",
            format!("{id} may involve destructive disk or hardware-driver mutation"),
            vec!["headless", "release"],
            source,
        ));
    }
}

fn finding(
    severity: &'static str,
    code: &'static str,
    message: String,
    promote_to_blocker_in: Vec<&'static str>,
    source: Option<NexSubstrateFindingSource>,
) -> NexSubstrateFinding {
    NexSubstrateFinding {
        severity,
        code,
        message,
        promote_to_blocker_in,
        source,
    }
}

fn item_source(item: &Value) -> Option<NexSubstrateFindingSource> {
    let source = item.get("source")?;
    Some(NexSubstrateFindingSource {
        report: "devenv_import",
        item_id: item.get("id").and_then(Value::as_str).map(ToString::to_string),
        file: source.get("file").and_then(Value::as_str).map(ToString::to_string),
        path: source.get("path").and_then(Value::as_str).map(ToString::to_string),
    })
}

fn summarize_policy(policy: &mut NexSubstratePolicy) {
    policy.summary = NexSubstratePolicySummary::default();
    for finding in &policy.findings {
        match finding.severity {
            "blocker" => policy.summary.blockers += 1,
            "warning" => policy.summary.warnings += 1,
            _ => {}
        }
        match finding.code {
            "requires_review" => policy.summary.review_items += 1,
            "secret_contract" => policy.summary.secret_contracts += 1,
            _ => {}
        }
    }
}

fn unavailable_report(path: String, mode: String) -> NexSubstrateReport {
    let mut policy = NexSubstratePolicy::default();
    policy.findings.push(NexSubstrateFinding {
        severity: "warning",
        code: "nex_unavailable",
        message: "Nex is not available on PATH; substrate inspection was skipped".to_string(),
        promote_to_blocker_in: vec!["headless", "release"],
        source: None,
    });
    summarize_policy(&mut policy);
    NexSubstrateReport {
        schema: REPORT_SCHEMA,
        source: "nex",
        nex_available: false,
        path,
        mode,
        reports: NexSubstrateReports::default(),
        policy,
        diagnostics: Vec::new(),
    }
}

fn execution_error_report(path: String, diagnostic: String) -> NexSubstrateReport {
    let mut report = unavailable_report(path, "devenv".to_string());
    report.nex_available = true;
    report.policy.findings[0].code = "nex_execution_failed";
    report.policy.findings[0].message = "Nex substrate inspection failed".to_string();
    report.diagnostics.push(diagnostic);
    report
}

fn find_on_path(command: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH")?
        .pipe(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .into_iter()
        .flat_map(|dir| {
            let candidate = dir.join(command);
            [candidate.clone(), candidate.with_extension("exe")]
        })
        .find(|candidate| candidate.is_file())
}

pub fn summary_text(report: &NexSubstrateReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Nex substrate inspection: {}",
        if report.nex_available { "available" } else { "unavailable" }
    ));
    lines.push(format!("Path: {}", report.path));
    if let Some(schema) = report
        .reports
        .devenv_import
        .as_ref()
        .and_then(|report| report.get("schema"))
        .and_then(Value::as_str)
    {
        lines.push(format!("Report: {schema}"));
    }
    if let Some(summary) = report
        .reports
        .devenv_import
        .as_ref()
        .and_then(|report| report.get("summary"))
    {
        lines.push(format!(
            "Items: portable={} project={} machine={} review={} unsupported={}",
            summary.get("portable").and_then(Value::as_u64).unwrap_or(0),
            summary.get("projectScoped").and_then(Value::as_u64).unwrap_or(0),
            summary
                .get("machineScopedCandidate")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            summary.get("requiresReview").and_then(Value::as_u64).unwrap_or(0),
            summary.get("unsupported").and_then(Value::as_u64).unwrap_or(0),
        ));
    }
    lines.push(format!(
        "Policy: {} blockers, {} warnings, {} secret contracts",
        report.policy.summary.blockers,
        report.policy.summary.warnings,
        report.policy.summary.secret_contracts,
    ));
    for finding in report
        .policy
        .findings
        .iter()
        .filter(|finding| finding.severity != "info")
        .take(5)
    {
        lines.push(format!("- {}: {}", finding.code, finding.message));
    }
    for diagnostic in report.diagnostics.iter().filter(|d| !d.is_empty()).take(3) {
        lines.push(format!("diagnostic: {diagnostic}"));
    }
    lines.join("\n")
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn derives_policy_from_nex_items() {
        let report = json!({
            "schema": NEX_DEVENV_REPORT_SCHEMA,
            "items": [
                {
                    "id": "devenv.nix:enterShell",
                    "kind": "shell-hook",
                    "bucket": "requiresReview",
                    "safety": ["arbitrary-command"],
                    "source": {"file": "devenv.nix", "path": "enterShell"},
                    "review": {"required": true}
                },
                {
                    "id": "secretspec:default:API_TOKEN",
                    "kind": "secret-contract",
                    "bucket": "portable",
                    "safety": ["secret-contract"],
                    "source": {"file": "secretspec.toml", "path": "profiles.default.API_TOKEN"},
                    "review": {"required": false}
                }
            ]
        });

        let policy = derive_policy(&report);

        assert_eq!(policy.summary.warnings, 2);
        assert_eq!(policy.summary.review_items, 1);
        assert_eq!(policy.summary.secret_contracts, 1);
        assert!(policy.findings.iter().any(|finding| finding.code == "arbitrary_command"));
        assert!(policy.findings.iter().any(|finding| finding.code == "secret_contract"));
    }

    #[test]
    fn unknown_schema_is_warning() {
        let policy = derive_policy(&json!({"schema": "other", "items": []}));
        assert_eq!(policy.summary.warnings, 1);
        assert_eq!(policy.findings[0].code, "schema_unknown");
    }
}
