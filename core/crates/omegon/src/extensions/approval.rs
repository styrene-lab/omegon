use agent_client_protocol::schema::{
    Meta, PermissionOption, PermissionOptionId, PermissionOptionKind, RequestPermissionOutcome,
    RequestPermissionRequest, SessionId, ToolCallId, ToolCallUpdate, ToolCallUpdateFields,
};
use omegon_extension::{HostAction, HostActionError, HostActionOutcome, HostActionStatus};
use serde_json::{Value, json};

use super::host_actions::{HostActionOriginKind, ScopedHostActionId};

pub(super) const HOST_ACTION_APPROVAL_META_KEY: &str = "omegon/hostActionApproval";
pub(super) const ALLOW_ONCE_OPTION_ID: &str = "allow-once";
pub(super) const REJECT_ONCE_OPTION_ID: &str = "reject-once";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HostActionApprovalDecision {
    Approved,
    Rejected,
    Cancelled,
    Unavailable,
}

pub(crate) fn build_host_action_permission_request(
    session_id: impl Into<SessionId>,
    tool_name: &str,
    scoped_id: &ScopedHostActionId,
    action: &HostAction,
    reason: &str,
) -> RequestPermissionRequest {
    let mut meta = Meta::new();
    meta.insert(
        HOST_ACTION_APPROVAL_META_KEY.to_string(),
        host_action_approval_meta(tool_name, scoped_id, action, reason),
    );

    let title = format!("Approve HostAction {}", action.action_type);
    let tool_call = ToolCallUpdate::new(
        ToolCallId::new(scoped_id.tool_call_id.clone()),
        ToolCallUpdateFields::new()
            .title(title)
            .raw_input(Some(json!({
                "kind": "host_action",
                "action": action,
            }))),
    );

    RequestPermissionRequest::new(
        session_id,
        tool_call,
        vec![
            PermissionOption::new(
                PermissionOptionId::new(ALLOW_ONCE_OPTION_ID),
                "Allow once",
                PermissionOptionKind::AllowOnce,
            ),
            PermissionOption::new(
                PermissionOptionId::new(REJECT_ONCE_OPTION_ID),
                "Reject once",
                PermissionOptionKind::RejectOnce,
            ),
        ],
    )
    .meta(Some(meta))
}

fn host_action_approval_meta(
    tool_name: &str,
    scoped_id: &ScopedHostActionId,
    action: &HostAction,
    reason: &str,
) -> Value {
    let (origin, extension, server) = match scoped_id.origin.kind {
        HostActionOriginKind::NativeExtension => (
            "native_extension",
            Some(scoped_id.origin.identity.clone()),
            None,
        ),
        HostActionOriginKind::Mcp => ("mcp", None, Some(scoped_id.origin.identity.clone())),
        HostActionOriginKind::Internal => {
            ("internal", Some(scoped_id.origin.identity.clone()), None)
        }
    };

    json!({
        "kind": "host_action",
        "origin": origin,
        "extension": extension,
        "server": server,
        "tool": tool_name,
        "tool_call_id": scoped_id.tool_call_id,
        "runtime_action_id": scoped_id.action_id,
        "action": action,
        "policy": {
            "execution": "manual",
            "reason": reason,
        }
    })
}

pub(crate) fn decision_from_permission_outcome(
    outcome: RequestPermissionOutcome,
) -> HostActionApprovalDecision {
    match outcome {
        RequestPermissionOutcome::Cancelled => HostActionApprovalDecision::Cancelled,
        RequestPermissionOutcome::Selected(selected) => {
            let id = selected.option_id.0.as_ref();
            if id == ALLOW_ONCE_OPTION_ID {
                HostActionApprovalDecision::Approved
            } else {
                HostActionApprovalDecision::Rejected
            }
        }
        _ => HostActionApprovalDecision::Rejected,
    }
}

pub(crate) fn denied_approval_outcome(
    scoped_id: &ScopedHostActionId,
    action: &HostAction,
    decision: HostActionApprovalDecision,
) -> HostActionOutcome {
    let (code, message) = match decision {
        HostActionApprovalDecision::Approved => unreachable!("approved decision is not denied"),
        HostActionApprovalDecision::Rejected => (
            "operator_denied",
            "HostAction was rejected by the ACP client/operator",
        ),
        HostActionApprovalDecision::Cancelled => (
            "approval_cancelled",
            "HostAction approval request was cancelled before a decision",
        ),
        HostActionApprovalDecision::Unavailable => (
            "approval_unavailable",
            "HostAction requires ACP approval, but no approval channel is available",
        ),
    };

    HostActionOutcome {
        action_id: action.id.clone(),
        status: HostActionStatus::Denied,
        result: Some(json!({
            "origin": match scoped_id.origin.kind {
                HostActionOriginKind::NativeExtension => "native_extension",
                HostActionOriginKind::Mcp => "mcp",
                HostActionOriginKind::Internal => "internal",
            },
            "identity": scoped_id.origin.identity,
            "tool_call_id": scoped_id.tool_call_id,
            "runtime_action_id": scoped_id.action_id,
            "decision": code,
        })),
        error: Some(HostActionError {
            code: code.to_string(),
            message: message.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::{RequestPermissionOutcome, SelectedPermissionOutcome};
    use omegon_extension::HostActionExecution;

    fn scoped(kind: HostActionOriginKind) -> ScopedHostActionId {
        let identity = match kind {
            HostActionOriginKind::NativeExtension => "omegon-reader",
            HostActionOriginKind::Mcp => "reader-mcp",
            HostActionOriginKind::Internal => "internal",
        };
        ScopedHostActionId {
            origin: super::super::host_actions::HostActionOrigin {
                kind,
                identity: identity.into(),
            },
            session_id: "session-1".into(),
            tool_call_id: "call-1".into(),
            action_id: "runtime-open".into(),
        }
    }

    fn action() -> HostAction {
        let mut action = HostAction::new(
            "open-reader",
            "terminal.create@1",
            json!({"command": "bookokrat"}),
        )
        .unwrap();
        action.execution = Some(HostActionExecution::Manual);
        action
    }

    #[test]
    fn permission_request_preserves_native_host_action_candidate_in_meta() {
        let action = action();
        let req = build_host_action_permission_request(
            SessionId::new("sid"),
            "reader_open",
            &scoped(HostActionOriginKind::NativeExtension),
            &action,
            "host action requires approval",
        );

        let meta = req.meta.expect("meta");
        let payload = &meta[HOST_ACTION_APPROVAL_META_KEY];
        assert_eq!(payload["kind"], "host_action");
        assert_eq!(payload["origin"], "native_extension");
        assert_eq!(payload["extension"], "omegon-reader");
        assert_eq!(payload["server"], Value::Null);
        assert_eq!(payload["tool"], "reader_open");
        assert_eq!(payload["tool_call_id"], "call-1");
        assert_eq!(payload["action"]["id"], "open-reader");
        assert_eq!(payload["action"]["type"], "terminal.create@1");
        assert_eq!(payload["action"]["params"]["command"], "bookokrat");
        assert_eq!(req.options.len(), 2);
        assert_eq!(req.options[0].option_id.0.as_ref(), ALLOW_ONCE_OPTION_ID);
        assert_eq!(req.options[1].option_id.0.as_ref(), REJECT_ONCE_OPTION_ID);
    }

    #[test]
    fn permission_request_preserves_mcp_origin_in_meta() {
        let action = action();
        let req = build_host_action_permission_request(
            SessionId::new("sid"),
            "open",
            &scoped(HostActionOriginKind::Mcp),
            &action,
            "mcp action requires approval",
        );
        let payload = &req.meta.expect("meta")[HOST_ACTION_APPROVAL_META_KEY];
        assert_eq!(payload["origin"], "mcp");
        assert_eq!(payload["extension"], Value::Null);
        assert_eq!(payload["server"], "reader-mcp");
        assert_eq!(payload["tool"], "open");
    }

    #[test]
    fn permission_outcome_maps_to_approval_decisions() {
        let allow = RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
            PermissionOptionId::new(ALLOW_ONCE_OPTION_ID),
        ));
        let reject = RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
            PermissionOptionId::new(REJECT_ONCE_OPTION_ID),
        ));

        assert_eq!(
            decision_from_permission_outcome(allow),
            HostActionApprovalDecision::Approved
        );
        assert_eq!(
            decision_from_permission_outcome(reject),
            HostActionApprovalDecision::Rejected
        );
        assert_eq!(
            decision_from_permission_outcome(RequestPermissionOutcome::Cancelled),
            HostActionApprovalDecision::Cancelled
        );
    }

    #[test]
    fn denied_approval_outcome_reports_unavailable_channel() {
        let action = action();
        let outcome = denied_approval_outcome(
            &scoped(HostActionOriginKind::NativeExtension),
            &action,
            HostActionApprovalDecision::Unavailable,
        );
        assert_eq!(outcome.status, HostActionStatus::Denied);
        assert_eq!(outcome.error.unwrap().code, "approval_unavailable");
        assert_eq!(outcome.result.unwrap()["decision"], "approval_unavailable");
    }
}
