//! Lifecycle Engine — design, specification, and decomposition as cognitive modes.
//!
//! The lifecycle is not a feature crate. It's how the agent loop thinks about
//! structured work. Phase detection, ambient capture, design state management,
//! spec validation, and autonomous decomposition all live here.
//!
//! Phase 1a (current): read-only parsing + context injection.
//! Phase 1b: full mutation tools when Rust becomes the interactive parent.

pub mod archive;
pub mod capture;
pub mod codex_export;
pub mod context;
pub mod design;
pub mod doctor;
pub mod read_model;
pub mod spec;
pub mod sync;
pub mod types;
// pub mod decompose; // TODO: decomposition engine (assess, fork, harvest, merge)
// pub mod store;     // TODO: lifecycle.db sqlite schema + queries
