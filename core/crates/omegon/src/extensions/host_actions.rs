use omegon_extension::{ExtensionManifest, HostAction, HostActionOutcome, HostActionStatus};
use serde_json::Value;

/// Host-attached origin for an untrusted HostAction candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum HostActionOriginKind {
    NativeExtension,
    Mcp,
    Internal,
}

/// Trusted runtime origin attached by Omegon before policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HostActionOrigin {
    pub kind: HostActionOriginKind,
    pub identity: String,
}

impl HostActionOrigin {
    pub fn native_extension(identity: impl Into<String>) -> Self {
        Self {
            kind: HostActionOriginKind::NativeExtension,
            identity: identity.into(),
        }
    }

    #[allow(dead_code)]
    pub fn mcp(identity: impl Into<String>) -> Self {
        Self {
            kind: HostActionOriginKind::Mcp,
            identity: identity.into(),
        }
    }

    #[allow(dead_code)]
    pub fn internal(identity: impl Into<String>) -> Self {
        Self {
            kind: HostActionOriginKind::Internal,
            identity: identity.into(),
        }
    }
}

/// Session/tool-call scoped action identity. Extension-provided action ids are
/// local labels only; this type is the runtime identity used for policy/audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ScopedHostActionId {
    pub origin: HostActionOrigin,
    pub session_id: String,
    pub tool_call_id: String,
    pub action_id: String,
}

/// Policy gates that are external to the extension manifest.
#[derive(Debug, Clone, Default)]
pub(super) struct RuntimeHostActionPolicy {
    pub project_allows_auto: bool,
    pub runtime_allows_auto: bool,
    pub origin_trusted_for_auto: bool,
    pub operator_approved: bool,
}

/// Minimal executor registry seam for Phase C. Issue #76 registers real
/// `terminal.create@1` execution later.
#[derive(Debug, Clone, Default)]
pub(super) struct HostActionExecutorRegistry {
    supported_types: Vec<String>,
}

impl HostActionExecutorRegistry {
    pub fn with_supported_types(types: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            supported_types: types.into_iter().map(Into::into).collect(),
        }
    }

    fn supports(&self, action_type: &str) -> bool {
        self.supported_types.iter().any(|ty| ty == action_type)
    }
}

pub(super) fn process_host_action_candidate(
    candidate: Value,
    manifest: &ExtensionManifest,
    _scoped_id: ScopedHostActionId,
    runtime_policy: &RuntimeHostActionPolicy,
    executors: &HostActionExecutorRegistry,
) -> HostActionOutcome {
    let action: HostAction = match serde_json::from_value(candidate) {
        Ok(action) => action,
        Err(err) => {
            return outcome(
                "<invalid>",
                HostActionStatus::Invalid,
                "invalid_action",
                format!("invalid HostAction candidate: {err}"),
            );
        }
    };

    if !action.action_type.contains('@') {
        return outcome(
            action.id,
            HostActionStatus::Invalid,
            "invalid_action_type",
            "HostAction type must include an explicit version suffix",
        );
    }

    if !executors.supports(&action.action_type) {
        return outcome(
            action.id,
            HostActionStatus::Unsupported,
            "unsupported_action",
            format!("unsupported HostAction type '{}'", action.action_type),
        );
    }

    if !manifest.allows_host_action_type(&action.action_type) {
        return outcome(
            action.id,
            HostActionStatus::Denied,
            "manifest_denied",
            format!(
                "manifest does not allow HostAction type '{}'",
                action.action_type
            ),
        );
    }

    if matches!(
        action.execution,
        Some(omegon_extension::HostActionExecution::AutoIfAllowed)
    ) && !(runtime_policy.project_allows_auto
        && runtime_policy.runtime_allows_auto
        && runtime_policy.origin_trusted_for_auto
        && runtime_policy.operator_approved)
    {
        return outcome(
            action.id,
            HostActionStatus::Denied,
            "auto_not_allowed",
            "auto_if_allowed requires manifest, project, runtime, origin, and operator approval",
        );
    }

    outcome(
        action.id,
        HostActionStatus::Unsupported,
        "executor_unavailable",
        "HostAction executor registry seam is present, but no executor ran in Phase C",
    )
}

fn outcome(
    action_id: impl Into<String>,
    status: HostActionStatus,
    code: impl Into<String>,
    message: impl Into<String>,
) -> HostActionOutcome {
    HostActionOutcome {
        action_id: action_id.into(),
        status,
        result: None,
        error: Some(omegon_extension::HostActionError {
            code: code.into(),
            message: message.into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest(allowed: &[&str]) -> ExtensionManifest {
        let allowed = allowed
            .iter()
            .map(|allowed| format!("\"{allowed}\""))
            .collect::<Vec<_>>()
            .join(", ");
        toml::from_str(&format!(
            r#"
[extension]
name = "reader"
version = "0.1.0"

[runtime]
type = "native"
binary = "target/release/reader"

[permissions.host_actions]
allowed = [{allowed}]
"#
        ))
        .unwrap()
    }

    fn scoped() -> ScopedHostActionId {
        ScopedHostActionId {
            origin: HostActionOrigin::native_extension("reader"),
            session_id: "session-1".to_string(),
            tool_call_id: "call-1".to_string(),
            action_id: "open-reader".to_string(),
        }
    }

    fn registry() -> HostActionExecutorRegistry {
        HostActionExecutorRegistry::with_supported_types(["terminal.create@1"])
    }

    #[test]
    fn malformed_action_candidate_returns_invalid_outcome() {
        let outcome = process_host_action_candidate(
            json!({"id": "broken", "params": {}}),
            &manifest(&["terminal.create@1"]),
            scoped(),
            &RuntimeHostActionPolicy::default(),
            &registry(),
        );

        assert_eq!(outcome.status, HostActionStatus::Invalid);
        assert_eq!(outcome.error.unwrap().code, "invalid_action");
    }

    #[test]
    fn unversioned_action_type_returns_invalid_outcome() {
        let outcome = process_host_action_candidate(
            json!({"id": "open-reader", "type": "terminal.create", "params": {}}),
            &manifest(&["terminal.create@1"]),
            scoped(),
            &RuntimeHostActionPolicy::default(),
            &registry(),
        );

        assert_eq!(outcome.status, HostActionStatus::Invalid);
        assert_eq!(outcome.error.unwrap().code, "invalid_action_type");
    }

    #[test]
    fn unsupported_action_type_returns_unsupported_outcome() {
        let outcome = process_host_action_candidate(
            json!({"id": "open-file", "type": "file.open@1", "params": {}}),
            &manifest(&["file.open@1"]),
            scoped(),
            &RuntimeHostActionPolicy::default(),
            &registry(),
        );

        assert_eq!(outcome.status, HostActionStatus::Unsupported);
        assert_eq!(outcome.error.unwrap().code, "unsupported_action");
    }

    #[test]
    fn manifest_denied_action_returns_denied_outcome() {
        let outcome = process_host_action_candidate(
            json!({"id": "open-file", "type": "file.open@1", "params": {}}),
            &manifest(&["terminal.create@1"]),
            scoped(),
            &RuntimeHostActionPolicy::default(),
            &HostActionExecutorRegistry::with_supported_types(["file.open@1"]),
        );

        assert_eq!(outcome.status, HostActionStatus::Denied);
        assert_eq!(outcome.error.unwrap().code, "manifest_denied");
    }

    #[test]
    fn auto_if_allowed_is_conservative() {
        let outcome = process_host_action_candidate(
            json!({
                "id": "open-reader",
                "type": "terminal.create@1",
                "execution": "auto_if_allowed",
                "params": {"command": "bookokrat"}
            }),
            &manifest(&["terminal.create@1"]),
            scoped(),
            &RuntimeHostActionPolicy::default(),
            &registry(),
        );

        assert_eq!(outcome.status, HostActionStatus::Denied);
        assert_eq!(outcome.error.unwrap().code, "auto_not_allowed");
    }

    #[test]
    fn scoped_action_ids_preserve_local_id_but_distinguish_origin() {
        let left = ScopedHostActionId {
            origin: HostActionOrigin::native_extension("reader-a"),
            session_id: "session".to_string(),
            tool_call_id: "call".to_string(),
            action_id: "open-reader".to_string(),
        };
        let right = ScopedHostActionId {
            origin: HostActionOrigin::native_extension("reader-b"),
            session_id: "session".to_string(),
            tool_call_id: "call".to_string(),
            action_id: "open-reader".to_string(),
        };

        assert_ne!(left, right);
    }
}
