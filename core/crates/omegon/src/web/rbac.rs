//! RBAC helpers for the web/backend API surface.
//!
//! This module keeps authorization error shape and tracing consistent while the
//! current implementation maps precise `omegon.*` capabilities onto the coarse
//! `styrene-rbac` base lattice.

use axum::http::StatusCode;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Default)]
pub struct RbacContext<'a> {
    pub route: &'static str,
    pub session_id: Option<&'a str>,
    pub action_id: Option<&'a str>,
    pub assistant_profile_id: Option<&'a str>,
    pub client_id: Option<&'a str>,
    pub daemon_event_id: Option<&'a str>,
    pub trigger_kind: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub enum RbacError {
    Unauthorized,
    InvalidRole {
        role: String,
    },
    Forbidden {
        role: styrene_rbac::Role,
        operation: omegon_rbac::OmegonOperation,
    },
    Misconfigured {
        operation: &'static str,
    },
    PolicyUnavailable {
        reason: &'static str,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct RbacErrorResponse {
    pub schema_version: u8,
    pub error: &'static str,
    pub reason: &'static str,
    pub operation: Option<&'static str>,
    pub capability: Option<&'static str>,
    pub required_base: Option<&'static str>,
    pub role: Option<&'static str>,
    pub mode: &'static str,
}

impl RbacError {
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::InvalidRole { .. } => StatusCode::BAD_REQUEST,
            Self::Forbidden { .. } => StatusCode::FORBIDDEN,
            Self::Misconfigured { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::PolicyUnavailable { .. } => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    pub fn response(&self) -> RbacErrorResponse {
        match self {
            Self::Unauthorized => RbacErrorResponse {
                schema_version: 1,
                error: "unauthorized",
                reason: "missing_or_invalid_token",
                operation: None,
                capability: None,
                required_base: None,
                role: None,
                mode: "styrene-mapped",
            },
        RbacError::InvalidRole { role } => RbacErrorResponse {
                schema_version: 1,
                error: "invalid_role",
                reason: if role == "missing" {
                    "missing_role"
                } else {
                    "unknown_role"
                },
                operation: None,
                capability: None,
                required_base: None,
                role: None,
                mode: "styrene-mapped",
            },
            Self::Forbidden { role, operation } => RbacErrorResponse {
                schema_version: 1,
                error: "forbidden",
                reason: "capability_not_granted",
                operation: Some(operation.id()),
                capability: Some(operation.capability()),
                required_base: operation.styrene_base(),
                role: Some(role.as_str()),
                mode: "styrene-mapped",
            },
            Self::Misconfigured { operation } => RbacErrorResponse {
                schema_version: 1,
                error: "rbac_misconfigured",
                reason: "operation_missing_capability",
                operation: Some(operation),
                capability: None,
                required_base: None,
                role: None,
                mode: "styrene-mapped",
            },
            Self::PolicyUnavailable { reason } => RbacErrorResponse {
                schema_version: 1,
                error: "policy_unavailable",
                reason,
                operation: None,
                capability: None,
                required_base: None,
                role: None,
                mode: "styrene-mapped",
            },
        }
    }
}

pub fn parse_control_role(label: Option<&str>) -> Result<styrene_rbac::Role, RbacError> {
    let label = label.unwrap_or("admin");
    omegon_rbac::role_from_control_label(label).ok_or_else(|| RbacError::InvalidRole {
        role: label.to_string(),
    })
}

/// Current local web role. Until signed identities/session claims land, local
/// browser-native requests run as Admin but still pass through the same operation
/// gate so handler wiring and descriptors are stable.
pub fn current_web_role(state: &super::WebState) -> styrene_rbac::Role {
    state.web_role
}

pub fn require_operation(
    role: styrene_rbac::Role,
    operation: omegon_rbac::OmegonOperation,
    ctx: &RbacContext<'_>,
) -> Result<(), RbacError> {
    let Some(required_base) = operation.styrene_base() else {
        tracing::error!(
            rbac.mode = "styrene-mapped",
            rbac.decision = "error",
            rbac.operation = operation.id(),
            http.route = ctx.route,
            "rbac operation missing Styrene base mapping"
        );
        return Err(RbacError::Misconfigured {
            operation: operation.id(),
        });
    };

    if omegon_rbac::role_allows_operation(role, operation) {
        tracing::debug!(
            rbac.mode = "styrene-mapped",
            rbac.decision = "allow",
            rbac.operation = operation.id(),
            rbac.capability = operation.capability(),
            rbac.required_base = required_base,
            rbac.role = role.as_str(),
            http.route = ctx.route,
            session.id = ctx.session_id.unwrap_or(""),
            action.id = ctx.action_id.unwrap_or(""),
            assistant_profile.id = ctx.assistant_profile_id.unwrap_or(""),
            client.id = ctx.client_id.unwrap_or(""),
            daemon_event.id = ctx.daemon_event_id.unwrap_or(""),
            daemon_event.trigger_kind = ctx.trigger_kind.unwrap_or(""),
            "rbac allowed operation"
        );
        Ok(())
    } else {
        tracing::warn!(
            rbac.mode = "styrene-mapped",
            rbac.decision = "deny",
            rbac.operation = operation.id(),
            rbac.capability = operation.capability(),
            rbac.required_base = required_base,
            rbac.role = role.as_str(),
            http.route = ctx.route,
            session.id = ctx.session_id.unwrap_or(""),
            action.id = ctx.action_id.unwrap_or(""),
            assistant_profile.id = ctx.assistant_profile_id.unwrap_or(""),
            client.id = ctx.client_id.unwrap_or(""),
            daemon_event.id = ctx.daemon_event_id.unwrap_or(""),
            daemon_event.trigger_kind = ctx.trigger_kind.unwrap_or(""),
            reason = "capability_not_granted",
            "rbac denied operation"
        );
        Err(RbacError::Forbidden { role, operation })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RbacOperationDescriptor {
    pub id: &'static str,
    pub capability: &'static str,
    pub styrene_base: Option<&'static str>,
    pub allowed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RbacPolicyDescriptor {
    pub schema_version: u8,
    pub mode: &'static str,
    pub role: &'static str,
    pub fine_grained_grants: bool,
    pub operations: Vec<RbacOperationDescriptor>,
    pub warnings: Vec<RbacPolicyWarning>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RbacPolicyWarning {
    pub code: &'static str,
    pub message: &'static str,
}

pub fn policy_descriptor(role: styrene_rbac::Role) -> RbacPolicyDescriptor {
    RbacPolicyDescriptor {
        schema_version: 1,
        mode: "styrene-mapped",
        role: role.as_str(),
        fine_grained_grants: false,
        operations: omegon_rbac::OmegonOperation::ALL
            .iter()
            .map(|operation| RbacOperationDescriptor {
                id: operation.id(),
                capability: operation.capability(),
                styrene_base: operation.styrene_base(),
                allowed: omegon_rbac::role_allows_operation(role, *operation),
            })
            .collect(),
        warnings: vec![RbacPolicyWarning {
            code: "coarse_styrene_mapping",
            message: "Omegon capabilities are mapped to Styrene base capabilities; explicit omegon.* grants are not enforced by styrene-rbac 0.1.0.",
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_can_read_but_not_mutate() {
        let role = styrene_rbac::Role::Monitor;
        assert!(
            require_operation(
                role,
                omegon_rbac::OmegonOperation::SurfaceRead,
                &RbacContext {
                    route: "/api/web/surfaces",
                    ..RbacContext::default()
                },
            )
            .is_ok()
        );
        assert!(matches!(
            require_operation(
                role,
                omegon_rbac::OmegonOperation::NativeSessionAction,
                &RbacContext {
                    route: "/api/sessions/default/actions",
                    ..RbacContext::default()
                },
            ),
            Err(RbacError::Forbidden { .. })
        ));
    }

    #[test]
    fn descriptor_includes_every_operation() {
        let descriptor = policy_descriptor(styrene_rbac::Role::Operator);
        assert_eq!(
            descriptor.operations.len(),
            omegon_rbac::OmegonOperation::ALL.len()
        );
        assert!(
            descriptor
                .operations
                .iter()
                .any(|op| op.id == "native_session.action" && op.allowed)
        );
    }
}
