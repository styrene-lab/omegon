//! Backend endpoint registry shared by protocol adapters.
//!
//! This module is intentionally metadata-only for now: dispatch still lives in
//! the ACP, HTTP, slash, and tool adapters that own transport semantics. The
//! registry gives those adapters one canonical inventory for capability
//! advertisement, documentation, and consistency tests.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendDomain {
    Runtime,
    Lifecycle,
    Provider,
    Extensions,
    Secrets,
    Packages,
    Plans,
    Tasks,
    ExternalTasks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendMutability {
    Read,
    Write,
    Dangerous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendPermission {
    Read,
    Edit,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackendTransport {
    AcpExt {
        method: &'static str,
    },
    Http {
        method: &'static str,
        path: &'static str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BackendEndpoint {
    pub id: &'static str,
    pub version: u32,
    pub domain: BackendDomain,
    pub mutability: BackendMutability,
    pub permission: BackendPermission,
    pub transports: &'static [BackendTransport],
    pub side_effects: &'static [&'static str],
    pub description: &'static str,
}

macro_rules! acp_read_endpoint {
    ($id:literal, $domain:expr, $description:literal) => {
        BackendEndpoint {
            id: $id,
            version: 1,
            domain: $domain,
            mutability: BackendMutability::Read,
            permission: BackendPermission::Read,
            transports: &[BackendTransport::AcpExt { method: $id }],
            side_effects: &[],
            description: $description,
        }
    };
}

macro_rules! acp_write_endpoint {
    ($id:literal, $domain:expr, $permission:expr, $side_effects:expr, $description:literal) => {
        BackendEndpoint {
            id: $id,
            version: 1,
            domain: $domain,
            mutability: BackendMutability::Write,
            permission: $permission,
            transports: &[BackendTransport::AcpExt { method: $id }],
            side_effects: $side_effects,
            description: $description,
        }
    };
}

pub const BACKEND_ENDPOINTS: &[BackendEndpoint] = &[
    acp_read_endpoint!(
        "_runtime/capabilities",
        BackendDomain::Runtime,
        "Versioned inventory of ACP backend surfaces and feature flags."
    ),
    acp_read_endpoint!(
        "_runtime/status",
        BackendDomain::Runtime,
        "Current runtime/session status for reconnecting ACP clients."
    ),
    BackendEndpoint {
        id: "_lifecycle/snapshot",
        version: 1,
        domain: BackendDomain::Lifecycle,
        mutability: BackendMutability::Read,
        permission: BackendPermission::Read,
        transports: &[
            BackendTransport::AcpExt {
                method: "_lifecycle/snapshot",
            },
            BackendTransport::Http {
                method: "GET",
                path: "/api/lifecycle/snapshot",
            },
        ],
        side_effects: &[],
        description: "Joined OpenSpec/design/tasking lifecycle snapshot.",
    },
    BackendEndpoint {
        id: "_lifecycle/design/list",
        version: 1,
        domain: BackendDomain::Lifecycle,
        mutability: BackendMutability::Read,
        permission: BackendPermission::Read,
        transports: &[
            BackendTransport::AcpExt {
                method: "_lifecycle/design/list",
            },
            BackendTransport::Http {
                method: "GET",
                path: "/api/lifecycle/design",
            },
        ],
        side_effects: &[],
        description: "Active design-node list projection.",
    },
    BackendEndpoint {
        id: "_lifecycle/design/get",
        version: 1,
        domain: BackendDomain::Lifecycle,
        mutability: BackendMutability::Read,
        permission: BackendPermission::Read,
        transports: &[
            BackendTransport::AcpExt {
                method: "_lifecycle/design/get",
            },
            BackendTransport::Http {
                method: "GET",
                path: "/api/lifecycle/design/:id",
            },
        ],
        side_effects: &[],
        description: "Detailed design-node projection by id.",
    },
    BackendEndpoint {
        id: "_lifecycle/design/ready",
        version: 1,
        domain: BackendDomain::Lifecycle,
        mutability: BackendMutability::Read,
        permission: BackendPermission::Read,
        transports: &[
            BackendTransport::AcpExt {
                method: "_lifecycle/design/ready",
            },
            BackendTransport::Http {
                method: "GET",
                path: "/api/lifecycle/design/ready",
            },
        ],
        side_effects: &[],
        description: "Implementation-ready design-node projection.",
    },
    BackendEndpoint {
        id: "_lifecycle/design/blocked",
        version: 1,
        domain: BackendDomain::Lifecycle,
        mutability: BackendMutability::Read,
        permission: BackendPermission::Read,
        transports: &[
            BackendTransport::AcpExt {
                method: "_lifecycle/design/blocked",
            },
            BackendTransport::Http {
                method: "GET",
                path: "/api/lifecycle/design/blocked",
            },
        ],
        side_effects: &[],
        description: "Blocked design-node projection.",
    },
    BackendEndpoint {
        id: "_lifecycle/design/frontier",
        version: 1,
        domain: BackendDomain::Lifecycle,
        mutability: BackendMutability::Read,
        permission: BackendPermission::Read,
        transports: &[
            BackendTransport::AcpExt {
                method: "_lifecycle/design/frontier",
            },
            BackendTransport::Http {
                method: "GET",
                path: "/api/lifecycle/design/frontier",
            },
        ],
        side_effects: &[],
        description: "Design-node frontier projection.",
    },
    acp_read_endpoint!(
        "_provider/status",
        BackendDomain::Provider,
        "Active provider/auth/model readiness."
    ),
    acp_read_endpoint!(
        "_extensions/list",
        BackendDomain::Extensions,
        "Installed and loaded extension inventory."
    ),
    acp_write_endpoint!(
        "_extensions/call",
        BackendDomain::Extensions,
        BackendPermission::Edit,
        &["extension_rpc", "host_action_policy_may_apply"],
        "Generic RPC bridge for loaded extensions."
    ),
    acp_read_endpoint!(
        "_packages/list",
        BackendDomain::Packages,
        "Package inventory projection."
    ),
    acp_read_endpoint!(
        "_secrets/capabilities",
        BackendDomain::Secrets,
        "Secret-management capability and safety policy inventory."
    ),
    acp_read_endpoint!(
        "_secrets/list",
        BackendDomain::Secrets,
        "Configured secret recipe metadata without resolving values."
    ),
    acp_read_endpoint!(
        "_secrets/check",
        BackendDomain::Secrets,
        "Secret availability check without returning secret values."
    ),
    acp_write_endpoint!(
        "_secrets/set_value",
        BackendDomain::Secrets,
        BackendPermission::Edit,
        &["keyring_write"],
        "Write-only secret value storage."
    ),
    acp_write_endpoint!(
        "_secrets/set_recipe",
        BackendDomain::Secrets,
        BackendPermission::Edit,
        &["secret_recipe_write"],
        "Advanced secret resolver recipe storage."
    ),
    acp_read_endpoint!("_plans/list", BackendDomain::Plans, "Plan list projection."),
    acp_read_endpoint!(
        "_plans/show",
        BackendDomain::Plans,
        "Plan detail projection."
    ),
    acp_read_endpoint!(
        "_plans/events",
        BackendDomain::Plans,
        "Plan event projection."
    ),
    acp_write_endpoint!(
        "_plans/switch",
        BackendDomain::Plans,
        BackendPermission::Edit,
        &["plan_session_state_mutation"],
        "Switch active plan in session state."
    ),
    acp_write_endpoint!(
        "_plans/detach",
        BackendDomain::Plans,
        BackendPermission::Edit,
        &["plan_session_state_mutation"],
        "Detach active plan from session state."
    ),
    acp_read_endpoint!(
        "_tasks/list",
        BackendDomain::Tasks,
        "Plan task list projection."
    ),
    acp_read_endpoint!(
        "_tasks/show",
        BackendDomain::Tasks,
        "Plan task detail projection."
    ),
    acp_write_endpoint!(
        "_tasks/bind",
        BackendDomain::Tasks,
        BackendPermission::Edit,
        &["task_binding_mutation"],
        "Bind an ACP plan task to a durable task reference."
    ),
    acp_read_endpoint!(
        "_tasks/events",
        BackendDomain::Tasks,
        "Task event projection."
    ),
    acp_write_endpoint!(
        "_external_tasks/import",
        BackendDomain::ExternalTasks,
        BackendPermission::Edit,
        &["session_task_import"],
        "Import external task context into the session."
    ),
];

pub fn all_backend_endpoints() -> &'static [BackendEndpoint] {
    BACKEND_ENDPOINTS
}

pub fn acp_ext_endpoints() -> impl Iterator<Item = &'static BackendEndpoint> {
    BACKEND_ENDPOINTS.iter().filter(|endpoint| {
        endpoint
            .transports
            .iter()
            .any(|transport| matches!(transport, BackendTransport::AcpExt { .. }))
    })
}

pub fn find_by_acp_method(method: &str) -> Option<&'static BackendEndpoint> {
    let canonical = method.strip_prefix('_').unwrap_or(method);
    BACKEND_ENDPOINTS.iter().find(|endpoint| {
        endpoint.transports.iter().any(|transport| match transport {
            BackendTransport::AcpExt { method } => {
                method.strip_prefix('_').unwrap_or(method) == canonical
            }
            BackendTransport::Http { .. } => false,
        })
    })
}

pub fn acp_capability_surfaces_json() -> serde_json::Value {
    let surfaces = acp_ext_endpoints()
        .map(|endpoint| {
            (
                endpoint.id.to_string(),
                serde_json::json!({ "version": endpoint.version }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    serde_json::Value::Object(surfaces)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acp_registry_contains_lifecycle_http_aliases() {
        let snapshot = find_by_acp_method("_lifecycle/snapshot").unwrap();
        assert_eq!(snapshot.domain, BackendDomain::Lifecycle);
        assert!(snapshot.transports.contains(&BackendTransport::Http {
            method: "GET",
            path: "/api/lifecycle/snapshot",
        }));
    }

    #[test]
    fn acp_lookup_accepts_stripped_and_prefixed_methods() {
        assert_eq!(
            find_by_acp_method("_runtime/status").unwrap().id,
            "_runtime/status"
        );
        assert_eq!(
            find_by_acp_method("runtime/status").unwrap().id,
            "_runtime/status"
        );
    }

    #[test]
    fn sensitive_surfaces_declare_mutability_and_side_effects() {
        let set_value = find_by_acp_method("_secrets/set_value").unwrap();
        assert_eq!(set_value.mutability, BackendMutability::Write);
        assert!(set_value.side_effects.contains(&"keyring_write"));

        let secret_list = find_by_acp_method("_secrets/list").unwrap();
        assert_eq!(secret_list.mutability, BackendMutability::Read);
        assert!(secret_list.side_effects.is_empty());
    }

    #[test]
    fn capability_json_is_generated_from_acp_registry() {
        let surfaces = acp_capability_surfaces_json();
        assert_eq!(surfaces["_runtime/status"]["version"], 1);
        assert_eq!(surfaces["_lifecycle/design/frontier"]["version"], 1);
        assert_eq!(surfaces["_external_tasks/import"]["version"], 1);
    }
}
