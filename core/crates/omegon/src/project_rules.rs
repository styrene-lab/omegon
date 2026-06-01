//! Project Rules read model and explicit checker.
//!
//! Project Rules turn advisory evidence findings into project/context-scoped
//! policy reports. They do not mutate OpenSpec state and they do not hard-block
//! lifecycle commands unless an explicit project-rules check is invoked in an
//! enforcing context.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::evidence::{ClaimSupportStatus, EvidenceStore};
use crate::lifecycle::spec::{self, EvidenceGateDecision};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuleMode {
    Warn,
    Enforce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuleSeverity {
    Info,
    Warn,
    Block,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectRulesConfig {
    pub schema_version: Option<u32>,
    pub mode: Option<RuleMode>,
    #[serde(default)]
    pub contexts: HashMap<String, ContextConfig>,
    #[serde(default)]
    pub rules: Vec<ProjectRule>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContextConfig {
    pub mode: Option<RuleMode>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectRule {
    pub id: String,
    pub kind: String,
    pub description: Option<String>,
    pub severity: Option<RuleSeverity>,
    pub contexts: Option<Vec<String>>,
    pub claim: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectRulesReport {
    pub context: String,
    pub mode: RuleMode,
    pub passed: bool,
    pub findings: Vec<ProjectRuleFinding>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectRuleFinding {
    pub rule_id: String,
    pub severity: RuleSeverity,
    pub enforced: bool,
    pub subject: String,
    pub message: String,
}

impl Default for ProjectRulesConfig {
    fn default() -> Self {
        Self {
            schema_version: Some(1),
            mode: Some(RuleMode::Warn),
            contexts: HashMap::new(),
            rules: vec![
                ProjectRule {
                    id: "no-refuted-evidence-claims".to_string(),
                    kind: "no-refuted-evidence-claims".to_string(),
                    description: Some(
                        "Explicitly attached OpenSpec evidence claims should not be refuted"
                            .to_string(),
                    ),
                    severity: Some(RuleSeverity::Block),
                    contexts: None,
                    claim: None,
                },
                ProjectRule {
                    id: "evidence-map-parses".to_string(),
                    kind: "evidence-map-parses".to_string(),
                    description: Some("The .omegon/evidence map should parse".to_string()),
                    severity: Some(RuleSeverity::Block),
                    contexts: None,
                    claim: None,
                },
            ],
        }
    }
}

impl ProjectRulesConfig {
    pub fn load(repo_path: &Path) -> anyhow::Result<Self> {
        let path = repo_path.join(".omegon/project-rules.toml");
        if !path.is_file() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
    }

    fn mode_for_context(&self, context: &str) -> RuleMode {
        self.contexts
            .get(context)
            .and_then(|c| c.mode)
            .or(self.mode)
            .unwrap_or(RuleMode::Warn)
    }
}

pub fn check(repo_path: &Path, context: &str) -> ProjectRulesReport {
    let config = match ProjectRulesConfig::load(repo_path) {
        Ok(config) => config,
        Err(err) => {
            return ProjectRulesReport {
                context: context.to_string(),
                mode: RuleMode::Warn,
                passed: true,
                findings: vec![ProjectRuleFinding {
                    rule_id: "project-rules-config".to_string(),
                    severity: RuleSeverity::Warn,
                    enforced: false,
                    subject: ".omegon/project-rules.toml".to_string(),
                    message: err.to_string(),
                }],
            };
        }
    };
    let mode = config.mode_for_context(context);
    let mut findings = Vec::new();
    for rule in config
        .rules
        .iter()
        .filter(|rule| rule_applies(rule, context))
    {
        evaluate_rule(repo_path, rule, mode, &mut findings);
    }
    let passed = !findings
        .iter()
        .any(|finding| finding.enforced && finding.severity == RuleSeverity::Block);
    ProjectRulesReport {
        context: context.to_string(),
        mode,
        passed,
        findings,
    }
}

fn rule_applies(rule: &ProjectRule, context: &str) -> bool {
    rule.contexts
        .as_ref()
        .is_none_or(|contexts| contexts.iter().any(|c| c == context))
}

fn enforced(mode: RuleMode, severity: RuleSeverity) -> bool {
    mode == RuleMode::Enforce && severity == RuleSeverity::Block
}

fn evaluate_rule(
    repo_path: &Path,
    rule: &ProjectRule,
    mode: RuleMode,
    findings: &mut Vec<ProjectRuleFinding>,
) {
    match rule.kind.as_str() {
        "evidence-map-parses" => evaluate_evidence_map_parses(repo_path, rule, mode, findings),
        "no-refuted-evidence-claims" => evaluate_no_refuted_claims(repo_path, rule, mode, findings),
        "claim-supported" => evaluate_claim_supported(repo_path, rule, mode, findings),
        _ => findings.push(ProjectRuleFinding {
            rule_id: rule.id.clone(),
            severity: RuleSeverity::Warn,
            enforced: false,
            subject: rule.kind.clone(),
            message: format!("unknown project rule kind '{}'", rule.kind),
        }),
    }
}

fn evaluate_evidence_map_parses(
    repo_path: &Path,
    rule: &ProjectRule,
    mode: RuleMode,
    findings: &mut Vec<ProjectRuleFinding>,
) {
    let severity = rule.severity.unwrap_or(RuleSeverity::Block);
    let root = repo_path.join(".omegon/evidence");
    let required = [
        root.join("manifest.json"),
        root.join("claims.jsonl"),
        root.join("records.jsonl"),
        root.join("edges.jsonl"),
    ];
    let missing: Vec<_> = required.iter().filter(|path| !path.is_file()).collect();
    if !missing.is_empty() {
        findings.push(ProjectRuleFinding {
            rule_id: rule.id.clone(),
            severity,
            enforced: enforced(mode, severity),
            subject: ".omegon/evidence".to_string(),
            message: format!(
                "missing evidence files: {}",
                missing
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
        return;
    }
    let manifest_result = fs::read_to_string(root.join("manifest.json"))
        .map_err(anyhow::Error::from)
        .and_then(|text| {
            serde_json::from_str::<serde_json::Value>(&text).map_err(anyhow::Error::from)
        });
    if let Err(err) = manifest_result.and_then(|_| EvidenceStore::load(repo_path).map(|_| ())) {
        findings.push(ProjectRuleFinding {
            rule_id: rule.id.clone(),
            severity,
            enforced: enforced(mode, severity),
            subject: ".omegon/evidence".to_string(),
            message: err.to_string(),
        });
    }
}

fn evaluate_no_refuted_claims(
    repo_path: &Path,
    rule: &ProjectRule,
    mode: RuleMode,
    findings: &mut Vec<ProjectRuleFinding>,
) {
    let severity = rule.severity.unwrap_or(RuleSeverity::Block);
    for change in spec::list_changes(repo_path) {
        for finding in spec::evaluate_evidence_gates(&change) {
            if matches!(finding.decision, EvidenceGateDecision::Block) {
                findings.push(ProjectRuleFinding {
                    rule_id: rule.id.clone(),
                    severity,
                    enforced: enforced(mode, severity),
                    subject: finding.claim_id,
                    message: finding.detail,
                });
            }
        }
    }
}

fn evaluate_claim_supported(
    repo_path: &Path,
    rule: &ProjectRule,
    mode: RuleMode,
    findings: &mut Vec<ProjectRuleFinding>,
) {
    let Some(claim_id) = &rule.claim else {
        findings.push(ProjectRuleFinding {
            rule_id: rule.id.clone(),
            severity: RuleSeverity::Warn,
            enforced: false,
            subject: rule.id.clone(),
            message: "claim-supported rule is missing claim".to_string(),
        });
        return;
    };
    let severity = rule.severity.unwrap_or(RuleSeverity::Warn);
    match EvidenceStore::load(repo_path) {
        Ok(store) => {
            let summary = store.support_summary(claim_id);
            if summary.status != ClaimSupportStatus::Supported {
                findings.push(ProjectRuleFinding {
                    rule_id: rule.id.clone(),
                    severity,
                    enforced: enforced(mode, severity),
                    subject: claim_id.clone(),
                    message: format!("claim {claim_id} is {:?}", summary.status),
                });
            }
        }
        Err(err) => findings.push(ProjectRuleFinding {
            rule_id: rule.id.clone(),
            severity,
            enforced: enforced(mode, severity),
            subject: claim_id.clone(),
            message: err.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_warn_mode_does_not_enforce_block_findings() {
        let dir = tempfile::tempdir().unwrap();
        let report = check(dir.path(), "default");
        assert!(report.passed);
        assert_eq!(report.mode, RuleMode::Warn);
    }

    #[test]
    fn enforcing_context_fails_block_findings() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".omegon")).unwrap();
        fs::write(
            dir.path().join(".omegon/project-rules.toml"),
            r#"
schema_version = 1
mode = "warn"

[contexts.ci]
mode = "enforce"

[[rules]]
id = "evidence-map-parses"
kind = "evidence-map-parses"
severity = "block"
contexts = ["ci"]
"#,
        )
        .unwrap();
        let report = check(dir.path(), "ci");
        assert!(!report.passed);
        assert!(report.findings.iter().any(|f| f.enforced));
    }
}
