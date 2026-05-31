//! Nex profiles — deterministic sandbox isolation for Omegon agents.
//!
//! A Nex profile is a declarative environment specification that materializes
//! as an OCI container. Agents spawned with a Nex profile run inside the
//! container with scoped filesystem, network, and tool access.
//!
//! # Architecture
//!
//! ```text
//! NexManifest (TOML)  ──parse──→  NexProfile (Rust types)
//!                                      │
//!                     ┌────────────────┼────────────────┐
//!                     ↓                ↓                ↓
//!               NexRegistry      materialize()    bind_identity()
//!            (lookup by name)   (→ podman run)   (→ RuntimeIdentity)
//! ```
//!
//! Profiles are deterministic: same manifest content → same profile hash →
//! same OCI image. Identity binding links a profile to its creator via
//! Styrene Identity (when available) or local-operator placeholders.

pub mod capabilities;
pub mod compose;
mod container;
mod manifest;
mod profile;
mod registry;
pub mod spawn;
pub mod substrate;

pub use container::materialize_container;
pub use manifest::NexManifest;
pub use profile::{
    NexCapabilities, NexDomain, NexEgressFilter, NexIdentityBinding, NexNetworkPolicy, NexOverlay,
    NexPortMapping, NexPortProtocol, NexProfile, NexResourceLimits,
};
pub use registry::NexRegistry;
pub use spawn::{detect_container_runtime_public, spawn_containerized_child_agent};
