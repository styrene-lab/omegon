//! Pure permission policy evaluation.
//!
//! This module intentionally has no UI, filesystem, or dispatch side effects.
//! It answers one question: given a tool call subject, should the harness allow,
//! prompt, or deny before execution?

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Policy action for a tool call or matching pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionAction {
    Allow,
    Prompt,
    Deny,
}

impl PermissionAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Prompt => "prompt",
            Self::Deny => "deny",
        }
    }
}

/// Per-tool policy rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolPermissionRule {
    /// Shorthand: `write = "allow"`.
    Action(PermissionAction),
    /// Expanded form: `bash = { action = "prompt", patterns = ["rm *"] }`.
    ///
    /// `action` applies only when a pattern matches. Unmatched subjects default
    /// to allow unless `otherwise` is supplied. This keeps sensitive-file rules
    /// like `write = { action = "prompt", patterns = ["*.env"] }` from
    /// accidentally prompting every write.
    Patterned {
        action: PermissionAction,
        #[serde(default)]
        patterns: Vec<String>,
        #[serde(default)]
        otherwise: Option<PermissionAction>,
    },
}

impl ToolPermissionRule {
    fn base_action(&self) -> PermissionAction {
        match self {
            Self::Action(action) => *action,
            Self::Patterned { action, .. } => *action,
        }
    }

    fn patterns(&self) -> &[String] {
        match self {
            Self::Action(_) => &[],
            Self::Patterned { patterns, .. } => patterns,
        }
    }
}

/// Pure policy table keyed by tool name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionPolicy {
    #[serde(default)]
    pub tools: BTreeMap<String, ToolPermissionRule>,
}

/// Evaluation result with provenance for audit/UI rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDecision {
    pub action: PermissionAction,
    pub reason: PermissionDecisionReason,
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecisionReason {
    NoRule,
    ToolRule { tool: String },
    PatternRule { tool: String, pattern: String },
}

impl PermissionPolicy {
    /// Evaluate a tool call subject. `subject` is the normalized string the
    /// rule patterns match against: for bash this is the command; for file
    /// tools this is normally the requested path.
    ///
    /// Pattern rules apply `action` only when a pattern matches. Unmatched
    /// patterned subjects default to allow unless `otherwise` is explicit.
    /// Cross-layer precedence (Lex/persona/project/session) is outside this
    /// first pure evaluator slice.
    pub fn evaluate(&self, tool: &str, subject: &str) -> PermissionDecision {
        let Some(rule) = self.tools.get(tool) else {
            return PermissionDecision {
                action: PermissionAction::Allow,
                reason: PermissionDecisionReason::NoRule,
            };
        };

        for pattern in rule.patterns() {
            if wildcard_match(pattern, subject) {
                return PermissionDecision {
                    action: rule.base_action(),
                    reason: PermissionDecisionReason::PatternRule {
                        tool: tool.to_string(),
                        pattern: pattern.clone(),
                    },
                };
            }
        }

        let action = match rule {
            ToolPermissionRule::Action(action) => *action,
            ToolPermissionRule::Patterned { otherwise, .. } => {
                otherwise.unwrap_or(PermissionAction::Allow)
            }
        };

        let reason = match rule {
            ToolPermissionRule::Action(_) => PermissionDecisionReason::ToolRule {
                tool: tool.to_string(),
            },
            ToolPermissionRule::Patterned {
                otherwise: Some(_), ..
            } => PermissionDecisionReason::ToolRule {
                tool: tool.to_string(),
            },
            ToolPermissionRule::Patterned {
                otherwise: None, ..
            } => PermissionDecisionReason::NoRule,
        };

        PermissionDecision { action, reason }
    }
}

/// Normalized subject extracted from a tool call for policy matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionSubject {
    pub tool: String,
    pub value: String,
    pub kind: PermissionSubjectKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionSubjectKind {
    Command,
    Path,
    Opaque,
}

impl PermissionPolicy {
    /// Evaluate multiple subjects for a single tool call and return the
    /// strongest decision. This is required for multi-target tools such as
    /// `change` and `validate`.
    pub fn evaluate_subjects<'a>(
        &self,
        tool: &str,
        subjects: impl IntoIterator<Item = &'a PermissionSubject>,
    ) -> PermissionDecision {
        let mut strongest = PermissionDecision {
            action: PermissionAction::Allow,
            reason: PermissionDecisionReason::NoRule,
        };
        let mut saw_subject = false;
        for subject in subjects {
            saw_subject = true;
            let decision = self.evaluate(tool, &subject.value);
            if decision.action.strength() > strongest.action.strength() {
                strongest = decision;
            }
        }
        if saw_subject {
            strongest
        } else {
            self.evaluate(tool, "")
        }
    }
}

impl PermissionAction {
    fn strength(self) -> u8 {
        match self {
            Self::Allow => 0,
            Self::Prompt => 1,
            Self::Deny => 2,
        }
    }
}

/// Extract policy subjects from core tool arguments.
///
/// This function is pure and intentionally avoids secrets. For tools that carry
/// sensitive values, match only on names/paths/commands, never contents.
pub fn subjects_from_tool_args(tool: &str, args: &serde_json::Value) -> Vec<PermissionSubject> {
    match tool {
        crate::tool_registry::core::BASH | crate::tool_registry::core::TERMINAL => args
            .get("command")
            .and_then(|v| v.as_str())
            .map(|command| vec![subject(tool, command, PermissionSubjectKind::Command)])
            .unwrap_or_default(),
        crate::tool_registry::core::READ
        | crate::tool_registry::core::WRITE
        | crate::tool_registry::core::EDIT => args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|path| vec![subject(tool, path, PermissionSubjectKind::Path)])
            .unwrap_or_default(),
        crate::tool_registry::core::CHANGE => args
            .get("edits")
            .and_then(|v| v.as_array())
            .map(|edits| {
                edits
                    .iter()
                    .filter_map(|edit| edit.get("file").and_then(|v| v.as_str()))
                    .map(|file| subject(tool, file, PermissionSubjectKind::Path))
                    .collect()
            })
            .unwrap_or_default(),
        crate::tool_registry::core::VALIDATE => args
            .get("paths")
            .and_then(|v| v.as_array())
            .map(|paths| {
                paths
                    .iter()
                    .filter_map(|path| path.as_str())
                    .map(|path| subject(tool, path, PermissionSubjectKind::Path))
                    .collect()
            })
            .unwrap_or_default(),
        crate::tool_registry::secrets::SECRET_SET
        | crate::tool_registry::secrets::SECRET_DELETE => args
            .get("name")
            .and_then(|v| v.as_str())
            .map(|name| vec![subject(tool, name, PermissionSubjectKind::Opaque)])
            .unwrap_or_default(),
        crate::tool_registry::web_search::WEB_FETCH => args
            .get("url")
            .and_then(|v| v.as_str())
            .map(|url| vec![subject(tool, url, PermissionSubjectKind::Opaque)])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn subject(tool: &str, value: &str, kind: PermissionSubjectKind) -> PermissionSubject {
    PermissionSubject {
        tool: tool.to_string(),
        value: value.to_string(),
        kind,
    }
}

/// Permission layer provenance. Ordering is intentional and mirrors the
/// Styrene RBAC posture: immutable framework constraints first, then persona,
/// project policy, and finally session/operator grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PermissionLayer {
    Lex = 0,
    Persona = 1,
    Project = 2,
    Session = 3,
}

impl PermissionLayer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lex => "lex",
            Self::Persona => "persona",
            Self::Project => "project",
            Self::Session => "session",
        }
    }
}

/// Policy plus layer provenance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayeredPermissionPolicy {
    pub lex: PermissionPolicy,
    pub persona: PermissionPolicy,
    pub project: PermissionPolicy,
    pub session: PermissionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayeredPermissionDecision {
    pub action: PermissionAction,
    pub layer: Option<PermissionLayer>,
    pub decision: PermissionDecision,
}

impl LayeredPermissionPolicy {
    /// Evaluate layered policy with deny-overrides precedence. Lower layers may
    /// tighten higher layers, but session/project allows cannot override a Lex
    /// or persona deny. Prompts beat allows, denies beat everything.
    pub fn evaluate_subjects<'a>(
        &self,
        tool: &str,
        subjects: impl IntoIterator<Item = &'a PermissionSubject>,
    ) -> LayeredPermissionDecision {
        let subjects: Vec<&PermissionSubject> = subjects.into_iter().collect();
        let mut best = LayeredPermissionDecision {
            action: PermissionAction::Allow,
            layer: None,
            decision: PermissionDecision {
                action: PermissionAction::Allow,
                reason: PermissionDecisionReason::NoRule,
            },
        };
        for (layer, policy) in [
            (PermissionLayer::Lex, &self.lex),
            (PermissionLayer::Persona, &self.persona),
            (PermissionLayer::Project, &self.project),
            (PermissionLayer::Session, &self.session),
        ] {
            let decision = policy.evaluate_subjects(tool, subjects.iter().copied());
            if decision.action.strength() > best.action.strength()
                || (decision.action == best.action
                    && decision.action != PermissionAction::Allow
                    && best.layer.is_none())
            {
                best = LayeredPermissionDecision {
                    action: decision.action,
                    layer: Some(layer),
                    decision,
                };
            }
            if best.action == PermissionAction::Deny {
                break;
            }
        }
        best
    }
}

/// Styrene RBAC capability associated with an Omegon tool surface.
///
/// This deliberately reuses `styrene-rbac` capability constants so Omegon's
/// local permission model can converge with StyreneIdentity/StyreneRBAC
/// rosters without inventing a second access vocabulary.
pub fn styrene_capability_for_tool(tool: &str) -> Option<&'static str> {
    match tool {
        crate::tool_registry::core::BASH | crate::tool_registry::core::TERMINAL => {
            Some(styrene_rbac::Capability::TERMINAL_RESTRICTED)
        }
        crate::tool_registry::core::READ | crate::tool_registry::web_search::WEB_FETCH => {
            Some(styrene_rbac::Capability::WEB_READ)
        }
        crate::tool_registry::core::WRITE
        | crate::tool_registry::core::EDIT
        | crate::tool_registry::core::CHANGE => Some(styrene_rbac::Capability::WEB_WRITE),
        crate::tool_registry::core::VALIDATE => Some(styrene_rbac::Capability::RPC_STATUS),
        crate::tool_registry::secrets::SECRET_SET
        | crate::tool_registry::secrets::SECRET_DELETE => {
            Some(styrene_rbac::Capability::RPC_CONFIG_UPDATE)
        }
        _ => None,
    }
}

pub fn styrene_role_allows_tool(role: styrene_rbac::Role, tool: &str) -> bool {
    styrene_capability_for_tool(tool)
        .map(|cap| {
            styrene_rbac::RosterEntry::new("00000000000000000000000000000000", role)
                .has_capability(cap)
        })
        .unwrap_or(true)
}

/// Build the runtime permission policy snapshot from settings.
pub fn layered_policy_from_settings(
    settings: &crate::settings::Settings,
) -> LayeredPermissionPolicy {
    LayeredPermissionPolicy {
        project: PermissionPolicy {
            tools: settings.permissions.tools.clone(),
        },
        ..Default::default()
    }
}

/// Optional Styrene RBAC role configured for the active runtime.
pub fn styrene_role_from_settings(
    settings: &crate::settings::Settings,
) -> Option<styrene_rbac::Role> {
    settings
        .permissions
        .role
        .as_deref()
        .and_then(styrene_rbac::Role::from_name)
}

/// Minimal glob-ish matcher for policy subjects.
///
/// Supports `*` as any sequence and `?` as one character. Matching is over
/// chars, not path components; this is deliberate because subjects include
/// shell commands as well as paths.
fn wildcard_match(pattern: &str, subject: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = subject.chars().collect();
    let (mut pi, mut si) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut star_match = 0usize;

    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            star_match = si;
            pi += 1;
        } else if let Some(star_idx) = star {
            pi = star_idx + 1;
            star_match += 1;
            si = star_match;
        } else {
            return false;
        }
    }

    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }

    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(entries: &[(&str, ToolPermissionRule)]) -> PermissionPolicy {
        PermissionPolicy {
            tools: entries
                .iter()
                .map(|(tool, rule)| ((*tool).to_string(), rule.clone()))
                .collect(),
        }
    }

    #[test]
    fn no_rule_defaults_to_allow() {
        let decision = PermissionPolicy::default().evaluate("read", "src/main.rs");
        assert_eq!(decision.action, PermissionAction::Allow);
        assert_eq!(decision.reason, PermissionDecisionReason::NoRule);
    }

    #[test]
    fn unknown_tool_defaults_to_allow_for_extension_compatibility() {
        let layered = LayeredPermissionPolicy::default();
        let decision = layered.evaluate_subjects("extension_future_tool", std::iter::empty());
        assert_eq!(decision.action, PermissionAction::Allow);
        assert_eq!(decision.layer, None);
        assert_eq!(decision.decision.reason, PermissionDecisionReason::NoRule);
    }

    #[test]
    fn exact_tool_rule_can_deny() {
        let p = policy(&[("bash", ToolPermissionRule::Action(PermissionAction::Deny))]);
        let decision = p.evaluate("bash", "echo ok");
        assert_eq!(decision.action, PermissionAction::Deny);
        assert_eq!(
            decision.reason,
            PermissionDecisionReason::ToolRule {
                tool: "bash".into()
            }
        );
    }

    #[test]
    fn patterned_rule_prompts_matching_bash_command() {
        let p = policy(&[(
            "bash",
            ToolPermissionRule::Patterned {
                action: PermissionAction::Prompt,
                patterns: vec!["rm *".into(), "sudo *".into()],
                otherwise: None,
            },
        )]);
        let decision = p.evaluate("bash", "rm -rf target/tmp");
        assert_eq!(decision.action, PermissionAction::Prompt);
        assert_eq!(
            decision.reason,
            PermissionDecisionReason::PatternRule {
                tool: "bash".into(),
                pattern: "rm *".into(),
            }
        );
    }

    #[test]
    fn patterned_rule_falls_back_to_tool_action_when_no_pattern_matches() {
        let p = policy(&[(
            "write",
            ToolPermissionRule::Patterned {
                action: PermissionAction::Prompt,
                patterns: vec!["*.env".into(), "*.pem".into()],
                otherwise: None,
            },
        )]);
        let decision = p.evaluate("write", "src/lib.rs");
        assert_eq!(decision.action, PermissionAction::Allow);
        assert_eq!(decision.reason, PermissionDecisionReason::NoRule);
    }

    #[test]
    fn patterned_rule_can_override_unmatched_subject() {
        let p = policy(&[(
            "write",
            ToolPermissionRule::Patterned {
                action: PermissionAction::Prompt,
                patterns: vec!["*.env".into()],
                otherwise: Some(PermissionAction::Deny),
            },
        )]);
        let decision = p.evaluate("write", "src/lib.rs");
        assert_eq!(decision.action, PermissionAction::Deny);
        assert_eq!(
            decision.reason,
            PermissionDecisionReason::ToolRule {
                tool: "write".into(),
            }
        );
    }

    #[test]
    fn patterned_rule_prompts_matching_sensitive_file() {
        let p = policy(&[(
            "write",
            ToolPermissionRule::Patterned {
                action: PermissionAction::Prompt,
                patterns: vec!["*.env".into(), "*.pem".into()],
                otherwise: None,
            },
        )]);
        let decision = p.evaluate("write", "project/.env");
        assert_eq!(decision.action, PermissionAction::Prompt);
    }

    #[test]
    fn extracts_core_tool_subjects_without_secret_values() {
        let write = subjects_from_tool_args(
            crate::tool_registry::core::WRITE,
            &serde_json::json!({"path":"src/lib.rs", "content":"secret content"}),
        );
        assert_eq!(write.len(), 1);
        assert_eq!(write[0].value, "src/lib.rs");
        assert_eq!(write[0].kind, PermissionSubjectKind::Path);

        let secret = subjects_from_tool_args(
            crate::tool_registry::secrets::SECRET_SET,
            &serde_json::json!({"name":"github", "value":"ghp_should_not_match"}),
        );
        assert_eq!(secret.len(), 1);
        assert_eq!(secret[0].value, "github");
    }

    #[test]
    fn extracts_multi_path_subjects_for_change_and_validate() {
        let change = subjects_from_tool_args(
            crate::tool_registry::core::CHANGE,
            &serde_json::json!({"edits":[{"file":"src/a.rs"},{"file":".env"}]}),
        );
        assert_eq!(
            change.iter().map(|s| s.value.as_str()).collect::<Vec<_>>(),
            vec!["src/a.rs", ".env"]
        );

        let validate = subjects_from_tool_args(
            crate::tool_registry::core::VALIDATE,
            &serde_json::json!({"paths":["src/a.rs", "tests/a.rs"]}),
        );
        assert_eq!(validate.len(), 2);
        assert!(
            validate
                .iter()
                .all(|s| s.kind == PermissionSubjectKind::Path)
        );
    }

    #[test]
    fn multi_subject_evaluation_uses_strongest_decision() {
        let p = policy(&[(
            "change",
            ToolPermissionRule::Patterned {
                action: PermissionAction::Prompt,
                patterns: vec!["*.env".into()],
                otherwise: None,
            },
        )]);
        let subjects = subjects_from_tool_args(
            crate::tool_registry::core::CHANGE,
            &serde_json::json!({"edits":[{"file":"src/a.rs"},{"file":"project/.env"}]}),
        );
        let decision = p.evaluate_subjects("change", &subjects);
        assert_eq!(decision.action, PermissionAction::Prompt);
    }

    #[test]
    fn layered_policy_is_monotonic_and_session_deny_can_tighten_lex_allow() {
        let layered = LayeredPermissionPolicy {
            lex: policy(&[(
                crate::tool_registry::core::BASH,
                ToolPermissionRule::Action(PermissionAction::Allow),
            )]),
            session: policy(&[(
                crate::tool_registry::core::BASH,
                ToolPermissionRule::Action(PermissionAction::Deny),
            )]),
            ..Default::default()
        };
        let subjects = subjects_from_tool_args(
            crate::tool_registry::core::BASH,
            &serde_json::json!({"command":"echo ok"}),
        );
        let decision = layered.evaluate_subjects(crate::tool_registry::core::BASH, &subjects);
        assert_eq!(decision.action, PermissionAction::Deny);
        assert_eq!(decision.layer, Some(PermissionLayer::Session));
    }

    #[test]
    fn layered_policy_session_allow_cannot_loosen_project_prompt() {
        let layered = LayeredPermissionPolicy {
            project: policy(&[(
                crate::tool_registry::core::BASH,
                ToolPermissionRule::Action(PermissionAction::Prompt),
            )]),
            session: policy(&[(
                crate::tool_registry::core::BASH,
                ToolPermissionRule::Action(PermissionAction::Allow),
            )]),
            ..Default::default()
        };
        let subjects = subjects_from_tool_args(
            crate::tool_registry::core::BASH,
            &serde_json::json!({"command":"echo ok"}),
        );
        let decision = layered.evaluate_subjects(crate::tool_registry::core::BASH, &subjects);
        assert_eq!(decision.action, PermissionAction::Prompt);
        assert_eq!(decision.layer, Some(PermissionLayer::Project));
    }

    #[test]
    fn layered_policy_denies_override_lower_allows() {
        let layered = LayeredPermissionPolicy {
            lex: policy(&[(
                crate::tool_registry::core::BASH,
                ToolPermissionRule::Action(PermissionAction::Deny),
            )]),
            session: policy(&[(
                crate::tool_registry::core::BASH,
                ToolPermissionRule::Action(PermissionAction::Allow),
            )]),
            ..Default::default()
        };
        let subjects = subjects_from_tool_args(
            crate::tool_registry::core::BASH,
            &serde_json::json!({"command":"echo ok"}),
        );
        let decision = layered.evaluate_subjects(crate::tool_registry::core::BASH, &subjects);
        assert_eq!(decision.action, PermissionAction::Deny);
        assert_eq!(decision.layer, Some(PermissionLayer::Lex));
    }

    #[test]
    fn styrene_role_mapping_gates_write_and_terminal() {
        assert!(!styrene_role_allows_tool(
            styrene_rbac::Role::Monitor,
            crate::tool_registry::core::WRITE,
        ));
        assert!(styrene_role_allows_tool(
            styrene_rbac::Role::Operator,
            crate::tool_registry::core::WRITE,
        ));
        assert!(!styrene_role_allows_tool(
            styrene_rbac::Role::Monitor,
            crate::tool_registry::core::BASH,
        ));
        assert!(styrene_role_allows_tool(
            styrene_rbac::Role::Operator,
            crate::tool_registry::core::BASH,
        ));
    }

    #[test]
    fn styrene_role_is_ceiling_over_project_policy_allow() {
        let layered = LayeredPermissionPolicy {
            project: policy(&[(
                crate::tool_registry::core::WRITE,
                ToolPermissionRule::Action(PermissionAction::Allow),
            )]),
            ..Default::default()
        };
        let subjects = subjects_from_tool_args(
            crate::tool_registry::core::WRITE,
            &serde_json::json!({"path":"src/lib.rs"}),
        );
        assert_eq!(
            layered
                .evaluate_subjects(crate::tool_registry::core::WRITE, &subjects)
                .action,
            PermissionAction::Allow
        );
        assert!(!styrene_role_allows_tool(
            styrene_rbac::Role::Monitor,
            crate::tool_registry::core::WRITE,
        ));
    }

    #[test]
    fn settings_snapshot_carries_project_tools_and_role() {
        let mut settings = crate::settings::Settings::default();
        settings.permissions.tools.insert(
            crate::tool_registry::core::BASH.to_string(),
            ToolPermissionRule::Action(PermissionAction::Deny),
        );
        settings.permissions.role = Some("monitor".into());
        let layered = layered_policy_from_settings(&settings);
        let subjects = subjects_from_tool_args(
            crate::tool_registry::core::BASH,
            &serde_json::json!({"command":"echo ok"}),
        );
        assert_eq!(
            layered
                .evaluate_subjects(crate::tool_registry::core::BASH, &subjects)
                .action,
            PermissionAction::Deny
        );
        assert_eq!(
            styrene_role_from_settings(&settings),
            Some(styrene_rbac::Role::Monitor)
        );
    }

    #[test]
    fn wildcard_patterns_match_paths_and_commands() {
        assert!(wildcard_match("*.env", "project/.env"));
        assert!(wildcard_match("sudo *", "sudo systemctl restart x"));
        assert!(wildcard_match("secret-??.pem", "secret-01.pem"));
        assert!(!wildcard_match("secret-??.pem", "secret-001.pem"));
    }
}
