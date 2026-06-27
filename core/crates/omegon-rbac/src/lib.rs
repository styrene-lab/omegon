//! Omegon/Auspex-specific RBAC vocabulary layered on `styrene-rbac`.
//!
//! `styrene-rbac` owns the generic role lattice and shared mesh/fleet
//! capabilities. This crate owns application-specific capabilities for Omegon's
//! native web backend, Auspex UI, lifecycle, memory, and tool surfaces.

/// Omegon/Auspex capability constants.
pub struct OmegonCapability;

impl OmegonCapability {
    // Native first-party sessions.
    pub const SESSION_READ: &str = "omegon.session.read";
    pub const SESSION_CREATE: &str = "omegon.session.create";
    pub const SESSION_ACTION: &str = "omegon.session.action";
    pub const SESSION_STREAM: &str = "omegon.session.stream";

    // Browser-native semantic surfaces.
    pub const SURFACE_READ: &str = "omegon.surface.read";
    pub const SURFACE_STREAM: &str = "omegon.surface.stream";

    // Assistant profiles and launch readiness.
    pub const ASSISTANT_PROFILE_READ: &str = "omegon.assistant_profile.read";
    pub const ASSISTANT_LAUNCH: &str = "omegon.assistant.launch";

    // Runtime/inventory/status surfaces.
    pub const RUNTIME_STATUS_READ: &str = "omegon.runtime.status.read";
    pub const PROVIDER_STATUS_READ: &str = "omegon.provider.status.read";
    pub const EXTENSION_STATUS_READ: &str = "omegon.extension.status.read";
    pub const EVENT_READ: &str = "omegon.event.read";
    pub const EVENT_INGRESS: &str = "omegon.event.ingress";

    // Lifecycle/design tree.
    pub const LIFECYCLE_READ: &str = "omegon.lifecycle.read";
    pub const LIFECYCLE_MUTATE: &str = "omegon.lifecycle.mutate";

    // Memory and knowledge.
    pub const MEMORY_READ: &str = "omegon.memory.read";
    pub const MEMORY_MUTATE: &str = "omegon.memory.mutate";

    // Tools / host effects.
    pub const TOOL_READ: &str = "omegon.tool.read";
    pub const TOOL_WRITE: &str = "omegon.tool.write";
    pub const TOOL_EXECUTE: &str = "omegon.tool.execute";
    pub const TOOL_SECRET_MUTATE: &str = "omegon.tool.secret.mutate";
}

pub const ALL_OMEGON_CAPABILITIES: &[&str] = &[
    OmegonCapability::SESSION_READ,
    OmegonCapability::SESSION_CREATE,
    OmegonCapability::SESSION_ACTION,
    OmegonCapability::SESSION_STREAM,
    OmegonCapability::SURFACE_READ,
    OmegonCapability::SURFACE_STREAM,
    OmegonCapability::ASSISTANT_PROFILE_READ,
    OmegonCapability::ASSISTANT_LAUNCH,
    OmegonCapability::RUNTIME_STATUS_READ,
    OmegonCapability::PROVIDER_STATUS_READ,
    OmegonCapability::EXTENSION_STATUS_READ,
    OmegonCapability::EVENT_READ,
    OmegonCapability::EVENT_INGRESS,
    OmegonCapability::LIFECYCLE_READ,
    OmegonCapability::LIFECYCLE_MUTATE,
    OmegonCapability::MEMORY_READ,
    OmegonCapability::MEMORY_MUTATE,
    OmegonCapability::TOOL_READ,
    OmegonCapability::TOOL_WRITE,
    OmegonCapability::TOOL_EXECUTE,
    OmegonCapability::TOOL_SECRET_MUTATE,
];

pub fn is_omegon_capability(capability: &str) -> bool {
    ALL_OMEGON_CAPABILITIES.contains(&capability)
}

/// Return the Styrene base capability associated with an Omegon capability.
///
/// This keeps enforcement compatible with `styrene-rbac` 0.1.0, whose
/// `RosterEntry` validates grants against its own fixed vocabulary. The more
/// precise Omegon strings remain local metadata until Styrene grows a typed
/// custom namespace / dynamic capability registry.
pub fn styrene_base_for_omegon(capability: &str) -> Option<&'static str> {
    match capability {
        OmegonCapability::SESSION_READ
        | OmegonCapability::SESSION_STREAM
        | OmegonCapability::SURFACE_READ
        | OmegonCapability::SURFACE_STREAM
        | OmegonCapability::ASSISTANT_PROFILE_READ
        | OmegonCapability::RUNTIME_STATUS_READ
        | OmegonCapability::PROVIDER_STATUS_READ
        | OmegonCapability::EXTENSION_STATUS_READ
        | OmegonCapability::EVENT_READ
        | OmegonCapability::LIFECYCLE_READ
        | OmegonCapability::MEMORY_READ
        | OmegonCapability::TOOL_READ => Some(styrene_rbac::Capability::WEB_READ),

        OmegonCapability::SESSION_CREATE
        | OmegonCapability::SESSION_ACTION
        | OmegonCapability::ASSISTANT_LAUNCH
        | OmegonCapability::EVENT_INGRESS
        | OmegonCapability::LIFECYCLE_MUTATE
        | OmegonCapability::MEMORY_MUTATE
        | OmegonCapability::TOOL_WRITE => Some(styrene_rbac::Capability::WEB_WRITE),

        OmegonCapability::TOOL_EXECUTE => Some(styrene_rbac::Capability::TERMINAL_RESTRICTED),
        OmegonCapability::TOOL_SECRET_MUTATE => Some(styrene_rbac::Capability::RPC_CONFIG_UPDATE),
        _ => None,
    }
}

/// Tool-name to precise Omegon capability mapping.
pub fn capability_for_tool(tool: &str) -> Option<&'static str> {
    match tool {
        "bash" | "terminal" => Some(OmegonCapability::TOOL_EXECUTE),
        "read" | "web_fetch" => Some(OmegonCapability::TOOL_READ),
        "write" | "edit" | "change" => Some(OmegonCapability::TOOL_WRITE),
        "validate" => Some(OmegonCapability::RUNTIME_STATUS_READ),
        "secret_set" | "secret_delete" => Some(OmegonCapability::TOOL_SECRET_MUTATE),
        _ => None,
    }
}

/// Backward-compatible tool-name to Styrene base capability mapping.
pub fn styrene_capability_for_tool(tool: &str) -> Option<&'static str> {
    capability_for_tool(tool).and_then(styrene_base_for_omegon)
}

pub fn role_allows_omegon_capability(role: styrene_rbac::Role, capability: &str) -> bool {
    styrene_base_for_omegon(capability)
        .map(|base| {
            styrene_rbac::RosterEntry::new("00000000000000000000000000000000", role)
                .has_capability(base)
        })
        .unwrap_or(false)
}

pub fn role_allows_tool(role: styrene_rbac::Role, tool: &str) -> bool {
    capability_for_tool(tool)
        .map(|capability| role_allows_omegon_capability(role, capability))
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_capabilities_are_namespaced() {
        for capability in ALL_OMEGON_CAPABILITIES {
            assert!(capability.starts_with("omegon."), "{capability}");
            assert!(is_omegon_capability(capability));
        }
    }

    #[test]
    fn maps_precise_capabilities_to_current_styrene_bases() {
        assert_eq!(
            styrene_base_for_omegon(OmegonCapability::SESSION_READ),
            Some(styrene_rbac::Capability::WEB_READ)
        );
        assert_eq!(
            styrene_base_for_omegon(OmegonCapability::SESSION_ACTION),
            Some(styrene_rbac::Capability::WEB_WRITE)
        );
        assert_eq!(
            styrene_base_for_omegon(OmegonCapability::TOOL_EXECUTE),
            Some(styrene_rbac::Capability::TERMINAL_RESTRICTED)
        );
    }

    #[test]
    fn role_checks_use_styrene_role_lattice() {
        assert!(role_allows_omegon_capability(
            styrene_rbac::Role::Monitor,
            OmegonCapability::SURFACE_READ
        ));
        assert!(!role_allows_omegon_capability(
            styrene_rbac::Role::Monitor,
            OmegonCapability::SESSION_ACTION
        ));
        assert!(role_allows_omegon_capability(
            styrene_rbac::Role::Operator,
            OmegonCapability::SESSION_ACTION
        ));
    }

    #[test]
    fn tool_mapping_preserves_existing_policy_shape() {
        assert_eq!(
            capability_for_tool("bash"),
            Some(OmegonCapability::TOOL_EXECUTE)
        );
        assert_eq!(
            styrene_capability_for_tool("bash"),
            Some(styrene_rbac::Capability::TERMINAL_RESTRICTED)
        );
        assert!(!role_allows_tool(styrene_rbac::Role::Monitor, "bash"));
        assert!(role_allows_tool(styrene_rbac::Role::Operator, "bash"));
        assert!(role_allows_tool(styrene_rbac::Role::Monitor, "read"));
    }
}
