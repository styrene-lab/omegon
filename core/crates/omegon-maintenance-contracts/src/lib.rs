//! Versioned wire contracts shared by Omegon and its maintenance companion.
//!
//! This crate owns data framing, canonical key derivation, advisory lock
//! semantics, and pure recovery classification. It intentionally contains no
//! normal runtime startup, contribution loading, or mutation orchestration.

mod canonical;
mod home;
mod key;
mod lock;
mod process;
mod records;
mod recovery;
mod selector;
mod state;

pub use canonical::{canonical_json, parse_record};
pub use home::{
    HomeContinuityV1, HomeRecoveryIntentV1, HomeRecoveryJournalV1, HomeRecoveryPhase,
    ensure_home_recovery_settled, home_binding_matches, same_home_directory,
    stable_home_volume_uuid,
};
pub use key::{
    AuthorityKey, CommandSemanticsV1, canonical_digest, command_fingerprint,
    contribution_domain_key, derive_key, entry_key, path_key, resource_domain_key, scope_key,
    session_domain_key, session_key, workspace_key,
};
pub use lock::{LockMode, ProtocolLock};
pub use process::{
    ProcessObservation, current_boot_id, current_monotonic_ns, observe_process_start,
};
pub use records::*;
pub use recovery::{
    DetachObservation, ReconciliationDecision, RecordObservation, reconcile_detach,
    reconcile_record,
};
pub use selector::{ContributionSelector, ListScope, resolve_list_scope};
pub use state::{
    ContributionAdmissionGuard, ContributionMutationGuard, MaintenanceStateV1, SessionResumeGuard,
    append_bytes_at, audit_receipt, create_record_no_replace_at, entry_identity_at, file_identity,
    open_or_create_secure_dir_at, open_secure_dir_at, open_secure_root, path_identity,
    read_bytes_at, read_record_at, read_record_with_identity_at, record_identity_at,
    remove_record_at, rename_entry_no_replace_at, replace_record_at,
};

pub const SCHEMA_VERSION: u32 = 1;
pub const AUDIT_SEGMENT_RECORDS: u64 = 100_000;
pub const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_RESULT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ContractError {
    #[error(
        "installation state is bound to a different home identity; inspect with omegon-maintain home inspect"
    )]
    HomeIdentityMismatch {
        stored: Box<PathIdentityV1>,
        observed: Box<PathIdentityV1>,
    },
    #[error(
        "installation home recovery is incomplete; resume the original maintenance recovery request"
    )]
    HomeRecoveryPending,
    #[error("record exceeds the {MAX_RECORD_BYTES}-byte limit")]
    RecordTooLarge,
    #[error("record is not valid UTF-8 JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("record contains a duplicate object key: {0}")]
    DuplicateKey(String),
    #[error("record contains a floating-point number")]
    FloatingPoint,
    #[error("record has an invalid authority key: {0}")]
    InvalidKey(String),
    #[error("record uses unsupported schema version {0}")]
    UnsupportedSchema(u32),
    #[error("record kind mismatch: expected {expected}, got {actual}")]
    RecordKind {
        expected: &'static str,
        actual: String,
    },
    #[error("invalid protocol value: {0}")]
    InvalidValue(String),
    #[error("protocol lock operation failed: {0}")]
    Lock(#[source] std::io::Error),
    #[error("maintenance filesystem operation failed: {0}")]
    Filesystem(#[source] std::io::Error),
    #[error("session resume is denied by maintenance policy")]
    SessionResumeDenied,
}

pub type Result<T> = std::result::Result<T, ContractError>;
