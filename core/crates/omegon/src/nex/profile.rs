//! Core Nex profile types.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A Nex profile — declarative environment specification for agent sandboxing.
///
/// Deterministic: same profile_hash = same OCI image.
/// Identity-bound: signed_by links to a Styrene Identity principal.
/// Materializable: resolves to an OCI image reference for container execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NexProfile {
    /// Human-readable name (e.g., "coding-python", "infra-k8s-prod").
    pub name: String,

    /// Content-addressed hash of the profile manifest (SHA-256).
    /// Same hash = same environment. Computed from the canonicalized manifest.
    pub profile_hash: String,

    /// Base domain from nix/profiles.nix this inherits from.
    pub base_domain: NexDomain,

    /// Additional package layers on top of the base domain.
    #[serde(default)]
    pub overlays: Vec<NexOverlay>,

    /// Resource constraints for the container.
    #[serde(default)]
    pub resource_limits: NexResourceLimits,

    /// Capability grants — what this profile is allowed to do.
    #[serde(default)]
    pub capabilities: NexCapabilities,

    /// OCI image reference. Populated after build/resolve.
    /// e.g. "ghcr.io/styrene-lab/omegon-coding-python:0.17.6"
    #[serde(default)]
    pub image_ref: Option<String>,

    /// Identity binding — who created/signed this profile.
    #[serde(default)]
    pub signed_by: Option<NexIdentityBinding>,
}

/// Base domain — maps to nix/profiles.nix domain definitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NexDomain {
    Chat,
    Coding,
    CodingPython,
    CodingNode,
    CodingRust,
    Infra,
    Full,
    Custom(String),
}

impl NexDomain {
    /// Default OCI image tag suffix for this domain.
    pub fn image_suffix(&self) -> &str {
        match self {
            Self::Chat => "omegon-chat",
            Self::Coding => "omegon",
            Self::CodingPython => "omegon-coding-python",
            Self::CodingNode => "omegon-coding-node",
            Self::CodingRust => "omegon-coding-rust",
            Self::Infra => "omegon-infra",
            Self::Full => "omegon-full",
            Self::Custom(name) => name.as_str(),
        }
    }

    /// Resolve to a default image reference for a given version.
    pub fn default_image_ref(&self, registry: &str, version: &str) -> String {
        format!("{}/{}:{}", registry, self.image_suffix(), version)
    }
}

impl std::fmt::Display for NexDomain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Chat => write!(f, "chat"),
            Self::Coding => write!(f, "coding"),
            Self::CodingPython => write!(f, "coding-python"),
            Self::CodingNode => write!(f, "coding-node"),
            Self::CodingRust => write!(f, "coding-rust"),
            Self::Infra => write!(f, "infra"),
            Self::Full => write!(f, "full"),
            Self::Custom(name) => write!(f, "{name}"),
        }
    }
}

/// Named package overlay layered on top of the base domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NexOverlay {
    pub name: String,
    /// Nix packages to add (e.g., "python312Packages.torch").
    #[serde(default)]
    pub packages: Vec<String>,
}

/// Container resource constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NexResourceLimits {
    /// Memory limit in megabytes. None = unlimited.
    #[serde(default)]
    pub memory_mb: Option<u64>,

    /// CPU shares (relative weight). None = fair share.
    #[serde(default)]
    pub cpu_shares: Option<u64>,

    /// Maximum number of processes. None = unlimited.
    #[serde(default)]
    pub pids_limit: Option<u32>,

    /// Mount the root filesystem read-only. Mounted volumes remain writable.
    #[serde(default = "default_true")]
    pub readonly_rootfs: bool,
}

impl Default for NexResourceLimits {
    fn default() -> Self {
        Self {
            memory_mb: None,
            cpu_shares: None,
            pids_limit: None,
            readonly_rootfs: true,
        }
    }
}

/// Network isolation policy — graduated from total isolation to full host access.
///
/// Replaces the old binary `network_access` capability + `NexNetworkMode` pair
/// with a single coherent policy that supports filtered egress.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "policy", rename_all = "lowercase")]
pub enum NexNetworkPolicy {
    /// No network stack at all (default — safest for agent sandboxing).
    /// Maps to `--network=none`.
    #[default]
    Isolated,

    /// Outbound-only bridge network with optional domain/port filtering.
    /// No inbound connections. Unfiltered egress if `filter` is None.
    /// Maps to `--network=bridge` + optional iptables rules.
    Egress {
        #[serde(default)]
        filter: Option<NexEgressFilter>,
    },

    /// Standard bridge network with optional inbound port mappings.
    /// Full outbound + selective inbound.
    /// Maps to `--network=bridge` + `--publish` per port.
    Bridge {
        #[serde(default)]
        ports: Vec<NexPortMapping>,
    },

    /// Host network namespace — maximum access, minimum isolation.
    /// Maps to `--network=host`. Use only when bridge is insufficient.
    Host,

    /// Named custom podman/docker network.
    Custom(String),
}

impl NexNetworkPolicy {
    /// Container `--network=` flag value.
    pub fn network_flag(&self) -> &str {
        match self {
            Self::Isolated => "none",
            Self::Egress { .. } => "bridge",
            Self::Bridge { .. } => "bridge",
            Self::Host => "host",
            Self::Custom(name) => name.as_str(),
        }
    }

    /// Whether this policy grants any outbound network access.
    pub fn has_network_access(&self) -> bool {
        !matches!(self, Self::Isolated)
    }

    /// Human-readable label for status display.
    pub fn display_label(&self) -> &str {
        match self {
            Self::Isolated => "isolated",
            Self::Egress { filter: Some(_) } => "egress (filtered)",
            Self::Egress { filter: None } => "egress",
            Self::Bridge { .. } => "bridge",
            Self::Host => "host",
            Self::Custom(_) => "custom",
        }
    }
}

impl std::fmt::Display for NexNetworkPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_label())
    }
}

/// Egress filter — restrict outbound connections to specific destinations.
///
/// When attached to an `Egress` policy, only traffic matching at least one
/// allow rule is permitted. Everything else is dropped via iptables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NexEgressFilter {
    /// Allowed destination hostnames/domains.
    /// Supports leading wildcard: `*.example.com` matches `api.example.com`.
    /// Resolved to IPs at container start via DNS.
    #[serde(default)]
    pub allow_hosts: Vec<String>,

    /// Allowed destination CIDRs (e.g., `10.0.0.0/8`, `203.0.113.0/24`).
    #[serde(default)]
    pub allow_cidrs: Vec<String>,

    /// Allowed destination ports. Empty = all ports allowed.
    #[serde(default)]
    pub allow_ports: Vec<u16>,

    /// Block RFC1918 private ranges (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16).
    /// Applied after allow rules — an explicit allow_cidrs entry can override.
    #[serde(default = "default_true")]
    pub deny_private: bool,

    /// Block cloud metadata endpoints (169.254.169.254, fd00:ec2::254).
    /// Prevents SSRF-style credential theft from cloud instances.
    #[serde(default = "default_true")]
    pub deny_metadata: bool,
}

impl Default for NexEgressFilter {
    fn default() -> Self {
        Self {
            allow_hosts: Vec::new(),
            allow_cidrs: Vec::new(),
            allow_ports: Vec::new(),
            deny_private: true,
            deny_metadata: true,
        }
    }
}

/// Port mapping for bridge network mode — exposes container ports to host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NexPortMapping {
    /// Port on the host.
    pub host: u16,
    /// Port inside the container.
    pub container: u16,
    /// Protocol (defaults to TCP).
    #[serde(default)]
    pub protocol: NexPortProtocol,
}

/// Port mapping protocol.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NexPortProtocol {
    #[default]
    Tcp,
    Udp,
}

/// Capability grants scoping what the sandboxed agent can do.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NexCapabilities {
    /// Allow writing to the mounted workspace filesystem.
    #[serde(default = "default_true")]
    pub filesystem_write: bool,

    /// Network isolation policy — graduated from total isolation to full host access.
    /// Defaults to `Isolated` (no network stack).
    #[serde(default)]
    pub network: NexNetworkPolicy,

    /// Mount the operator's current working directory into the container.
    #[serde(default = "default_true")]
    pub mount_cwd: bool,

    /// Additional host paths to mount (read-only unless filesystem_write is true).
    #[serde(default)]
    pub mount_paths: Vec<PathBuf>,

    /// Environment variables to pass through from host to container.
    #[serde(default)]
    pub env_passthrough: Vec<String>,

    /// Allowlist of tools the agent may use. Empty = all tools allowed.
    #[serde(default)]
    pub allowed_tools: Vec<String>,

    /// Denylist of tools the agent may not use. Checked after allowed_tools.
    #[serde(default)]
    pub denied_tools: Vec<String>,
}

impl Default for NexCapabilities {
    fn default() -> Self {
        Self {
            filesystem_write: true,
            network: NexNetworkPolicy::Isolated,
            mount_cwd: true,
            mount_paths: Vec::new(),
            env_passthrough: Vec::new(),
            allowed_tools: Vec::new(),
            denied_tools: Vec::new(),
        }
    }
}

/// Identity binding — links a profile to its creator/signer.
///
/// In Phase 4, `signature` will be populated via Styrene Identity Ed25519.
/// Until then, the principal fields provide traceability without crypto.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NexIdentityBinding {
    /// Principal who created/signed this profile.
    pub principal_id: String,

    /// Identity issuer (e.g., "local-session", "styrene-identity").
    #[serde(default)]
    pub issuer: Option<String>,

    /// Ed25519 signature over the profile manifest (Phase 4+).
    #[serde(default)]
    pub signature: Option<String>,
}

impl NexProfile {
    /// Bind this profile to the current operator identity.
    pub fn bind_identity(&mut self, identity: &crate::settings::RuntimeIdentity) {
        self.signed_by = Some(NexIdentityBinding {
            principal_id: identity
                .principal_id
                .clone()
                .unwrap_or_else(|| "anonymous".into()),
            issuer: identity.issuer.clone(),
            signature: None,
        });
    }

    /// Derive an AuthorizationContext from this profile's capabilities.
    pub fn authorization_context(&self) -> crate::settings::AuthorizationContext {
        let mut caps = Vec::new();
        if self.capabilities.filesystem_write {
            caps.push("fs:write".into());
        }
        match &self.capabilities.network {
            NexNetworkPolicy::Isolated => {}
            NexNetworkPolicy::Egress { filter: Some(_) } => {
                caps.push("net:egress-filtered".into());
            }
            NexNetworkPolicy::Egress { filter: None } => {
                caps.push("net:egress".into());
            }
            NexNetworkPolicy::Bridge { .. } => {
                caps.push("net:bridge".into());
            }
            NexNetworkPolicy::Host => {
                caps.push("net:host".into());
            }
            NexNetworkPolicy::Custom(name) => {
                caps.push(format!("net:custom:{name}"));
            }
        }
        if self.capabilities.mount_cwd {
            caps.push("fs:mount-cwd".into());
        }
        if !self.capabilities.mount_paths.is_empty() {
            caps.push(format!(
                "fs:mount-extra:{}",
                self.capabilities.mount_paths.len()
            ));
        }
        crate::settings::AuthorizationContext {
            roles: vec!["nex-agent".into()],
            capabilities: caps,
            trust_domain: self.signed_by.as_ref().and_then(|b| b.issuer.clone()),
        }
    }
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_image_suffix() {
        assert_eq!(
            NexDomain::CodingPython.image_suffix(),
            "omegon-coding-python"
        );
        assert_eq!(NexDomain::Coding.image_suffix(), "omegon");
    }

    #[test]
    fn default_resource_limits_are_restrictive() {
        let limits = NexResourceLimits::default();
        assert!(limits.readonly_rootfs);
    }

    #[test]
    fn default_capabilities_isolate_network() {
        let caps = NexCapabilities::default();
        assert!(caps.filesystem_write);
        assert_eq!(caps.network, NexNetworkPolicy::Isolated);
        assert!(!caps.network.has_network_access());
        assert!(caps.mount_cwd);
    }

    #[test]
    fn network_policy_flags() {
        assert_eq!(NexNetworkPolicy::Isolated.network_flag(), "none");
        assert_eq!(
            NexNetworkPolicy::Egress { filter: None }.network_flag(),
            "bridge"
        );
        assert_eq!(NexNetworkPolicy::Host.network_flag(), "host");
        assert_eq!(
            NexNetworkPolicy::Bridge { ports: vec![] }.network_flag(),
            "bridge"
        );
        assert_eq!(
            NexNetworkPolicy::Custom("mynet".into()).network_flag(),
            "mynet"
        );
    }

    #[test]
    fn network_policy_access() {
        assert!(!NexNetworkPolicy::Isolated.has_network_access());
        assert!(NexNetworkPolicy::Egress { filter: None }.has_network_access());
        assert!(
            NexNetworkPolicy::Egress {
                filter: Some(NexEgressFilter::default()),
            }
            .has_network_access()
        );
        assert!(NexNetworkPolicy::Bridge { ports: vec![] }.has_network_access());
        assert!(NexNetworkPolicy::Host.has_network_access());
    }

    #[test]
    fn network_policy_display() {
        assert_eq!(NexNetworkPolicy::Isolated.display_label(), "isolated");
        assert_eq!(
            NexNetworkPolicy::Egress { filter: None }.display_label(),
            "egress"
        );
        assert_eq!(
            NexNetworkPolicy::Egress {
                filter: Some(NexEgressFilter::default())
            }
            .display_label(),
            "egress (filtered)"
        );
        assert_eq!(NexNetworkPolicy::Host.display_label(), "host");
    }

    #[test]
    fn egress_filter_defaults_deny_private_and_metadata() {
        let filter = NexEgressFilter::default();
        assert!(filter.deny_private);
        assert!(filter.deny_metadata);
        assert!(filter.allow_hosts.is_empty());
        assert!(filter.allow_ports.is_empty());
    }
}
