//! RBAC helpers for the web/backend API surface.
//!
//! This module keeps authorization error shape and tracing consistent while the
//! current implementation maps precise `omegon.*` capabilities onto the coarse
//! `styrene-rbac` base lattice.

use axum::http::{HeaderMap, StatusCode};
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
    ProxyIdentityRequired,
    ProxyIdentityMismatch,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebPrincipalIssuer {
    LocalToken,
    TrustedProxy,
    SessionCookie,
    InternalDaemon,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebPrincipal {
    pub subject: String,
    pub display_name: Option<String>,
    pub issuer: WebPrincipalIssuer,
    pub auth_source: String,
    pub role: styrene_rbac::Role,
    pub session_id: Option<String>,
    pub client_id: Option<String>,
}

impl WebPrincipal {
    pub fn from_state(state: &super::WebState) -> Self {
        Self {
            subject: "local-web".to_string(),
            display_name: None,
            issuer: WebPrincipalIssuer::LocalToken,
            auth_source: state.web_auth.source_name().to_string(),
            role: state.web_role,
            session_id: None,
            client_id: None,
        }
    }
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
            Self::Unauthorized | Self::ProxyIdentityRequired => StatusCode::UNAUTHORIZED,
            Self::ProxyIdentityMismatch => StatusCode::FORBIDDEN,
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
            Self::ProxyIdentityRequired => RbacErrorResponse {
                schema_version: 1,
                error: "unauthorized",
                reason: "proxy_identity_required",
                operation: None,
                capability: None,
                required_base: None,
                role: None,
                mode: "styrene-mapped",
            },
            Self::ProxyIdentityMismatch => RbacErrorResponse {
                schema_version: 1,
                error: "forbidden",
                reason: "proxy_identity_mismatch",
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

pub const HEADER_PRINCIPAL_ISSUER: &str = "omegon-principal-issuer";
pub const HEADER_PRINCIPAL_SUBJECT: &str = "omegon-principal-subject";
pub const HEADER_PRINCIPAL_ROLE: &str = "omegon-principal-role";
pub const HEADER_PRINCIPAL_DISPLAY_NAME: &str = "omegon-principal-display-name";
pub const HEADER_PRINCIPAL_SESSION_ID: &str = "omegon-principal-session-id";
pub const HEADER_PRINCIPAL_CLIENT_ID: &str = "omegon-principal-client-id";
pub const HEADER_BACK_URL: &str = "omegon-back-url";
pub const HEADER_AUSPEX_PROXY_IDENTITY_FINGERPRINT: &str = "auspex-proxy-identity-fingerprint";
const TRUSTED_PROXY_ISSUER_AUSPEX: &str = "auspex";

pub fn parse_control_role(label: &str) -> Result<styrene_rbac::Role, RbacError> {
    omegon_rbac::role_from_control_label(label).ok_or_else(|| RbacError::InvalidRole {
        role: label.to_string(),
    })
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    header_str(headers, axum::http::header::AUTHORIZATION.as_str())
        .and_then(|value| value.strip_prefix("Bearer "))
}

fn trusted_proxy_name(headers: &HeaderMap) -> Option<&str> {
    header_str(headers, HEADER_PRINCIPAL_ISSUER)
}

fn is_trusted_proxy(name: &str) -> bool {
    name == TRUSTED_PROXY_ISSUER_AUSPEX
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebProxyIdentityAssertion {
    pub issuer: String,
    pub subject: String,
    pub role: styrene_rbac::Role,
    pub client_id: Option<String>,
    pub back_url: Option<String>,
    pub fingerprint: Option<String>,
}

pub fn proxy_identity_assertion_from_headers(
    headers: &HeaderMap,
) -> Result<Option<WebProxyIdentityAssertion>, RbacError> {
    let Some(issuer) = trusted_proxy_name(headers) else {
        return Ok(None);
    };
    if !is_trusted_proxy(issuer) {
        return Err(RbacError::PolicyUnavailable {
            reason: "untrusted_proxy",
        });
    }
    let subject = header_str(headers, HEADER_PRINCIPAL_SUBJECT)
        .filter(|subject| !subject.trim().is_empty())
        .ok_or(RbacError::PolicyUnavailable {
            reason: "missing_proxy_subject",
        })?;
    let role_label = header_str(headers, HEADER_PRINCIPAL_ROLE)
        .filter(|role| !role.trim().is_empty())
        .ok_or_else(|| RbacError::InvalidRole {
            role: "missing".to_string(),
        })?;
    Ok(Some(WebProxyIdentityAssertion {
        issuer: issuer.to_string(),
        subject: subject.to_string(),
        role: parse_control_role(role_label)?,
        client_id: header_str(headers, HEADER_PRINCIPAL_CLIENT_ID).map(str::to_string),
        back_url: header_str(headers, HEADER_BACK_URL).map(str::to_string),
        fingerprint: header_str(headers, HEADER_AUSPEX_PROXY_IDENTITY_FINGERPRINT)
            .map(str::to_string),
    }))
}

pub fn proxy_identity_assertion_for_state(
    state: &super::WebState,
    headers: &HeaderMap,
) -> Result<Option<WebProxyIdentityAssertion>, RbacError> {
    match proxy_identity_assertion_from_headers(headers) {
        Ok(assertion) => Ok(assertion),
        Err(error) if state.web_authority.require_proxy_identity => match error {
            RbacError::PolicyUnavailable {
                reason: "missing_proxy_subject",
            }
            | RbacError::InvalidRole { .. } => Err(RbacError::ProxyIdentityRequired),
            _ => Err(RbacError::ProxyIdentityMismatch),
        },
        Err(error) => Err(error),
    }
}

pub fn validate_proxy_identity_headers_for_config(
    web_authority: &super::WebAuthorityConfig,
    headers: &HeaderMap,
) -> Result<Option<WebProxyIdentityAssertion>, RbacError> {
    let assertion = proxy_identity_assertion_from_headers(headers).or_else(|error| {
        if web_authority.require_proxy_identity {
            match error {
                RbacError::PolicyUnavailable {
                    reason: "missing_proxy_subject",
                }
                | RbacError::InvalidRole { .. } => Err(RbacError::ProxyIdentityRequired),
                _ => Err(RbacError::ProxyIdentityMismatch),
            }
        } else {
            Err(error)
        }
    })?;
    validate_proxy_identity_assertion_for_config(web_authority, assertion.as_ref())?;
    Ok(assertion)
}

pub fn validate_proxy_identity_headers(
    state: &super::WebState,
    headers: &HeaderMap,
) -> Result<Option<WebProxyIdentityAssertion>, RbacError> {
    validate_proxy_identity_headers_for_config(&state.web_authority, headers)
}

pub fn validate_proxy_identity_assertion_for_config(
    web_authority: &super::WebAuthorityConfig,
    assertion: Option<&WebProxyIdentityAssertion>,
) -> Result<(), RbacError> {
    if !web_authority.require_proxy_identity {
        return Ok(());
    }
    let Some(assertion) = assertion else {
        return Err(RbacError::ProxyIdentityRequired);
    };
    let Some(trusted) = web_authority.trusted_proxy.as_ref() else {
        return Err(RbacError::ProxyIdentityRequired);
    };
    if assertion.issuer != TRUSTED_PROXY_ISSUER_AUSPEX
        || assertion.subject != trusted.subject
        || assertion.fingerprint.as_deref() != Some(trusted.fingerprint.as_str())
    {
        return Err(RbacError::ProxyIdentityMismatch);
    }
    Ok(())
}

pub fn validate_proxy_identity_assertion(
    state: &super::WebState,
    assertion: Option<&WebProxyIdentityAssertion>,
) -> Result<(), RbacError> {
    validate_proxy_identity_assertion_for_config(&state.web_authority, assertion)
}

pub fn principal_from_headers(
    state: &super::WebState,
    headers: &HeaderMap,
) -> Result<WebPrincipal, RbacError> {
    let token = bearer_token(headers);
    if !state.web_auth.verify_query_token(token) {
        return Err(RbacError::Unauthorized);
    }

    let assertion = validate_proxy_identity_headers(state, headers)?;

    let Some(assertion) = assertion else {
        return Ok(WebPrincipal::from_state(state));
    };

    Ok(WebPrincipal {
        subject: assertion.subject,
        display_name: header_str(headers, HEADER_PRINCIPAL_DISPLAY_NAME).map(str::to_string),
        issuer: WebPrincipalIssuer::TrustedProxy,
        auth_source: format!("trusted-proxy:{}", assertion.issuer),
        role: assertion.role,
        session_id: header_str(headers, HEADER_PRINCIPAL_SESSION_ID).map(str::to_string),
        client_id: assertion.client_id,
    })
}

pub fn role_to_control_role(role: styrene_rbac::Role) -> crate::control_actions::ControlRole {
    match role {
        styrene_rbac::Role::Monitor => crate::control_actions::ControlRole::Read,
        styrene_rbac::Role::Operator => crate::control_actions::ControlRole::Edit,
        styrene_rbac::Role::Admin => crate::control_actions::ControlRole::Admin,
        _ => crate::control_actions::ControlRole::Read,
    }
}

pub fn current_web_principal(state: &super::WebState) -> WebPrincipal {
    WebPrincipal::from_state(state)
}

/// Current local web role. Until signed identities/session claims land, local
/// browser-native requests run as the configured web role but still pass through
/// the same operation gate so handler wiring and descriptors are stable.
pub fn current_web_role(state: &super::WebState) -> styrene_rbac::Role {
    current_web_principal(state).role
}

pub fn require_principal_operation(
    principal: &WebPrincipal,
    operation: omegon_rbac::OmegonOperation,
    ctx: &RbacContext<'_>,
) -> Result<(), RbacError> {
    let Some(required_base) = operation.styrene_base() else {
        tracing::error!(
            rbac.mode = "styrene-mapped",
            rbac.decision = "error",
            rbac.operation = operation.id(),
            principal.subject = principal.subject.as_str(),
            principal.issuer = ?principal.issuer,
            principal.auth_source = principal.auth_source.as_str(),
            http.route = ctx.route,
            "rbac operation missing Styrene base mapping"
        );
        return Err(RbacError::Misconfigured {
            operation: operation.id(),
        });
    };

    if omegon_rbac::role_allows_operation(principal.role, operation) {
        tracing::debug!(
            rbac.mode = "styrene-mapped",
            rbac.decision = "allow",
            rbac.operation = operation.id(),
            rbac.capability = operation.capability(),
            rbac.required_base = required_base,
            rbac.role = principal.role.as_str(),
            principal.subject = principal.subject.as_str(),
            principal.issuer = ?principal.issuer,
            principal.auth_source = principal.auth_source.as_str(),
            principal.session_id = principal.session_id.as_deref().unwrap_or(""),
            principal.client_id = principal.client_id.as_deref().unwrap_or(""),
            http.route = ctx.route,
            session.id = ctx.session_id.unwrap_or(""),
            action.id = ctx.action_id.unwrap_or(""),
            assistant_profile.id = ctx.assistant_profile_id.unwrap_or(""),
            client.id = ctx.client_id.unwrap_or(""),
            daemon_event.id = ctx.daemon_event_id.unwrap_or(""),
            daemon_event.trigger_kind = ctx.trigger_kind.unwrap_or(""),
            "rbac allowed operation for principal"
        );
        Ok(())
    } else {
        tracing::warn!(
            rbac.mode = "styrene-mapped",
            rbac.decision = "deny",
            rbac.operation = operation.id(),
            rbac.capability = operation.capability(),
            rbac.required_base = required_base,
            rbac.role = principal.role.as_str(),
            principal.subject = principal.subject.as_str(),
            principal.issuer = ?principal.issuer,
            principal.auth_source = principal.auth_source.as_str(),
            principal.session_id = principal.session_id.as_deref().unwrap_or(""),
            principal.client_id = principal.client_id.as_deref().unwrap_or(""),
            http.route = ctx.route,
            session.id = ctx.session_id.unwrap_or(""),
            action.id = ctx.action_id.unwrap_or(""),
            assistant_profile.id = ctx.assistant_profile_id.unwrap_or(""),
            client.id = ctx.client_id.unwrap_or(""),
            daemon_event.id = ctx.daemon_event_id.unwrap_or(""),
            daemon_event.trigger_kind = ctx.trigger_kind.unwrap_or(""),
            reason = "capability_not_granted",
            "rbac denied operation for principal"
        );
        Err(RbacError::Forbidden {
            role: principal.role,
            operation,
        })
    }
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

    fn test_state() -> super::super::WebState {
        super::super::WebState::with_auth_state(
            super::super::DashboardHandles::default(),
            tokio::sync::broadcast::channel(1).0,
            super::super::auth::WebAuthState::ephemeral_generated("test".to_string()),
        )
    }

    fn bearer_headers(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        headers
    }

    #[test]
    fn web_principal_uses_configured_local_role_and_auth_source() {
        let state = test_state();
        let principal = WebPrincipal::from_state(&state);

        assert_eq!(principal.subject, "local-web");
        assert_eq!(principal.issuer, WebPrincipalIssuer::LocalToken);
        assert_eq!(principal.auth_source, "generated");
        assert_eq!(principal.role, styrene_rbac::Role::Admin);
    }

    #[test]
    fn principal_from_headers_accepts_local_bearer_token() {
        let state = test_state();
        let principal = principal_from_headers(&state, &bearer_headers("test"))
            .expect("local bearer principal");

        assert_eq!(principal.subject, "local-web");
        assert_eq!(principal.issuer, WebPrincipalIssuer::LocalToken);
        assert_eq!(principal.role, styrene_rbac::Role::Admin);
    }

    #[test]
    fn principal_from_headers_rejects_invalid_bearer_token() {
        let state = test_state();

        assert!(matches!(
            principal_from_headers(&state, &bearer_headers("wrong")),
            Err(RbacError::Unauthorized)
        ));
    }

    #[test]
    fn principal_from_headers_ignores_stray_role_without_proxy_marker() {
        let state = test_state();
        let mut headers = bearer_headers("test");
        headers.insert(
            "omegon-principal-role",
            axum::http::HeaderValue::from_static("admin"),
        );

        let principal = principal_from_headers(&state, &headers).expect("local bearer principal");

        assert_eq!(principal.issuer, WebPrincipalIssuer::LocalToken);
        assert_eq!(principal.subject, "local-web");
    }

    #[test]
    fn principal_from_headers_accepts_trusted_proxy_identity() {
        let state = test_state();
        let mut headers = bearer_headers("test");
        headers.insert(
            "omegon-principal-issuer",
            axum::http::HeaderValue::from_static("auspex"),
        );
        headers.insert(
            "omegon-principal-subject",
            axum::http::HeaderValue::from_static("user:alice"),
        );
        headers.insert(
            "omegon-principal-role",
            axum::http::HeaderValue::from_static("operator"),
        );
        headers.insert(
            "omegon-principal-display-name",
            axum::http::HeaderValue::from_static("Alice"),
        );
        headers.insert(
            "omegon-principal-session-id",
            axum::http::HeaderValue::from_static("s-1"),
        );

        let principal = principal_from_headers(&state, &headers).expect("proxy principal");

        assert_eq!(principal.issuer, WebPrincipalIssuer::TrustedProxy);
        assert_eq!(principal.subject, "user:alice");
        assert_eq!(principal.display_name.as_deref(), Some("Alice"));
        assert_eq!(principal.auth_source, "trusted-proxy:auspex");
        assert_eq!(principal.role, styrene_rbac::Role::Operator);
        assert_eq!(principal.session_id.as_deref(), Some("s-1"));
    }

    #[test]
    fn principal_from_headers_rejects_invalid_proxy_identity() {
        let state = test_state();
        let mut untrusted = bearer_headers("test");
        untrusted.insert(
            "omegon-principal-issuer",
            axum::http::HeaderValue::from_static("evil"),
        );
        assert!(matches!(
            principal_from_headers(&state, &untrusted),
            Err(RbacError::PolicyUnavailable {
                reason: "untrusted_proxy"
            })
        ));

        let mut missing_subject = bearer_headers("test");
        missing_subject.insert(
            "omegon-principal-issuer",
            axum::http::HeaderValue::from_static("auspex"),
        );
        missing_subject.insert(
            "omegon-principal-role",
            axum::http::HeaderValue::from_static("operator"),
        );
        assert!(matches!(
            principal_from_headers(&state, &missing_subject),
            Err(RbacError::PolicyUnavailable {
                reason: "missing_proxy_subject"
            })
        ));

        let mut invalid_role = bearer_headers("test");
        invalid_role.insert(
            "omegon-principal-issuer",
            axum::http::HeaderValue::from_static("auspex"),
        );
        invalid_role.insert(
            "omegon-principal-subject",
            axum::http::HeaderValue::from_static("user:alice"),
        );
        invalid_role.insert(
            "omegon-principal-role",
            axum::http::HeaderValue::from_static("root"),
        );
        assert!(matches!(
            principal_from_headers(&state, &invalid_role),
            Err(RbacError::InvalidRole { role }) if role == "root"
        ));
    }

    #[test]
    fn strict_proxy_identity_requires_configured_subject_and_fingerprint() {
        let mut state = test_state().with_web_authority(super::super::WebAuthorityConfig {
            trusted_proxy: Some(super::super::WebTrustedProxyIdentity {
                schema_version: 1,
                subject: "auspex-local".to_string(),
                fingerprint: "fp-123".to_string(),
                strict_daemon_identity: true,
            }),
            require_proxy_identity: true,
        });

        let mut headers = bearer_headers("test");
        headers.insert(
            HEADER_PRINCIPAL_ISSUER,
            axum::http::HeaderValue::from_static("auspex"),
        );
        headers.insert(
            HEADER_PRINCIPAL_SUBJECT,
            axum::http::HeaderValue::from_static("auspex-local"),
        );
        headers.insert(
            HEADER_PRINCIPAL_ROLE,
            axum::http::HeaderValue::from_static("monitor"),
        );
        headers.insert(
            HEADER_AUSPEX_PROXY_IDENTITY_FINGERPRINT,
            axum::http::HeaderValue::from_static("fp-123"),
        );

        let principal = principal_from_headers(&state, &headers).expect("strict proxy principal");
        assert_eq!(principal.issuer, WebPrincipalIssuer::TrustedProxy);
        assert_eq!(principal.subject, "auspex-local");
        assert_eq!(principal.role, styrene_rbac::Role::Monitor);

        headers.insert(
            HEADER_AUSPEX_PROXY_IDENTITY_FINGERPRINT,
            axum::http::HeaderValue::from_static("wrong"),
        );
        assert!(matches!(
            principal_from_headers(&state, &headers),
            Err(RbacError::ProxyIdentityMismatch)
        ));

        state.web_authority.trusted_proxy = None;
        assert!(matches!(
            principal_from_headers(&state, &bearer_headers("test")),
            Err(RbacError::ProxyIdentityRequired)
        ));
    }

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
