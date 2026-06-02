//! Provider-neutral evidence graph read model.
//!
//! Evidence generators write canonical JSONL streams under `.omegon/evidence`.
//! Core reads those streams generically and leaves provider-specific parsing to
//! extensions.

pub mod schema;
pub mod store;
pub mod support;

// Re-exported types are the intended public surface of the internal evidence
// module; downstream OpenSpec integration will consume them directly.
#[allow(unused_imports)]
pub use schema::{ClaimRecord, EvidenceEdge, EvidenceRecord};
#[allow(unused_imports)]
pub use store::EvidenceStore;
#[allow(unused_imports)]
pub use support::{ClaimSupportStatus, ClaimSupportSummary};
