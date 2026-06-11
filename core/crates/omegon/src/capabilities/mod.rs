//! Capability graph read models for assistant/console surfaces.
//!
//! These projections are read-only DTOs over existing substrate such as extension
//! manifests/state, Armory catalog entries, skills, OpenAPI tools, and evidence.
//! Keep domain parsing here so CLI, ACP, HTTP APIs, and future Dioxus surfaces do
//! not each reinterpret capability metadata differently.

pub mod agents;
pub mod armory;
pub mod extensions;
pub mod inventory;
pub mod profiles;
pub mod secrets;
