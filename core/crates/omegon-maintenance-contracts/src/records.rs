use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{AuthorityKey, ContractError, Result, SCHEMA_VERSION, derive_key, path_key};

pub trait Record {
    const RECORD_KIND: &'static str;

    fn schema_version(&self) -> u32;
    fn record_kind(&self) -> &str;
    fn validate(&self) -> Result<()>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathIdentityV1 {
    pub dialect: PathDialect,
    pub path_bytes: String,
    pub device: u64,
    pub inode: u64,
    pub key: AuthorityKey,
}

impl PathIdentityV1 {
    pub fn unix(path_bytes: &[u8], device: u64, inode: u64) -> Result<Self> {
        if !path_bytes.starts_with(b"/") || path_bytes.contains(&0) {
            return Err(ContractError::InvalidValue(
                "Unix path identity must contain absolute NUL-free bytes".into(),
            ));
        }
        validate_absolute_unix_path(path_bytes)?;
        Ok(Self {
            dialect: PathDialect::Unix,
            path_bytes: URL_SAFE_NO_PAD.encode(path_bytes),
            device,
            inode,
            key: path_key("unix", path_bytes),
        })
    }

    pub fn decoded_path(&self) -> Result<Vec<u8>> {
        URL_SAFE_NO_PAD
            .decode(&self.path_bytes)
            .map_err(|error| ContractError::InvalidValue(format!("invalid path bytes: {error}")))
    }

    pub fn validate(&self) -> Result<()> {
        let path = self.decoded_path()?;
        if self.dialect != PathDialect::Unix || !path.starts_with(b"/") || path.contains(&0) {
            return Err(ContractError::InvalidValue(
                "unsupported or invalid path identity".into(),
            ));
        }
        validate_absolute_unix_path(&path)?;
        let expected = path_key("unix", &path);
        if self.key != expected {
            return Err(ContractError::InvalidValue(
                "path identity key does not match path bytes".into(),
            ));
        }
        Ok(())
    }
}

pub fn normalize_workspace_path(path: &[u8]) -> Result<Vec<u8>> {
    if !path.starts_with(b"/") || path.contains(&0) {
        return Err(ContractError::InvalidValue(
            "workspace path must be absolute and NUL-free".into(),
        ));
    }
    let mut components: Vec<&[u8]> = Vec::new();
    for component in path.split(|byte| *byte == b'/').skip(1) {
        match component {
            b"" | b"." => {}
            b".." => {
                if components.pop().is_none() {
                    return Err(ContractError::InvalidValue(
                        "workspace path escapes root".into(),
                    ));
                }
            }
            value => components.push(value),
        }
    }
    let mut normalized = Vec::with_capacity(path.len());
    normalized.push(b'/');
    for (index, component) in components.iter().enumerate() {
        if index > 0 {
            normalized.push(b'/');
        }
        normalized.extend_from_slice(component);
    }
    Ok(normalized)
}

pub fn validate_child_name(name: &[u8]) -> Result<()> {
    if name.is_empty()
        || name.contains(&0)
        || name.contains(&b'/')
        || matches!(name, b"." | b"..")
        || name.get(1) == Some(&b':')
    {
        return Err(ContractError::InvalidValue(
            "child path is absolute, traversing, or platform-prefixed".into(),
        ));
    }
    Ok(())
}

fn validate_absolute_unix_path(path: &[u8]) -> Result<()> {
    if normalize_workspace_path(path)? != path {
        return Err(ContractError::InvalidValue(
            "path identity bytes are not lexically canonical".into(),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathDialect {
    Unix,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallationStateV1 {
    pub schema_version: u32,
    pub record_kind: String,
    pub record_id: AuthorityKey,
    pub installation_uuid: String,
    pub home: PathIdentityV1,
    pub next_audit_sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DenyRecordV1 {
    pub schema_version: u32,
    pub record_kind: String,
    pub record_id: AuthorityKey,
    pub scope_key: AuthorityKey,
    pub contribution_kind: ContributionKind,
    pub entry_key: AuthorityKey,
    pub raw_name_digest: AuthorityKey,
    pub generation: u64,
    pub state: DenyState,
    pub request_id: String,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DenyStateV1 {
    pub schema_version: u32,
    pub record_kind: String,
    pub record_id: AuthorityKey,
    pub scope_key: AuthorityKey,
    pub generation: u64,
    pub entries: BTreeMap<String, DenyRecordV1>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionKind {
    Extension,
    Plugin,
    Skill,
    Prompt,
    Catalog,
    Workflow,
}

impl ContributionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Extension => "extension",
            Self::Plugin => "plugin",
            Self::Skill => "skill",
            Self::Prompt => "prompt",
            Self::Catalog => "catalog",
            Self::Workflow => "workflow",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DenyState {
    Denied,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionDenyRecordV1 {
    pub schema_version: u32,
    pub record_kind: String,
    pub record_id: AuthorityKey,
    pub session_key: AuthorityKey,
    pub session_id: String,
    pub workspace_key: AuthorityKey,
    pub state: SessionDenyState,
    pub request_id: String,
    pub created_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionDenyState {
    ResumeDenied,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnershipRecordV1 {
    pub schema_version: u32,
    pub record_kind: String,
    pub record_id: AuthorityKey,
    pub runtime_id: String,
    pub generation_id: String,
    pub workspace_key: AuthorityKey,
    pub boot_id: String,
    pub pid: u32,
    pub process_group: Option<i32>,
    pub process_start_token: String,
    pub lifecycle_boundary: LifecycleBoundary,
    pub cleanup_capability: CleanupCapability,
    pub writer: ArtifactIdentityV1,
    pub heartbeat_utc: String,
    pub heartbeat_monotonic_ticks: u64,
    pub expires_after_seconds: u32,
}

impl OwnershipRecordV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runtime_id: String,
        generation_id: String,
        workspace_key: AuthorityKey,
        boot_id: String,
        pid: u32,
        process_group: Option<i32>,
        process_start_token: String,
        lifecycle_boundary: LifecycleBoundary,
        cleanup_capability: CleanupCapability,
        writer: ArtifactIdentityV1,
        heartbeat_utc: String,
        heartbeat_monotonic_ticks: u64,
    ) -> Result<Self> {
        let value = Self {
            schema_version: SCHEMA_VERSION,
            record_kind: "ownership".into(),
            record_id: derive_key(
                "ownership",
                &[
                    workspace_key.as_bytes(),
                    runtime_id.as_bytes(),
                    generation_id.as_bytes(),
                ],
            ),
            runtime_id,
            generation_id,
            workspace_key,
            boot_id,
            pid,
            process_group,
            process_start_token,
            lifecycle_boundary,
            cleanup_capability,
            writer,
            heartbeat_utc,
            heartbeat_monotonic_ticks,
            expires_after_seconds: 300,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn refresh_heartbeat(
        &mut self,
        heartbeat_utc: String,
        heartbeat_monotonic_ticks: u64,
    ) -> Result<()> {
        self.heartbeat_utc = heartbeat_utc;
        self.heartbeat_monotonic_ticks = heartbeat_monotonic_ticks;
        self.validate()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleBoundary {
    OwnedProcessTree,
    CrossBoundary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupCapability {
    Strict,
    BestEffort,
    Unverifiable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransactionV1 {
    pub schema_version: u32,
    pub record_kind: String,
    pub record_id: AuthorityKey,
    pub request_id: String,
    pub command_fingerprint: AuthorityKey,
    pub domain_key: AuthorityKey,
    pub roots: Vec<PathIdentityV1>,
    pub steps: Vec<TransactionStepV1>,
    pub state: TransactionState,
    pub created_at: String,
    pub updated_at: String,
    pub audit_sequence: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransactionStepV1 {
    pub kind: TransactionStepKind,
    pub parent: PathIdentityV1,
    pub basename_bytes: String,
    pub basename_digest: AuthorityKey,
    pub destination_parent: Option<PathIdentityV1>,
    pub destination_basename_bytes: Option<String>,
    pub destination_basename_digest: Option<AuthorityKey>,
    pub expected_existing: Option<FileIdentityV1>,
    pub expected_absence: bool,
    pub intended_content_digest: Option<AuthorityKey>,
    pub state: TransactionStepState,
    pub observed: Option<PostStateV1>,
}

impl TransactionStepV1 {
    pub fn encode_basename(bytes: &[u8]) -> Result<(String, AuthorityKey)> {
        validate_child_name(bytes)?;
        Ok((
            URL_SAFE_NO_PAD.encode(bytes),
            AuthorityKey::from_bytes(Sha256::digest(bytes).into()),
        ))
    }

    pub fn basename(&self) -> Result<Vec<u8>> {
        decode_transaction_basename(&self.basename_bytes, self.basename_digest)
    }

    pub fn destination_basename(&self) -> Result<Option<Vec<u8>>> {
        match (
            self.destination_basename_bytes.as_deref(),
            self.destination_basename_digest,
        ) {
            (Some(bytes), Some(digest)) => decode_transaction_basename(bytes, digest).map(Some),
            (None, None) => Ok(None),
            _ => Err(ContractError::InvalidValue(
                "transaction destination basename framing is incomplete".into(),
            )),
        }
    }
}

fn decode_transaction_basename(encoded: &str, expected: AuthorityKey) -> Result<Vec<u8>> {
    let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|error| {
        ContractError::InvalidValue(format!("invalid transaction basename bytes: {error}"))
    })?;
    validate_child_name(&bytes)?;
    let actual = AuthorityKey::from_bytes(Sha256::digest(&bytes).into());
    if actual != expected {
        return Err(ContractError::InvalidValue(
            "transaction basename digest does not match bytes".into(),
        ));
    }
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionStepKind {
    DenyStateReplace,
    SessionDenyCreate,
    QuarantineDetach,
    QuarantineSymlinkUnlink,
    ResourceRecordPrune,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionStepState {
    Prepared,
    Dispatched,
    Settled,
    Aborted,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionState {
    Prepared,
    StepDispatched,
    StepSettled,
    TargetsSettled,
    Settled,
    Aborted,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileIdentityV1 {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub modified_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostStateV1 {
    pub source_present: bool,
    pub destination: Option<FileIdentityV1>,
    pub destination_content_digest: Option<AuthorityKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FenceV1 {
    pub schema_version: u32,
    pub record_kind: String,
    pub record_id: AuthorityKey,
    pub domain_key: AuthorityKey,
    pub transaction_record_id: AuthorityKey,
    pub state: FenceState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FenceState {
    Active,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditRecordV1 {
    pub schema_version: u32,
    pub record_kind: String,
    pub record_id: AuthorityKey,
    pub installation_uuid: String,
    pub sequence: u64,
    pub previous_digest: Option<AuthorityKey>,
    pub request_id: String,
    pub command: String,
    pub outcome: ResultStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditCheckpointV1 {
    pub schema_version: u32,
    pub record_kind: String,
    pub record_id: AuthorityKey,
    pub installation_uuid: String,
    pub last_sequence: u64,
    pub last_digest: AuthorityKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditFrontierV1 {
    pub schema_version: u32,
    pub record_kind: String,
    pub record_id: AuthorityKey,
    pub installation_uuid: String,
    pub current_segment_start: u64,
    pub current_segment_previous_digest: Option<AuthorityKey>,
    pub previous_segment_start: Option<u64>,
    pub previous_segment_previous_digest: Option<AuthorityKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditReceiptV1 {
    pub schema_version: u32,
    pub record_kind: String,
    pub record_id: AuthorityKey,
    pub installation_uuid: String,
    pub request_id: String,
    pub command: String,
    pub outcome: ResultStatus,
    pub sequence: u64,
    pub audit_digest: AuthorityKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageManifestV1 {
    pub schema_version: u32,
    pub record_kind: String,
    pub record_id: AuthorityKey,
    pub repository: String,
    pub workflow_identity: String,
    pub issuer: String,
    pub git_ref: String,
    pub tag: String,
    pub commit: String,
    pub version: String,
    pub target: String,
    pub archive_filename: String,
    pub archive_digest: AuthorityKey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_components: Option<Vec<ProductComponentInventoryV1>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk_extension_posture: Option<String>,
    pub members: Vec<PackageMemberV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub composition_locks: Vec<ArtifactCompositionLockV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub product_component_locks: Vec<ProductComponentLockV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductComponentInventoryV1 {
    pub component_id: String,
    pub wire_manifest_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductComponentLockV1 {
    pub schema_version: u32,
    pub component_id: String,
    pub wire_manifest_id: String,
    pub manifest_path: String,
    pub manifest_digest: AuthorityKey,
    pub executable_path: String,
    pub executable_digest: AuthorityKey,
    pub target: String,
    pub protocol_minimum: u32,
    pub protocol_maximum: u32,
    pub protocol_version: u32,
    pub fallback: String,
    pub signing_identity: SigningIdentityV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageMemberV1 {
    pub path: String,
    pub mode: u32,
    pub size: u64,
    pub digest: AuthorityKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactCompositionLockV1 {
    pub identity: String,
    pub artifact_path: String,
    pub artifact_digest: AuthorityKey,
    pub protocol_minimum: u32,
    pub protocol_maximum: u32,
    pub targets: Vec<String>,
    pub required: bool,
    pub fallback: String,
    pub resident_lock_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResidentCompositionLockV1 {
    pub schema_version: u32,
    pub executable_identity: String,
    pub executable_digest: AuthorityKey,
    pub target: String,
    pub protocol_minimum: u32,
    pub protocol_maximum: u32,
    pub contributions: Vec<ResidentContributionLockV1>,
    pub signing_identity: SigningIdentityV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResidentContributionLockV1 {
    pub identity: String,
    pub artifact_path: String,
    pub artifact_digest: AuthorityKey,
    pub protocol_minimum: u32,
    pub protocol_maximum: u32,
    pub targets: Vec<String>,
    pub required: bool,
    pub fallback: String,
    pub state: String,
}

pub const OMEGON_REQUIRED_RESIDENT_IDENTITIES: &[&str] = &[
    "system:constitutional-kernel",
    "system:default-loop",
    "system:host-effects",
];

pub const OMEGON_OPTIONAL_RESIDENT_IDENTITIES: &[&str] = &[
    "feature:codescan-adapter",
    "feature:context-compaction",
    "feature:git",
    "feature:lifecycle",
    "feature:memory",
];

pub const OMEGON_MAINTAIN_RESIDENT_IDENTITIES: &[&str] = &["system:maintenance-kernel"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningIdentityV1 {
    pub issuer: String,
    pub workflow_identity: String,
    pub verification: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactIdentityV1 {
    pub version: String,
    pub commit: String,
    pub target: String,
    pub digest: AuthorityKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceResultV1 {
    pub schema_version: u32,
    pub command: String,
    pub status: ResultStatus,
    pub request_id: String,
    pub artifact: ArtifactIdentityV1,
    pub composition: CompositionIdentityV1,
    pub deadline: DeadlineEvidenceV1,
    pub diagnostics: Vec<DiagnosticV1>,
    pub mutations: Vec<MutationResultV1>,
    pub errors: Vec<ErrorV1>,
    pub truncated: bool,
    pub next_cursor: Option<String>,
}

pub mod error_code {
    pub const ROOT_UNSAFE: &str = "root_unsafe";
    pub const RECORD_INVALID: &str = "record_invalid";
    pub const TRANSACTION_UNKNOWN: &str = "transaction_unknown";
    pub const DEADLINE_AFTER_DISPATCH: &str = "deadline_after_dispatch";
    pub const OUTPUT_FAILED: &str = "output_failed";
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultStatus {
    Success,
    Failure,
    Degraded,
}

impl ResultStatus {
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Failure => 1,
            Self::Degraded => 2,
        }
    }
}

impl MaintenanceResultV1 {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ContractError::UnsupportedSchema(self.schema_version));
        }
        for diagnostic in &self.diagnostics {
            validate_code(&diagnostic.code)?;
            validate_message(&diagnostic.message)?;
            if diagnostic
                .evidence
                .as_ref()
                .is_some_and(|value| value.len() > 4096)
            {
                return Err(ContractError::InvalidValue(
                    "diagnostic evidence exceeds 4096 bytes".into(),
                ));
            }
        }
        for error in &self.errors {
            validate_code(&error.code)?;
            validate_message(&error.message)?;
        }
        if self.status == ResultStatus::Success && !self.errors.is_empty() {
            return Err(ContractError::InvalidValue(
                "successful result cannot contain errors".into(),
            ));
        }
        if self.status == ResultStatus::Success
            && self.mutations.iter().any(|mutation| {
                !matches!(
                    mutation.state,
                    MutationState::Planned | MutationState::Settled
                )
            })
        {
            return Err(ContractError::InvalidValue(
                "successful result contains an unsettled mutation".into(),
            ));
        }
        let paginated = matches!(
            self.command.as_str(),
            "contribution.list" | "session.list" | "resource.list" | "audit.inspect"
        );
        if self.truncated && (!paginated || self.next_cursor.is_none()) {
            return Err(ContractError::InvalidValue(
                "only paginated list results may truncate and they require a cursor".into(),
            ));
        }
        if !self.truncated && self.next_cursor.is_some() {
            return Err(ContractError::InvalidValue(
                "complete results cannot contain a continuation cursor".into(),
            ));
        }
        if self.truncated && (!self.mutations.is_empty() || !self.errors.is_empty()) {
            return Err(ContractError::InvalidValue(
                "mutation and error outcomes cannot be truncated".into(),
            ));
        }
        // Diagnostic order is command-specific (for example contribution kind/scope/raw bytes).
        ensure_sorted(&self.errors, |value| {
            (
                value.code.clone(),
                value.phase.clone(),
                value.retry_safe,
                value.message.clone(),
            )
        })?;
        ensure_sorted(&self.mutations, |value| {
            (
                value.domain_key,
                value.kind.clone(),
                value.state,
                value.retry_safe,
            )
        })?;
        if crate::canonical_json(self)?.len() > crate::MAX_RESULT_BYTES {
            return Err(ContractError::InvalidValue(
                "result exceeds the 4 MiB envelope".into(),
            ));
        }
        Ok(())
    }
}

fn ensure_sorted<T, K: Ord>(values: &[T], key: impl Fn(&T) -> K) -> Result<()> {
    if values.windows(2).any(|pair| key(&pair[0]) > key(&pair[1])) {
        return Err(ContractError::InvalidValue(
            "result arrays must use deterministic order".into(),
        ));
    }
    Ok(())
}

fn validate_code(code: &str) -> Result<()> {
    const FAMILIES: &[&str] = &[
        "cli_",
        "root_",
        "path_",
        "limit_",
        "lock_",
        "record_",
        "deny_",
        "home_",
        "session_",
        "resource_",
        "release_",
        "transaction_",
        "audit_",
        "deadline_",
        "output_",
    ];
    if code.is_empty()
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        || !FAMILIES.iter().any(|family| code.starts_with(family))
    {
        return Err(ContractError::InvalidValue(
            "diagnostic/error code is outside stable v1 families".into(),
        ));
    }
    Ok(())
}

fn validate_message(message: &str) -> Result<()> {
    if message.len() > 4096 || message.chars().any(char::is_control) {
        return Err(ContractError::InvalidValue(
            "message is unbounded or contains control characters".into(),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionIdentityV1 {
    pub profile: String,
    pub generation: AuthorityKey,
    pub excluded_inputs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeadlineEvidenceV1 {
    pub requested_ms: u64,
    pub elapsed_ms: u64,
    pub expired: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticV1 {
    pub code: String,
    pub severity: Severity,
    pub scope: String,
    pub message: String,
    pub evidence: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationResultV1 {
    pub domain_key: AuthorityKey,
    pub kind: String,
    pub state: MutationState,
    pub retry_safe: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationState {
    Planned,
    Prepared,
    Dispatched,
    Applied,
    Settled,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorV1 {
    pub code: String,
    pub phase: String,
    pub retry_safe: bool,
    pub message: String,
}

macro_rules! impl_record {
    ($type:ty, $kind:literal, $validate:expr) => {
        impl Record for $type {
            const RECORD_KIND: &'static str = $kind;

            fn schema_version(&self) -> u32 {
                self.schema_version
            }

            fn record_kind(&self) -> &str {
                &self.record_kind
            }

            fn validate(&self) -> Result<()> {
                ($validate)(self)
            }
        }
    };
}

fn require_header<T: Record>(record: &T) -> Result<()> {
    if record.schema_version() != SCHEMA_VERSION {
        return Err(ContractError::UnsupportedSchema(record.schema_version()));
    }
    if record.record_kind() != T::RECORD_KIND {
        return Err(ContractError::RecordKind {
            expected: T::RECORD_KIND,
            actual: record.record_kind().to_owned(),
        });
    }
    Ok(())
}

fn require_record_id(actual: AuthorityKey, expected: AuthorityKey) -> Result<()> {
    if actual != expected {
        return Err(ContractError::InvalidValue(
            "record ID does not match authority fields".into(),
        ));
    }
    Ok(())
}

fn validate_uuid(value: &str) -> Result<()> {
    if value.len() != 36
        || value.bytes().enumerate().any(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte != b'-',
            _ => !byte.is_ascii_hexdigit() || byte.is_ascii_uppercase(),
        })
    {
        return Err(ContractError::InvalidValue(
            "request ID must be a lowercase UUID".into(),
        ));
    }
    Ok(())
}

fn validate_timestamp(value: &str) -> Result<(u32, u32, u32, u32, u32, u32)> {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
        || bytes.iter().enumerate().any(|(index, byte)| {
            !matches!(index, 4 | 7 | 10 | 13 | 16 | 19) && !byte.is_ascii_digit()
        })
    {
        return Err(ContractError::InvalidValue(
            "timestamp must use UTC second precision".into(),
        ));
    }
    let parse = |range: std::ops::Range<usize>| -> u32 {
        std::str::from_utf8(&bytes[range])
            .expect("timestamp digits are ASCII")
            .parse()
            .expect("timestamp fields contain only digits")
    };
    let year = parse(0..4);
    let month = parse(5..7);
    let day = parse(8..10);
    let hour = parse(11..13);
    let minute = parse(14..16);
    let second = parse(17..19);
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if day == 0 || day > days_in_month || hour > 23 || minute > 59 || second > 59 {
        return Err(ContractError::InvalidValue(
            "timestamp contains an out-of-range field".into(),
        ));
    }
    Ok((year, month, day, hour, minute, second))
}

fn validate_transaction_state(value: &TransactionV1) -> Result<()> {
    let states: Vec<_> = value.steps.iter().map(|step| step.state).collect();
    let prefix_then = |frontier: TransactionStepState, require_settled: bool| {
        let Some(index) = states
            .iter()
            .position(|state| *state != TransactionStepState::Settled)
        else {
            return false;
        };
        (!require_settled || index > 0)
            && states[index] == frontier
            && states[index + 1..]
                .iter()
                .all(|state| *state == TransactionStepState::Prepared)
    };
    let valid = match value.state {
        TransactionState::Prepared => states
            .iter()
            .all(|state| *state == TransactionStepState::Prepared),
        TransactionState::StepDispatched => prefix_then(TransactionStepState::Dispatched, false),
        TransactionState::StepSettled => prefix_then(TransactionStepState::Prepared, true),
        TransactionState::TargetsSettled => states
            .iter()
            .all(|state| *state == TransactionStepState::Settled),
        TransactionState::Settled => states
            .iter()
            .all(|state| *state == TransactionStepState::Settled),
        TransactionState::Aborted => prefix_then(TransactionStepState::Aborted, false),
        TransactionState::Unknown => {
            prefix_then(TransactionStepState::Unknown, false)
                || prefix_then(TransactionStepState::Dispatched, false)
        }
    };
    if !valid {
        return Err(ContractError::InvalidValue(
            "transaction state contradicts its step states".into(),
        ));
    }
    let audit_valid = match value.state {
        TransactionState::Settled | TransactionState::Aborted => value.audit_sequence.is_some(),
        TransactionState::Prepared
        | TransactionState::StepDispatched
        | TransactionState::StepSettled
        | TransactionState::TargetsSettled => value.audit_sequence.is_none(),
        TransactionState::Unknown => true,
    };
    if !audit_valid {
        return Err(ContractError::InvalidValue(
            "settled/aborted transactions require audit sequence; active transactions forbid it"
                .into(),
        ));
    }
    Ok(())
}

fn validate_step_evidence(step: &TransactionStepV1) -> Result<()> {
    match step.state {
        TransactionStepState::Prepared
        | TransactionStepState::Dispatched
        | TransactionStepState::Aborted
            if step.observed.is_some() =>
        {
            return Err(ContractError::InvalidValue(
                "undetermined transaction step cannot contain post-state evidence".into(),
            ));
        }
        TransactionStepState::Settled => {
            let observed = step.observed.as_ref().ok_or_else(|| {
                ContractError::InvalidValue("settled step requires post-state evidence".into())
            })?;
            match step.kind {
                TransactionStepKind::DenyStateReplace => {
                    if !observed.source_present
                        || observed.destination.is_none()
                        || observed.destination_content_digest != step.intended_content_digest
                    {
                        return Err(ContractError::InvalidValue(
                            "settled record write does not prove intended canonical bytes".into(),
                        ));
                    }
                }
                TransactionStepKind::SessionDenyCreate => {
                    if observed.source_present
                        || observed.destination.is_none()
                        || observed.destination_content_digest != step.intended_content_digest
                    {
                        return Err(ContractError::InvalidValue(
                            "settled session deny does not prove exclusive creation".into(),
                        ));
                    }
                }
                TransactionStepKind::QuarantineDetach => {
                    if observed.source_present
                        || observed.destination.as_ref() != step.expected_existing.as_ref()
                        || observed.destination_content_digest.is_some()
                    {
                        return Err(ContractError::InvalidValue(
                            "settled detach requires source absence and destination identity"
                                .into(),
                        ));
                    }
                }
                TransactionStepKind::QuarantineSymlinkUnlink
                | TransactionStepKind::ResourceRecordPrune => {
                    if observed.source_present
                        || observed.destination.is_some()
                        || observed.destination_content_digest.is_some()
                    {
                        return Err(ContractError::InvalidValue(
                            "settled prune requires proven source absence only".into(),
                        ));
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

impl_record!(
    InstallationStateV1,
    "installation_state",
    |value: &InstallationStateV1| {
        require_header(value)?;
        validate_uuid(&value.installation_uuid)?;
        value.home.validate()?;
        if value.next_audit_sequence == 0 {
            return Err(ContractError::InvalidValue(
                "installation next audit sequence must be nonzero".into(),
            ));
        }
        require_record_id(
            value.record_id,
            derive_key("installation", &[value.installation_uuid.as_bytes()]),
        )
    }
);
impl_record!(DenyRecordV1, "deny", |value: &DenyRecordV1| {
    require_header(value)?;
    validate_uuid(&value.request_id)?;
    validate_timestamp(&value.created_at)?;
    require_record_id(
        value.record_id,
        derive_key(
            "deny",
            &[
                value.scope_key.as_bytes(),
                value.entry_key.as_bytes(),
                value.request_id.as_bytes(),
            ],
        ),
    )
});
impl_record!(DenyStateV1, "deny_state", |value: &DenyStateV1| {
    require_header(value)?;
    require_record_id(
        value.record_id,
        derive_key(
            "deny-state",
            &[value.scope_key.as_bytes(), &value.generation.to_be_bytes()],
        ),
    )?;
    if value.entries.len() > 10_000 {
        return Err(ContractError::InvalidValue(
            "deny state exceeds entry limit".into(),
        ));
    }
    for (key, entry) in &value.entries {
        entry.validate()?;
        if key != &entry.entry_key.to_hex()
            || entry.scope_key != value.scope_key
            || entry.generation != value.generation
        {
            return Err(ContractError::InvalidValue(
                "deny entry does not match container".into(),
            ));
        }
    }
    Ok(())
});
impl_record!(
    SessionDenyRecordV1,
    "session_deny",
    |value: &SessionDenyRecordV1| {
        require_header(value)?;
        validate_uuid(&value.request_id)?;
        validate_timestamp(&value.created_at)?;
        require_record_id(
            value.session_key,
            crate::session_key(&value.session_id, value.workspace_key),
        )?;
        require_record_id(
            value.record_id,
            derive_key(
                "session-deny",
                &[value.session_key.as_bytes(), value.request_id.as_bytes()],
            ),
        )
    }
);
impl_record!(
    OwnershipRecordV1,
    "ownership",
    |value: &OwnershipRecordV1| {
        require_header(value)?;
        if value.expires_after_seconds != 300 {
            return Err(ContractError::InvalidValue(
                "ownership expiry must be 300 seconds".into(),
            ));
        }
        if value.runtime_id.is_empty()
            || value.generation_id.is_empty()
            || value.pid == 0
            || value.process_start_token.is_empty()
            || value.boot_id.is_empty()
            || value.writer.version.is_empty()
            || value.writer.commit.is_empty()
            || value.writer.target.is_empty()
            || matches!(
                (value.lifecycle_boundary, value.cleanup_capability),
                (LifecycleBoundary::CrossBoundary, CleanupCapability::Strict)
            )
        {
            return Err(ContractError::InvalidValue(
                "ownership record contains incomplete or contradictory evidence".into(),
            ));
        }
        let platform_identity_valid = if value.writer.target.contains("apple-darwin") {
            valid_tuple_token(&value.boot_id, "macos")
                && valid_tuple_token(&value.process_start_token, "macos")
        } else if value.writer.target.contains("linux") {
            valid_linux_boot_id(&value.boot_id)
                && valid_linux_process_token(&value.process_start_token)
        } else {
            false
        };
        if !platform_identity_valid {
            return Err(ContractError::InvalidValue(
                "ownership boot or process-start identity encoding is invalid".into(),
            ));
        }
        validate_timestamp(&value.heartbeat_utc)?;
        require_record_id(
            value.record_id,
            derive_key(
                "ownership",
                &[
                    value.workspace_key.as_bytes(),
                    value.runtime_id.as_bytes(),
                    value.generation_id.as_bytes(),
                ],
            ),
        )
    }
);

fn valid_tuple_token(value: &str, platform: &str) -> bool {
    let mut fields = value.split(':');
    matches!(
        (fields.next(), fields.next(), fields.next(), fields.next()),
        (Some(actual), Some(seconds), Some(micros), None)
            if actual == platform
                && seconds.parse::<u64>().is_ok()
                && micros.parse::<u32>().is_ok_and(|value| value < 1_000_000)
    )
}

fn valid_linux_boot_id(value: &str) -> bool {
    let Some(value) = value.strip_prefix("linux:") else {
        return false;
    };
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23) && byte == b'-'
                || !matches!(index, 8 | 13 | 18 | 23)
                    && (byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

fn valid_linux_process_token(value: &str) -> bool {
    value
        .strip_prefix("linux:")
        .is_some_and(|value| !value.is_empty() && value.parse::<u64>().is_ok())
}
impl_record!(TransactionV1, "transaction", |value: &TransactionV1| {
    require_header(value)?;
    require_record_id(
        value.record_id,
        derive_key("transaction", &[value.request_id.as_bytes()]),
    )?;
    if value.steps.is_empty() {
        return Err(ContractError::InvalidValue(
            "transaction must contain a step".into(),
        ));
    }
    if value.roots.is_empty() {
        return Err(ContractError::InvalidValue(
            "transaction must contain an opened root identity".into(),
        ));
    }
    for root in &value.roots {
        root.validate()?;
    }
    for step in &value.steps {
        step.parent.validate()?;
        step.basename()?;
        if let Some(parent) = &step.destination_parent {
            parent.validate()?;
        }
        if step.destination_parent.is_some() != step.destination_basename_digest.is_some()
            || step.destination_parent.is_some() != step.destination_basename_bytes.is_some()
        {
            return Err(ContractError::InvalidValue(
                "transaction destination requires both parent and basename digest".into(),
            ));
        }
        step.destination_basename()?;
        if step.expected_absence == step.expected_existing.is_some() {
            return Err(ContractError::InvalidValue(
                "transaction step must expect exactly absence or an existing identity".into(),
            ));
        }
        if step.observed.as_ref().is_some_and(|observed| {
            observed.destination.is_none() && observed.destination_content_digest.is_some()
        }) {
            return Err(ContractError::InvalidValue(
                "destination digest requires a destination identity".into(),
            ));
        }
        let kind_valid = match step.kind {
            TransactionStepKind::DenyStateReplace => {
                !step.expected_absence
                    && step.expected_existing.is_some()
                    && step.intended_content_digest.is_some()
                    && step.destination_parent.is_none()
            }
            TransactionStepKind::SessionDenyCreate => {
                step.expected_absence
                    && step.expected_existing.is_none()
                    && step.intended_content_digest.is_some()
                    && step.destination_parent.is_none()
            }
            TransactionStepKind::QuarantineDetach => {
                !step.expected_absence
                    && step.expected_existing.is_some()
                    && step.intended_content_digest.is_none()
                    && step.destination_parent.is_some()
            }
            TransactionStepKind::QuarantineSymlinkUnlink
            | TransactionStepKind::ResourceRecordPrune => {
                !step.expected_absence
                    && step.expected_existing.is_some()
                    && step.intended_content_digest.is_none()
                    && step.destination_parent.is_none()
            }
        };
        if !kind_valid {
            return Err(ContractError::InvalidValue(
                "transaction step evidence does not match its kind".into(),
            ));
        }
        validate_step_evidence(step)?;
    }
    validate_transaction_state(value)?;
    let created_at = validate_timestamp(&value.created_at)?;
    let updated_at = validate_timestamp(&value.updated_at)?;
    if created_at > updated_at {
        return Err(ContractError::InvalidValue(
            "transaction updated_at precedes created_at".into(),
        ));
    }
    validate_uuid(&value.request_id)?;
    Ok(())
});
impl_record!(FenceV1, "fence", |value: &FenceV1| {
    require_header(value)?;
    require_record_id(
        value.record_id,
        derive_key(
            "fence",
            &[
                value.domain_key.as_bytes(),
                value.transaction_record_id.as_bytes(),
            ],
        ),
    )
});
impl_record!(AuditRecordV1, "audit", |value: &AuditRecordV1| {
    require_header(value)?;
    validate_uuid(&value.installation_uuid)?;
    validate_uuid(&value.request_id)?;
    if value.sequence == 0 {
        return Err(ContractError::InvalidValue(
            "audit sequence must be nonzero".into(),
        ));
    }
    require_record_id(
        value.record_id,
        derive_key(
            "audit",
            &[
                value.installation_uuid.as_bytes(),
                &value.sequence.to_be_bytes(),
            ],
        ),
    )
});
impl_record!(AuditReceiptV1, "audit_receipt", |value: &AuditReceiptV1| {
    require_header(value)?;
    validate_uuid(&value.installation_uuid)?;
    validate_uuid(&value.request_id)?;
    if value.sequence == 0 || value.command.is_empty() || value.command.len() > 4096 {
        return Err(ContractError::InvalidValue(
            "audit receipt contains an invalid sequence or command".into(),
        ));
    }
    require_record_id(
        value.record_id,
        derive_key(
            "audit-receipt",
            &[
                value.installation_uuid.as_bytes(),
                value.request_id.as_bytes(),
                value.command.as_bytes(),
                result_status_bytes(value.outcome),
                &value.sequence.to_be_bytes(),
                value.audit_digest.as_bytes(),
            ],
        ),
    )
});

const fn result_status_bytes(value: ResultStatus) -> &'static [u8] {
    match value {
        ResultStatus::Success => b"success",
        ResultStatus::Failure => b"failure",
        ResultStatus::Degraded => b"degraded",
    }
}
impl_record!(
    AuditCheckpointV1,
    "audit_checkpoint",
    |value: &AuditCheckpointV1| {
        require_header(value)?;
        validate_uuid(&value.installation_uuid)?;
        let zero_digest = AuthorityKey::from_bytes([0; 32]);
        if (value.last_sequence == 0) != (value.last_digest == zero_digest) {
            return Err(ContractError::InvalidValue(
                "zero audit checkpoint sequence and digest must agree".into(),
            ));
        }
        require_record_id(
            value.record_id,
            derive_key(
                "audit-checkpoint",
                &[
                    value.installation_uuid.as_bytes(),
                    &value.last_sequence.to_be_bytes(),
                    value.last_digest.as_bytes(),
                ],
            ),
        )
    }
);
impl_record!(
    AuditFrontierV1,
    "audit_frontier",
    |value: &AuditFrontierV1| {
        require_header(value)?;
        validate_uuid(&value.installation_uuid)?;
        let previous_start = value
            .current_segment_start
            .checked_sub(crate::AUDIT_SEGMENT_RECORDS);
        if value.current_segment_start == 0
            || !(value.current_segment_start - 1).is_multiple_of(crate::AUDIT_SEGMENT_RECORDS)
            || value.current_segment_previous_digest.is_some() != (value.current_segment_start > 1)
            || value.previous_segment_start != previous_start
            || value.previous_segment_previous_digest.is_some()
                != previous_start.is_some_and(|start| start > 1)
        {
            return Err(ContractError::InvalidValue(
                "audit frontier segment anchors are inconsistent".into(),
            ));
        }
        let zero = AuthorityKey::from_bytes([0; 32]);
        let previous_start = value.previous_segment_start.unwrap_or(0);
        require_record_id(
            value.record_id,
            derive_key(
                "audit-frontier",
                &[
                    value.installation_uuid.as_bytes(),
                    &value.current_segment_start.to_be_bytes(),
                    value
                        .current_segment_previous_digest
                        .unwrap_or(zero)
                        .as_bytes(),
                    &previous_start.to_be_bytes(),
                    value
                        .previous_segment_previous_digest
                        .unwrap_or(zero)
                        .as_bytes(),
                ],
            ),
        )
    }
);
impl_record!(
    PackageManifestV1,
    "package_manifest",
    |value: &PackageManifestV1| {
        require_header(value)?;
        require_record_id(
            value.record_id,
            derive_key(
                "package",
                &[
                    value.archive_digest.as_bytes(),
                    value.target.as_bytes(),
                    value.version.as_bytes(),
                ],
            ),
        )?;
        if value.members.len() < 2
            || !value.members.iter().any(|member| member.path == "omegon")
            || !value
                .members
                .iter()
                .any(|member| member.path == "omegon-maintain")
        {
            return Err(ContractError::InvalidValue(
                "package manifest must contain both executables".into(),
            ));
        }
        match (
            value.host_profile.as_deref(),
            value.composition_class.as_deref(),
            value.core_components.as_deref(),
            value.sdk_extension_posture.as_deref(),
        ) {
            (None, None, None, None) => {}
            (
                Some("full-product"),
                Some("full-product"),
                Some(components),
                Some("operator-managed"),
            ) if components
                == [ProductComponentInventoryV1 {
                    component_id: "core:codescan".into(),
                    wire_manifest_id: "omegon-codescan".into(),
                }] => {}
            (Some("full-product"), Some("host-only"), Some([]), Some("operator-managed")) => {}
            _ => {
                return Err(ContractError::InvalidValue(
                    "package distribution composition is incomplete or noncanonical".into(),
                ));
            }
        }
        let has_codescan_manifest = value
            .members
            .iter()
            .any(|member| member.path == "share/omegon/extensions/omegon-codescan/manifest.toml");
        let has_codescan_executable = value.members.iter().any(|member| {
            member.path == "share/omegon/extensions/omegon-codescan/target/release/omegon-codescan"
        });
        if has_codescan_manifest != has_codescan_executable
            || matches!(value.composition_class.as_deref(), Some("full-product"))
                != (has_codescan_manifest && has_codescan_executable)
        {
            return Err(ContractError::InvalidValue(
                "package core-component inventory does not match its members".into(),
            ));
        }
        match value.composition_class.as_deref() {
            Some("full-product") => {
                let [component] = value.product_component_locks.as_slice() else {
                    return Err(ContractError::InvalidValue(
                        "full-product package requires exactly one product-component lock".into(),
                    ));
                };
                let members = value
                    .members
                    .iter()
                    .map(|member| (member.path.as_str(), member))
                    .collect::<BTreeMap<_, _>>();
                let manifest = members.get(component.manifest_path.as_str());
                let executable = members.get(component.executable_path.as_str());
                if component.schema_version != SCHEMA_VERSION
                    || component.component_id != "core:codescan"
                    || component.wire_manifest_id != "omegon-codescan"
                    || component.manifest_path
                        != "share/omegon/extensions/omegon-codescan/manifest.toml"
                    || component.executable_path
                        != "share/omegon/extensions/omegon-codescan/target/release/omegon-codescan"
                    || manifest.is_none_or(|member| member.digest != component.manifest_digest)
                    || executable.is_none_or(|member| member.digest != component.executable_digest)
                    || component.target != value.target
                    || component.protocol_minimum != 1
                    || component.protocol_maximum != 1
                    || component.protocol_version != 1
                    || component.fallback != "typed_unavailable"
                    || component.signing_identity.issuer != value.issuer
                    || component.signing_identity.workflow_identity != value.workflow_identity
                    || component.signing_identity.verification != "required"
                {
                    return Err(ContractError::InvalidValue(
                        "product-component evidence is incomplete, substituted, or self-promoted"
                            .into(),
                    ));
                }
            }
            Some("host-only") if !value.product_component_locks.is_empty() => {
                return Err(ContractError::InvalidValue(
                    "host-only package cannot own product-component evidence".into(),
                ));
            }
            _ => {}
        }
        for lock in &value.composition_locks {
            if lock.identity.trim().is_empty()
                || lock.artifact_path.trim().is_empty()
                || lock.protocol_minimum == 0
                || lock.protocol_minimum > lock.protocol_maximum
                || lock.targets.is_empty()
                || lock.fallback.trim().is_empty()
            {
                return Err(ContractError::InvalidValue(
                    "package composition lock is incomplete or inconsistent".into(),
                ));
            }
        }
        let identities = value
            .composition_locks
            .iter()
            .map(|lock| lock.identity.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let paths = value
            .composition_locks
            .iter()
            .map(|lock| lock.artifact_path.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if identities.len() != value.composition_locks.len()
            || paths.len() != value.composition_locks.len()
        {
            return Err(ContractError::InvalidValue(
                "package composition lock identities and artifact paths must be unique".into(),
            ));
        }
        Ok(())
    }
);
