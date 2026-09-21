use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use chrono::DateTime;
pub(crate) use omegon_kernel_runtime::{RouteLeaseRecorded, TurnClosed, TurnOutcome, TurnStarted};
use omegon_traits::{
    RuntimeCapabilityId, RuntimeCapabilityTransitionPolicy, RuntimeCompositionGenerationId,
    RuntimeContributionGenerationId, RuntimeContributionId, RuntimeEffect, RuntimeExecutionPolicy,
    RuntimeInvocationKind, RuntimeMutationDomainId, RuntimeMutationFenceKey, RuntimePrincipalClass,
    RuntimeSurface,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub(crate) use crate::session_blob_store::{ContentRef, ProjectionClass};

const ENVELOPE_VERSION: u16 = 1;
const EVENT_VERSION: u16 = 1;
const SNAPSHOT_VERSION: u16 = 5;
const REDUCER_VERSION: u16 = 5;
const MAX_RECORD_BYTES: usize = 1024 * 1024;
const MAX_ATTACHMENT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ASSISTANT_CHUNK_BYTES: u64 = 64 * 1024;
const MAX_MESSAGE_TOOL_CALLS: u32 = 65_535;
const MAX_USAGE_TOKENS: u64 = 1_000_000_000_000;
const RECOVERY_NAMESPACE: Uuid = Uuid::from_u128(0x5907_b852_acde_4b53_a6b1_2d1a_c964_868a);
const INVOCATION_COMMAND_NAMESPACE: Uuid =
    Uuid::from_u128(0x39b4_58e2_e917_4210_9b34_d45d_c14d_48da);
const INVOCATION_FENCE_NAMESPACE: Uuid = Uuid::from_u128(0x8fe0_670a_a844_40ce_9e2d_a57e_83f5_8b4a);

#[derive(Debug, thiserror::Error)]
pub(crate) enum AuthorityError {
    #[error("authority I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("authority JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Blob(#[from] crate::session_blob_store::SessionBlobError),
    #[error("authority record is invalid: {0}")]
    Invalid(String),
    #[error("authority transition is invalid at sequence {sequence}: {message}")]
    Transition { sequence: u64, message: String },
}

type Result<T> = std::result::Result<T, AuthorityError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionBindingMigrationRejection {
    NoProcessLocalBinding,
    StaleSource,
    ActiveTurn,
    UnresolvedInvocation,
    UnchangedTarget,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ExecutionBindingMigrationError {
    #[error("execution binding migration was rejected: {0:?}")]
    Rejected(ExecutionBindingMigrationRejection),
    #[error(transparent)]
    Authority(#[from] AuthorityError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QueueMode {
    InterruptAfterTurn,
    UntilReady,
    Immediate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InterruptionKind {
    Cancel,
    Revoke,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PromptRemovalReason {
    Withdrawn,
    SessionClosing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InvocationOutcome {
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActorIdentity {
    pub(crate) principal: String,
    pub(crate) ingress: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttachmentRef {
    pub(crate) digest: String,
    pub(crate) media_type: String,
    pub(crate) byte_length: u64,
    pub(crate) storage_ref: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PromptContent {
    pub(crate) text: String,
    pub(crate) attachments: Vec<AttachmentRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionCreated {
    pub(crate) workspace_identity: String,
    pub(crate) created_by: ActorIdentity,
    pub(crate) runtime_generation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutionBindingGeneration {
    pub(crate) driver_generation_id: RuntimeContributionGenerationId,
    pub(crate) provider_route_service_generation_id: RuntimeContributionGenerationId,
}

impl ExecutionBindingGeneration {
    pub(crate) fn new(
        driver_generation_id: impl Into<String>,
        provider_route_service_generation_id: impl Into<String>,
    ) -> Result<Self> {
        Ok(Self {
            driver_generation_id: RuntimeContributionGenerationId::new(driver_generation_id.into())
                .map_err(|error| AuthorityError::Invalid(error.into()))?,
            provider_route_service_generation_id: RuntimeContributionGenerationId::new(
                provider_route_service_generation_id.into(),
            )
            .map_err(|error| AuthorityError::Invalid(error.into()))?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutionBindingMigrated {
    pub(crate) from_generation: ExecutionBindingGeneration,
    pub(crate) target_generation: ExecutionBindingGeneration,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PromptAdmitted {
    pub(crate) submission_id: Uuid,
    pub(crate) prompt_id: Uuid,
    pub(crate) principal: String,
    pub(crate) ingress: String,
    pub(crate) queue_mode: QueueMode,
    pub(crate) content: PromptContent,
    pub(crate) metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PromptRejected {
    pub(crate) submission_id: Uuid,
    pub(crate) principal: String,
    pub(crate) ingress: String,
    pub(crate) reason_code: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PromptRemoved {
    pub(crate) prompt_id: Uuid,
    pub(crate) reason: PromptRemovalReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RouteEndpointProvenanceRecorded {
    pub(crate) lease_id: Uuid,
    pub(crate) endpoint_id: String,
    pub(crate) adapter_id: String,
    pub(crate) inventory_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompactionEndpointProvenanceRecorded {
    pub(crate) compaction_request_id: Uuid,
    pub(crate) endpoint_id: String,
    pub(crate) adapter_id: String,
    pub(crate) inventory_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StepStarted {
    pub(crate) step_id: Uuid,
    pub(crate) turn_id: Uuid,
    pub(crate) step_ordinal: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ModelRequestPurpose {
    Initial,
    ContextOverflowRepair,
    ProviderHistoryRepair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ModelContextRole {
    System,
    Developer,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ModelContextSourceKind {
    Prompt,
    AssistantMessage,
    ToolResult,
    SystemInstruction,
    DeveloperInstruction,
    CompactionSummary,
    ContributionContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContextSourceKind {
    SystemInstruction,
    DeveloperInstruction,
    ContributionContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContextSourceMaterialized {
    pub(crate) context_source_id: Uuid,
    pub(crate) source_kind: ContextSourceKind,
    pub(crate) source_identity: String,
    pub(crate) owner_id: String,
    pub(crate) owner_generation_id: RuntimeContributionGenerationId,
    pub(crate) content_ref: ContentRef,
}

pub(crate) fn is_legacy_compatibility_source(source: &ContextSourceMaterialized) -> bool {
    source.source_kind == ContextSourceKind::ContributionContext
        && source.source_identity == "legacy-compatibility-base-v1"
        && source.owner_id == "compatibility:session-resume"
        && source.owner_generation_id.as_str() == "session-resume:legacy-base-v1"
}

pub(crate) fn legacy_compatibility_base_bytes(
    compatibility: &[crate::bridge::LlmMessage],
) -> Result<Vec<u8>> {
    let message = crate::bridge::LlmMessage::User {
        content: format!(
            "[Legacy compatibility context - frozen at full-spine cutover]\n{}\n[End legacy compatibility context]",
            serde_json::to_string(compatibility)?
        ),
        images: Vec::new(),
    };
    crate::surfaces::session::canonical_json_bytes(&message)
        .map_err(|error| AuthorityError::Invalid(error.to_string()))
}

pub(crate) fn legacy_compatibility_prefix<'a>(
    replay: &crate::session_replay::SessionReplay,
    compatibility: &'a [crate::bridge::LlmMessage],
) -> &'a [crate::bridge::LlmMessage] {
    let latest_prompt = replay.records().iter().rev().find_map(|record| {
        let SessionFactPayload::PromptAdmitted(prompt) = record.payload() else {
            return None;
        };
        Some(&prompt.content.text)
    });
    let semantic_start = latest_prompt.and_then(|prompt| {
        compatibility.iter().rposition(|message| {
            matches!(message, crate::bridge::LlmMessage::User { content, .. } if content == prompt)
        })
    });
    semantic_start.map_or(compatibility, |index| &compatibility[..index])
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelContextProvenance {
    pub(crate) source_kind: ModelContextSourceKind,
    pub(crate) source_event_id: Option<Uuid>,
    pub(crate) source_identity: Option<String>,
    pub(crate) owner_id: Option<String>,
    pub(crate) owner_generation_id: Option<RuntimeContributionGenerationId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelContextItem {
    pub(crate) ordinal: u32,
    pub(crate) role: ModelContextRole,
    pub(crate) content_ref: ContentRef,
    pub(crate) provenance: ModelContextProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelSchemaIdentity {
    pub(crate) ordinal: u32,
    pub(crate) capability_id: RuntimeCapabilityId,
    pub(crate) contribution_id: RuntimeContributionId,
    pub(crate) owner_generation_id: RuntimeContributionGenerationId,
    pub(crate) schema_dialect: String,
    pub(crate) schema_content_ref: ContentRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelSchemaSet {
    pub(crate) schema_set_version: u16,
    pub(crate) composition_generation_id: RuntimeCompositionGenerationId,
    pub(crate) normalizer_contribution_id: RuntimeContributionId,
    pub(crate) normalizer_generation_id: RuntimeContributionGenerationId,
    pub(crate) schemas: Vec<ModelSchemaIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelRequestPrepared {
    pub(crate) request_id: Uuid,
    pub(crate) step_id: Uuid,
    pub(crate) turn_id: Uuid,
    pub(crate) request_ordinal: u32,
    pub(crate) purpose: ModelRequestPurpose,
    pub(crate) replaces_request_id: Option<Uuid>,
    pub(crate) continuity_refs: Vec<Uuid>,
    pub(crate) context_manifest_id: String,
    pub(crate) context_items: Vec<ModelContextItem>,
    pub(crate) schema_set_id: String,
    pub(crate) schema_set: ModelSchemaSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelRequestRouteJoined {
    pub(crate) request_id: Uuid,
    pub(crate) step_id: Uuid,
    pub(crate) turn_id: Uuid,
    pub(crate) lease_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ModelResponseAttemptFailure {
    ProviderError,
    Eof,
    TimedOut,
    TransportLost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ModelResponseAttemptRetryDisposition {
    RetrySameRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelResponseAttemptFailed {
    pub(crate) request_id: Uuid,
    pub(crate) step_id: Uuid,
    pub(crate) response_attempt_ordinal: u32,
    pub(crate) failure: ModelResponseAttemptFailure,
    pub(crate) reason_code: String,
    pub(crate) retry_disposition: ModelResponseAttemptRetryDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AssistantContentKind {
    Text,
    Thinking,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AssistantContentAppended {
    pub(crate) message_id: Uuid,
    pub(crate) request_id: Uuid,
    pub(crate) step_id: Uuid,
    pub(crate) response_attempt_ordinal: u32,
    pub(crate) content_kind: AssistantContentKind,
    pub(crate) chunk_ordinal: u32,
    pub(crate) content_ref: ContentRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AssistantContentManifest {
    pub(crate) content_kind: AssistantContentKind,
    pub(crate) chunk_refs: Vec<ContentRef>,
    pub(crate) content_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderCompletionEvidence {
    ProviderDone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AssistantUsage {
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AssistantMessageCommitted {
    pub(crate) message_id: Uuid,
    pub(crate) request_id: Uuid,
    pub(crate) step_id: Uuid,
    pub(crate) response_attempt_ordinal: u32,
    pub(crate) completion_evidence: ProviderCompletionEvidence,
    pub(crate) content: Vec<AssistantContentManifest>,
    pub(crate) usage: Option<AssistantUsage>,
    pub(crate) tool_call_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderContinuityKind {
    HiddenReasoning,
    OpaqueProviderState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderContinuityRequiredFor {
    NextRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RestrictedContinuityPolicy {
    pub(crate) allowed_kinds: Vec<ProviderContinuityKind>,
    pub(crate) max_blob_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderContinuityStored {
    pub(crate) continuity_id: Uuid,
    pub(crate) request_id: Uuid,
    pub(crate) step_id: Uuid,
    pub(crate) response_attempt_ordinal: u32,
    pub(crate) serving_provider_id: String,
    pub(crate) serving_model_id: String,
    pub(crate) provider_contribution_generation_id: String,
    pub(crate) continuity_kind: ProviderContinuityKind,
    pub(crate) required_for: ProviderContinuityRequiredFor,
    pub(crate) restricted_required: RestrictedContinuityPolicy,
    pub(crate) content_ref: ContentRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolCallRecorded {
    pub(crate) tool_call_id: Uuid,
    pub(crate) request_id: Uuid,
    pub(crate) step_id: Uuid,
    pub(crate) call_ordinal: u32,
    pub(crate) call_id: String,
    pub(crate) invocation_name: String,
    pub(crate) arguments_ref: ContentRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolResultDisposition {
    Denied,
    Settled,
    UnknownCompletion,
    NotDispatched,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolResultRecorded {
    pub(crate) tool_result_id: Uuid,
    pub(crate) tool_call_id: Uuid,
    pub(crate) step_id: Uuid,
    pub(crate) result_ordinal: u32,
    pub(crate) call_id: String,
    pub(crate) disposition: ToolResultDisposition,
    pub(crate) invocation_id: Option<Uuid>,
    pub(crate) lease_id: Option<Uuid>,
    pub(crate) content_ref: ContentRef,
    pub(crate) is_error: bool,
    pub(crate) reason_code: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ModelRequestOutcome {
    ResponseCompleted,
    ProviderFailed,
    Eof,
    Cancelled,
    TimedOut,
    Revoked,
    SupersededForContextRepair,
    SupersededForHistoryRepair,
    Abandoned,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelRequestClosed {
    pub(crate) request_id: Uuid,
    pub(crate) step_id: Uuid,
    pub(crate) response_attempt_ordinal: u32,
    pub(crate) outcome: ModelRequestOutcome,
    pub(crate) reason_code: String,
    pub(crate) recovery_rule_version: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StepOutcome {
    ContinueLoop,
    TurnCompleted,
    Failed,
    Eof,
    Cancelled,
    TimedOut,
    Revoked,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StepClosed {
    pub(crate) step_id: Uuid,
    pub(crate) turn_id: Uuid,
    pub(crate) outcome: StepOutcome,
    pub(crate) reason_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StepAbandoned {
    pub(crate) step_id: Uuid,
    pub(crate) turn_id: Uuid,
    pub(crate) reason_code: String,
    pub(crate) recovery_rule_version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SemanticTerminalization {
    pub(crate) turn_id: Uuid,
    pub(crate) request_outcome: ModelRequestOutcome,
    pub(crate) reason_code: String,
    pub(crate) rule_version: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TurnInterruptionRequested {
    pub(crate) interruption_id: Uuid,
    pub(crate) turn_id: Uuid,
    pub(crate) kind: InterruptionKind,
    pub(crate) principal: String,
    pub(crate) ingress: String,
    pub(crate) reason_code: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvocationRegistered {
    pub(crate) invocation_id: Uuid,
    pub(crate) turn_id: Uuid,
    pub(crate) call_id: String,
    pub(crate) owner_generation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvocationPrepared {
    pub(crate) invocation_id: Uuid,
    pub(crate) lease_id: Uuid,
    pub(crate) turn_id: Uuid,
    pub(crate) call_id: String,
    pub(crate) deduplication_id: Option<String>,
    pub(crate) invocation_kind: RuntimeInvocationKind,
    pub(crate) invocation_name: String,
    pub(crate) capability_id: RuntimeCapabilityId,
    pub(crate) contribution_id: RuntimeContributionId,
    pub(crate) owner_generation_id: RuntimeContributionGenerationId,
    pub(crate) issue_generation_id: RuntimeCompositionGenerationId,
    pub(crate) principal: String,
    pub(crate) principal_class: RuntimePrincipalClass,
    pub(crate) surface: RuntimeSurface,
    pub(crate) admitted_effects: Vec<RuntimeEffect>,
    pub(crate) execution: RuntimeExecutionPolicy,
    pub(crate) transition: RuntimeCapabilityTransitionPolicy,
    pub(crate) surfaces: Vec<RuntimeSurface>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvocationDispatched {
    pub(crate) invocation_id: Uuid,
    pub(crate) lease_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvocationAcknowledged {
    pub(crate) invocation_id: Uuid,
    pub(crate) lease_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvocationClassifiedUnknown {
    pub(crate) invocation_id: Uuid,
    pub(crate) reason_code: String,
    pub(crate) recovery_rule_version: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvocationSettled {
    pub(crate) invocation_id: Uuid,
    pub(crate) outcome: InvocationOutcome,
    pub(crate) terminal_evidence_reference: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InvocationFenceFailurePhase {
    Acknowledgement,
    TerminalSettlement,
    /// Reserved for compatibility with version-1 evidence; the current
    /// invocation path does not emit audit-settlement fences.
    AuditSettlement,
}

impl InvocationFenceFailurePhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Acknowledgement => "acknowledgement",
            Self::TerminalSettlement => "terminal_settlement",
            Self::AuditSettlement => "audit_settlement",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvocationMutationFenceEvidence {
    pub(crate) schema_version: u16,
    pub(crate) record_kind: String,
    pub(crate) fence_id: Uuid,
    pub(crate) mutation_domain: RuntimeMutationDomainId,
    pub(crate) fence_key: RuntimeMutationFenceKey,
    pub(crate) invocation_id: Uuid,
    pub(crate) call_id: String,
    pub(crate) capability_id: RuntimeCapabilityId,
    pub(crate) owner_contribution_id: RuntimeContributionId,
    pub(crate) owner_generation_id: RuntimeContributionGenerationId,
    pub(crate) issue_generation_id: RuntimeCompositionGenerationId,
    pub(crate) lease_id: Uuid,
    pub(crate) session_id: String,
    pub(crate) turn_id: Uuid,
    pub(crate) failure_phase: InvocationFenceFailurePhase,
    pub(crate) recorded_at: String,
    pub(crate) failure_reason: String,
}

pub(crate) struct InvocationMutationFenceEvidenceDraft {
    pub(crate) mutation_domain: RuntimeMutationDomainId,
    pub(crate) fence_key: RuntimeMutationFenceKey,
    pub(crate) invocation_id: Uuid,
    pub(crate) call_id: String,
    pub(crate) capability_id: RuntimeCapabilityId,
    pub(crate) owner_contribution_id: RuntimeContributionId,
    pub(crate) owner_generation_id: RuntimeContributionGenerationId,
    pub(crate) issue_generation_id: RuntimeCompositionGenerationId,
    pub(crate) lease_id: Uuid,
    pub(crate) session_id: String,
    pub(crate) turn_id: Uuid,
    pub(crate) failure_phase: InvocationFenceFailurePhase,
    pub(crate) recorded_at: String,
    pub(crate) failure_reason: String,
}

impl InvocationMutationFenceEvidence {
    pub(crate) fn new(draft: InvocationMutationFenceEvidenceDraft) -> Result<Self> {
        let fence_id = invocation_mutation_fence_id(
            &draft.mutation_domain,
            &draft.fence_key,
            draft.invocation_id,
            draft.lease_id,
            draft.failure_phase,
        );
        let evidence = Self {
            schema_version: 1,
            record_kind: "invocation_mutation_fence".into(),
            fence_id,
            mutation_domain: draft.mutation_domain,
            fence_key: draft.fence_key,
            invocation_id: draft.invocation_id,
            call_id: draft.call_id,
            capability_id: draft.capability_id,
            owner_contribution_id: draft.owner_contribution_id,
            owner_generation_id: draft.owner_generation_id,
            issue_generation_id: draft.issue_generation_id,
            lease_id: draft.lease_id,
            session_id: draft.session_id,
            turn_id: draft.turn_id,
            failure_phase: draft.failure_phase,
            recorded_at: draft.recorded_at,
            failure_reason: draft.failure_reason,
        };
        evidence.validate()?;
        Ok(evidence)
    }

    fn validate(&self) -> Result<()> {
        if self.schema_version != 1 || self.record_kind != "invocation_mutation_fence" {
            return Err(AuthorityError::Invalid(
                "unsupported invocation mutation fence record".into(),
            ));
        }
        if self.call_id.is_empty()
            || self.call_id.len() > 512
            || self.session_id.is_empty()
            || self.session_id.len() > 512
            || self.failure_reason.is_empty()
            || self.failure_reason.len() > 1024
        {
            return Err(AuthorityError::Invalid(
                "invocation mutation fence contains invalid bounded text".into(),
            ));
        }
        DateTime::parse_from_rfc3339(&self.recorded_at)
            .map_err(|_| AuthorityError::Invalid("fence recorded_at is not RFC3339".into()))?;
        let expected = invocation_mutation_fence_id(
            &self.mutation_domain,
            &self.fence_key,
            self.invocation_id,
            self.lease_id,
            self.failure_phase,
        );
        if self.fence_id != expected {
            return Err(AuthorityError::Invalid(
                "invocation mutation fence identity is invalid".into(),
            ));
        }
        Ok(())
    }
}

fn invocation_mutation_fence_id(
    domain: &RuntimeMutationDomainId,
    key: &RuntimeMutationFenceKey,
    invocation_id: Uuid,
    lease_id: Uuid,
    phase: InvocationFenceFailurePhase,
) -> Uuid {
    Uuid::new_v5(
        &INVOCATION_FENCE_NAMESPACE,
        format!(
            "{}\0{}\0{invocation_id}\0{lease_id}\0{}",
            domain.as_str(),
            key.as_str(),
            phase.as_str()
        )
        .as_bytes(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CompactionTrigger {
    ManualIdle,
    ContextPressure,
    ContextOverflow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CompactionOwnerScope {
    Turn { turn_id: Uuid, step_id: Uuid },
    SessionIdle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuthorityFrontierRef {
    pub(crate) sequence: u64,
    pub(crate) event_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompactionContextItem {
    pub(crate) ordinal: u32,
    pub(crate) source_event_id: Uuid,
    pub(crate) source_identity: String,
    pub(crate) content_ref: ContentRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompactionStarted {
    pub(crate) compaction_id: Uuid,
    pub(crate) owner_scope: CompactionOwnerScope,
    pub(crate) trigger: CompactionTrigger,
    pub(crate) source_frontier: AuthorityFrontierRef,
    pub(crate) source_context_revision: u64,
    pub(crate) input_manifest_id: String,
    pub(crate) input_items: Vec<CompactionContextItem>,
    pub(crate) retained_items: Vec<CompactionContextItem>,
    pub(crate) target_context_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompactionPromptTemplate {
    pub(crate) owner_id: String,
    pub(crate) owner_generation_id: String,
    pub(crate) content_ref: ContentRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CompactionRoute {
    TurnLease {
        lease_id: Uuid,
    },
    SessionIdle {
        selected_provider_id: String,
        selected_model_id: String,
        serving_provider_id: String,
        serving_model_id: String,
        schema_dialect: String,
        credential_source_class: String,
        fallback_reason: Option<String>,
        contribution_generation_id: String,
        route_policy: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompactionRequestPrepared {
    pub(crate) compaction_request_id: Uuid,
    pub(crate) compaction_id: Uuid,
    pub(crate) request_ordinal: u32,
    pub(crate) replaces_compaction_request_id: Option<Uuid>,
    pub(crate) prompt_template: CompactionPromptTemplate,
    pub(crate) route: CompactionRoute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CompactionResponseAttemptFailure {
    ProviderError,
    Eof,
    TimedOut,
    TransportLost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CompactionRetryDisposition {
    RetrySameRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompactionResponseAttemptFailed {
    pub(crate) compaction_request_id: Uuid,
    pub(crate) compaction_id: Uuid,
    pub(crate) response_attempt_ordinal: u32,
    pub(crate) failure: CompactionResponseAttemptFailure,
    pub(crate) reason_code: String,
    pub(crate) retry_disposition: CompactionRetryDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CompactionRequestOutcome {
    SummaryCommitted,
    ProviderFailed,
    Eof,
    Cancelled,
    TimedOut,
    SupersededForRouteChange,
    Abandoned,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompactionRequestClosed {
    pub(crate) compaction_request_id: Uuid,
    pub(crate) compaction_id: Uuid,
    pub(crate) response_attempt_ordinal: u32,
    pub(crate) outcome: CompactionRequestOutcome,
    pub(crate) reason_code: String,
    pub(crate) recovery_rule_version: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CompactionReplacementSourceKind {
    CompactionSummary,
    Retained,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompactionReplacementItem {
    pub(crate) ordinal: u32,
    pub(crate) source_kind: CompactionReplacementSourceKind,
    pub(crate) source_event_id: Uuid,
    pub(crate) source_identity: String,
    pub(crate) content_ref: ContentRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompactionUsage {
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompactionSummaryCommitted {
    pub(crate) compaction_summary_id: Uuid,
    pub(crate) compaction_request_id: Uuid,
    pub(crate) compaction_id: Uuid,
    pub(crate) response_attempt_ordinal: u32,
    pub(crate) completion_evidence: ProviderCompletionEvidence,
    pub(crate) summary_ref: ContentRef,
    pub(crate) summary_digest: String,
    pub(crate) replacement_manifest_id: String,
    pub(crate) replacement_items: Vec<CompactionReplacementItem>,
    pub(crate) usage: Option<CompactionUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompactionApplied {
    pub(crate) compaction_id: Uuid,
    pub(crate) compaction_summary_id: Uuid,
    pub(crate) source_context_revision: u64,
    pub(crate) target_context_revision: u64,
    pub(crate) replacement_manifest_id: String,
    pub(crate) recovery_rule_version: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompactionAbandoned {
    pub(crate) compaction_id: Uuid,
    pub(crate) reason_code: String,
    pub(crate) last_compaction_request_id: Option<Uuid>,
    pub(crate) last_response_attempt_ordinal: Option<u32>,
    pub(crate) recovery_rule_version: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SessionFactPayload {
    SessionCreated(SessionCreated),
    ExecutionBindingMigrated(ExecutionBindingMigrated),
    PromptAdmitted(PromptAdmitted),
    PromptRejected(PromptRejected),
    PromptRemoved(PromptRemoved),
    TurnStarted(TurnStarted),
    StepStarted(StepStarted),
    ContextSourceMaterialized(ContextSourceMaterialized),
    ModelRequestPrepared(ModelRequestPrepared),
    RouteLeaseRecorded(RouteLeaseRecorded),
    RouteEndpointProvenanceRecorded(RouteEndpointProvenanceRecorded),
    ModelRequestRouteJoined(ModelRequestRouteJoined),
    ModelResponseAttemptFailed(ModelResponseAttemptFailed),
    AssistantContentAppended(AssistantContentAppended),
    AssistantMessageCommitted(AssistantMessageCommitted),
    ProviderContinuityStored(ProviderContinuityStored),
    ToolCallRecorded(ToolCallRecorded),
    ToolResultRecorded(ToolResultRecorded),
    ModelRequestClosed(ModelRequestClosed),
    StepClosed(StepClosed),
    StepAbandoned(StepAbandoned),
    TurnInterruptionRequested(TurnInterruptionRequested),
    InvocationRegistered(InvocationRegistered),
    InvocationPrepared(InvocationPrepared),
    InvocationDispatched(InvocationDispatched),
    InvocationAcknowledged(InvocationAcknowledged),
    InvocationClassifiedUnknown(InvocationClassifiedUnknown),
    InvocationSettled(InvocationSettled),
    TurnClosed(TurnClosed),
    CompactionStarted(CompactionStarted),
    CompactionRequestPrepared(CompactionRequestPrepared),
    CompactionEndpointProvenanceRecorded(CompactionEndpointProvenanceRecorded),
    CompactionResponseAttemptFailed(CompactionResponseAttemptFailed),
    CompactionRequestClosed(CompactionRequestClosed),
    CompactionSummaryCommitted(CompactionSummaryCommitted),
    CompactionApplied(CompactionApplied),
    CompactionAbandoned(CompactionAbandoned),
}

impl SessionFactPayload {
    pub(crate) fn event_type(&self) -> &'static str {
        match self {
            Self::SessionCreated(_) => "session.created",
            Self::ExecutionBindingMigrated(_) => "session.execution_binding_migrated",
            Self::PromptAdmitted(_) => "prompt.admitted",
            Self::PromptRejected(_) => "prompt.rejected",
            Self::PromptRemoved(_) => "prompt.removed",
            Self::TurnStarted(_) => "turn.started",
            Self::StepStarted(_) => "step.started",
            Self::ContextSourceMaterialized(_) => "context.source_materialized",
            Self::ModelRequestPrepared(_) => "model.request_prepared",
            Self::RouteLeaseRecorded(_) => "route.lease_recorded",
            Self::RouteEndpointProvenanceRecorded(_) => "route.endpoint_provenance_recorded",
            Self::ModelRequestRouteJoined(_) => "model.request_route_joined",
            Self::ModelResponseAttemptFailed(_) => "model.response_attempt_failed",
            Self::AssistantContentAppended(_) => "assistant.content_appended",
            Self::AssistantMessageCommitted(_) => "assistant.message_committed",
            Self::ProviderContinuityStored(_) => "provider.continuity_stored",
            Self::ToolCallRecorded(_) => "tool.call_recorded",
            Self::ToolResultRecorded(_) => "tool.result_recorded",
            Self::ModelRequestClosed(_) => "model.request_closed",
            Self::StepClosed(_) => "step.closed",
            Self::StepAbandoned(_) => "step.abandoned",
            Self::TurnInterruptionRequested(_) => "turn.interruption_requested",
            Self::InvocationRegistered(_) => "invocation.registered",
            Self::InvocationPrepared(_) => "invocation.prepared",
            Self::InvocationDispatched(_) => "invocation.dispatched",
            Self::InvocationAcknowledged(_) => "invocation.acknowledged",
            Self::InvocationClassifiedUnknown(_) => "invocation.classified_unknown",
            Self::InvocationSettled(_) => "invocation.settled",
            Self::TurnClosed(_) => "turn.closed",
            Self::CompactionStarted(_) => "compaction.started",
            Self::CompactionRequestPrepared(_) => "compaction.request_prepared",
            Self::CompactionEndpointProvenanceRecorded(_) => {
                "compaction.endpoint_provenance_recorded"
            }
            Self::CompactionResponseAttemptFailed(_) => "compaction.response_attempt_failed",
            Self::CompactionRequestClosed(_) => "compaction.request_closed",
            Self::CompactionSummaryCommitted(_) => "compaction.summary_committed",
            Self::CompactionApplied(_) => "compaction.applied",
            Self::CompactionAbandoned(_) => "compaction.abandoned",
        }
    }

    fn to_value(&self) -> serde_json::Result<Value> {
        match self {
            Self::SessionCreated(value) => serde_json::to_value(value),
            Self::ExecutionBindingMigrated(value) => serde_json::to_value(value),
            Self::PromptAdmitted(value) => serde_json::to_value(value),
            Self::PromptRejected(value) => serde_json::to_value(value),
            Self::PromptRemoved(value) => serde_json::to_value(value),
            Self::TurnStarted(value) => serde_json::to_value(value),
            Self::StepStarted(value) => serde_json::to_value(value),
            Self::ContextSourceMaterialized(value) => serde_json::to_value(value),
            Self::ModelRequestPrepared(value) => serde_json::to_value(value),
            Self::RouteLeaseRecorded(value) => serde_json::to_value(value),
            Self::RouteEndpointProvenanceRecorded(value) => serde_json::to_value(value),
            Self::ModelRequestRouteJoined(value) => serde_json::to_value(value),
            Self::ModelResponseAttemptFailed(value) => serde_json::to_value(value),
            Self::AssistantContentAppended(value) => serde_json::to_value(value),
            Self::AssistantMessageCommitted(value) => serde_json::to_value(value),
            Self::ProviderContinuityStored(value) => serde_json::to_value(value),
            Self::ToolCallRecorded(value) => serde_json::to_value(value),
            Self::ToolResultRecorded(value) => serde_json::to_value(value),
            Self::ModelRequestClosed(value) => serde_json::to_value(value),
            Self::StepClosed(value) => serde_json::to_value(value),
            Self::StepAbandoned(value) => serde_json::to_value(value),
            Self::TurnInterruptionRequested(value) => serde_json::to_value(value),
            Self::InvocationRegistered(value) => serde_json::to_value(value),
            Self::InvocationPrepared(value) => serde_json::to_value(value),
            Self::InvocationDispatched(value) => serde_json::to_value(value),
            Self::InvocationAcknowledged(value) => serde_json::to_value(value),
            Self::InvocationClassifiedUnknown(value) => serde_json::to_value(value),
            Self::InvocationSettled(value) => serde_json::to_value(value),
            Self::TurnClosed(value) => serde_json::to_value(value),
            Self::CompactionStarted(value) => serde_json::to_value(value),
            Self::CompactionRequestPrepared(value) => serde_json::to_value(value),
            Self::CompactionEndpointProvenanceRecorded(value) => serde_json::to_value(value),
            Self::CompactionResponseAttemptFailed(value) => serde_json::to_value(value),
            Self::CompactionRequestClosed(value) => serde_json::to_value(value),
            Self::CompactionSummaryCommitted(value) => serde_json::to_value(value),
            Self::CompactionApplied(value) => serde_json::to_value(value),
            Self::CompactionAbandoned(value) => serde_json::to_value(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionFact {
    pub(crate) event_id: Uuid,
    pub(crate) session_id: String,
    pub(crate) stream_id: Uuid,
    pub(crate) sequence: u64,
    pub(crate) command_id: Uuid,
    pub(crate) command_fingerprint: String,
    pub(crate) causation_event_id: Option<Uuid>,
    pub(crate) recorded_at: String,
    pub(crate) payload: SessionFactPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthorityLineageLevel {
    #[default]
    LegacyOnly,
    Mixed,
    FullSpine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FullSpineBoundary {
    pub(crate) sequence: u64,
    pub(crate) event_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionFactWire {
    envelope_version: u16,
    event_id: Uuid,
    session_id: String,
    stream_id: Uuid,
    sequence: u64,
    event_type: String,
    event_version: u16,
    command_id: Uuid,
    command_fingerprint: String,
    causation_event_id: Option<Uuid>,
    recorded_at: String,
    payload: Value,
}

impl SessionFact {
    pub(crate) fn new(
        session_id: impl Into<String>,
        stream_id: Uuid,
        sequence: u64,
        command_id: Uuid,
        command_fingerprint: impl Into<String>,
        recorded_at: impl Into<String>,
        payload: SessionFactPayload,
    ) -> Self {
        Self {
            event_id: Uuid::new_v4(),
            session_id: session_id.into(),
            stream_id,
            sequence,
            command_id,
            command_fingerprint: command_fingerprint.into(),
            causation_event_id: None,
            recorded_at: recorded_at.into(),
            payload,
        }
    }

    fn encode(&self) -> Result<Vec<u8>> {
        self.validate_envelope()?;
        let wire = SessionFactWire {
            envelope_version: ENVELOPE_VERSION,
            event_id: self.event_id,
            session_id: self.session_id.clone(),
            stream_id: self.stream_id,
            sequence: self.sequence,
            event_type: self.payload.event_type().to_string(),
            event_version: EVENT_VERSION,
            command_id: self.command_id,
            command_fingerprint: self.command_fingerprint.clone(),
            causation_event_id: self.causation_event_id,
            recorded_at: self.recorded_at.clone(),
            payload: self.payload.to_value()?,
        };
        Ok(serde_json::to_vec(&wire)?)
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        let wire: SessionFactWire = serde_json::from_slice(bytes)?;
        if wire.envelope_version != ENVELOPE_VERSION {
            return Err(AuthorityError::Invalid(format!(
                "unsupported envelope version {}",
                wire.envelope_version
            )));
        }
        if wire.event_version != EVENT_VERSION {
            return Err(AuthorityError::Invalid(format!(
                "unsupported {} event version {}",
                wire.event_type, wire.event_version
            )));
        }
        validate_slice_5_uuid_text(&wire.event_type, &wire.payload)?;
        let payload = match wire.event_type.as_str() {
            "session.created" => {
                decode_payload(wire.payload).map(SessionFactPayload::SessionCreated)
            }
            "session.execution_binding_migrated" => {
                decode_payload(wire.payload).map(SessionFactPayload::ExecutionBindingMigrated)
            }
            "prompt.admitted" => {
                decode_payload(wire.payload).map(SessionFactPayload::PromptAdmitted)
            }
            "prompt.rejected" => {
                decode_payload(wire.payload).map(SessionFactPayload::PromptRejected)
            }
            "prompt.removed" => decode_payload(wire.payload).map(SessionFactPayload::PromptRemoved),
            "turn.started" => decode_payload(wire.payload).map(SessionFactPayload::TurnStarted),
            "step.started" => decode_payload(wire.payload).map(SessionFactPayload::StepStarted),
            "context.source_materialized" => {
                decode_payload(wire.payload).map(SessionFactPayload::ContextSourceMaterialized)
            }
            "model.request_prepared" => {
                decode_payload(wire.payload).map(SessionFactPayload::ModelRequestPrepared)
            }
            "route.lease_recorded" => {
                decode_payload(wire.payload).map(SessionFactPayload::RouteLeaseRecorded)
            }
            "route.endpoint_provenance_recorded" => decode_payload(wire.payload)
                .map(SessionFactPayload::RouteEndpointProvenanceRecorded),
            "model.request_route_joined" => {
                decode_payload(wire.payload).map(SessionFactPayload::ModelRequestRouteJoined)
            }
            "model.response_attempt_failed" => {
                decode_payload(wire.payload).map(SessionFactPayload::ModelResponseAttemptFailed)
            }
            "assistant.content_appended" => {
                decode_payload(wire.payload).map(SessionFactPayload::AssistantContentAppended)
            }
            "assistant.message_committed" => {
                decode_payload(wire.payload).map(SessionFactPayload::AssistantMessageCommitted)
            }
            "provider.continuity_stored" => {
                decode_payload(wire.payload).map(SessionFactPayload::ProviderContinuityStored)
            }
            "tool.call_recorded" => {
                decode_payload(wire.payload).map(SessionFactPayload::ToolCallRecorded)
            }
            "tool.result_recorded" => {
                decode_payload(wire.payload).map(SessionFactPayload::ToolResultRecorded)
            }
            "model.request_closed" => {
                decode_payload(wire.payload).map(SessionFactPayload::ModelRequestClosed)
            }
            "step.closed" => decode_payload(wire.payload).map(SessionFactPayload::StepClosed),
            "step.abandoned" => decode_payload(wire.payload).map(SessionFactPayload::StepAbandoned),
            "turn.interruption_requested" => {
                decode_payload(wire.payload).map(SessionFactPayload::TurnInterruptionRequested)
            }
            "invocation.registered" => {
                decode_payload(wire.payload).map(SessionFactPayload::InvocationRegistered)
            }
            "invocation.prepared" => {
                decode_payload(wire.payload).map(SessionFactPayload::InvocationPrepared)
            }
            "invocation.dispatched" => {
                decode_payload(wire.payload).map(SessionFactPayload::InvocationDispatched)
            }
            "invocation.acknowledged" => {
                decode_payload(wire.payload).map(SessionFactPayload::InvocationAcknowledged)
            }
            "invocation.classified_unknown" => {
                decode_payload(wire.payload).map(SessionFactPayload::InvocationClassifiedUnknown)
            }
            "invocation.settled" => {
                decode_payload(wire.payload).map(SessionFactPayload::InvocationSettled)
            }
            "turn.closed" => decode_payload(wire.payload).map(SessionFactPayload::TurnClosed),
            "compaction.started" => {
                decode_payload(wire.payload).map(SessionFactPayload::CompactionStarted)
            }
            "compaction.request_prepared" => {
                decode_payload(wire.payload).map(SessionFactPayload::CompactionRequestPrepared)
            }
            "compaction.endpoint_provenance_recorded" => decode_payload(wire.payload)
                .map(SessionFactPayload::CompactionEndpointProvenanceRecorded),
            "compaction.response_attempt_failed" => decode_payload(wire.payload)
                .map(SessionFactPayload::CompactionResponseAttemptFailed),
            "compaction.request_closed" => {
                decode_payload(wire.payload).map(SessionFactPayload::CompactionRequestClosed)
            }
            "compaction.summary_committed" => {
                decode_payload(wire.payload).map(SessionFactPayload::CompactionSummaryCommitted)
            }
            "compaction.applied" => {
                decode_payload(wire.payload).map(SessionFactPayload::CompactionApplied)
            }
            "compaction.abandoned" => {
                decode_payload(wire.payload).map(SessionFactPayload::CompactionAbandoned)
            }
            unknown => Err(AuthorityError::Invalid(format!(
                "unsupported authority event type {unknown}"
            ))),
        }?;
        let fact = Self {
            event_id: wire.event_id,
            session_id: wire.session_id,
            stream_id: wire.stream_id,
            sequence: wire.sequence,
            command_id: wire.command_id,
            command_fingerprint: wire.command_fingerprint,
            causation_event_id: wire.causation_event_id,
            recorded_at: wire.recorded_at,
            payload,
        };
        fact.validate_envelope()?;
        Ok(fact)
    }

    fn validate_envelope(&self) -> Result<()> {
        if self.session_id.is_empty() || self.session_id.len() > 256 {
            return Err(AuthorityError::Invalid("invalid session ID".into()));
        }
        if self.sequence == 0 {
            return Err(AuthorityError::Invalid("sequence zero is invalid".into()));
        }
        if !is_sha256_hex(&self.command_fingerprint) {
            return Err(AuthorityError::Invalid(
                "command fingerprint must be lowercase SHA-256 hex".into(),
            ));
        }
        let timestamp = DateTime::parse_from_rfc3339(&self.recorded_at)
            .map_err(|error| AuthorityError::Invalid(format!("invalid recorded_at: {error}")))?;
        if timestamp.offset().local_minus_utc() != 0 {
            return Err(AuthorityError::Invalid("recorded_at must be UTC".into()));
        }
        Ok(())
    }
}

fn decode_payload<T: DeserializeOwned>(value: Value) -> Result<T> {
    Ok(serde_json::from_value(value)?)
}

fn validate_slice_5_uuid_text(event_type: &str, payload: &Value) -> Result<()> {
    fn field<'a>(payload: &'a Value, name: &str) -> Result<Option<&'a str>> {
        match payload.get(name) {
            Some(Value::String(value)) => Ok(Some(value)),
            Some(Value::Null) | None => Ok(None),
            Some(_) => Err(AuthorityError::Invalid(format!(
                "{name} must be canonical UUID text"
            ))),
        }
    }

    fn canonical(value: &str, label: &str, entity: bool) -> Result<()> {
        let id = Uuid::parse_str(value)
            .map_err(|_| AuthorityError::Invalid(format!("{label} is not a UUID")))?;
        if id.to_string() != value || (entity && !matches!(id.get_version_num(), 4 | 7)) {
            return Err(AuthorityError::Invalid(format!(
                "{label} is not canonical UUIDv4 or UUIDv7 text"
            )));
        }
        Ok(())
    }

    let entity_fields: &[&str] = match event_type {
        "step.started" => &["step_id", "turn_id"],
        "context.source_materialized" => &["context_source_id"],
        "model.request_prepared" => &["request_id", "step_id", "turn_id", "replaces_request_id"],
        "model.request_route_joined" => &["request_id", "step_id", "turn_id", "lease_id"],
        "model.response_attempt_failed" => &["request_id", "step_id"],
        "assistant.content_appended" => &["message_id", "request_id", "step_id"],
        "assistant.message_committed" => &["message_id", "request_id", "step_id"],
        "provider.continuity_stored" => &["continuity_id", "request_id", "step_id"],
        "tool.call_recorded" => &["tool_call_id", "request_id", "step_id"],
        "tool.result_recorded" => &[
            "tool_result_id",
            "tool_call_id",
            "step_id",
            "invocation_id",
            "lease_id",
        ],
        "model.request_closed" => &["request_id", "step_id"],
        "step.closed" | "step.abandoned" => &["step_id", "turn_id"],
        "compaction.started" => &["compaction_id"],
        "compaction.request_prepared" => &[
            "compaction_request_id",
            "compaction_id",
            "replaces_compaction_request_id",
        ],
        "compaction.response_attempt_failed" | "compaction.request_closed" => {
            &["compaction_request_id", "compaction_id"]
        }
        "compaction.summary_committed" => &[
            "compaction_summary_id",
            "compaction_request_id",
            "compaction_id",
        ],
        "compaction.applied" => &["compaction_id", "compaction_summary_id"],
        "compaction.abandoned" => &["compaction_id", "last_compaction_request_id"],
        _ => return Ok(()),
    };
    for name in entity_fields {
        if let Some(value) = field(payload, name)? {
            canonical(value, name, true)?;
        }
    }
    if event_type == "model.request_prepared" {
        if let Some(continuity_refs) = payload.get("continuity_refs") {
            let refs = continuity_refs.as_array().ok_or_else(|| {
                AuthorityError::Invalid("continuity_refs must be an array".into())
            })?;
            for value in refs {
                let value = value.as_str().ok_or_else(|| {
                    AuthorityError::Invalid("continuity reference must be UUID text".into())
                })?;
                canonical(value, "continuity reference", true)?;
            }
        }
        if let Some(items) = payload.get("context_items").and_then(Value::as_array) {
            for item in items {
                if let Some(value) = item
                    .get("provenance")
                    .and_then(|value| value.get("source_event_id"))
                    .filter(|value| !value.is_null())
                {
                    let value = value.as_str().ok_or_else(|| {
                        AuthorityError::Invalid("source event ID must be UUID text".into())
                    })?;
                    canonical(value, "source event ID", false)?;
                }
            }
        }
    }
    if event_type == "compaction.started" {
        if let Some(owner) = payload.get("owner_scope") {
            for name in ["turn_id", "step_id"] {
                if let Some(value) = field(owner, name)? {
                    canonical(value, name, true)?;
                }
            }
        }
        if let Some(frontier) = payload.get("source_frontier")
            && let Some(value) = field(frontier, "event_id")?
        {
            canonical(value, "source frontier event ID", false)?;
        }
    }
    if event_type == "compaction.request_prepared"
        && let Some(route) = payload.get("route")
        && let Some(value) = field(route, "lease_id")?
    {
        canonical(value, "compaction route lease ID", true)?;
    }
    if matches!(
        event_type,
        "compaction.started" | "compaction.summary_committed"
    ) {
        let list_names: &[&str] = if event_type == "compaction.started" {
            &["input_items", "retained_items"]
        } else {
            &["replacement_items"]
        };
        for list_name in list_names {
            if let Some(items) = payload.get(list_name).and_then(Value::as_array) {
                for item in items {
                    if let Some(value) = field(item, "source_event_id")? {
                        canonical(value, "compaction source event ID", false)?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActiveTurn {
    pub(crate) turn_id: Uuid,
    pub(crate) prompt: PromptAdmitted,
    pub(crate) runtime_generation_id: String,
    pub(crate) accepted_interruption: Option<TurnInterruptionRequested>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum InvocationState {
    Registered {
        registration: InvocationRegistered,
    },
    Prepared {
        preparation: InvocationPrepared,
    },
    Dispatched {
        preparation: InvocationPrepared,
        dispatch: InvocationDispatched,
    },
    Acknowledged {
        preparation: InvocationPrepared,
        dispatch: InvocationDispatched,
        acknowledgement: InvocationAcknowledged,
    },
    Unknown {
        registration: InvocationRegistered,
        classification: InvocationClassifiedUnknown,
    },
    Settled {
        registration: InvocationRegistered,
        settlement: InvocationSettled,
    },
    DurableUnknown {
        preparation: InvocationPrepared,
        dispatch: InvocationDispatched,
        acknowledgement: Option<InvocationAcknowledged>,
        classification: InvocationClassifiedUnknown,
    },
    DurableSettled {
        preparation: InvocationPrepared,
        dispatch: InvocationDispatched,
        acknowledgement: InvocationAcknowledged,
        settlement: InvocationSettled,
    },
}

impl InvocationState {
    fn turn_id(&self) -> Uuid {
        match self {
            Self::Registered { registration }
            | Self::Unknown { registration, .. }
            | Self::Settled { registration, .. } => registration.turn_id,
            Self::Prepared { preparation }
            | Self::Dispatched { preparation, .. }
            | Self::Acknowledged { preparation, .. }
            | Self::DurableUnknown { preparation, .. }
            | Self::DurableSettled { preparation, .. } => preparation.turn_id,
        }
    }

    fn call_id(&self) -> &str {
        match self {
            Self::Registered { registration }
            | Self::Unknown { registration, .. }
            | Self::Settled { registration, .. } => &registration.call_id,
            Self::Prepared { preparation }
            | Self::Dispatched { preparation, .. }
            | Self::Acknowledged { preparation, .. }
            | Self::DurableUnknown { preparation, .. }
            | Self::DurableSettled { preparation, .. } => &preparation.call_id,
        }
    }

    fn preparation(&self) -> Option<&InvocationPrepared> {
        match self {
            Self::Prepared { preparation }
            | Self::Dispatched { preparation, .. }
            | Self::Acknowledged { preparation, .. }
            | Self::DurableUnknown { preparation, .. }
            | Self::DurableSettled { preparation, .. } => Some(preparation),
            Self::Registered { .. } | Self::Unknown { .. } | Self::Settled { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnknownRetryDisposition {
    None,
    Safe { invocation_id: Uuid },
    Unsafe { invocation_id: Uuid },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CommandReceipt {
    pub(crate) command_id: Uuid,
    pub(crate) fingerprint: String,
    pub(crate) event_id: Uuid,
    pub(crate) sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SubmissionDisposition {
    Admitted { prompt_id: Uuid },
    Rejected { reason_code: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActiveStep {
    pub(crate) start: StepStarted,
    pub(crate) active_request_id: Option<Uuid>,
    pub(crate) next_request_ordinal: u32,
    #[serde(default)]
    pub(crate) next_call_ordinal: u32,
    #[serde(default)]
    pub(crate) next_result_ordinal: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum StepTerminalState {
    Closed { closure: StepClosed },
    Abandoned { abandonment: StepAbandoned },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ModelRequestState {
    Open {
        preparation: ModelRequestPrepared,
        route_join: Option<ModelRequestRouteJoined>,
    },
    Closed {
        preparation: ModelRequestPrepared,
        route_join: Option<ModelRequestRouteJoined>,
        closure: ModelRequestClosed,
    },
}

impl ModelRequestState {
    pub(crate) fn preparation(&self) -> &ModelRequestPrepared {
        match self {
            Self::Open { preparation, .. } | Self::Closed { preparation, .. } => preparation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CompactionRequestState {
    Open {
        preparation: CompactionRequestPrepared,
    },
    Closed {
        preparation: CompactionRequestPrepared,
        closure: CompactionRequestClosed,
    },
}

impl CompactionRequestState {
    pub(crate) fn preparation(&self) -> &CompactionRequestPrepared {
        match self {
            Self::Open { preparation } | Self::Closed { preparation, .. } => preparation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CompactionTerminalState {
    Applied { application: CompactionApplied },
    Abandoned { abandonment: CompactionAbandoned },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionAuthorityState {
    pub(crate) session_id: Option<String>,
    pub(crate) stream_id: Option<Uuid>,
    pub(crate) workspace_identity: Option<String>,
    pub(crate) runtime_generation_id: Option<String>,
    #[serde(default)]
    pub(crate) execution_binding_generation: Option<ExecutionBindingGeneration>,
    pub(crate) last_sequence: u64,
    pub(crate) last_event_id: Option<Uuid>,
    #[serde(default)]
    pub(crate) lineage_level: AuthorityLineageLevel,
    #[serde(default)]
    pub(crate) full_spine_boundary: Option<FullSpineBoundary>,
    pub(crate) submissions: BTreeMap<Uuid, SubmissionDisposition>,
    pub(crate) prompt_ids: BTreeMap<Uuid, Uuid>,
    #[serde(default)]
    pub(crate) prompts: BTreeMap<Uuid, PromptAdmitted>,
    pub(crate) queued_prompts: Vec<PromptAdmitted>,
    pub(crate) turn_starts: BTreeMap<Uuid, TurnStarted>,
    #[serde(default)]
    pub(crate) prompt_source_events: BTreeMap<Uuid, Uuid>,
    #[serde(default)]
    pub(crate) materialized_context_sources: BTreeMap<Uuid, ContextSourceMaterialized>,
    #[serde(default)]
    pub(crate) materialized_context_source_events: BTreeMap<Uuid, Uuid>,
    #[serde(default)]
    pub(crate) steps: BTreeMap<Uuid, StepStarted>,
    #[serde(default)]
    pub(crate) next_step_ordinals: BTreeMap<Uuid, u32>,
    #[serde(default)]
    pub(crate) active_step: Option<ActiveStep>,
    #[serde(default)]
    pub(crate) model_requests: BTreeMap<Uuid, ModelRequestState>,
    #[serde(default)]
    pub(crate) model_request_source_events: BTreeMap<Uuid, Uuid>,
    #[serde(default)]
    pub(crate) route_leases: BTreeMap<Uuid, RouteLeaseRecorded>,
    #[serde(default)]
    pub(crate) route_endpoint_provenance: BTreeMap<Uuid, RouteEndpointProvenanceRecorded>,
    #[serde(default)]
    pub(crate) request_route_joins: BTreeMap<Uuid, ModelRequestRouteJoined>,
    #[serde(default)]
    pub(crate) joined_route_leases: BTreeMap<Uuid, Uuid>,
    #[serde(default)]
    pub(crate) response_attempt_failures: BTreeMap<Uuid, BTreeMap<u32, ModelResponseAttemptFailed>>,
    #[serde(default)]
    pub(crate) assistant_chunks: BTreeMap<Uuid, Vec<AssistantContentAppended>>,
    #[serde(default)]
    pub(crate) assistant_messages: BTreeMap<Uuid, AssistantMessageCommitted>,
    #[serde(default)]
    pub(crate) request_message_commits: BTreeMap<Uuid, Uuid>,
    #[serde(default)]
    pub(crate) assistant_message_source_events: BTreeMap<Uuid, Uuid>,
    #[serde(default)]
    pub(crate) provider_continuity: BTreeMap<Uuid, ProviderContinuityStored>,
    #[serde(default)]
    pub(crate) tool_calls: BTreeMap<Uuid, ToolCallRecorded>,
    #[serde(default)]
    pub(crate) turn_call_ids: BTreeMap<Uuid, BTreeMap<String, Uuid>>,
    #[serde(default)]
    pub(crate) tool_results: BTreeMap<Uuid, ToolResultRecorded>,
    #[serde(default)]
    pub(crate) call_results: BTreeMap<Uuid, Uuid>,
    #[serde(default)]
    pub(crate) tool_result_source_events: BTreeMap<Uuid, Uuid>,
    #[serde(default)]
    pub(crate) terminal_steps: BTreeMap<Uuid, StepTerminalState>,
    pub(crate) interruption_requests: BTreeMap<Uuid, TurnInterruptionRequested>,
    pub(crate) active_turn: Option<ActiveTurn>,
    pub(crate) invocations: BTreeMap<Uuid, InvocationState>,
    pub(crate) closed_turns: BTreeMap<Uuid, TurnClosed>,
    #[serde(default)]
    pub(crate) context_revision: u64,
    #[serde(default)]
    pub(crate) active_compaction: Option<Uuid>,
    #[serde(default)]
    pub(crate) compaction_starts: BTreeMap<Uuid, CompactionStarted>,
    #[serde(default)]
    pub(crate) compaction_requests: BTreeMap<Uuid, CompactionRequestState>,
    #[serde(default)]
    pub(crate) compaction_endpoint_provenance: BTreeMap<Uuid, CompactionEndpointProvenanceRecorded>,
    #[serde(default)]
    pub(crate) compaction_attempt_failures:
        BTreeMap<Uuid, BTreeMap<u32, CompactionResponseAttemptFailed>>,
    #[serde(default)]
    pub(crate) compaction_summaries: BTreeMap<Uuid, CompactionSummaryCommitted>,
    #[serde(default)]
    pub(crate) compaction_summary_source_events: BTreeMap<Uuid, Uuid>,
    #[serde(default)]
    pub(crate) compaction_summary_by_operation: BTreeMap<Uuid, Uuid>,
    #[serde(default)]
    pub(crate) compaction_terminals: BTreeMap<Uuid, CompactionTerminalState>,
    #[serde(default)]
    pub(crate) replacement_manifests: BTreeMap<String, Vec<CompactionReplacementItem>>,
    pub(crate) command_receipts: BTreeMap<Uuid, CommandReceipt>,
}

impl SessionAuthorityState {
    fn unknown_retry_disposition(&self, call_id: &str) -> Result<UnknownRetryDisposition> {
        let mut disposition = UnknownRetryDisposition::None;
        for (invocation_id, invocation) in &self.invocations {
            if invocation.call_id() != call_id {
                continue;
            }
            let candidate = match invocation {
                InvocationState::Unknown { .. } => UnknownRetryDisposition::Unsafe {
                    invocation_id: *invocation_id,
                },
                InvocationState::DurableUnknown { preparation, .. }
                    if omegon_traits::runtime_effects_mutate(&preparation.admitted_effects) =>
                {
                    let safe = preparation.execution.idempotency
                        == omegon_traits::RuntimeIdempotency::Idempotent
                        || (preparation.execution.deduplication
                            == omegon_traits::RuntimeDeduplication::OwnerEnforcedStableCallId
                            && preparation.deduplication_id.as_deref() == Some(call_id));
                    if safe {
                        UnknownRetryDisposition::Safe {
                            invocation_id: *invocation_id,
                        }
                    } else {
                        UnknownRetryDisposition::Unsafe {
                            invocation_id: *invocation_id,
                        }
                    }
                }
                _ => UnknownRetryDisposition::None,
            };
            if candidate != UnknownRetryDisposition::None {
                if disposition != UnknownRetryDisposition::None {
                    return Err(AuthorityError::Invalid(
                        "multiple unknown invocations share one stable call identity".into(),
                    ));
                }
                disposition = candidate;
            }
        }
        Ok(disposition)
    }

    pub(crate) fn apply(&mut self, fact: &SessionFact) -> Result<()> {
        let expected_sequence =
            self.last_sequence
                .checked_add(1)
                .ok_or_else(|| AuthorityError::Transition {
                    sequence: fact.sequence,
                    message: "sequence overflow".into(),
                })?;
        if fact.sequence != expected_sequence {
            return self.transition_error(
                fact.sequence,
                format!(
                    "expected sequence {expected_sequence}, got {}",
                    fact.sequence
                ),
            );
        }
        if self.command_receipts.contains_key(&fact.command_id) {
            return self
                .transition_error(fact.sequence, "duplicate command ID in authority stream");
        }
        if self
            .command_receipts
            .values()
            .any(|receipt| receipt.event_id == fact.event_id)
        {
            return self.transition_error(fact.sequence, "duplicate event ID in authority stream");
        }

        if self.last_sequence == 0 {
            let SessionFactPayload::SessionCreated(created) = &fact.payload else {
                return self
                    .transition_error(fact.sequence, "sequence one must create the session");
            };
            if fact.sequence != 1 {
                return self
                    .transition_error(fact.sequence, "session creation must be sequence one");
            }
            self.session_id = Some(fact.session_id.clone());
            self.stream_id = Some(fact.stream_id);
            self.workspace_identity = Some(created.workspace_identity.clone());
            self.runtime_generation_id = Some(created.runtime_generation_id.clone());
        } else {
            if self.session_id.as_deref() != Some(fact.session_id.as_str())
                || self.stream_id != Some(fact.stream_id)
            {
                return self.transition_error(fact.sequence, "session or stream identity changed");
            }
            self.apply_transition(fact)?;
            self.record_full_spine_boundary(fact);
        }

        self.last_sequence = fact.sequence;
        self.last_event_id = Some(fact.event_id);
        self.command_receipts.insert(
            fact.command_id,
            CommandReceipt {
                command_id: fact.command_id,
                fingerprint: fact.command_fingerprint.clone(),
                event_id: fact.event_id,
                sequence: fact.sequence,
            },
        );
        Ok(())
    }

    fn apply_transition(&mut self, fact: &SessionFact) -> Result<()> {
        if self.active_compaction.is_some()
            && !matches!(
                fact.payload,
                SessionFactPayload::RouteLeaseRecorded(_)
                    | SessionFactPayload::RouteEndpointProvenanceRecorded(_)
                    | SessionFactPayload::CompactionRequestPrepared(_)
                    | SessionFactPayload::CompactionEndpointProvenanceRecorded(_)
                    | SessionFactPayload::CompactionResponseAttemptFailed(_)
                    | SessionFactPayload::CompactionRequestClosed(_)
                    | SessionFactPayload::CompactionSummaryCommitted(_)
                    | SessionFactPayload::CompactionApplied(_)
                    | SessionFactPayload::CompactionAbandoned(_)
            )
        {
            return self.transition_error(
                fact.sequence,
                "ordinary session transition is blocked by active compaction",
            );
        }
        match &fact.payload {
            SessionFactPayload::SessionCreated(_) => {
                self.transition_error(fact.sequence, "session is already created")
            }
            SessionFactPayload::ExecutionBindingMigrated(migration) => {
                if self.active_turn.is_some() {
                    return self.transition_error(
                        fact.sequence,
                        "execution binding cannot migrate during an active turn",
                    );
                }
                if self.invocations.values().any(invocation_blocks_migration) {
                    return self.transition_error(
                        fact.sequence,
                        "execution binding cannot migrate with an unresolved invocation",
                    );
                }
                if self
                    .execution_binding_generation
                    .as_ref()
                    .is_some_and(|current| current != &migration.from_generation)
                {
                    return self.transition_error(
                        fact.sequence,
                        "execution binding migration source is stale",
                    );
                }
                if migration.from_generation == migration.target_generation {
                    return self.transition_error(
                        fact.sequence,
                        "execution binding migration target is unchanged",
                    );
                }
                self.execution_binding_generation = Some(migration.target_generation.clone());
                Ok(())
            }
            SessionFactPayload::PromptAdmitted(prompt) => {
                if self.active_compaction.is_some() {
                    return self.transition_error(
                        fact.sequence,
                        "prompt admission is blocked by active compaction",
                    );
                }
                if self.prompt_ids.contains_key(&prompt.prompt_id)
                    || self.submissions.contains_key(&prompt.submission_id)
                {
                    return self
                        .transition_error(fact.sequence, "prompt identity is already present");
                }
                self.submissions.insert(
                    prompt.submission_id,
                    SubmissionDisposition::Admitted {
                        prompt_id: prompt.prompt_id,
                    },
                );
                self.prompt_ids
                    .insert(prompt.prompt_id, prompt.submission_id);
                self.prompts.insert(prompt.prompt_id, prompt.clone());
                self.prompt_source_events
                    .insert(prompt.prompt_id, fact.event_id);
                self.queued_prompts.push(prompt.clone());
                Ok(())
            }
            SessionFactPayload::PromptRejected(rejected) => {
                if self.submissions.contains_key(&rejected.submission_id) {
                    return self.transition_error(
                        fact.sequence,
                        "submission identity already has an outcome",
                    );
                }
                self.submissions.insert(
                    rejected.submission_id,
                    SubmissionDisposition::Rejected {
                        reason_code: rejected.reason_code.clone(),
                    },
                );
                Ok(())
            }
            SessionFactPayload::PromptRemoved(removed) => {
                let Some(index) = self
                    .queued_prompts
                    .iter()
                    .position(|prompt| prompt.prompt_id == removed.prompt_id)
                else {
                    return self.transition_error(fact.sequence, "queued prompt was not found");
                };
                self.queued_prompts.remove(index);
                Ok(())
            }
            SessionFactPayload::TurnStarted(started) => {
                if self.active_turn.is_some() {
                    return self.transition_error(fact.sequence, "a turn is already active");
                }
                if self.turn_starts.contains_key(&started.turn_id) {
                    return self
                        .transition_error(fact.sequence, "turn identity is already present");
                }
                let Some(prompt) = self.queued_prompts.first() else {
                    return self.transition_error(fact.sequence, "prompt queue is empty");
                };
                if prompt.prompt_id != started.prompt_id {
                    return self
                        .transition_error(fact.sequence, "turn must start from FIFO queue head");
                }
                let prompt = self.queued_prompts.remove(0);
                self.turn_starts.insert(started.turn_id, started.clone());
                self.active_turn = Some(ActiveTurn {
                    turn_id: started.turn_id,
                    prompt,
                    runtime_generation_id: started.runtime_generation_id.clone(),
                    accepted_interruption: None,
                });
                Ok(())
            }
            SessionFactPayload::StepStarted(started) => {
                let Some(active) = self.active_turn.as_ref() else {
                    return self.transition_error(fact.sequence, "there is no active turn");
                };
                if active.turn_id != started.turn_id {
                    return self.transition_error(fact.sequence, "step targets a stale turn");
                }
                if active.accepted_interruption.is_some() {
                    return self.transition_error(
                        fact.sequence,
                        "step cannot start after interruption admission",
                    );
                }
                if self.active_step.is_some() {
                    return self.transition_error(fact.sequence, "a step is already active");
                }
                if let Some(previous) = self
                    .terminal_steps
                    .values()
                    .filter_map(|terminal| match terminal {
                        StepTerminalState::Closed { closure }
                            if closure.turn_id == started.turn_id =>
                        {
                            Some(closure)
                        }
                        _ => None,
                    })
                    .max_by_key(|closure| self.steps[&closure.step_id].step_ordinal)
                    && previous.outcome != StepOutcome::ContinueLoop
                {
                    return self.transition_error(
                        fact.sequence,
                        "prior step outcome does not permit continuation",
                    );
                }
                validate_entity_uuid(started.step_id, "step ID")
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                if self.steps.contains_key(&started.step_id)
                    || self.model_requests.contains_key(&started.step_id)
                    || self.route_leases.contains_key(&started.step_id)
                {
                    return self
                        .transition_error(fact.sequence, "step identity is already present");
                }
                let expected = self
                    .next_step_ordinals
                    .get(&started.turn_id)
                    .copied()
                    .unwrap_or(0);
                if started.step_ordinal != expected {
                    return self.transition_error(
                        fact.sequence,
                        format!(
                            "expected step ordinal {expected}, got {}",
                            started.step_ordinal
                        ),
                    );
                }
                let next = expected
                    .checked_add(1)
                    .ok_or_else(|| AuthorityError::Transition {
                        sequence: fact.sequence,
                        message: "step ordinal overflow".into(),
                    })?;
                self.next_step_ordinals.insert(started.turn_id, next);
                self.steps.insert(started.step_id, started.clone());
                self.active_step = Some(ActiveStep {
                    start: started.clone(),
                    active_request_id: None,
                    next_request_ordinal: 0,
                    next_call_ordinal: 0,
                    next_result_ordinal: 0,
                });
                Ok(())
            }
            SessionFactPayload::ContextSourceMaterialized(source) => {
                validate_entity_uuid(source.context_source_id, "context source ID")
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                if self
                    .materialized_context_sources
                    .contains_key(&source.context_source_id)
                    || source.source_identity.trim().is_empty()
                    || source.source_identity.len() > 512
                    || source.owner_id.trim().is_empty()
                    || source.owner_id.len() > 512
                    || source.content_ref.projection_class() != ProjectionClass::Default
                    || source.content_ref.byte_length() == 0
                {
                    return self.transition_error(
                        fact.sequence,
                        "materialized context source identity, owner, or content is invalid",
                    );
                }
                self.materialized_context_sources
                    .insert(source.context_source_id, source.clone());
                self.materialized_context_source_events
                    .insert(source.context_source_id, fact.event_id);
                Ok(())
            }
            SessionFactPayload::ModelRequestPrepared(preparation) => {
                let Some(active_turn) = self.active_turn.as_ref() else {
                    return self.transition_error(fact.sequence, "there is no active turn");
                };
                if active_turn.turn_id != preparation.turn_id {
                    return self.transition_error(fact.sequence, "request targets a stale turn");
                }
                if active_turn.accepted_interruption.is_some() {
                    return self.transition_error(
                        fact.sequence,
                        "request cannot prepare after interruption admission",
                    );
                }
                let Some(active_step) = self.active_step.as_ref() else {
                    return self.transition_error(fact.sequence, "there is no active step");
                };
                if active_step.start.step_id != preparation.step_id
                    || active_step.start.turn_id != preparation.turn_id
                {
                    return self.transition_error(fact.sequence, "request targets the wrong step");
                }
                if active_step.active_request_id.is_some() {
                    return self.transition_error(fact.sequence, "a request is already active");
                }
                validate_entity_uuid(preparation.request_id, "request ID")
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                if self.model_requests.contains_key(&preparation.request_id)
                    || self.steps.contains_key(&preparation.request_id)
                    || self.route_leases.contains_key(&preparation.request_id)
                {
                    return self
                        .transition_error(fact.sequence, "request identity is already present");
                }
                if preparation.request_ordinal != active_step.next_request_ordinal {
                    return self.transition_error(
                        fact.sequence,
                        format!(
                            "expected request ordinal {}, got {}",
                            active_step.next_request_ordinal, preparation.request_ordinal
                        ),
                    );
                }
                self.validate_request_preparation(preparation, fact.sequence)?;
                let next = active_step
                    .next_request_ordinal
                    .checked_add(1)
                    .ok_or_else(|| AuthorityError::Transition {
                        sequence: fact.sequence,
                        message: "request ordinal overflow".into(),
                    })?;
                self.model_requests.insert(
                    preparation.request_id,
                    ModelRequestState::Open {
                        preparation: preparation.clone(),
                        route_join: None,
                    },
                );
                self.model_request_source_events
                    .insert(preparation.request_id, fact.event_id);
                let active_step = self
                    .active_step
                    .as_mut()
                    .expect("active step checked above");
                active_step.active_request_id = Some(preparation.request_id);
                active_step.next_request_ordinal = next;
                Ok(())
            }
            SessionFactPayload::RouteLeaseRecorded(lease) => {
                let Some(active) = self.active_turn.as_ref() else {
                    return self.transition_error(fact.sequence, "there is no active turn");
                };
                if active.turn_id != lease.turn_id {
                    return self
                        .transition_error(fact.sequence, "route lease targets a stale turn");
                }
                if self.route_leases.contains_key(&lease.lease_id) {
                    return self.transition_error(
                        fact.sequence,
                        "route lease identity is already present",
                    );
                }
                if self.steps.contains_key(&lease.lease_id)
                    || self.model_requests.contains_key(&lease.lease_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "route lease identity collides with a step or request",
                    );
                }
                if [
                    &lease.selected_provider_id,
                    &lease.selected_model_id,
                    &lease.serving_provider_id,
                    &lease.serving_model_id,
                    &lease.schema_dialect,
                    &lease.credential_source_class,
                    &lease.contribution_generation_id,
                    &lease.route_policy,
                ]
                .into_iter()
                .any(|value| value.trim().is_empty())
                {
                    return self.transition_error(
                        fact.sequence,
                        "route lease contains an empty required identity",
                    );
                }
                self.route_leases.insert(lease.lease_id, lease.clone());
                Ok(())
            }
            SessionFactPayload::RouteEndpointProvenanceRecorded(provenance) => {
                let Some(lease) = self.route_leases.get(&provenance.lease_id) else {
                    return self.transition_error(
                        fact.sequence,
                        "route endpoint provenance has no recorded lease",
                    );
                };
                if lease.route_policy != "admitted_manifest_endpoint_v1" {
                    return self.transition_error(
                        fact.sequence,
                        "route endpoint provenance requires a manifest route lease",
                    );
                }
                if self
                    .route_endpoint_provenance
                    .contains_key(&provenance.lease_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "route endpoint provenance is already present",
                    );
                }
                if provenance.endpoint_id.trim().is_empty()
                    || provenance.adapter_id.trim().is_empty()
                {
                    return self.transition_error(
                        fact.sequence,
                        "route endpoint provenance identity is empty",
                    );
                }
                self.route_endpoint_provenance
                    .insert(provenance.lease_id, provenance.clone());
                Ok(())
            }
            SessionFactPayload::ModelRequestRouteJoined(join) => {
                let Some(active_turn) = self.active_turn.as_ref() else {
                    return self.transition_error(fact.sequence, "there is no active turn");
                };
                if active_turn.turn_id != join.turn_id {
                    return self.transition_error(fact.sequence, "route join targets a stale turn");
                }
                let Some(active_step) = self.active_step.as_ref() else {
                    return self.transition_error(fact.sequence, "there is no active step");
                };
                if active_step.start.step_id != join.step_id
                    || active_step.active_request_id != Some(join.request_id)
                {
                    return self
                        .transition_error(fact.sequence, "route join targets the wrong request");
                }
                let Some(lease) = self.route_leases.get(&join.lease_id) else {
                    return self
                        .transition_error(fact.sequence, "route join lease was not recorded");
                };
                if lease.request_id != join.request_id || lease.turn_id != join.turn_id {
                    return self.transition_error(
                        fact.sequence,
                        "route join identity contradicts its lease",
                    );
                }
                if lease.route_policy == "admitted_manifest_endpoint_v1"
                    && !self.route_endpoint_provenance.contains_key(&join.lease_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "manifest route join is missing endpoint provenance",
                    );
                }
                if self.joined_route_leases.contains_key(&join.lease_id)
                    || self.request_route_joins.contains_key(&join.request_id)
                {
                    return self
                        .transition_error(fact.sequence, "request or lease is already joined");
                }
                let Some(current) = self.model_requests.get(&join.request_id).cloned() else {
                    return self.transition_error(fact.sequence, "request was not prepared");
                };
                let ModelRequestState::Open {
                    preparation,
                    route_join: None,
                } = current
                else {
                    return self
                        .transition_error(fact.sequence, "request is not awaiting a route join");
                };
                for continuity_id in &preparation.continuity_refs {
                    let continuity = self
                        .provider_continuity
                        .get(continuity_id)
                        .expect("continuity references validated at preparation");
                    if continuity.serving_provider_id != lease.serving_provider_id
                        || continuity.serving_model_id != lease.serving_model_id
                        || continuity.provider_contribution_generation_id
                            != lease.contribution_generation_id
                        || continuity.required_for != ProviderContinuityRequiredFor::NextRequest
                    {
                        return self.transition_error(
                            fact.sequence,
                            "request continuity lineage contradicts its joined route lease",
                        );
                    }
                }
                self.model_requests.insert(
                    join.request_id,
                    ModelRequestState::Open {
                        preparation,
                        route_join: Some(join.clone()),
                    },
                );
                self.request_route_joins
                    .insert(join.request_id, join.clone());
                self.joined_route_leases
                    .insert(join.lease_id, join.request_id);
                Ok(())
            }
            SessionFactPayload::ModelResponseAttemptFailed(failure) => {
                self.validate_response_attempt(
                    failure.request_id,
                    failure.step_id,
                    failure.response_attempt_ordinal,
                    fact.sequence,
                    "response-attempt failure",
                )?;
                validate_reason_code(&failure.reason_code)
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                if self
                    .request_message_commits
                    .contains_key(&failure.request_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "response-attempt failure cannot follow provider Done or message commit",
                    );
                }
                self.response_attempt_failures
                    .entry(failure.request_id)
                    .or_default()
                    .insert(failure.response_attempt_ordinal, failure.clone());
                Ok(())
            }
            SessionFactPayload::AssistantContentAppended(chunk) => {
                self.validate_response_attempt(
                    chunk.request_id,
                    chunk.step_id,
                    chunk.response_attempt_ordinal,
                    fact.sequence,
                    "assistant chunk",
                )?;
                validate_entity_uuid(chunk.message_id, "message ID")
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                if self.assistant_messages.contains_key(&chunk.message_id)
                    || self.request_message_commits.contains_key(&chunk.request_id)
                    || self.steps.contains_key(&chunk.message_id)
                    || self.model_requests.contains_key(&chunk.message_id)
                    || self.route_leases.contains_key(&chunk.message_id)
                    || self.provider_continuity.contains_key(&chunk.message_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "assistant chunk cannot follow message commit",
                    );
                }
                if self.assistant_chunks.iter().any(|(request_id, chunks)| {
                    *request_id != chunk.request_id
                        && chunks
                            .iter()
                            .any(|stored| stored.message_id == chunk.message_id)
                }) {
                    return self.transition_error(
                        fact.sequence,
                        "assistant message identity belongs to another request",
                    );
                }
                let chunks = self.assistant_chunks.entry(chunk.request_id).or_default();
                if chunks
                    .first()
                    .is_some_and(|stored| stored.message_id != chunk.message_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "request already has another assistant message identity",
                    );
                }
                let expected = chunks
                    .iter()
                    .filter(|stored| {
                        stored.response_attempt_ordinal == chunk.response_attempt_ordinal
                            && stored.content_kind == chunk.content_kind
                    })
                    .count();
                if usize::try_from(chunk.chunk_ordinal).ok() != Some(expected) {
                    return self.transition_error(
                        fact.sequence,
                        format!(
                            "expected {:?} chunk ordinal {expected}, got {}",
                            chunk.content_kind, chunk.chunk_ordinal
                        ),
                    );
                }
                if chunk.content_ref.projection_class() != ProjectionClass::Default
                    || chunk.content_ref.media_type() != "text/plain"
                    || chunk.content_ref.byte_length() == 0
                    || chunk.content_ref.byte_length() > MAX_ASSISTANT_CHUNK_BYTES
                {
                    return self.transition_error(
                        fact.sequence,
                        "assistant chunks must be non-empty text/plain default content at most 64 KiB",
                    );
                }
                chunks.push(chunk.clone());
                Ok(())
            }
            SessionFactPayload::ProviderContinuityStored(continuity) => {
                let lease = self.validate_response_attempt(
                    continuity.request_id,
                    continuity.step_id,
                    continuity.response_attempt_ordinal,
                    fact.sequence,
                    "provider continuity",
                )?;
                validate_entity_uuid(continuity.continuity_id, "continuity ID")
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                if self
                    .provider_continuity
                    .contains_key(&continuity.continuity_id)
                    || self.steps.contains_key(&continuity.continuity_id)
                    || self.model_requests.contains_key(&continuity.continuity_id)
                    || self.route_leases.contains_key(&continuity.continuity_id)
                    || self
                        .assistant_messages
                        .contains_key(&continuity.continuity_id)
                {
                    return self
                        .transition_error(fact.sequence, "continuity identity is already present");
                }
                if self.provider_continuity.values().any(|stored| {
                    stored.request_id == continuity.request_id
                        && stored.response_attempt_ordinal == continuity.response_attempt_ordinal
                        && stored.continuity_kind == continuity.continuity_kind
                }) {
                    return self.transition_error(
                        fact.sequence,
                        "continuity kind is already stored for this request",
                    );
                }
                if self
                    .request_message_commits
                    .contains_key(&continuity.request_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "provider continuity cannot follow message commit",
                    );
                }
                if continuity.serving_provider_id != lease.serving_provider_id
                    || continuity.serving_model_id != lease.serving_model_id
                    || continuity.provider_contribution_generation_id
                        != lease.contribution_generation_id
                {
                    return self.transition_error(
                        fact.sequence,
                        "continuity serving lineage contradicts the joined route lease",
                    );
                }
                let policy = &continuity.restricted_required;
                if policy.allowed_kinds.is_empty()
                    || policy.allowed_kinds.len() > 2
                    || policy
                        .allowed_kinds
                        .windows(2)
                        .any(|pair| pair[0] >= pair[1])
                    || !policy.allowed_kinds.contains(&continuity.continuity_kind)
                    || policy.max_blob_bytes == 0
                    || policy.max_blob_bytes > crate::session_blob_store::MAX_SESSION_BLOB_BYTES
                    || continuity.content_ref.byte_length() == 0
                    || continuity.content_ref.byte_length() > policy.max_blob_bytes
                    || continuity.content_ref.projection_class()
                        != ProjectionClass::RestrictedContinuity
                    || continuity.content_ref.media_type() != "application/octet-stream"
                {
                    return self.transition_error(
                        fact.sequence,
                        "continuity violates its restricted_required policy",
                    );
                }
                self.provider_continuity
                    .insert(continuity.continuity_id, continuity.clone());
                Ok(())
            }
            SessionFactPayload::AssistantMessageCommitted(commit) => {
                self.validate_response_attempt(
                    commit.request_id,
                    commit.step_id,
                    commit.response_attempt_ordinal,
                    fact.sequence,
                    "assistant message commit",
                )?;
                validate_entity_uuid(commit.message_id, "message ID")
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                if self.assistant_messages.contains_key(&commit.message_id)
                    || self
                        .request_message_commits
                        .contains_key(&commit.request_id)
                {
                    return self
                        .transition_error(fact.sequence, "assistant message is already committed");
                }
                if self.assistant_chunks.iter().any(|(request_id, chunks)| {
                    *request_id != commit.request_id
                        && chunks
                            .iter()
                            .any(|chunk| chunk.message_id == commit.message_id)
                }) {
                    return self.transition_error(
                        fact.sequence,
                        "assistant message identity belongs to another request",
                    );
                }
                if self
                    .assistant_messages
                    .values()
                    .any(|stored| stored.step_id == commit.step_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "step already has a committed assistant message",
                    );
                }
                if commit.tool_call_count > MAX_MESSAGE_TOOL_CALLS
                    || commit.usage.as_ref().is_some_and(|usage| {
                        usage.input_tokens > MAX_USAGE_TOKENS
                            || usage.output_tokens > MAX_USAGE_TOKENS
                            || usage
                                .input_tokens
                                .checked_add(usage.output_tokens)
                                .is_none_or(|total| total > MAX_USAGE_TOKENS)
                    })
                {
                    return self.transition_error(
                        fact.sequence,
                        "assistant message usage or tool-call count exceeds protocol bounds",
                    );
                }
                let chunks = self
                    .assistant_chunks
                    .get(&commit.request_id)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                if chunks
                    .iter()
                    .filter(|chunk| {
                        chunk.response_attempt_ordinal == commit.response_attempt_ordinal
                    })
                    .any(|chunk| chunk.message_id != commit.message_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "assistant commit message identity contradicts its chunks",
                    );
                }
                let committed_chunks = chunks
                    .iter()
                    .filter(|chunk| {
                        chunk.response_attempt_ordinal == commit.response_attempt_ordinal
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                if !commit_content_matches_chunks(commit, &committed_chunks) {
                    return self.transition_error(
                        fact.sequence,
                        "assistant commit does not exactly match accumulated content",
                    );
                }
                if commit.content.is_empty() && commit.tool_call_count == 0 {
                    return self.transition_error(
                        fact.sequence,
                        "empty assistant message requires canonical tool calls",
                    );
                }
                self.assistant_messages
                    .insert(commit.message_id, commit.clone());
                self.request_message_commits
                    .insert(commit.request_id, commit.message_id);
                self.assistant_message_source_events
                    .insert(commit.message_id, fact.event_id);
                Ok(())
            }
            SessionFactPayload::ToolCallRecorded(call) => {
                self.validate_open_joined_request(
                    call.request_id,
                    call.step_id,
                    fact.sequence,
                    "tool call",
                )?;
                validate_entity_uuid(call.tool_call_id, "tool-call ID")
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                let Some(message_id) = self.request_message_commits.get(&call.request_id) else {
                    return self.transition_error(
                        fact.sequence,
                        "tool call requires a committed assistant message",
                    );
                };
                let commit = &self.assistant_messages[message_id];
                let active_step = self.active_step.as_ref().expect("active step validated");
                if call.call_ordinal != active_step.next_call_ordinal {
                    return self.transition_error(
                        fact.sequence,
                        format!(
                            "expected tool-call ordinal {}, got {}",
                            active_step.next_call_ordinal, call.call_ordinal
                        ),
                    );
                }
                if call.call_ordinal >= commit.tool_call_count {
                    return self.transition_error(
                        fact.sequence,
                        "tool call exceeds the committed message call count",
                    );
                }
                if call.call_id.is_empty()
                    || call.call_id.len() > 512
                    || call.invocation_name.is_empty()
                    || call.invocation_name.len() > 512
                {
                    return self.transition_error(
                        fact.sequence,
                        "tool call contains an invalid bounded call ID or tool name",
                    );
                }
                if call.arguments_ref.projection_class() != ProjectionClass::Default
                    || call.arguments_ref.byte_length() == 0
                {
                    return self.transition_error(
                        fact.sequence,
                        "tool arguments require non-empty default-projection content",
                    );
                }
                if self.tool_calls.contains_key(&call.tool_call_id) {
                    return self
                        .transition_error(fact.sequence, "tool-call identity is already present");
                }
                let turn_id = active_step.start.turn_id;
                if self
                    .turn_call_ids
                    .get(&turn_id)
                    .is_some_and(|calls| calls.contains_key(&call.call_id))
                    || self.invocations.values().any(|invocation| {
                        invocation.turn_id() == turn_id && invocation.call_id() == call.call_id
                    })
                {
                    return self.transition_error(
                        fact.sequence,
                        "provider call identity is already present in this turn",
                    );
                }
                let next = active_step
                    .next_call_ordinal
                    .checked_add(1)
                    .ok_or_else(|| AuthorityError::Transition {
                        sequence: fact.sequence,
                        message: "tool-call ordinal overflow".into(),
                    })?;
                self.tool_calls.insert(call.tool_call_id, call.clone());
                self.turn_call_ids
                    .entry(turn_id)
                    .or_default()
                    .insert(call.call_id.clone(), call.tool_call_id);
                self.active_step
                    .as_mut()
                    .expect("active step validated")
                    .next_call_ordinal = next;
                Ok(())
            }
            SessionFactPayload::ModelRequestClosed(closure) => {
                let Some(active_step) = self.active_step.as_ref() else {
                    return self.transition_error(fact.sequence, "there is no active step");
                };
                if active_step.start.step_id != closure.step_id
                    || active_step.active_request_id != Some(closure.request_id)
                {
                    return self
                        .transition_error(fact.sequence, "closure targets the wrong request");
                }
                validate_reason_code(&closure.reason_code)
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                validate_request_closure_recovery(fact, closure)
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                let Some(current) = self.model_requests.get(&closure.request_id).cloned() else {
                    return self.transition_error(fact.sequence, "request was not prepared");
                };
                let ModelRequestState::Open {
                    preparation,
                    route_join,
                } = current
                else {
                    return self.transition_error(fact.sequence, "request is already closed");
                };
                let expected_attempt = if closure.outcome == ModelRequestOutcome::Abandoned {
                    latest_response_attempt(self, closure.request_id)
                } else {
                    u32::try_from(
                        self.response_attempt_failures
                            .get(&closure.request_id)
                            .map_or(0, BTreeMap::len),
                    )
                    .map_err(|_| AuthorityError::Transition {
                        sequence: fact.sequence,
                        message: "response-attempt ordinal overflow".into(),
                    })?
                };
                if closure.response_attempt_ordinal != expected_attempt {
                    return self.transition_error(
                        fact.sequence,
                        format!(
                            "request closure must target response attempt {expected_attempt}, got {}",
                            closure.response_attempt_ordinal
                        ),
                    );
                }
                if closure.outcome == ModelRequestOutcome::ResponseCompleted
                    && !self
                        .request_message_commits
                        .contains_key(&closure.request_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "response_completed requires a committed assistant message with provider completion evidence",
                    );
                }
                if let Some(message_id) = self.request_message_commits.get(&closure.request_id)
                    && self.assistant_messages[message_id].response_attempt_ordinal
                        != closure.response_attempt_ordinal
                {
                    return self.transition_error(
                        fact.sequence,
                        "request closure response attempt contradicts its committed message",
                    );
                }
                if !matches!(
                    closure.outcome,
                    ModelRequestOutcome::ResponseCompleted | ModelRequestOutcome::Abandoned
                ) && self
                    .request_message_commits
                    .contains_key(&closure.request_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "a committed assistant message requires response_completed closure",
                    );
                }
                self.model_requests.insert(
                    closure.request_id,
                    ModelRequestState::Closed {
                        preparation,
                        route_join,
                        closure: closure.clone(),
                    },
                );
                self.active_step
                    .as_mut()
                    .expect("active step checked above")
                    .active_request_id = None;
                Ok(())
            }
            SessionFactPayload::ToolResultRecorded(result) => {
                let Some(active_step) = self.active_step.as_ref() else {
                    return self.transition_error(fact.sequence, "there is no active step");
                };
                if active_step.start.step_id != result.step_id
                    || active_step.active_request_id.is_some()
                {
                    return self.transition_error(
                        fact.sequence,
                        "tool result requires its request to be closed in the active step",
                    );
                }
                validate_entity_uuid(result.tool_result_id, "tool-result ID")
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                if result.result_ordinal != active_step.next_result_ordinal {
                    return self.transition_error(
                        fact.sequence,
                        format!(
                            "expected tool-result ordinal {}, got {}",
                            active_step.next_result_ordinal, result.result_ordinal
                        ),
                    );
                }
                let Some(call) = self.tool_calls.get(&result.tool_call_id) else {
                    return self
                        .transition_error(fact.sequence, "tool result references an unknown call");
                };
                if call.step_id != result.step_id
                    || call.call_id != result.call_id
                    || call.call_ordinal != result.result_ordinal
                {
                    return self.transition_error(
                        fact.sequence,
                        "tool result order or call identity contradicts the provider call",
                    );
                }
                if self.tool_results.contains_key(&result.tool_result_id)
                    || self.call_results.contains_key(&result.tool_call_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "tool result identity or call cardinality is already terminal",
                    );
                }
                if result.content_ref.projection_class() != ProjectionClass::Default
                    || result.content_ref.byte_length() == 0
                {
                    return self.transition_error(
                        fact.sequence,
                        "tool result requires non-empty final default-projection content",
                    );
                }
                self.validate_tool_result_linkage(result, call, fact.sequence)?;
                let next = active_step
                    .next_result_ordinal
                    .checked_add(1)
                    .ok_or_else(|| AuthorityError::Transition {
                        sequence: fact.sequence,
                        message: "tool-result ordinal overflow".into(),
                    })?;
                self.tool_results
                    .insert(result.tool_result_id, result.clone());
                self.call_results
                    .insert(result.tool_call_id, result.tool_result_id);
                self.tool_result_source_events
                    .insert(result.tool_result_id, fact.event_id);
                self.active_step
                    .as_mut()
                    .expect("active step validated")
                    .next_result_ordinal = next;
                Ok(())
            }
            SessionFactPayload::StepClosed(closure) => {
                self.validate_step_close(closure, fact.sequence)?;
                self.terminal_steps.insert(
                    closure.step_id,
                    StepTerminalState::Closed {
                        closure: closure.clone(),
                    },
                );
                self.active_step = None;
                Ok(())
            }
            SessionFactPayload::StepAbandoned(abandonment) => {
                let Some(active_step) = self.active_step.as_ref() else {
                    return self.transition_error(fact.sequence, "there is no active step");
                };
                if active_step.start.step_id != abandonment.step_id
                    || active_step.start.turn_id != abandonment.turn_id
                {
                    return self
                        .transition_error(fact.sequence, "abandonment targets a stale step");
                }
                if active_step.active_request_id.is_some() {
                    return self.transition_error(
                        fact.sequence,
                        "step abandonment requires request closure first",
                    );
                }
                validate_step_abandonment_recovery(fact, abandonment)
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                self.terminal_steps.insert(
                    abandonment.step_id,
                    StepTerminalState::Abandoned {
                        abandonment: abandonment.clone(),
                    },
                );
                self.active_step = None;
                Ok(())
            }
            SessionFactPayload::TurnInterruptionRequested(request) => {
                let Some(active) = self.active_turn.as_mut() else {
                    return self.transition_error(fact.sequence, "there is no active turn");
                };
                if active.turn_id != request.turn_id {
                    return self
                        .transition_error(fact.sequence, "interruption targets a stale turn");
                }
                if active.accepted_interruption.is_some() {
                    return self
                        .transition_error(fact.sequence, "an interruption is already accepted");
                }
                active.accepted_interruption = Some(request.clone());
                self.interruption_requests
                    .insert(request.turn_id, request.clone());
                Ok(())
            }
            SessionFactPayload::InvocationRegistered(registration) => {
                let Some(active) = self.active_turn.as_ref() else {
                    return self.transition_error(fact.sequence, "there is no active turn");
                };
                if active.turn_id != registration.turn_id {
                    return self.transition_error(fact.sequence, "invocation targets a stale turn");
                }
                if active
                    .accepted_interruption
                    .as_ref()
                    .is_some_and(|request| request.kind == InterruptionKind::Revoke)
                {
                    return self.transition_error(
                        fact.sequence,
                        "invocation cannot register after revocation",
                    );
                }
                if self.invocations.contains_key(&registration.invocation_id) {
                    return self
                        .transition_error(fact.sequence, "invocation identity is already present");
                }
                self.invocations.insert(
                    registration.invocation_id,
                    InvocationState::Registered {
                        registration: registration.clone(),
                    },
                );
                Ok(())
            }
            SessionFactPayload::InvocationPrepared(preparation) => {
                let Some(active) = self.active_turn.as_ref() else {
                    return self.transition_error(fact.sequence, "there is no active turn");
                };
                if active.turn_id != preparation.turn_id {
                    return self.transition_error(fact.sequence, "invocation targets a stale turn");
                }
                if active
                    .accepted_interruption
                    .as_ref()
                    .is_some_and(|request| request.kind == InterruptionKind::Revoke)
                {
                    return self.transition_error(
                        fact.sequence,
                        "invocation cannot prepare after revocation",
                    );
                }
                if self.invocations.contains_key(&preparation.invocation_id) {
                    return self
                        .transition_error(fact.sequence, "invocation identity is already present");
                }
                if let Some(active_step) = self.active_step.as_ref() {
                    let call = self
                        .turn_call_ids
                        .get(&preparation.turn_id)
                        .and_then(|calls| calls.get(&preparation.call_id))
                        .and_then(|tool_call_id| self.tool_calls.get(tool_call_id));
                    let Some(call) = call else {
                        return self.transition_error(
                            fact.sequence,
                            "invocation preparation requires a previously recorded tool call",
                        );
                    };
                    if call.step_id != active_step.start.step_id
                        || call.invocation_name != preparation.invocation_name
                    {
                        return self.transition_error(
                            fact.sequence,
                            "invocation preparation contradicts its recorded tool call",
                        );
                    }
                }
                if self.invocations.values().any(|invocation| {
                    invocation.turn_id() == preparation.turn_id
                        && invocation.call_id() == preparation.call_id
                }) {
                    return self.transition_error(
                        fact.sequence,
                        "invocation call identity is already present in this turn",
                    );
                }
                self.invocations.insert(
                    preparation.invocation_id,
                    InvocationState::Prepared {
                        preparation: preparation.clone(),
                    },
                );
                Ok(())
            }
            SessionFactPayload::InvocationDispatched(dispatch) => {
                let Some(current) = self.invocations.get(&dispatch.invocation_id).cloned() else {
                    return self.transition_error(fact.sequence, "invocation was not prepared");
                };
                let InvocationState::Prepared { preparation } = current else {
                    return self
                        .transition_error(fact.sequence, "invocation is not awaiting dispatch");
                };
                if preparation.lease_id != dispatch.lease_id {
                    return self.transition_error(
                        fact.sequence,
                        "dispatch lease does not match prepared invocation",
                    );
                }
                if let Some(tool_call_id) = self
                    .turn_call_ids
                    .get(&preparation.turn_id)
                    .and_then(|calls| calls.get(&preparation.call_id))
                {
                    let call = &self.tool_calls[tool_call_id];
                    if !matches!(
                        self.model_requests.get(&call.request_id),
                        Some(ModelRequestState::Closed { .. })
                    ) {
                        return self.transition_error(
                            fact.sequence,
                            "invocation dispatch requires canonical request closure",
                        );
                    }
                }
                self.invocations.insert(
                    dispatch.invocation_id,
                    InvocationState::Dispatched {
                        preparation,
                        dispatch: dispatch.clone(),
                    },
                );
                Ok(())
            }
            SessionFactPayload::InvocationAcknowledged(acknowledgement) => {
                let Some(current) = self
                    .invocations
                    .get(&acknowledgement.invocation_id)
                    .cloned()
                else {
                    return self.transition_error(fact.sequence, "invocation was not dispatched");
                };
                let InvocationState::Dispatched {
                    preparation,
                    dispatch,
                } = current
                else {
                    return self.transition_error(
                        fact.sequence,
                        "invocation is not awaiting acknowledgement",
                    );
                };
                if preparation.lease_id != acknowledgement.lease_id {
                    return self.transition_error(
                        fact.sequence,
                        "acknowledgement lease does not match prepared invocation",
                    );
                }
                self.invocations.insert(
                    acknowledgement.invocation_id,
                    InvocationState::Acknowledged {
                        preparation,
                        dispatch,
                        acknowledgement: acknowledgement.clone(),
                    },
                );
                Ok(())
            }
            SessionFactPayload::InvocationClassifiedUnknown(classification) => {
                let Some(current) = self.invocations.get(&classification.invocation_id).cloned()
                else {
                    return self.transition_error(fact.sequence, "invocation was not registered");
                };
                let next = match current {
                    InvocationState::Registered { registration } => InvocationState::Unknown {
                        registration,
                        classification: classification.clone(),
                    },
                    InvocationState::Dispatched {
                        preparation,
                        dispatch,
                    } => InvocationState::DurableUnknown {
                        preparation,
                        dispatch,
                        acknowledgement: None,
                        classification: classification.clone(),
                    },
                    InvocationState::Acknowledged {
                        preparation,
                        dispatch,
                        acknowledgement,
                    } => InvocationState::DurableUnknown {
                        preparation,
                        dispatch,
                        acknowledgement: Some(acknowledgement),
                        classification: classification.clone(),
                    },
                    _ => {
                        return self.transition_error(
                            fact.sequence,
                            "invocation is already classified or settled",
                        );
                    }
                };
                self.invocations.insert(classification.invocation_id, next);
                Ok(())
            }
            SessionFactPayload::InvocationSettled(settlement) => {
                let Some(current) = self.invocations.get(&settlement.invocation_id).cloned() else {
                    return self.transition_error(fact.sequence, "invocation was not registered");
                };
                let next = match current {
                    InvocationState::Registered { registration }
                    | InvocationState::Unknown { registration, .. } => InvocationState::Settled {
                        registration,
                        settlement: settlement.clone(),
                    },
                    InvocationState::Acknowledged {
                        preparation,
                        dispatch,
                        acknowledgement,
                    } => InvocationState::DurableSettled {
                        preparation,
                        dispatch,
                        acknowledgement,
                        settlement: settlement.clone(),
                    },
                    InvocationState::DurableUnknown {
                        preparation,
                        dispatch,
                        acknowledgement: Some(acknowledgement),
                        ..
                    } if settlement.terminal_evidence_reference.is_some() => {
                        InvocationState::DurableSettled {
                            preparation,
                            dispatch,
                            acknowledgement,
                            settlement: settlement.clone(),
                        }
                    }
                    InvocationState::Prepared { .. }
                    | InvocationState::Dispatched { .. }
                    | InvocationState::DurableUnknown { .. }
                    | InvocationState::Settled { .. }
                    | InvocationState::DurableSettled { .. } => {
                        return self.transition_error(
                            fact.sequence,
                            "invocation cannot settle from its current state",
                        );
                    }
                };
                self.invocations.insert(settlement.invocation_id, next);
                Ok(())
            }
            SessionFactPayload::CompactionStarted(start) => {
                validate_entity_uuid(start.compaction_id, "compaction ID")
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                if self.active_compaction.is_some()
                    || self.compaction_starts.contains_key(&start.compaction_id)
                    || self.compaction_requests.contains_key(&start.compaction_id)
                    || self.compaction_summaries.contains_key(&start.compaction_id)
                    || self.steps.contains_key(&start.compaction_id)
                    || self.model_requests.contains_key(&start.compaction_id)
                    || self.route_leases.contains_key(&start.compaction_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "compaction identity is active or already present",
                    );
                }
                if start.source_frontier.sequence != self.last_sequence
                    || Some(start.source_frontier.event_id) != self.last_event_id
                {
                    return self.transition_error(
                        fact.sequence,
                        "compaction source frontier is not the current authority frontier",
                    );
                }
                if start.source_context_revision != self.context_revision
                    || start.target_context_revision
                        != self.context_revision.checked_add(1).ok_or_else(|| {
                            AuthorityError::Transition {
                                sequence: fact.sequence,
                                message: "context revision overflow".into(),
                            }
                        })?
                {
                    return self.transition_error(
                        fact.sequence,
                        "compaction context revisions are not consecutive",
                    );
                }
                match &start.owner_scope {
                    CompactionOwnerScope::Turn { turn_id, step_id } => {
                        if self.active_turn.as_ref().map(|turn| turn.turn_id) != Some(*turn_id)
                            || self.active_step.as_ref().map(|step| step.start.step_id)
                                != Some(*step_id)
                        {
                            return self.transition_error(
                                fact.sequence,
                                "turn compaction does not own the active turn and step",
                            );
                        }
                        if start.trigger == CompactionTrigger::ManualIdle {
                            return self.transition_error(
                                fact.sequence,
                                "turn compaction cannot use the manual-idle trigger",
                            );
                        }
                    }
                    CompactionOwnerScope::SessionIdle => {
                        if start.trigger != CompactionTrigger::ManualIdle
                            || self.active_turn.is_some()
                            || self.active_step.is_some()
                            || self.invocations.values().any(invocation_blocks_migration)
                        {
                            return self.transition_error(
                                fact.sequence,
                                "idle compaction requires a completely idle session",
                            );
                        }
                    }
                }
                validate_compaction_context_items(
                    &start.input_items,
                    &start.retained_items,
                    &self.command_receipts,
                )
                .map_err(|error| self.at_sequence(fact.sequence, error))?;
                if start.input_items.is_empty() {
                    return self.transition_error(
                        fact.sequence,
                        "compaction input manifest cannot be empty",
                    );
                }
                let expected_manifest = compaction_input_manifest_id(start)?;
                if start.input_manifest_id != expected_manifest {
                    return self.transition_error(
                        fact.sequence,
                        "compaction input manifest digest is invalid",
                    );
                }
                self.active_compaction = Some(start.compaction_id);
                self.compaction_starts
                    .insert(start.compaction_id, start.clone());
                Ok(())
            }
            SessionFactPayload::CompactionRequestPrepared(preparation) => {
                if self.active_compaction != Some(preparation.compaction_id) {
                    return self.transition_error(
                        fact.sequence,
                        "compaction request targets no active compaction",
                    );
                }
                validate_entity_uuid(preparation.compaction_request_id, "compaction request ID")
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                if self
                    .compaction_requests
                    .contains_key(&preparation.compaction_request_id)
                    || self
                        .compaction_starts
                        .contains_key(&preparation.compaction_request_id)
                    || self
                        .compaction_summaries
                        .contains_key(&preparation.compaction_request_id)
                    || self.steps.contains_key(&preparation.compaction_request_id)
                    || self
                        .model_requests
                        .contains_key(&preparation.compaction_request_id)
                    || self
                        .route_leases
                        .contains_key(&preparation.compaction_request_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "compaction request identity is already present",
                    );
                }
                let requests = self
                    .compaction_requests
                    .values()
                    .filter(|request| {
                        request.preparation().compaction_id == preparation.compaction_id
                    })
                    .collect::<Vec<_>>();
                if usize::try_from(preparation.request_ordinal).ok() != Some(requests.len()) {
                    return self.transition_error(
                        fact.sequence,
                        "compaction request ordinal is not contiguous",
                    );
                }
                let start = &self.compaction_starts[&preparation.compaction_id];
                match (&start.owner_scope, &preparation.route) {
                    (
                        CompactionOwnerScope::Turn { turn_id, .. },
                        CompactionRoute::TurnLease { lease_id },
                    ) => {
                        let Some(lease) = self.route_leases.get(lease_id) else {
                            return self.transition_error(
                                fact.sequence,
                                "compaction route lease is absent",
                            );
                        };
                        if lease.turn_id != *turn_id
                            || lease.request_id != preparation.compaction_request_id
                            || self.joined_route_leases.contains_key(lease_id)
                        {
                            return self.transition_error(
                                fact.sequence,
                                "compaction route lease identity is invalid or already joined",
                            );
                        }
                        self.joined_route_leases
                            .insert(*lease_id, preparation.compaction_request_id);
                    }
                    (
                        CompactionOwnerScope::SessionIdle,
                        CompactionRoute::SessionIdle {
                            selected_provider_id,
                            selected_model_id,
                            serving_provider_id,
                            serving_model_id,
                            schema_dialect,
                            credential_source_class,
                            contribution_generation_id,
                            route_policy,
                            ..
                        },
                    ) => {
                        if [
                            selected_provider_id,
                            selected_model_id,
                            serving_provider_id,
                            serving_model_id,
                            schema_dialect,
                            credential_source_class,
                            contribution_generation_id,
                            route_policy,
                        ]
                        .into_iter()
                        .any(|value| value.trim().is_empty())
                        {
                            return self.transition_error(
                                fact.sequence,
                                "idle compaction route contains an empty identity",
                            );
                        }
                    }
                    _ => {
                        return self.transition_error(
                            fact.sequence,
                            "compaction route kind contradicts owner scope",
                        );
                    }
                }
                let manifest_idle = matches!(
                    &preparation.route,
                    CompactionRoute::SessionIdle { route_policy, .. }
                        if route_policy == "admitted_manifest_endpoint_v1"
                );
                let has_endpoint_provenance = self
                    .compaction_endpoint_provenance
                    .contains_key(&preparation.compaction_request_id);
                if manifest_idle != has_endpoint_provenance {
                    return self.transition_error(
                        fact.sequence,
                        "idle compaction route contradicts endpoint provenance",
                    );
                }
                if preparation.prompt_template.owner_id.trim().is_empty()
                    || preparation
                        .prompt_template
                        .owner_generation_id
                        .trim()
                        .is_empty()
                    || preparation.prompt_template.content_ref.projection_class()
                        != ProjectionClass::Default
                    || preparation.prompt_template.content_ref.byte_length() == 0
                {
                    return self
                        .transition_error(fact.sequence, "compaction prompt template is invalid");
                }
                match (
                    preparation.request_ordinal,
                    preparation.replaces_compaction_request_id,
                ) {
                    (0, None) => {}
                    (0, Some(_)) | (_, None) => {
                        return self.transition_error(
                            fact.sequence,
                            "compaction replacement identity is invalid",
                        );
                    }
                    (_, Some(previous_id)) => {
                        let Some(CompactionRequestState::Closed {
                            preparation: previous,
                            closure,
                        }) = self.compaction_requests.get(&previous_id)
                        else {
                            return self.transition_error(
                                fact.sequence,
                                "replaced compaction request is not closed",
                            );
                        };
                        if previous.compaction_id != preparation.compaction_id
                            || previous.request_ordinal + 1 != preparation.request_ordinal
                            || closure.outcome != CompactionRequestOutcome::SupersededForRouteChange
                        {
                            return self.transition_error(
                                fact.sequence,
                                "compaction replacement does not follow a route-change closure",
                            );
                        }
                    }
                }
                self.compaction_requests.insert(
                    preparation.compaction_request_id,
                    CompactionRequestState::Open {
                        preparation: preparation.clone(),
                    },
                );
                Ok(())
            }
            SessionFactPayload::CompactionEndpointProvenanceRecorded(provenance) => {
                if self.active_compaction.is_none() {
                    return self.transition_error(
                        fact.sequence,
                        "compaction endpoint provenance has no active compaction",
                    );
                }
                if self
                    .compaction_requests
                    .contains_key(&provenance.compaction_request_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "compaction endpoint provenance must precede request preparation",
                    );
                }
                if self
                    .compaction_endpoint_provenance
                    .contains_key(&provenance.compaction_request_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "compaction endpoint provenance is already present",
                    );
                }
                if provenance.endpoint_id.trim().is_empty()
                    || provenance.adapter_id.trim().is_empty()
                {
                    return self.transition_error(
                        fact.sequence,
                        "compaction endpoint provenance identity is empty",
                    );
                }
                self.compaction_endpoint_provenance
                    .insert(provenance.compaction_request_id, provenance.clone());
                Ok(())
            }
            SessionFactPayload::CompactionResponseAttemptFailed(failure) => {
                let Some(CompactionRequestState::Open { preparation }) =
                    self.compaction_requests.get(&failure.compaction_request_id)
                else {
                    return self.transition_error(
                        fact.sequence,
                        "compaction response failure targets no open request",
                    );
                };
                if preparation.compaction_id != failure.compaction_id
                    || self
                        .compaction_summary_by_operation
                        .contains_key(&failure.compaction_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "compaction response failure identity is invalid",
                    );
                }
                if matches!(
                    &preparation.route,
                    CompactionRoute::SessionIdle { route_policy, .. }
                        if route_policy == "admitted_manifest_endpoint_v1"
                ) && !self
                    .compaction_endpoint_provenance
                    .contains_key(&failure.compaction_request_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "manifest compaction failure is missing endpoint provenance",
                    );
                }
                validate_reason_code(&failure.reason_code)
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                let failures = self
                    .compaction_attempt_failures
                    .entry(failure.compaction_request_id)
                    .or_default();
                if usize::try_from(failure.response_attempt_ordinal).ok() != Some(failures.len()) {
                    return self.transition_error(
                        fact.sequence,
                        "compaction response-attempt ordinal is not contiguous",
                    );
                }
                failures.insert(failure.response_attempt_ordinal, failure.clone());
                Ok(())
            }
            SessionFactPayload::CompactionSummaryCommitted(summary) => {
                let Some(CompactionRequestState::Open { preparation }) =
                    self.compaction_requests.get(&summary.compaction_request_id)
                else {
                    return self.transition_error(
                        fact.sequence,
                        "compaction summary targets no open request",
                    );
                };
                let next_attempt = self
                    .compaction_attempt_failures
                    .get(&summary.compaction_request_id)
                    .map_or(0, BTreeMap::len);
                if matches!(
                    &preparation.route,
                    CompactionRoute::SessionIdle { route_policy, .. }
                        if route_policy == "admitted_manifest_endpoint_v1"
                ) && !self
                    .compaction_endpoint_provenance
                    .contains_key(&summary.compaction_request_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "manifest compaction summary is missing endpoint provenance",
                    );
                }
                if preparation.compaction_id != summary.compaction_id
                    || usize::try_from(summary.response_attempt_ordinal).ok() != Some(next_attempt)
                    || self
                        .compaction_summary_by_operation
                        .contains_key(&summary.compaction_id)
                    || self
                        .compaction_summaries
                        .contains_key(&summary.compaction_summary_id)
                    || self.steps.contains_key(&summary.compaction_summary_id)
                    || self
                        .model_requests
                        .contains_key(&summary.compaction_summary_id)
                    || self
                        .route_leases
                        .contains_key(&summary.compaction_summary_id)
                    || self
                        .compaction_requests
                        .contains_key(&summary.compaction_summary_id)
                    || self
                        .compaction_starts
                        .contains_key(&summary.compaction_summary_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "compaction summary identity or response attempt is invalid",
                    );
                }
                validate_entity_uuid(summary.compaction_summary_id, "compaction summary ID")
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                if summary.summary_ref.projection_class() != ProjectionClass::Default
                    || summary.summary_ref.media_type() != "text/plain"
                    || summary.summary_ref.byte_length() == 0
                    || summary.summary_digest != summary.summary_ref.digest()
                {
                    return self.transition_error(
                        fact.sequence,
                        "compaction summary reference or digest is invalid",
                    );
                }
                let start = &self.compaction_starts[&summary.compaction_id];
                validate_replacement_items(summary, start, fact.event_id)
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                if summary.replacement_manifest_id
                    != compaction_replacement_manifest_id(summary, start)?
                {
                    return self.transition_error(
                        fact.sequence,
                        "compaction replacement manifest digest is invalid",
                    );
                }
                if summary.usage.as_ref().is_some_and(|usage| {
                    usage.input_tokens > MAX_USAGE_TOKENS || usage.output_tokens > MAX_USAGE_TOKENS
                }) {
                    return self.transition_error(
                        fact.sequence,
                        "compaction usage exceeds supported bounds",
                    );
                }
                self.replacement_manifests.insert(
                    summary.replacement_manifest_id.clone(),
                    summary.replacement_items.clone(),
                );
                self.compaction_summary_by_operation
                    .insert(summary.compaction_id, summary.compaction_summary_id);
                self.compaction_summaries
                    .insert(summary.compaction_summary_id, summary.clone());
                self.compaction_summary_source_events
                    .insert(summary.compaction_summary_id, fact.event_id);
                Ok(())
            }
            SessionFactPayload::CompactionRequestClosed(closure) => {
                let Some(CompactionRequestState::Open { preparation }) = self
                    .compaction_requests
                    .get(&closure.compaction_request_id)
                    .cloned()
                else {
                    return self.transition_error(fact.sequence, "compaction request is not open");
                };
                let next_attempt = self
                    .compaction_attempt_failures
                    .get(&closure.compaction_request_id)
                    .map_or(0, BTreeMap::len);
                if preparation.compaction_id != closure.compaction_id
                    || usize::try_from(closure.response_attempt_ordinal).ok() != Some(next_attempt)
                {
                    return self.transition_error(
                        fact.sequence,
                        "compaction request closure attempt is invalid",
                    );
                }
                if matches!(
                    &preparation.route,
                    CompactionRoute::SessionIdle { route_policy, .. }
                        if route_policy == "admitted_manifest_endpoint_v1"
                ) && !self
                    .compaction_endpoint_provenance
                    .contains_key(&closure.compaction_request_id)
                {
                    return self.transition_error(
                        fact.sequence,
                        "manifest compaction closure is missing endpoint provenance",
                    );
                }
                validate_reason_code(&closure.reason_code)
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                let has_summary = self
                    .compaction_summary_by_operation
                    .contains_key(&closure.compaction_id);
                if (closure.outcome == CompactionRequestOutcome::SummaryCommitted) != has_summary {
                    return self.transition_error(
                        fact.sequence,
                        "only a committed summary may close compaction successfully",
                    );
                }
                self.compaction_requests.insert(
                    closure.compaction_request_id,
                    CompactionRequestState::Closed {
                        preparation,
                        closure: closure.clone(),
                    },
                );
                Ok(())
            }
            SessionFactPayload::CompactionApplied(application) => {
                if self.active_compaction != Some(application.compaction_id)
                    || self
                        .compaction_terminals
                        .contains_key(&application.compaction_id)
                {
                    return self
                        .transition_error(fact.sequence, "compaction is not open for apply");
                }
                let Some(summary) = self
                    .compaction_summaries
                    .get(&application.compaction_summary_id)
                else {
                    return self
                        .transition_error(fact.sequence, "applied compaction summary is absent");
                };
                let start = &self.compaction_starts[&application.compaction_id];
                let request_closed = matches!(self.compaction_requests.get(&summary.compaction_request_id), Some(CompactionRequestState::Closed { closure, .. }) if closure.outcome == CompactionRequestOutcome::SummaryCommitted);
                if summary.compaction_id != application.compaction_id
                    || !request_closed
                    || application.source_context_revision != start.source_context_revision
                    || application.target_context_revision != start.target_context_revision
                    || application.replacement_manifest_id != summary.replacement_manifest_id
                {
                    return self.transition_error(
                        fact.sequence,
                        "compaction apply contradicts start or committed summary",
                    );
                }
                self.context_revision = application.target_context_revision;
                self.active_compaction = None;
                self.compaction_terminals.insert(
                    application.compaction_id,
                    CompactionTerminalState::Applied {
                        application: application.clone(),
                    },
                );
                Ok(())
            }
            SessionFactPayload::CompactionAbandoned(abandonment) => {
                if self.active_compaction != Some(abandonment.compaction_id)
                    || self
                        .compaction_summary_by_operation
                        .contains_key(&abandonment.compaction_id)
                    || self
                        .compaction_terminals
                        .contains_key(&abandonment.compaction_id)
                {
                    return self.transition_error(fact.sequence, "compaction cannot be abandoned");
                }
                validate_reason_code(&abandonment.reason_code)
                    .map_err(|error| self.at_sequence(fact.sequence, error))?;
                let latest = self
                    .compaction_requests
                    .values()
                    .filter(|request| {
                        request.preparation().compaction_id == abandonment.compaction_id
                    })
                    .max_by_key(|request| request.preparation().request_ordinal);
                match latest {
                    None if abandonment.last_compaction_request_id.is_none()
                        && abandonment.last_response_attempt_ordinal.is_none() => {}
                    Some(request)
                        if abandonment.last_compaction_request_id
                            == Some(request.preparation().compaction_request_id)
                            && abandonment.last_response_attempt_ordinal
                                == Some(
                                    self.compaction_attempt_failures
                                        .get(&request.preparation().compaction_request_id)
                                        .map_or(0, BTreeMap::len)
                                        as u32,
                                ) => {}
                    _ => {
                        return self.transition_error(
                            fact.sequence,
                            "compaction abandonment lineage is invalid",
                        );
                    }
                }
                if latest
                    .is_some_and(|request| matches!(request, CompactionRequestState::Open { .. }))
                {
                    return self.transition_error(
                        fact.sequence,
                        "open compaction request must close before abandonment",
                    );
                }
                self.active_compaction = None;
                self.compaction_terminals.insert(
                    abandonment.compaction_id,
                    CompactionTerminalState::Abandoned {
                        abandonment: abandonment.clone(),
                    },
                );
                Ok(())
            }
            SessionFactPayload::TurnClosed(closed) => {
                let Some(active) = self.active_turn.as_ref() else {
                    return self.transition_error(fact.sequence, "there is no active turn");
                };
                if active.turn_id != closed.turn_id {
                    return self.transition_error(fact.sequence, "closure targets a stale turn");
                }
                if self.active_step.is_some() {
                    return self.transition_error(
                        fact.sequence,
                        "turn cannot close with an active step or request",
                    );
                }
                self.validate_turn_close_from_step(closed, fact.sequence)?;
                if self.invocations.values().any(|invocation| {
                    invocation.turn_id() == closed.turn_id
                        && matches!(invocation, InvocationState::Registered { .. })
                }) {
                    return self.transition_error(
                        fact.sequence,
                        "turn cannot close with unclassified invocations",
                    );
                }
                self.active_turn = None;
                self.closed_turns.insert(closed.turn_id, closed.clone());
                Ok(())
            }
        }
    }

    fn record_full_spine_boundary(&mut self, fact: &SessionFact) {
        if self.full_spine_boundary.is_some()
            || !matches!(
                fact.payload,
                SessionFactPayload::StepStarted(_)
                    | SessionFactPayload::CompactionStarted(_)
                    | SessionFactPayload::ContextSourceMaterialized(_)
            )
        {
            return;
        }
        let imports_legacy_base = matches!(
            &fact.payload,
            SessionFactPayload::ContextSourceMaterialized(source)
                if is_legacy_compatibility_source(source)
        );
        let has_legacy_operation = imports_legacy_base
            || !self.closed_turns.is_empty()
            || !self.route_leases.is_empty()
            || !self.invocations.is_empty();
        self.lineage_level = if has_legacy_operation {
            AuthorityLineageLevel::Mixed
        } else {
            AuthorityLineageLevel::FullSpine
        };
        self.full_spine_boundary = Some(FullSpineBoundary {
            sequence: fact.sequence,
            event_id: fact.event_id,
        });
    }

    fn validate_tool_result_linkage(
        &self,
        result: &ToolResultRecorded,
        call: &ToolCallRecorded,
        sequence: u64,
    ) -> Result<()> {
        match result.disposition {
            ToolResultDisposition::Denied | ToolResultDisposition::NotDispatched => {
                if result.invocation_id.is_some()
                    || result.lease_id.is_some()
                    || !result.is_error
                    || result.reason_code.as_deref().is_none()
                {
                    return self.transition_error(
                        sequence,
                        "denied or not-dispatched result cannot carry invocation linkage and requires an error reason",
                    );
                }
                validate_reason_code(result.reason_code.as_deref().expect("checked above"))
                    .map_err(|error| self.at_sequence(sequence, error))?;
                if self.invocations.values().any(|invocation| {
                    invocation.turn_id() == self.steps[&call.step_id].turn_id
                        && invocation.call_id() == call.call_id
                }) {
                    return self.transition_error(
                        sequence,
                        "denied or not-dispatched result contradicts existing invocation authority",
                    );
                }
            }
            ToolResultDisposition::Settled => {
                if result.reason_code.is_some() {
                    return self.transition_error(
                        sequence,
                        "settled tool result cannot carry a disposition reason",
                    );
                }
                let (invocation_id, lease_id) = required_result_linkage(result, sequence)?;
                let Some(InvocationState::DurableSettled {
                    preparation,
                    settlement,
                    ..
                }) = self.invocations.get(&invocation_id)
                else {
                    return self.transition_error(
                        sequence,
                        "settled tool result requires a matching terminal invocation",
                    );
                };
                if preparation.lease_id != lease_id
                    || preparation.call_id != call.call_id
                    || preparation.invocation_name != call.invocation_name
                    || preparation.turn_id != self.steps[&call.step_id].turn_id
                    || result.is_error != (settlement.outcome != InvocationOutcome::Completed)
                {
                    return self.transition_error(
                        sequence,
                        "settled tool result contradicts invocation identity, lease, outcome, or tool name",
                    );
                }
            }
            ToolResultDisposition::UnknownCompletion => {
                let (invocation_id, lease_id) = required_result_linkage(result, sequence)?;
                let Some(InvocationState::DurableUnknown {
                    preparation,
                    classification,
                    ..
                }) = self.invocations.get(&invocation_id)
                else {
                    return self.transition_error(
                        sequence,
                        "unknown-completion result requires matching durable unknown invocation",
                    );
                };
                if preparation.lease_id != lease_id
                    || preparation.call_id != call.call_id
                    || preparation.invocation_name != call.invocation_name
                    || preparation.turn_id != self.steps[&call.step_id].turn_id
                    || !result.is_error
                    || result.reason_code.as_deref() != Some(classification.reason_code.as_str())
                {
                    return self.transition_error(
                        sequence,
                        "unknown-completion result contradicts invocation identity, lease, or classification",
                    );
                }
                validate_reason_code(&classification.reason_code)
                    .map_err(|error| self.at_sequence(sequence, error))?;
            }
        }
        Ok(())
    }

    fn validate_step_close(&self, closure: &StepClosed, sequence: u64) -> Result<()> {
        validate_reason_code(&closure.reason_code)
            .map_err(|error| self.at_sequence(sequence, error))?;
        let Some(active_step) = self.active_step.as_ref() else {
            return self.transition_error(sequence, "there is no active step");
        };
        if active_step.start.step_id != closure.step_id
            || active_step.start.turn_id != closure.turn_id
        {
            return self.transition_error(sequence, "step closure targets a stale step");
        }
        if active_step.active_request_id.is_some() {
            return self.transition_error(sequence, "step cannot close with an open model request");
        }
        if self.terminal_steps.contains_key(&closure.step_id) {
            return self.transition_error(sequence, "step is already terminal");
        }
        let commit = self
            .assistant_messages
            .values()
            .find(|message| message.step_id == closure.step_id);
        let call_count = self
            .tool_calls
            .values()
            .filter(|call| call.step_id == closure.step_id)
            .count();
        if commit.is_some_and(|message| message.tool_call_count as usize != call_count) {
            return self.transition_error(
                sequence,
                "committed assistant message tool-call count is not fully recorded",
            );
        }
        if self
            .tool_calls
            .values()
            .filter(|call| call.step_id == closure.step_id)
            .any(|call| !self.call_results.contains_key(&call.tool_call_id))
        {
            return self.transition_error(sequence, "step cannot close with a missing tool result");
        }
        if self.invocations.values().any(|invocation| {
            invocation.preparation().is_some_and(|preparation| {
                preparation.turn_id == closure.turn_id
                    && self
                        .turn_call_ids
                        .get(&closure.turn_id)
                        .and_then(|calls| calls.get(&preparation.call_id))
                        .is_some_and(|tool_call_id| {
                            self.tool_calls[tool_call_id].step_id == closure.step_id
                        })
                    && !matches!(
                        invocation,
                        InvocationState::DurableSettled { .. }
                            | InvocationState::DurableUnknown { .. }
                    )
            })
        }) {
            return self.transition_error(
                sequence,
                "step cannot close with an unresolved admitted invocation",
            );
        }
        let final_request = self
            .model_requests
            .values()
            .filter(|request| request.preparation().step_id == closure.step_id)
            .max_by_key(|request| request.preparation().request_ordinal);
        let Some(ModelRequestState::Closed {
            closure: request, ..
        }) = final_request
        else {
            return self.transition_error(sequence, "step has no terminal model request");
        };
        let valid = match closure.outcome {
            StepOutcome::ContinueLoop => {
                request.outcome == ModelRequestOutcome::ResponseCompleted
                    && commit.is_some()
                    && (call_count > 0 || closure.reason_code.ends_with("policy_continuation"))
            }
            StepOutcome::TurnCompleted => {
                request.outcome == ModelRequestOutcome::ResponseCompleted
                    && commit.is_some()
                    && call_count == 0
            }
            StepOutcome::Failed => request.outcome == ModelRequestOutcome::ProviderFailed,
            StepOutcome::Eof => request.outcome == ModelRequestOutcome::Eof,
            StepOutcome::Cancelled => request.outcome == ModelRequestOutcome::Cancelled,
            StepOutcome::TimedOut => request.outcome == ModelRequestOutcome::TimedOut,
            StepOutcome::Revoked => request.outcome == ModelRequestOutcome::Revoked,
            StepOutcome::Unknown => request.outcome == ModelRequestOutcome::Unknown,
        };
        if !valid {
            return self.transition_error(
                sequence,
                "step outcome and continuation contradict final request or committed message",
            );
        }
        Ok(())
    }

    fn validate_turn_close_from_step(&self, closed: &TurnClosed, sequence: u64) -> Result<()> {
        let terminal = self
            .terminal_steps
            .values()
            .filter(|terminal| match terminal {
                StepTerminalState::Closed { closure } => closure.turn_id == closed.turn_id,
                StepTerminalState::Abandoned { abandonment } => {
                    abandonment.turn_id == closed.turn_id
                }
            })
            .max_by_key(|terminal| match terminal {
                StepTerminalState::Closed { closure } => self.steps[&closure.step_id].step_ordinal,
                StepTerminalState::Abandoned { abandonment } => {
                    self.steps[&abandonment.step_id].step_ordinal
                }
            });
        let Some(terminal) = terminal else {
            if self.lineage_level != AuthorityLineageLevel::LegacyOnly
                && closed.outcome == TurnOutcome::Completed
            {
                return self.transition_error(
                    sequence,
                    "completed full-spine turn requires a terminal semantic step",
                );
            }
            // Legacy turns and pre-step abnormal exits retain their existing terminal semantics.
            return Ok(());
        };
        let valid = match terminal {
            StepTerminalState::Abandoned { .. } => matches!(
                closed.outcome,
                TurnOutcome::Interrupted
                    | TurnOutcome::Failed
                    | TurnOutcome::Cancelled
                    | TurnOutcome::TimedOut
                    | TurnOutcome::Revoked
                    | TurnOutcome::Unknown
            ),
            StepTerminalState::Closed { closure } => match closure.outcome {
                StepOutcome::ContinueLoop => {
                    self.active_turn
                        .as_ref()
                        .is_some_and(|turn| turn.accepted_interruption.is_some())
                        && !matches!(closed.outcome, TurnOutcome::Completed)
                }
                StepOutcome::TurnCompleted => closed.outcome == TurnOutcome::Completed,
                StepOutcome::Failed | StepOutcome::Eof => closed.outcome == TurnOutcome::Failed,
                StepOutcome::Cancelled => closed.outcome == TurnOutcome::Cancelled,
                StepOutcome::TimedOut => closed.outcome == TurnOutcome::TimedOut,
                StepOutcome::Revoked => closed.outcome == TurnOutcome::Revoked,
                StepOutcome::Unknown => closed.outcome == TurnOutcome::Unknown,
            },
        };
        if !valid {
            return self.transition_error(
                sequence,
                "turn outcome contradicts the terminal step continuation",
            );
        }
        Ok(())
    }

    fn validate_request_preparation(
        &self,
        preparation: &ModelRequestPrepared,
        sequence: u64,
    ) -> Result<()> {
        if preparation.request_ordinal == 0 {
            if preparation.purpose != ModelRequestPurpose::Initial
                || preparation.replaces_request_id.is_some()
            {
                return self.transition_error(
                    sequence,
                    "request zero must be initial and replace no request",
                );
            }
        } else {
            let predecessor = self.model_requests.values().find(|request| {
                let previous = request.preparation();
                previous.step_id == preparation.step_id
                    && previous.request_ordinal + 1 == preparation.request_ordinal
            });
            let Some(ModelRequestState::Closed {
                preparation: previous,
                closure,
                ..
            }) = predecessor
            else {
                return self.transition_error(
                    sequence,
                    "repair request has no immediately closed predecessor",
                );
            };
            let expected_purpose = match closure.outcome {
                ModelRequestOutcome::SupersededForContextRepair => {
                    ModelRequestPurpose::ContextOverflowRepair
                }
                ModelRequestOutcome::SupersededForHistoryRepair => {
                    ModelRequestPurpose::ProviderHistoryRepair
                }
                _ => {
                    return self.transition_error(
                        sequence,
                        "prior request outcome does not permit repair",
                    );
                }
            };
            if preparation.purpose != expected_purpose
                || preparation.replaces_request_id != Some(previous.request_id)
            {
                return self.transition_error(
                    sequence,
                    "repair purpose or replacement identity is invalid",
                );
            }
        }
        let mut seen_continuity = BTreeSet::new();
        for continuity_id in &preparation.continuity_refs {
            if !seen_continuity.insert(*continuity_id) {
                return self
                    .transition_error(sequence, "continuity references must be duplicate-free");
            }
            let Some(continuity) = self.provider_continuity.get(continuity_id) else {
                return self.transition_error(sequence, "continuity reference is unknown");
            };
            let Some(source_request) = self.model_requests.get(&continuity.request_id) else {
                return self.transition_error(sequence, "continuity source request is unknown");
            };
            if source_request.preparation().request_ordinal >= preparation.request_ordinal
                || source_request.preparation().step_id != preparation.step_id
            {
                return self.transition_error(
                    sequence,
                    "continuity reference is outside the prior request lineage",
                );
            }
        }
        if preparation
            .context_items
            .iter()
            .enumerate()
            .any(|(ordinal, item)| item.ordinal as usize != ordinal)
        {
            return self.transition_error(sequence, "context item ordinals are not contiguous");
        }
        for item in &preparation.context_items {
            if item.content_ref.projection_class() != ProjectionClass::Default
                || item.content_ref.byte_length() == 0
            {
                return self.transition_error(
                    sequence,
                    "context content must be non-empty default-projection content",
                );
            }
            self.validate_context_provenance(&item.provenance, sequence)?;
        }
        let expected_manifest = canonical_sha256(&preparation.context_items)?;
        if preparation.context_manifest_id != expected_manifest {
            return self.transition_error(sequence, "context manifest identity is invalid");
        }
        if preparation.schema_set.schema_set_version != 1 {
            return self.transition_error(sequence, "unsupported schema-set version");
        }
        if preparation
            .schema_set
            .schemas
            .iter()
            .enumerate()
            .any(|(ordinal, schema)| schema.ordinal as usize != ordinal)
        {
            return self.transition_error(sequence, "schema ordinals are not contiguous");
        }
        for schema in &preparation.schema_set.schemas {
            if schema.schema_dialect.trim().is_empty()
                || schema.schema_content_ref.projection_class() != ProjectionClass::Default
                || schema.schema_content_ref.byte_length() == 0
            {
                return self.transition_error(
                    sequence,
                    "schema identity contains invalid dialect or content",
                );
            }
        }
        let expected_schema_set = canonical_sha256(&preparation.schema_set)?;
        if preparation.schema_set_id != expected_schema_set {
            return self.transition_error(sequence, "schema-set identity is invalid");
        }
        Ok(())
    }

    fn validate_open_joined_request(
        &self,
        request_id: Uuid,
        step_id: Uuid,
        sequence: u64,
        subject: &str,
    ) -> Result<RouteLeaseRecorded> {
        let Some(active_step) = self.active_step.as_ref() else {
            return self.transition_error(sequence, "there is no active step");
        };
        if self
            .active_turn
            .as_ref()
            .is_some_and(|turn| turn.accepted_interruption.is_some())
        {
            return self.transition_error(
                sequence,
                format!("{subject} cannot follow interruption admission"),
            );
        }
        if active_step.start.step_id != step_id || active_step.active_request_id != Some(request_id)
        {
            return self.transition_error(sequence, format!("{subject} targets the wrong request"));
        }
        let Some(ModelRequestState::Open {
            route_join: Some(join),
            ..
        }) = self.model_requests.get(&request_id)
        else {
            return self.transition_error(
                sequence,
                format!("{subject} requires a joined open request"),
            );
        };
        let lease =
            self.route_leases
                .get(&join.lease_id)
                .ok_or_else(|| AuthorityError::Transition {
                    sequence,
                    message: format!("{subject} joined route lease is absent"),
                })?;
        Ok(lease.clone())
    }

    fn validate_response_attempt(
        &self,
        request_id: Uuid,
        step_id: Uuid,
        response_attempt_ordinal: u32,
        sequence: u64,
        subject: &str,
    ) -> Result<RouteLeaseRecorded> {
        let lease = self.validate_open_joined_request(request_id, step_id, sequence, subject)?;
        let failures = self.response_attempt_failures.get(&request_id);
        let expected = failures.map_or(0, BTreeMap::len);
        let expected = u32::try_from(expected).map_err(|_| AuthorityError::Transition {
            sequence,
            message: "response-attempt ordinal overflow".into(),
        })?;
        if failures.is_some_and(|failures| {
            failures
                .keys()
                .enumerate()
                .any(|(ordinal, stored)| u32::try_from(ordinal).ok() != Some(*stored))
        }) {
            return self.transition_error(
                sequence,
                "durable response-attempt failure lineage is not contiguous",
            );
        }
        if response_attempt_ordinal != expected {
            return self.transition_error(
                sequence,
                format!(
                    "{subject} requires current response attempt ordinal {expected}, got {response_attempt_ordinal}"
                ),
            );
        }
        Ok(lease)
    }

    fn validate_context_provenance(
        &self,
        provenance: &ModelContextProvenance,
        sequence: u64,
    ) -> Result<()> {
        if provenance.owner_id.is_some() != provenance.owner_generation_id.is_some() {
            return self.transition_error(sequence, "context owner fields must appear together");
        }
        match provenance.source_kind {
            ModelContextSourceKind::Prompt => {
                if provenance.owner_id.is_some() {
                    return self.transition_error(sequence, "prompt context cannot name an owner");
                }
                let Some(source_event_id) = provenance.source_event_id else {
                    return self
                        .transition_error(sequence, "prompt context requires a source event");
                };
                let source_identity = parse_source_uuid(&provenance.source_identity)
                    .map_err(|error| self.at_sequence(sequence, error))?;
                if self.prompt_source_events.get(&source_identity) != Some(&source_event_id) {
                    return self.transition_error(
                        sequence,
                        "prompt context source identity or event is unknown",
                    );
                }
            }
            ModelContextSourceKind::AssistantMessage | ModelContextSourceKind::ToolResult => {
                if provenance.owner_id.is_some() {
                    return self.transition_error(
                        sequence,
                        "assistant and tool-result context cannot name an owner",
                    );
                }
                let Some(source_event_id) = provenance.source_event_id else {
                    return self.transition_error(
                        sequence,
                        "assistant and tool-result context requires a source event",
                    );
                };
                let source_identity = parse_source_uuid(&provenance.source_identity)
                    .map_err(|error| self.at_sequence(sequence, error))?;
                let expected = match provenance.source_kind {
                    ModelContextSourceKind::AssistantMessage => {
                        self.assistant_message_source_events.get(&source_identity)
                    }
                    ModelContextSourceKind::ToolResult => {
                        self.tool_result_source_events.get(&source_identity)
                    }
                    _ => unreachable!(),
                };
                if expected != Some(&source_event_id) {
                    return self.transition_error(
                        sequence,
                        "assistant or tool-result context source identity or event is unknown",
                    );
                }
            }
            ModelContextSourceKind::SystemInstruction
            | ModelContextSourceKind::DeveloperInstruction
            | ModelContextSourceKind::ContributionContext => {
                if provenance
                    .source_identity
                    .as_deref()
                    .is_none_or(|value| value.is_empty() || value.len() > 512)
                    || provenance
                        .owner_id
                        .as_deref()
                        .is_none_or(|value| value.is_empty() || value.len() > 512)
                    || provenance.owner_generation_id.is_none()
                {
                    return self.transition_error(
                        sequence,
                        "instruction context requires source and owner provenance",
                    );
                }
                match provenance.source_event_id {
                    Some(source_event_id) => {
                        let Some(source_id) = provenance
                            .source_identity
                            .as_deref()
                            .and_then(|value| Uuid::parse_str(value).ok())
                        else {
                            return self.transition_error(
                                sequence,
                                "materialized context source identity must be a UUID",
                            );
                        };
                        let Some(source) = self.materialized_context_sources.get(&source_id) else {
                            return self.transition_error(sequence, "context source is unknown");
                        };
                        let expected_kind = match provenance.source_kind {
                            ModelContextSourceKind::SystemInstruction => {
                                ContextSourceKind::SystemInstruction
                            }
                            ModelContextSourceKind::DeveloperInstruction => {
                                ContextSourceKind::DeveloperInstruction
                            }
                            ModelContextSourceKind::ContributionContext => {
                                ContextSourceKind::ContributionContext
                            }
                            _ => unreachable!(),
                        };
                        if self.materialized_context_source_events.get(&source_id)
                            != Some(&source_event_id)
                            || source.source_kind != expected_kind
                            || source.owner_id != provenance.owner_id.as_deref().unwrap()
                            || Some(&source.owner_generation_id)
                                != provenance.owner_generation_id.as_ref()
                        {
                            return self.transition_error(
                                sequence,
                                "materialized context provenance does not match its source event",
                            );
                        }
                    }
                    None if provenance
                        .source_identity
                        .as_deref()
                        .is_some_and(|identity| identity.starts_with("legacy-")) => {}
                    None => {
                        return self.transition_error(
                            sequence,
                            "post-boundary generated context requires a materialized source event",
                        );
                    }
                }
            }
            ModelContextSourceKind::CompactionSummary => {
                if provenance.owner_id.is_some() {
                    return self.transition_error(
                        sequence,
                        "compaction summary context cannot name a generated-context owner",
                    );
                }
                let Some(source_event_id) = provenance.source_event_id else {
                    return self.transition_error(
                        sequence,
                        "compaction summary context requires a source event",
                    );
                };
                let source_identity = parse_source_uuid(&provenance.source_identity)
                    .map_err(|error| self.at_sequence(sequence, error))?;
                if self.compaction_summary_source_events.get(&source_identity)
                    != Some(&source_event_id)
                {
                    return self.transition_error(
                        sequence,
                        "compaction summary context source identity or event is unknown",
                    );
                }
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn terminalize_active_step_for_test(&mut self) {
        assert!(
            self.active_step
                .as_ref()
                .is_some_and(|step| step.active_request_id.is_none()),
            "test step must have no active request"
        );
        self.active_step = None;
    }

    fn transition_error<T>(&self, sequence: u64, message: impl Into<String>) -> Result<T> {
        Err(AuthorityError::Transition {
            sequence,
            message: message.into(),
        })
    }

    fn at_sequence(&self, sequence: u64, error: AuthorityError) -> AuthorityError {
        AuthorityError::Transition {
            sequence,
            message: error.to_string(),
        }
    }
}

fn validate_entity_uuid(id: Uuid, label: &str) -> Result<()> {
    if id.is_nil() || !matches!(id.get_version_num(), 4 | 7) {
        return Err(AuthorityError::Invalid(format!(
            "{label} must be a non-nil UUIDv4 or UUIDv7"
        )));
    }
    Ok(())
}

fn parse_source_uuid(value: &Option<String>) -> Result<Uuid> {
    let value = value
        .as_deref()
        .ok_or_else(|| AuthorityError::Invalid("context source identity is absent".into()))?;
    let id = Uuid::parse_str(value)
        .map_err(|_| AuthorityError::Invalid("context source identity is not a UUID".into()))?;
    validate_entity_uuid(id, "context source identity")?;
    if id.to_string() != value {
        return Err(AuthorityError::Invalid(
            "context source identity is not canonical lowercase UUID text".into(),
        ));
    }
    Ok(id)
}

fn required_result_linkage(result: &ToolResultRecorded, sequence: u64) -> Result<(Uuid, Uuid)> {
    match (result.invocation_id, result.lease_id) {
        (Some(invocation_id), Some(lease_id)) => Ok((invocation_id, lease_id)),
        _ => Err(AuthorityError::Transition {
            sequence,
            message: "settled or unknown-completion result requires invocation and lease linkage"
                .into(),
        }),
    }
}

fn validate_reason_code(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'_' | b'.' | b':' | b'-')
        })
    {
        return Err(AuthorityError::Invalid("invalid stable reason code".into()));
    }
    Ok(())
}

fn validate_request_closure_recovery(
    fact: &SessionFact,
    closure: &ModelRequestClosed,
) -> Result<()> {
    if closure.outcome != ModelRequestOutcome::Abandoned {
        if closure.recovery_rule_version.is_some() {
            return Err(AuthorityError::Invalid(
                "live request closure cannot carry a recovery rule version".into(),
            ));
        }
        return Ok(());
    }
    let Some(rule) = closure.recovery_rule_version else {
        return Err(AuthorityError::Invalid(
            "abandoned request closure requires a recovery rule version".into(),
        ));
    };
    let identity = format!(
        "{}:{}:model.request_closed:{}:{rule}",
        fact.stream_id, closure.request_id, closure.reason_code
    );
    let expected_event = Uuid::new_v5(&RECOVERY_NAMESPACE, identity.as_bytes());
    let expected_command = Uuid::new_v5(
        &RECOVERY_NAMESPACE,
        format!("command:{identity}").as_bytes(),
    );
    if fact.event_id != expected_event
        || fact.command_id != expected_command
        || fact.command_fingerprint != recovery_fingerprint(&identity)
    {
        return Err(AuthorityError::Invalid(
            "abandoned request closure is not deterministic recovery evidence".into(),
        ));
    }
    Ok(())
}

fn validate_step_abandonment_recovery(
    fact: &SessionFact,
    abandonment: &StepAbandoned,
) -> Result<()> {
    validate_reason_code(&abandonment.reason_code)?;
    if abandonment.recovery_rule_version == 0 {
        return Err(AuthorityError::Invalid(
            "step abandonment requires a terminalization rule version".into(),
        ));
    }
    let rule = abandonment.recovery_rule_version;
    let identity = format!(
        "{}:{}:step.abandoned:{}:{rule}",
        fact.stream_id, abandonment.step_id, abandonment.reason_code
    );
    let expected_event = Uuid::new_v5(&RECOVERY_NAMESPACE, identity.as_bytes());
    let expected_command = Uuid::new_v5(
        &RECOVERY_NAMESPACE,
        format!("command:{identity}").as_bytes(),
    );
    if fact.event_id != expected_event
        || fact.command_id != expected_command
        || fact.command_fingerprint != recovery_fingerprint(&identity)
    {
        return Err(AuthorityError::Invalid(
            "step abandonment is not deterministic recovery evidence".into(),
        ));
    }
    Ok(())
}

fn canonical_sha256<T: Serialize>(value: &T) -> Result<String> {
    fn sort(value: &mut Value) {
        match value {
            Value::Array(values) => values.iter_mut().for_each(sort),
            Value::Object(values) => {
                let mut sorted = BTreeMap::new();
                for (key, mut value) in std::mem::take(values) {
                    sort(&mut value);
                    sorted.insert(key, value);
                }
                values.extend(sorted);
            }
            _ => {}
        }
    }

    let mut value = serde_json::to_value(value)?;
    sort(&mut value);
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(&value)?)))
}

fn commit_content_matches_chunks(
    commit: &AssistantMessageCommitted,
    chunks: &[AssistantContentAppended],
) -> bool {
    if commit
        .content
        .windows(2)
        .any(|pair| pair[0].content_kind >= pair[1].content_kind)
        || commit
            .content
            .iter()
            .any(|manifest| !is_sha256_hex(&manifest.content_digest))
    {
        return false;
    }
    let kinds = [AssistantContentKind::Text, AssistantContentKind::Thinking];
    let expected_kinds = kinds
        .into_iter()
        .filter(|kind| chunks.iter().any(|chunk| chunk.content_kind == *kind));
    let mut manifests = commit.content.iter();
    for kind in expected_kinds {
        let Some(manifest) = manifests.next() else {
            return false;
        };
        if manifest.content_kind != kind
            || manifest.chunk_refs
                != chunks
                    .iter()
                    .filter(|chunk| chunk.content_kind == kind)
                    .map(|chunk| chunk.content_ref.clone())
                    .collect::<Vec<_>>()
        {
            return false;
        }
    }
    manifests.next().is_none()
}

fn invocation_blocks_migration(invocation: &InvocationState) -> bool {
    !matches!(
        invocation,
        InvocationState::Settled { .. } | InvocationState::DurableSettled { .. }
    )
}

pub(crate) fn invocation_blocks_session_replacement(invocation: &InvocationState) -> bool {
    invocation_blocks_migration(invocation)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionAuthoritySnapshot {
    snapshot_version: u16,
    reducer_version: u16,
    session_id: String,
    stream_id: Uuid,
    last_sequence: u64,
    last_event_id: Uuid,
    state: SessionAuthorityState,
}

impl SessionAuthoritySnapshot {
    fn from_state(state: &SessionAuthorityState) -> Result<Self> {
        Ok(Self {
            snapshot_version: SNAPSHOT_VERSION,
            reducer_version: REDUCER_VERSION,
            session_id: state
                .session_id
                .clone()
                .ok_or_else(|| AuthorityError::Invalid("snapshot session is absent".into()))?,
            stream_id: state
                .stream_id
                .ok_or_else(|| AuthorityError::Invalid("snapshot stream is absent".into()))?,
            last_sequence: state.last_sequence,
            last_event_id: state
                .last_event_id
                .ok_or_else(|| AuthorityError::Invalid("snapshot cursor is absent".into()))?,
            state: state.clone(),
        })
    }

    fn validate(&self) -> Result<()> {
        if self.snapshot_version != SNAPSHOT_VERSION || self.reducer_version != REDUCER_VERSION {
            return Err(AuthorityError::Invalid(
                "unsupported snapshot or reducer version".into(),
            ));
        }
        if self.state.session_id.as_deref() != Some(self.session_id.as_str())
            || self.state.stream_id != Some(self.stream_id)
            || self.state.last_sequence != self.last_sequence
            || self.state.last_event_id != Some(self.last_event_id)
        {
            return Err(AuthorityError::Invalid(
                "snapshot envelope does not match state".into(),
            ));
        }
        Ok(())
    }
}

pub(crate) fn reconstruct(facts: &[SessionFact]) -> Result<SessionAuthorityState> {
    let mut state = SessionAuthorityState::default();
    for fact in facts {
        state.apply(fact)?;
    }
    Ok(state)
}

#[derive(Debug)]
pub(crate) struct SessionAuthority {
    store: SessionAuthorityStore,
    session_snapshot: PathBuf,
    _writer_lease: crate::filelock::FileLockGuard,
    state: SessionAuthorityState,
    session_id: String,
    stream_id: Uuid,
    runtime_generation_id: String,
    boot_execution_binding: Option<ExecutionBindingGeneration>,
    mutation_fence_poisoned: bool,
    projection_wake: Option<crate::session_shadow_projection::SessionProjectionWakeHandle>,
}

#[derive(Clone)]
pub(crate) struct SessionAuthorityHandle(Arc<Mutex<SessionAuthority>>);

impl std::fmt::Debug for SessionAuthorityHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionAuthorityHandle")
            .field("session_id", &self.session_id())
            .finish_non_exhaustive()
    }
}

impl PartialEq for SessionAuthorityHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SessionAuthorityHandle {}

impl SessionAuthorityHandle {
    pub(crate) fn new(authority: SessionAuthority) -> Self {
        Self(Arc::new(Mutex::new(authority)))
    }

    fn lock(&self) -> MutexGuard<'_, SessionAuthority> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn state(&self) -> SessionAuthorityState {
        self.lock().state().clone()
    }

    pub(crate) fn activity_source(
        &self,
    ) -> (SessionAuthorityState, Result<Option<(u64, TurnClosed)>>) {
        let authority = self.lock();
        let state = authority.state().clone();
        let terminal = authority.store.read_stable_facts().map(|facts| {
            facts.into_iter().rev().find_map(|fact| match fact.payload {
                SessionFactPayload::TurnClosed(turn) => Some((fact.sequence, turn)),
                _ => None,
            })
        });
        (state, terminal)
    }

    pub(crate) fn unknown_retry_disposition(
        &self,
        call_id: &str,
    ) -> Result<UnknownRetryDisposition> {
        self.lock().state().unknown_retry_disposition(call_id)
    }

    pub(crate) fn session_id(&self) -> String {
        self.lock().session_id.clone()
    }

    pub(crate) fn projection_worker_descriptor(
        &self,
    ) -> crate::session_shadow_projection::SessionProjectionWorkerDescriptor {
        let authority = self.lock();
        crate::session_shadow_projection::SessionProjectionWorkerDescriptor {
            session_snapshot: authority.session_snapshot.clone(),
            session_id: authority.session_id.clone(),
            stream_id: authority.stream_id,
        }
    }

    pub(crate) fn set_projection_wake(
        &self,
        wake: crate::session_shadow_projection::SessionProjectionWakeHandle,
    ) {
        self.lock().projection_wake = Some(wake);
    }

    pub(crate) fn clear_projection_wake(&self) {
        self.lock().projection_wake = None;
    }

    pub(crate) fn import_legacy_compatibility_base(
        &self,
        compatibility: &[crate::bridge::LlmMessage],
        recorded_at: &str,
    ) -> Result<bool> {
        self.lock()
            .import_legacy_compatibility_base(compatibility, recorded_at)
    }

    pub(crate) fn stage_attachment(&self, source: &Path) -> Result<AttachmentRef> {
        self.lock().stage_attachment(source)
    }

    pub(crate) fn validate_attachment(&self, attachment: &AttachmentRef) -> Result<PathBuf> {
        self.lock().validate_attachment(attachment)
    }

    pub(crate) fn write_content(
        &self,
        bytes: &[u8],
        media_type: &str,
        projection_class: ProjectionClass,
    ) -> Result<ContentRef> {
        self.lock()
            .write_content(bytes, media_type, projection_class)
    }

    pub(crate) fn read_content(
        &self,
        content_ref: &ContentRef,
        required_projection: ProjectionClass,
    ) -> Result<Vec<u8>> {
        self.lock().read_content(content_ref, required_projection)
    }

    pub(crate) fn validate_content_ref(
        &self,
        content_ref: &ContentRef,
        required_projection: ProjectionClass,
    ) -> Result<()> {
        self.lock()
            .validate_content_ref(content_ref, required_projection)
    }

    pub(crate) fn admit_prompt(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        admission: PromptAdmitted,
    ) -> Result<bool> {
        self.lock().admit_prompt(command_id, recorded_at, admission)
    }

    pub(crate) fn remove_prompt(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        prompt_id: Uuid,
        reason: PromptRemovalReason,
    ) -> Result<bool> {
        self.lock()
            .remove_prompt(command_id, recorded_at, prompt_id, reason)
    }

    pub(crate) fn start_turn(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        turn_id: Uuid,
        prompt_id: Uuid,
    ) -> Result<bool> {
        self.lock()
            .start_turn(command_id, recorded_at, turn_id, prompt_id)
    }

    pub(crate) fn request_interruption(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        request: TurnInterruptionRequested,
    ) -> Result<bool> {
        self.lock()
            .request_interruption(command_id, recorded_at, request)
    }

    pub(crate) fn record_route_lease(
        &self,
        recorded_at: &str,
        lease: RouteLeaseRecorded,
    ) -> Result<bool> {
        self.lock().record_route_lease(recorded_at, lease)
    }

    pub(crate) fn record_route_endpoint_provenance(
        &self,
        recorded_at: &str,
        provenance: RouteEndpointProvenanceRecorded,
    ) -> Result<bool> {
        self.lock()
            .record_route_endpoint_provenance(recorded_at, provenance)
    }

    pub(crate) fn record_compaction_endpoint_provenance(
        &self,
        recorded_at: &str,
        provenance: CompactionEndpointProvenanceRecorded,
    ) -> Result<bool> {
        self.lock()
            .record_compaction_endpoint_provenance(recorded_at, provenance)
    }

    pub(crate) fn start_step(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        start: StepStarted,
    ) -> Result<bool> {
        self.lock().start_step(command_id, recorded_at, start)
    }

    pub(crate) fn materialize_context_source(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        source: ContextSourceMaterialized,
    ) -> Result<Uuid> {
        self.lock()
            .materialize_context_source(command_id, recorded_at, source)
    }

    pub(crate) fn prepare_model_request(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        preparation: ModelRequestPrepared,
    ) -> Result<bool> {
        self.lock()
            .prepare_model_request(command_id, recorded_at, preparation)
    }

    pub(crate) fn join_model_request_route(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        join: ModelRequestRouteJoined,
    ) -> Result<bool> {
        self.lock()
            .join_model_request_route(command_id, recorded_at, join)
    }

    pub(crate) fn append_assistant_content(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        chunk: AssistantContentAppended,
    ) -> Result<bool> {
        self.lock()
            .append_assistant_content(command_id, recorded_at, chunk)
    }

    pub(crate) fn fail_model_response_attempt(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        failure: ModelResponseAttemptFailed,
    ) -> Result<bool> {
        self.lock()
            .fail_model_response_attempt(command_id, recorded_at, failure)
    }

    pub(crate) fn store_provider_continuity(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        continuity: ProviderContinuityStored,
    ) -> Result<bool> {
        self.lock()
            .store_provider_continuity(command_id, recorded_at, continuity)
    }

    pub(crate) fn commit_assistant_message(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        commit: AssistantMessageCommitted,
    ) -> Result<bool> {
        self.lock()
            .commit_assistant_message(command_id, recorded_at, commit)
    }

    pub(crate) fn record_tool_call(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        call: ToolCallRecorded,
    ) -> Result<bool> {
        self.lock().record_tool_call(command_id, recorded_at, call)
    }

    pub(crate) fn record_tool_result(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        result: ToolResultRecorded,
    ) -> Result<bool> {
        self.lock()
            .record_tool_result(command_id, recorded_at, result)
    }

    pub(crate) fn close_model_request(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        closure: ModelRequestClosed,
    ) -> Result<bool> {
        self.lock()
            .close_model_request(command_id, recorded_at, closure)
    }

    pub(crate) fn close_step(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        closure: StepClosed,
    ) -> Result<bool> {
        self.lock().close_step(command_id, recorded_at, closure)
    }

    pub(crate) fn terminalize_active_semantic_step(
        &self,
        recorded_at: &str,
        terminalization: SemanticTerminalization,
    ) -> Result<bool> {
        self.lock()
            .terminalize_active_semantic_step(recorded_at, terminalization)
    }

    pub(crate) fn close_turn(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        closure: TurnClosed,
    ) -> Result<bool> {
        self.lock().close_turn(command_id, recorded_at, closure)
    }

    pub(crate) fn start_compaction(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        start: CompactionStarted,
    ) -> Result<bool> {
        self.lock().start_compaction(command_id, recorded_at, start)
    }

    pub(crate) fn prepare_compaction_request(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        preparation: CompactionRequestPrepared,
    ) -> Result<bool> {
        self.lock()
            .prepare_compaction_request(command_id, recorded_at, preparation)
    }

    pub(crate) fn fail_compaction_response_attempt(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        failure: CompactionResponseAttemptFailed,
    ) -> Result<bool> {
        self.lock()
            .fail_compaction_response_attempt(command_id, recorded_at, failure)
    }

    pub(crate) fn commit_compaction_summary(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        summary: CompactionSummaryCommitted,
    ) -> Result<bool> {
        self.lock()
            .commit_compaction_summary(command_id, recorded_at, summary)
    }

    pub(crate) fn close_compaction_request(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        closure: CompactionRequestClosed,
    ) -> Result<bool> {
        self.lock()
            .close_compaction_request(command_id, recorded_at, closure)
    }

    pub(crate) fn apply_compaction(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        application: CompactionApplied,
    ) -> Result<bool> {
        self.lock()
            .apply_compaction(command_id, recorded_at, application)
    }

    pub(crate) fn abandon_compaction(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        abandonment: CompactionAbandoned,
    ) -> Result<bool> {
        self.lock()
            .abandon_compaction(command_id, recorded_at, abandonment)
    }

    pub(crate) fn bind_execution_at_boot(&self, binding: ExecutionBindingGeneration) -> Result<()> {
        self.lock().bind_execution_at_boot(binding)
    }

    pub(crate) fn migrate_execution_binding(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        from_generation: ExecutionBindingGeneration,
        target_generation: ExecutionBindingGeneration,
    ) -> Result<bool> {
        self.lock().migrate_execution_binding(
            command_id,
            recorded_at,
            from_generation,
            target_generation,
        )
    }

    pub(crate) fn migrate_execution_binding_typed(
        &self,
        command_id: Uuid,
        recorded_at: &str,
        from_generation: ExecutionBindingGeneration,
        target_generation: ExecutionBindingGeneration,
    ) -> std::result::Result<bool, ExecutionBindingMigrationError> {
        let mut authority = self.lock();
        if let Some(rejection) =
            authority.execution_binding_migration_rejection(&from_generation, &target_generation)
        {
            return Err(ExecutionBindingMigrationError::Rejected(rejection));
        }
        authority
            .migrate_execution_binding(command_id, recorded_at, from_generation, target_generation)
            .map_err(ExecutionBindingMigrationError::Authority)
    }

    pub(crate) fn prepare_invocation(
        &self,
        recorded_at: &str,
        preparation: InvocationPrepared,
    ) -> Result<bool> {
        self.lock().prepare_invocation(recorded_at, preparation)
    }

    pub(crate) fn mark_invocation_dispatched(
        &self,
        recorded_at: &str,
        dispatch: InvocationDispatched,
    ) -> Result<bool> {
        self.lock()
            .mark_invocation_dispatched(recorded_at, dispatch)
    }

    pub(crate) fn acknowledge_invocation(
        &self,
        recorded_at: &str,
        acknowledgement: InvocationAcknowledged,
    ) -> Result<bool> {
        self.lock()
            .acknowledge_invocation(recorded_at, acknowledgement)
    }

    pub(crate) fn settle_invocation(
        &self,
        recorded_at: &str,
        settlement: InvocationSettled,
    ) -> Result<bool> {
        self.lock().settle_invocation(recorded_at, settlement)
    }

    pub(crate) fn classify_invocation_unknown(
        &self,
        recorded_at: &str,
        classification: InvocationClassifiedUnknown,
    ) -> Result<bool> {
        self.lock()
            .classify_invocation_unknown(recorded_at, classification)
    }

    pub(crate) fn record_mutation_fence(
        &self,
        evidence: &InvocationMutationFenceEvidence,
    ) -> Result<()> {
        let mut authority = self.lock();
        match authority.store.record_mutation_fence(evidence) {
            Ok(()) => Ok(()),
            Err(error) => {
                authority.mutation_fence_poisoned = true;
                Err(error)
            }
        }
    }

    pub(crate) fn active_mutation_fence(
        &self,
        domain: &RuntimeMutationDomainId,
        key: &RuntimeMutationFenceKey,
    ) -> Result<Option<InvocationMutationFenceEvidence>> {
        let authority = self.lock();
        if authority.mutation_fence_poisoned {
            return Err(AuthorityError::Invalid(
                "invocation mutation fence writer is poisoned".into(),
            ));
        }
        authority.store.active_mutation_fence(domain, key)
    }

    #[cfg(test)]
    pub(crate) fn make_next_append_fail(&self) {
        let path = self.lock().store.log_path.clone();
        let mut permissions = fs::metadata(&path)
            .expect("authority log metadata")
            .permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).expect("make authority log read-only");
    }
}

impl SessionAuthority {
    pub(crate) fn open(
        session_snapshot: &Path,
        session_id: impl Into<String>,
        workspace_identity: impl Into<String>,
        runtime_generation_id: impl Into<String>,
        created_by: ActorIdentity,
        recorded_at: &str,
    ) -> Result<Self> {
        let session_id = session_id.into();
        let workspace_identity = workspace_identity.into();
        let runtime_generation_id = runtime_generation_id.into();
        let store = SessionAuthorityStore::adjacent_to(session_snapshot)?;
        let writer_lease = crate::filelock::try_acquire_lock(&store.writer_lease_path())
            .map_err(|error| AuthorityError::Invalid(error.to_string()))?
            .ok_or_else(|| {
                AuthorityError::Invalid("session authority already has an active writer".into())
            })?;
        store.open_blob_store()?;
        store.ensure_emergency_fence_dir()?;
        let initial = store.load()?;

        if initial.last_sequence > 0 && initial.session_id.as_deref() != Some(session_id.as_str()) {
            return Err(AuthorityError::Invalid(
                "authority stream belongs to a different session".into(),
            ));
        }
        if initial.last_sequence > 0
            && initial.workspace_identity.as_deref() != Some(workspace_identity.as_str())
        {
            return Err(AuthorityError::Invalid(
                "authority stream belongs to a different workspace".into(),
            ));
        }

        let mut state = store.recover(recorded_at)?;

        if state.last_sequence == 0 {
            let stream_id = Uuid::new_v4();
            let payload = SessionFactPayload::SessionCreated(SessionCreated {
                workspace_identity: workspace_identity.clone(),
                created_by,
                runtime_generation_id: runtime_generation_id.clone(),
            });
            append_payload(
                &store,
                &mut state,
                &session_id,
                stream_id,
                Uuid::new_v4(),
                recorded_at,
                payload,
            )?;
        }

        if state.session_id.as_deref() != Some(session_id.as_str()) {
            return Err(AuthorityError::Invalid(
                "authority stream belongs to a different session".into(),
            ));
        }
        if state.workspace_identity.as_deref() != Some(workspace_identity.as_str()) {
            return Err(AuthorityError::Invalid(
                "authority stream belongs to a different workspace".into(),
            ));
        }
        let stream_id = state
            .stream_id
            .ok_or_else(|| AuthorityError::Invalid("authority stream has no identity".into()))?;
        let runtime_generation_id = state.runtime_generation_id.clone().ok_or_else(|| {
            AuthorityError::Invalid("authority stream has no runtime generation".into())
        })?;

        Ok(Self {
            store,
            session_snapshot: session_snapshot.to_path_buf(),
            _writer_lease: writer_lease,
            state,
            session_id,
            stream_id,
            runtime_generation_id,
            boot_execution_binding: None,
            mutation_fence_poisoned: false,
            projection_wake: None,
        })
    }

    pub(crate) fn import_legacy_compatibility_base(
        &mut self,
        compatibility: &[crate::bridge::LlmMessage],
        recorded_at: &str,
    ) -> Result<bool> {
        if self.state.full_spine_boundary.is_some()
            || compatibility.is_empty()
            || self
                .state
                .materialized_context_sources
                .values()
                .any(is_legacy_compatibility_source)
        {
            return Ok(false);
        }
        let replay = crate::session_replay::SessionReplay::replay_prefix(
            &self.session_snapshot,
            &self.session_id,
            self.stream_id,
            crate::session_replay::ReplayEnd::EndOfStream,
        )?;
        let legacy = legacy_compatibility_prefix(&replay, compatibility);
        if legacy.is_empty() {
            return Ok(false);
        }
        let bytes = legacy_compatibility_base_bytes(legacy)?;
        let content_ref =
            self.store
                .write_content(&bytes, "application/json", ProjectionClass::Default)?;
        let owner_generation_id =
            RuntimeContributionGenerationId::new("session-resume:legacy-base-v1")
                .map_err(|error| AuthorityError::Invalid(error.to_string()))?;
        append_payload(
            &self.store,
            &mut self.state,
            &self.session_id,
            self.stream_id,
            Uuid::new_v5(
                &Uuid::NAMESPACE_URL,
                format!("omegon:{}:legacy-compatibility-base-v1", self.session_id).as_bytes(),
            ),
            recorded_at,
            SessionFactPayload::ContextSourceMaterialized(ContextSourceMaterialized {
                context_source_id: Uuid::new_v4(),
                source_kind: ContextSourceKind::ContributionContext,
                source_identity: "legacy-compatibility-base-v1".into(),
                owner_id: "compatibility:session-resume".into(),
                owner_generation_id,
                content_ref,
            }),
        )?;
        Ok(true)
    }

    pub(crate) fn state(&self) -> &SessionAuthorityState {
        &self.state
    }

    pub(crate) fn write_content(
        &self,
        bytes: &[u8],
        media_type: &str,
        projection_class: ProjectionClass,
    ) -> Result<ContentRef> {
        self.store
            .write_content(bytes, media_type, projection_class)
    }

    pub(crate) fn read_content(
        &self,
        content_ref: &ContentRef,
        required_projection: ProjectionClass,
    ) -> Result<Vec<u8>> {
        self.store.read_content(content_ref, required_projection)
    }

    pub(crate) fn validate_content_ref(
        &self,
        content_ref: &ContentRef,
        required_projection: ProjectionClass,
    ) -> Result<()> {
        self.store
            .validate_content_ref(content_ref, required_projection)
    }

    pub(crate) fn bind_execution_at_boot(
        &mut self,
        binding: ExecutionBindingGeneration,
    ) -> Result<()> {
        if self
            .state
            .execution_binding_generation
            .as_ref()
            .is_some_and(|durable| durable != &binding)
        {
            return Err(AuthorityError::Invalid(
                "boot execution binding does not match durable session binding".into(),
            ));
        }
        if self
            .boot_execution_binding
            .as_ref()
            .is_some_and(|current| current != &binding)
        {
            return Err(AuthorityError::Invalid(
                "boot execution binding is already established".into(),
            ));
        }
        self.boot_execution_binding = Some(binding);
        Ok(())
    }

    pub(crate) fn stage_attachment(&self, source: &Path) -> Result<AttachmentRef> {
        self.store.stage_attachment(source)
    }

    pub(crate) fn validate_attachment(&self, attachment: &AttachmentRef) -> Result<PathBuf> {
        self.store.validate_attachment(attachment)
    }

    pub(crate) fn admit_prompt(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        admission: PromptAdmitted,
    ) -> Result<bool> {
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::PromptAdmitted(admission),
        )
    }

    pub(crate) fn remove_prompt(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        prompt_id: Uuid,
        reason: PromptRemovalReason,
    ) -> Result<bool> {
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::PromptRemoved(PromptRemoved { prompt_id, reason }),
        )
    }

    pub(crate) fn start_turn(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        turn_id: Uuid,
        prompt_id: Uuid,
    ) -> Result<bool> {
        let runtime_generation_id = self.runtime_generation_id.clone();
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::TurnStarted(TurnStarted {
                turn_id,
                prompt_id,
                runtime_generation_id,
            }),
        )
    }

    pub(crate) fn request_interruption(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        request: TurnInterruptionRequested,
    ) -> Result<bool> {
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::TurnInterruptionRequested(request),
        )
    }

    pub(crate) fn record_route_lease(
        &mut self,
        recorded_at: &str,
        lease: RouteLeaseRecorded,
    ) -> Result<bool> {
        self.append(
            lease.lease_id,
            recorded_at,
            SessionFactPayload::RouteLeaseRecorded(lease),
        )
    }

    pub(crate) fn record_route_endpoint_provenance(
        &mut self,
        recorded_at: &str,
        provenance: RouteEndpointProvenanceRecorded,
    ) -> Result<bool> {
        self.append(
            Uuid::new_v4(),
            recorded_at,
            SessionFactPayload::RouteEndpointProvenanceRecorded(provenance),
        )
    }

    pub(crate) fn record_compaction_endpoint_provenance(
        &mut self,
        recorded_at: &str,
        provenance: CompactionEndpointProvenanceRecorded,
    ) -> Result<bool> {
        self.append(
            Uuid::new_v4(),
            recorded_at,
            SessionFactPayload::CompactionEndpointProvenanceRecorded(provenance),
        )
    }

    pub(crate) fn start_step(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        start: StepStarted,
    ) -> Result<bool> {
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::StepStarted(start),
        )
    }

    pub(crate) fn materialize_context_source(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        source: ContextSourceMaterialized,
    ) -> Result<Uuid> {
        self.store
            .validate_content_ref(&source.content_ref, ProjectionClass::Default)?;
        let source_id = source.context_source_id;
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::ContextSourceMaterialized(source),
        )?;
        self.state
            .materialized_context_source_events
            .get(&source_id)
            .copied()
            .ok_or_else(|| {
                AuthorityError::Invalid("materialized context source was not reduced".into())
            })
    }

    pub(crate) fn start_compaction(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        start: CompactionStarted,
    ) -> Result<bool> {
        for item in start.input_items.iter().chain(&start.retained_items) {
            self.store
                .validate_content_ref(&item.content_ref, ProjectionClass::Default)?;
        }
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::CompactionStarted(start),
        )
    }

    pub(crate) fn prepare_compaction_request(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        preparation: CompactionRequestPrepared,
    ) -> Result<bool> {
        self.store.validate_content_ref(
            &preparation.prompt_template.content_ref,
            ProjectionClass::Default,
        )?;
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::CompactionRequestPrepared(preparation),
        )
    }

    pub(crate) fn fail_compaction_response_attempt(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        failure: CompactionResponseAttemptFailed,
    ) -> Result<bool> {
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::CompactionResponseAttemptFailed(failure),
        )
    }

    pub(crate) fn commit_compaction_summary(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        mut summary: CompactionSummaryCommitted,
    ) -> Result<bool> {
        self.store
            .validate_content_ref(&summary.summary_ref, ProjectionClass::Default)?;
        let event_id = Uuid::new_v4();
        let summary_item = summary.replacement_items.first_mut().ok_or_else(|| {
            AuthorityError::Invalid("compaction replacement has no summary item".into())
        })?;
        summary_item.source_event_id = event_id;
        let start = self
            .state
            .compaction_starts
            .get(&summary.compaction_id)
            .ok_or_else(|| AuthorityError::Invalid("compaction start is absent".into()))?;
        summary.replacement_manifest_id = compaction_replacement_manifest_id(&summary, start)?;
        let payload = SessionFactPayload::CompactionSummaryCommitted(summary);
        let sequence = self
            .state
            .last_sequence
            .checked_add(1)
            .ok_or_else(|| AuthorityError::Invalid("authority sequence overflow".into()))?;
        let mut fact = SessionFact::new(
            &self.session_id,
            self.stream_id,
            sequence,
            command_id,
            command_fingerprint(&payload)?,
            recorded_at,
            payload,
        );
        fact.event_id = event_id;
        fact.causation_event_id = self.state.last_event_id;
        self.store.append(&mut self.state, &fact)
    }

    pub(crate) fn close_compaction_request(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        closure: CompactionRequestClosed,
    ) -> Result<bool> {
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::CompactionRequestClosed(closure),
        )
    }

    pub(crate) fn apply_compaction(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        application: CompactionApplied,
    ) -> Result<bool> {
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::CompactionApplied(application),
        )
    }

    pub(crate) fn abandon_compaction(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        abandonment: CompactionAbandoned,
    ) -> Result<bool> {
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::CompactionAbandoned(abandonment),
        )
    }

    pub(crate) fn prepare_model_request(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        preparation: ModelRequestPrepared,
    ) -> Result<bool> {
        self.store.validate_request_content(&preparation)?;
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::ModelRequestPrepared(preparation),
        )
    }

    pub(crate) fn join_model_request_route(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        join: ModelRequestRouteJoined,
    ) -> Result<bool> {
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::ModelRequestRouteJoined(join),
        )
    }

    pub(crate) fn append_assistant_content(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        chunk: AssistantContentAppended,
    ) -> Result<bool> {
        let bytes = self
            .store
            .read_content(&chunk.content_ref, ProjectionClass::Default)?;
        if std::str::from_utf8(&bytes).is_err() {
            return Err(AuthorityError::Invalid(
                "assistant content must be valid UTF-8".into(),
            ));
        }
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::AssistantContentAppended(chunk),
        )
    }

    pub(crate) fn fail_model_response_attempt(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        failure: ModelResponseAttemptFailed,
    ) -> Result<bool> {
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::ModelResponseAttemptFailed(failure),
        )
    }

    pub(crate) fn store_provider_continuity(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        continuity: ProviderContinuityStored,
    ) -> Result<bool> {
        self.store.validate_content_ref(
            &continuity.content_ref,
            ProjectionClass::RestrictedContinuity,
        )?;
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::ProviderContinuityStored(continuity),
        )
    }

    pub(crate) fn commit_assistant_message(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        commit: AssistantMessageCommitted,
    ) -> Result<bool> {
        self.store.validate_message_content(&commit)?;
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::AssistantMessageCommitted(commit),
        )
    }

    pub(crate) fn record_tool_call(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        call: ToolCallRecorded,
    ) -> Result<bool> {
        self.store
            .validate_content_ref(&call.arguments_ref, ProjectionClass::Default)?;
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::ToolCallRecorded(call),
        )
    }

    pub(crate) fn record_tool_result(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        result: ToolResultRecorded,
    ) -> Result<bool> {
        self.store
            .validate_content_ref(&result.content_ref, ProjectionClass::Default)?;
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::ToolResultRecorded(result),
        )
    }

    pub(crate) fn close_model_request(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        closure: ModelRequestClosed,
    ) -> Result<bool> {
        if closure.outcome == ModelRequestOutcome::Abandoned {
            return Err(AuthorityError::Invalid(
                "live commands cannot abandon a model request".into(),
            ));
        }
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::ModelRequestClosed(closure),
        )
    }

    pub(crate) fn close_step(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        closure: StepClosed,
    ) -> Result<bool> {
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::StepClosed(closure),
        )
    }

    pub(crate) fn terminalize_active_semantic_step(
        &mut self,
        recorded_at: &str,
        terminalization: SemanticTerminalization,
    ) -> Result<bool> {
        validate_reason_code(&terminalization.reason_code)?;
        if terminalization.rule_version == 0 {
            return Err(AuthorityError::Invalid(
                "semantic terminalization rule version must be non-zero".into(),
            ));
        }
        if self.state.active_turn.as_ref().map(|turn| turn.turn_id) != Some(terminalization.turn_id)
        {
            return Err(AuthorityError::Invalid(
                "semantic terminalization targets a stale turn".into(),
            ));
        }

        let unresolved = self
            .state
            .invocations
            .values()
            .filter_map(|invocation| match invocation {
                InvocationState::Dispatched { preparation, .. }
                | InvocationState::Acknowledged { preparation, .. }
                    if preparation.turn_id == terminalization.turn_id =>
                {
                    Some(preparation.invocation_id)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut changed = false;
        for invocation_id in unresolved {
            changed |= self.classify_invocation_unknown(
                recorded_at,
                InvocationClassifiedUnknown {
                    invocation_id,
                    reason_code: terminalization.reason_code.clone(),
                    recovery_rule_version: 2,
                },
            )?;
        }

        let Some(active_step) = self.state.active_step.clone() else {
            return Ok(changed);
        };
        if active_step.start.turn_id != terminalization.turn_id {
            return Err(AuthorityError::Invalid(
                "active semantic step belongs to another turn".into(),
            ));
        }
        if let Some(request_id) = active_step.active_request_id {
            let outcome = if self.state.request_message_commits.contains_key(&request_id) {
                ModelRequestOutcome::Abandoned
            } else {
                terminalization.request_outcome
            };
            let closure = ModelRequestClosed {
                request_id,
                step_id: active_step.start.step_id,
                response_attempt_ordinal: latest_response_attempt(&self.state, request_id),
                outcome,
                reason_code: terminalization.reason_code.clone(),
                recovery_rule_version: (outcome == ModelRequestOutcome::Abandoned)
                    .then_some(terminalization.rule_version),
            };
            if outcome == ModelRequestOutcome::Abandoned {
                changed |= self.append_terminalization_fact(
                    recorded_at,
                    request_id,
                    &terminalization.reason_code,
                    terminalization.rule_version,
                    SessionFactPayload::ModelRequestClosed(closure),
                )?;
            } else {
                changed |= self.close_model_request(Uuid::new_v4(), recorded_at, closure)?;
            }
        }
        changed |= self.append_terminalization_fact(
            recorded_at,
            active_step.start.step_id,
            &terminalization.reason_code,
            terminalization.rule_version,
            SessionFactPayload::StepAbandoned(StepAbandoned {
                step_id: active_step.start.step_id,
                turn_id: terminalization.turn_id,
                reason_code: terminalization.reason_code.clone(),
                recovery_rule_version: terminalization.rule_version,
            }),
        )?;
        Ok(changed)
    }

    fn append_terminalization_fact(
        &mut self,
        recorded_at: &str,
        subject_id: Uuid,
        reason_code: &str,
        rule_version: u16,
        payload: SessionFactPayload,
    ) -> Result<bool> {
        let sequence = self
            .state
            .last_sequence
            .checked_add(1)
            .ok_or_else(|| AuthorityError::Invalid("authority sequence overflow".into()))?;
        let kind = payload.event_type();
        let fact = deterministic_terminal_fact(
            &self.session_id,
            self.stream_id,
            sequence,
            recorded_at,
            kind,
            subject_id,
            reason_code,
            rule_version,
            payload,
        )?;
        self.store.append(&mut self.state, &fact)
    }

    pub(crate) fn close_turn(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        closure: TurnClosed,
    ) -> Result<bool> {
        if self.state.invocations.values().any(|invocation| {
            invocation.turn_id() == closure.turn_id
                && matches!(
                    invocation,
                    InvocationState::Dispatched { .. } | InvocationState::Acknowledged { .. }
                )
        }) {
            return Err(AuthorityError::Invalid(
                "turn cannot close with an unresolved dispatched invocation".into(),
            ));
        }
        self.append(
            command_id,
            recorded_at,
            SessionFactPayload::TurnClosed(closure),
        )
    }

    pub(crate) fn migrate_execution_binding(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        from_generation: ExecutionBindingGeneration,
        target_generation: ExecutionBindingGeneration,
    ) -> Result<bool> {
        let payload = SessionFactPayload::ExecutionBindingMigrated(ExecutionBindingMigrated {
            from_generation: from_generation.clone(),
            target_generation: target_generation.clone(),
        });
        let fingerprint = command_fingerprint(&payload)?;
        if let Some(receipt) = self.state.command_receipts.get(&command_id) {
            if receipt.fingerprint == fingerprint {
                return Ok(false);
            }
            return Err(AuthorityError::Invalid(
                "command ID was reused with a conflicting event or fingerprint".into(),
            ));
        }
        let Some(current) = self.boot_execution_binding.as_ref() else {
            return Err(AuthorityError::Invalid(
                "session has no process-local execution binding".into(),
            ));
        };
        if current != &from_generation {
            return Err(AuthorityError::Invalid(
                "execution binding migration source is stale".into(),
            ));
        }
        if self.state.active_turn.is_some() {
            return Err(AuthorityError::Invalid(
                "execution binding cannot migrate during an active turn".into(),
            ));
        }
        if self
            .state
            .invocations
            .values()
            .any(invocation_blocks_migration)
        {
            return Err(AuthorityError::Invalid(
                "execution binding cannot migrate with an unresolved invocation".into(),
            ));
        }
        if from_generation == target_generation {
            return Err(AuthorityError::Invalid(
                "execution binding migration target is unchanged".into(),
            ));
        }
        let appended = self.append(command_id, recorded_at, payload)?;
        self.boot_execution_binding = Some(target_generation);
        Ok(appended)
    }

    fn execution_binding_migration_rejection(
        &self,
        from_generation: &ExecutionBindingGeneration,
        target_generation: &ExecutionBindingGeneration,
    ) -> Option<ExecutionBindingMigrationRejection> {
        let Some(current) = self.boot_execution_binding.as_ref() else {
            return Some(ExecutionBindingMigrationRejection::NoProcessLocalBinding);
        };
        if current != from_generation
            || self
                .state
                .execution_binding_generation
                .as_ref()
                .is_some_and(|durable| durable != from_generation)
        {
            return Some(ExecutionBindingMigrationRejection::StaleSource);
        }
        if self.state.active_turn.is_some() {
            return Some(ExecutionBindingMigrationRejection::ActiveTurn);
        }
        if self
            .state
            .invocations
            .values()
            .any(invocation_blocks_migration)
        {
            return Some(ExecutionBindingMigrationRejection::UnresolvedInvocation);
        }
        if from_generation == target_generation {
            return Some(ExecutionBindingMigrationRejection::UnchangedTarget);
        }
        None
    }

    pub(crate) fn prepare_invocation(
        &mut self,
        recorded_at: &str,
        preparation: InvocationPrepared,
    ) -> Result<bool> {
        self.append(
            invocation_phase_command_id(preparation.invocation_id, "prepared"),
            recorded_at,
            SessionFactPayload::InvocationPrepared(preparation),
        )
    }

    pub(crate) fn mark_invocation_dispatched(
        &mut self,
        recorded_at: &str,
        dispatch: InvocationDispatched,
    ) -> Result<bool> {
        self.append(
            invocation_phase_command_id(dispatch.invocation_id, "dispatched"),
            recorded_at,
            SessionFactPayload::InvocationDispatched(dispatch),
        )
    }

    pub(crate) fn acknowledge_invocation(
        &mut self,
        recorded_at: &str,
        acknowledgement: InvocationAcknowledged,
    ) -> Result<bool> {
        self.append(
            invocation_phase_command_id(acknowledgement.invocation_id, "acknowledged"),
            recorded_at,
            SessionFactPayload::InvocationAcknowledged(acknowledgement),
        )
    }

    pub(crate) fn settle_invocation(
        &mut self,
        recorded_at: &str,
        settlement: InvocationSettled,
    ) -> Result<bool> {
        self.append(
            invocation_phase_command_id(settlement.invocation_id, "settled"),
            recorded_at,
            SessionFactPayload::InvocationSettled(settlement),
        )
    }

    pub(crate) fn classify_invocation_unknown(
        &mut self,
        recorded_at: &str,
        classification: InvocationClassifiedUnknown,
    ) -> Result<bool> {
        self.append(
            invocation_phase_command_id(classification.invocation_id, "unknown"),
            recorded_at,
            SessionFactPayload::InvocationClassifiedUnknown(classification),
        )
    }

    fn append(
        &mut self,
        command_id: Uuid,
        recorded_at: &str,
        payload: SessionFactPayload,
    ) -> Result<bool> {
        let immediate = matches!(
            payload,
            SessionFactPayload::StepClosed(_)
                | SessionFactPayload::StepAbandoned(_)
                | SessionFactPayload::TurnClosed(_)
                | SessionFactPayload::CompactionApplied(_)
                | SessionFactPayload::CompactionAbandoned(_)
        );
        let appended = append_payload(
            &self.store,
            &mut self.state,
            &self.session_id,
            self.stream_id,
            command_id,
            recorded_at,
            payload,
        )?;
        if appended && let Some(wake) = &self.projection_wake {
            wake.hint(immediate);
        }
        Ok(appended)
    }
}

pub(crate) fn invocation_phase_command_id(invocation_id: Uuid, phase: &str) -> Uuid {
    Uuid::new_v5(
        &INVOCATION_COMMAND_NAMESPACE,
        format!("{invocation_id}:{phase}:1").as_bytes(),
    )
}

fn append_payload(
    store: &SessionAuthorityStore,
    state: &mut SessionAuthorityState,
    session_id: &str,
    stream_id: Uuid,
    command_id: Uuid,
    recorded_at: &str,
    payload: SessionFactPayload,
) -> Result<bool> {
    let sequence = state
        .last_sequence
        .checked_add(1)
        .ok_or_else(|| AuthorityError::Invalid("authority sequence overflow".into()))?;
    let fingerprint = command_fingerprint(&payload)?;
    let mut fact = SessionFact::new(
        session_id,
        stream_id,
        sequence,
        command_id,
        fingerprint,
        recorded_at,
        payload,
    );
    fact.causation_event_id = state.last_event_id;
    store.append(state, &fact)
}

fn command_fingerprint(payload: &SessionFactPayload) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"omegon-session-command-v1\0");
    hasher.update(payload.event_type().as_bytes());
    hasher.update(b"\0");
    hasher.update(serde_json::to_vec(&payload.to_value()?)?);
    Ok(format!("{:x}", hasher.finalize()))
}

fn validate_compaction_context_items(
    input: &[CompactionContextItem],
    retained: &[CompactionContextItem],
    receipts: &BTreeMap<Uuid, CommandReceipt>,
) -> Result<()> {
    let known_events = receipts
        .values()
        .map(|receipt| receipt.event_id)
        .collect::<BTreeSet<_>>();
    let mut identities = BTreeSet::new();
    for items in [input, retained] {
        for (expected, item) in items.iter().enumerate() {
            if usize::try_from(item.ordinal).ok() != Some(expected) {
                return Err(AuthorityError::Invalid(
                    "compaction context ordinals are not contiguous".into(),
                ));
            }
            if item.source_identity.trim().is_empty()
                || !known_events.contains(&item.source_event_id)
            {
                return Err(AuthorityError::Invalid(
                    "compaction context source identity or event is invalid".into(),
                ));
            }
            if item.content_ref.projection_class() != ProjectionClass::Default
                || item.content_ref.byte_length() == 0
            {
                return Err(AuthorityError::Invalid(
                    "compaction context content must be non-empty default content".into(),
                ));
            }
            if !identities.insert((item.source_event_id, item.source_identity.clone())) {
                return Err(AuthorityError::Invalid(
                    "compaction input and retained lists must be duplicate-free".into(),
                ));
            }
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct CompactionInputManifest<'a> {
    source_frontier: &'a AuthorityFrontierRef,
    source_context_revision: u64,
    owner_scope: &'a CompactionOwnerScope,
    input_items: &'a [CompactionContextItem],
    retained_items: &'a [CompactionContextItem],
}

pub(crate) fn compaction_input_manifest_id(start: &CompactionStarted) -> Result<String> {
    canonical_sha256(&CompactionInputManifest {
        source_frontier: &start.source_frontier,
        source_context_revision: start.source_context_revision,
        owner_scope: &start.owner_scope,
        input_items: &start.input_items,
        retained_items: &start.retained_items,
    })
}

fn validate_replacement_items(
    summary: &CompactionSummaryCommitted,
    start: &CompactionStarted,
    summary_event_id: Uuid,
) -> Result<()> {
    if summary.replacement_items.len() != start.retained_items.len() + 1 {
        return Err(AuthorityError::Invalid(
            "compaction replacement must contain one summary and every retained item".into(),
        ));
    }
    for (expected, item) in summary.replacement_items.iter().enumerate() {
        if usize::try_from(item.ordinal).ok() != Some(expected) {
            return Err(AuthorityError::Invalid(
                "compaction replacement ordinals are not contiguous".into(),
            ));
        }
        if expected == 0 {
            if item.source_kind != CompactionReplacementSourceKind::CompactionSummary
                || item.source_event_id != summary_event_id
                || item.source_identity != summary.compaction_summary_id.to_string()
                || item.content_ref != summary.summary_ref
            {
                return Err(AuthorityError::Invalid(
                    "compaction replacement summary item is invalid".into(),
                ));
            }
            continue;
        }
        let retained = &start.retained_items[expected - 1];
        if item.source_kind != CompactionReplacementSourceKind::Retained
            || item.source_event_id != retained.source_event_id
            || item.source_identity != retained.source_identity
            || item.content_ref != retained.content_ref
        {
            return Err(AuthorityError::Invalid(
                "compaction replacement retained item changed".into(),
            ));
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct CompactionReplacementManifest<'a> {
    compaction_id: Uuid,
    target_context_revision: u64,
    replacement_items: &'a [CompactionReplacementItem],
}

pub(crate) fn compaction_replacement_manifest_id(
    summary: &CompactionSummaryCommitted,
    start: &CompactionStarted,
) -> Result<String> {
    canonical_sha256(&CompactionReplacementManifest {
        compaction_id: summary.compaction_id,
        target_context_revision: start.target_context_revision,
        replacement_items: &summary.replacement_items,
    })
}

fn read_strict_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path)?;
    let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
    let value = T::deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(value)
}

#[derive(Debug, Clone)]
pub(crate) struct SessionAuthorityStore {
    log_path: PathBuf,
    snapshot_path: PathBuf,
    attachment_dir: PathBuf,
    blob_store: crate::session_blob_store::SessionBlobStore,
    emergency_fence_dir: PathBuf,
}

impl SessionAuthorityStore {
    pub(crate) fn adjacent_to(session_snapshot: &Path) -> Result<Self> {
        let stem = session_snapshot
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| AuthorityError::Invalid("session snapshot has no UTF-8 stem".into()))?;
        let parent = session_snapshot
            .parent()
            .ok_or_else(|| AuthorityError::Invalid("session snapshot has no parent".into()))?;
        Ok(Self {
            log_path: parent.join(format!("{stem}.authority.jsonl")),
            snapshot_path: parent.join(format!("{stem}.authority.snapshot.json")),
            attachment_dir: parent.join(format!("{stem}.authority.attachments")),
            blob_store: crate::session_blob_store::SessionBlobStore::at(
                parent.join(format!("{stem}.authority.blobs")),
            ),
            emergency_fence_dir: parent.join("invocation-mutation-fences"),
        })
    }

    fn writer_lease_path(&self) -> PathBuf {
        let mut path = self.log_path.as_os_str().to_os_string();
        path.push(".writer");
        PathBuf::from(path)
    }

    #[cfg(test)]
    fn from_paths(log_path: PathBuf, snapshot_path: PathBuf) -> Self {
        Self {
            attachment_dir: log_path.with_extension("attachments"),
            blob_store: crate::session_blob_store::SessionBlobStore::at(
                log_path.with_extension("blobs"),
            ),
            emergency_fence_dir: log_path
                .parent()
                .expect("test authority log has a parent")
                .join("invocation-mutation-fences"),
            log_path,
            snapshot_path,
        }
    }

    fn open_blob_store(&self) -> Result<()> {
        self.blob_store.open()?;
        Ok(())
    }

    fn write_content(
        &self,
        bytes: &[u8],
        media_type: &str,
        projection_class: ProjectionClass,
    ) -> Result<ContentRef> {
        Ok(self.blob_store.write(bytes, media_type, projection_class)?)
    }

    pub(crate) fn read_content(
        &self,
        content_ref: &ContentRef,
        required_projection: ProjectionClass,
    ) -> Result<Vec<u8>> {
        Ok(self.blob_store.read(content_ref, required_projection)?)
    }

    fn validate_content_ref(
        &self,
        content_ref: &ContentRef,
        required_projection: ProjectionClass,
    ) -> Result<()> {
        self.blob_store.validate(content_ref, required_projection)?;
        Ok(())
    }

    fn validate_request_content(&self, preparation: &ModelRequestPrepared) -> Result<()> {
        for item in &preparation.context_items {
            self.validate_content_ref(&item.content_ref, ProjectionClass::Default)?;
        }
        for schema in &preparation.schema_set.schemas {
            self.validate_content_ref(&schema.schema_content_ref, ProjectionClass::Default)?;
        }
        Ok(())
    }

    fn validate_message_content(&self, commit: &AssistantMessageCommitted) -> Result<()> {
        for manifest in &commit.content {
            let mut hasher = Sha256::new();
            for content_ref in &manifest.chunk_refs {
                let bytes = self.read_content(content_ref, ProjectionClass::Default)?;
                if std::str::from_utf8(&bytes).is_err() {
                    return Err(AuthorityError::Invalid(
                        "assistant content must be valid UTF-8".into(),
                    ));
                }
                hasher.update(bytes);
            }
            if manifest.content_digest != format!("{:x}", hasher.finalize()) {
                return Err(AuthorityError::Invalid(
                    "assistant content digest does not match stored chunk bytes".into(),
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn validate_state_content(&self, state: &SessionAuthorityState) -> Result<()> {
        self.validate_state_content_with(state, &mut |reference, projection| {
            self.read_content(reference, projection)
        })
    }

    pub(crate) fn validate_state_content_with(
        &self,
        state: &SessionAuthorityState,
        read: &mut dyn FnMut(&ContentRef, ProjectionClass) -> Result<Vec<u8>>,
    ) -> Result<()> {
        for source in state.materialized_context_sources.values() {
            read(&source.content_ref, ProjectionClass::Default)?;
        }
        for request in state.model_requests.values() {
            for item in &request.preparation().context_items {
                read(&item.content_ref, ProjectionClass::Default)?;
            }
            for schema in &request.preparation().schema_set.schemas {
                read(&schema.schema_content_ref, ProjectionClass::Default)?;
            }
        }
        for chunks in state.assistant_chunks.values() {
            for chunk in chunks {
                let bytes = read(&chunk.content_ref, ProjectionClass::Default)?;
                if std::str::from_utf8(&bytes).is_err() {
                    return Err(AuthorityError::Invalid(
                        "assistant content must be valid UTF-8".into(),
                    ));
                }
            }
        }
        for commit in state.assistant_messages.values() {
            for manifest in &commit.content {
                let mut hasher = Sha256::new();
                for reference in &manifest.chunk_refs {
                    let bytes = read(reference, ProjectionClass::Default)?;
                    if std::str::from_utf8(&bytes).is_err() {
                        return Err(AuthorityError::Invalid(
                            "assistant content must be valid UTF-8".into(),
                        ));
                    }
                    hasher.update(bytes);
                }
                if manifest.content_digest != format!("{:x}", hasher.finalize()) {
                    return Err(AuthorityError::Invalid(
                        "assistant content digest does not match stored chunk bytes".into(),
                    ));
                }
            }
        }
        for continuity in state.provider_continuity.values() {
            read(
                &continuity.content_ref,
                ProjectionClass::RestrictedContinuity,
            )?;
        }
        for call in state.tool_calls.values() {
            read(&call.arguments_ref, ProjectionClass::Default)?;
        }
        for result in state.tool_results.values() {
            read(&result.content_ref, ProjectionClass::Default)?;
        }
        for start in state.compaction_starts.values() {
            for item in start.input_items.iter().chain(&start.retained_items) {
                read(&item.content_ref, ProjectionClass::Default)?;
            }
        }
        for request in state.compaction_requests.values() {
            read(
                &request.preparation().prompt_template.content_ref,
                ProjectionClass::Default,
            )?;
        }
        for summary in state.compaction_summaries.values() {
            let bytes = read(&summary.summary_ref, ProjectionClass::Default)?;
            if bytes.is_empty()
                || std::str::from_utf8(&bytes).is_err()
                || format!("{:x}", Sha256::digest(&bytes)) != summary.summary_digest
            {
                return Err(AuthorityError::Invalid(
                    "compaction summary blob or digest is invalid".into(),
                ));
            }
            for item in &summary.replacement_items {
                read(&item.content_ref, ProjectionClass::Default)?;
            }
        }
        Ok(())
    }

    fn ensure_emergency_fence_dir(&self) -> Result<()> {
        if !self.emergency_fence_dir.exists() {
            fs::create_dir_all(&self.emergency_fence_dir)?;
            sync_parent(&self.emergency_fence_dir)?;
        }
        if !self.emergency_fence_dir.is_dir() {
            return Err(AuthorityError::Invalid(
                "invocation mutation fence path is not a directory".into(),
            ));
        }
        Ok(())
    }

    fn record_mutation_fence(&self, evidence: &InvocationMutationFenceEvidence) -> Result<()> {
        self.ensure_emergency_fence_dir()?;
        evidence.validate()?;
        let path = self
            .emergency_fence_dir
            .join(format!("{}.json", evidence.fence_id));
        let encoded = serde_json::to_vec(evidence)?;
        if encoded.len() > MAX_RECORD_BYTES {
            return Err(AuthorityError::Invalid(
                "invocation mutation fence exceeds 1 MiB".into(),
            ));
        }
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(&encoded)?;
                file.write_all(b"\n")?;
                file.sync_all()?;
                File::open(&self.emergency_fence_dir)?.sync_all()?;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = read_strict_json::<InvocationMutationFenceEvidence>(&path)?;
                existing.validate()?;
                if existing == *evidence {
                    Ok(())
                } else {
                    Err(AuthorityError::Invalid(
                        "invocation mutation fence identity collision".into(),
                    ))
                }
            }
            Err(error) => Err(error.into()),
        }
    }

    fn active_mutation_fence(
        &self,
        domain: &RuntimeMutationDomainId,
        key: &RuntimeMutationFenceKey,
    ) -> Result<Option<InvocationMutationFenceEvidence>> {
        self.ensure_emergency_fence_dir()?;
        let mut paths = fs::read_dir(&self.emergency_fence_dir)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();
        let mut matching = None;
        for path in paths {
            let metadata = fs::symlink_metadata(&path)?;
            if !metadata.file_type().is_file()
                || path.extension().and_then(|value| value.to_str()) != Some("json")
            {
                return Err(AuthorityError::Invalid(
                    "invocation mutation fence directory contains an invalid entry".into(),
                ));
            }
            if metadata.len() > MAX_RECORD_BYTES as u64 {
                return Err(AuthorityError::Invalid(
                    "invocation mutation fence exceeds 1 MiB".into(),
                ));
            }
            let evidence = read_strict_json::<InvocationMutationFenceEvidence>(&path)?;
            evidence.validate()?;
            if &evidence.mutation_domain == domain && &evidence.fence_key == key {
                matching = Some(evidence);
            }
        }
        Ok(matching)
    }

    pub(crate) fn load(&self) -> Result<SessionAuthorityState> {
        let facts = read_facts(&self.log_path)?;
        if facts.is_empty() {
            return Ok(SessionAuthorityState::default());
        }

        if let Ok(snapshot) = self.read_snapshot()
            && let Some(prefix_end) = facts.iter().position(|fact| {
                fact.sequence == snapshot.last_sequence && fact.event_id == snapshot.last_event_id
            })
            && let Ok(prefix_state) = reconstruct(&facts[..=prefix_end])
            && prefix_state == snapshot.state
        {
            self.validate_state_content(&snapshot.state)?;
            let mut state = snapshot.state;
            for fact in &facts[prefix_end + 1..] {
                state.apply(fact)?;
            }
            self.validate_state_content(&state)?;
            return Ok(state);
        }

        let state = reconstruct(&facts)?;
        self.validate_state_content(&state)?;
        Ok(state)
    }

    pub(crate) fn read_stable_facts(&self) -> Result<Vec<SessionFact>> {
        read_facts_stable(&self.log_path)
    }

    pub(crate) fn read_bounded_facts(
        &self,
        max_bytes: usize,
        max_records: usize,
        max_record_bytes: usize,
        check: &mut dyn FnMut(usize) -> crate::session_blob_store::Result<()>,
    ) -> Result<Vec<SessionFact>> {
        let bytes = crate::session_blob_store::read_bounded_file(&self.log_path, max_bytes, check)?;
        if bytes.is_empty() || !bytes.ends_with(b"\n") {
            return Err(AuthorityError::Invalid(
                "empty or truncated authority stream".into(),
            ));
        }
        let mut facts = Vec::new();
        for line in bytes[..bytes.len() - 1].split(|byte| *byte == b'\n') {
            check(0)?;
            if facts.len() >= max_records
                || line.is_empty()
                || line.len() > max_record_bytes.min(MAX_RECORD_BYTES)
            {
                return Err(AuthorityError::Invalid(
                    "authority replay record limit exceeded".into(),
                ));
            }
            facts.push(SessionFact::decode(line)?);
        }
        check(0)?;
        Ok(facts)
    }

    pub(crate) fn read_bounded_content(
        &self,
        reference: &ContentRef,
        projection: ProjectionClass,
        max_file_bytes: usize,
        check: &mut dyn FnMut(usize) -> crate::session_blob_store::Result<()>,
    ) -> Result<Vec<u8>> {
        Ok(self
            .blob_store
            .read_bounded(reference, projection, max_file_bytes, check)?)
    }

    pub(crate) fn validate_bounded_attachment(
        &self,
        attachment: &AttachmentRef,
        max_file_bytes: usize,
        check: &mut dyn FnMut(usize) -> crate::session_blob_store::Result<()>,
    ) -> Result<()> {
        let path = PathBuf::from(&attachment.storage_ref);
        if path.parent() != Some(self.attachment_dir.as_path())
            || path.file_name().and_then(|name| name.to_str()) != Some(attachment.digest.as_str())
        {
            return Err(AuthorityError::Invalid(
                "authority attachment is outside content-addressed storage".into(),
            ));
        }
        let bytes = crate::session_blob_store::read_bounded_file(&path, max_file_bytes, check)?;
        let mut digest = Sha256::new();
        for chunk in bytes.chunks(64 * 1024) {
            check(0)?;
            digest.update(chunk);
        }
        if bytes.len() as u64 != attachment.byte_length
            || format!("{:x}", digest.finalize()) != attachment.digest
        {
            return Err(AuthorityError::Invalid(
                "authority attachment content changed".into(),
            ));
        }
        check(0)?;
        Ok(())
    }

    pub(crate) fn append(
        &self,
        state: &mut SessionAuthorityState,
        fact: &SessionFact,
    ) -> Result<bool> {
        let _guard = crate::filelock::acquire_lock(&self.log_path)
            .map_err(|error| AuthorityError::Invalid(error.to_string()))?;
        let durable = reconstruct(&read_facts(&self.log_path)?)?;
        self.validate_state_content(&durable)?;
        if let Some(receipt) = durable.command_receipts.get(&fact.command_id) {
            if receipt.fingerprint == fact.command_fingerprint {
                *state = durable;
                return Ok(false);
            }
            return Err(AuthorityError::Invalid(
                "command ID was reused with a conflicting event or fingerprint".into(),
            ));
        }
        if state.last_sequence != durable.last_sequence
            || state.last_event_id != durable.last_event_id
        {
            return Err(AuthorityError::Invalid(
                "in-memory authority cursor is stale".into(),
            ));
        }

        let mut next = durable;
        next.apply(fact)?;
        self.validate_state_content(&next)?;
        let mut encoded = fact.encode()?;
        if encoded.len() > MAX_RECORD_BYTES {
            return Err(AuthorityError::Invalid(
                "authority record exceeds 1 MiB".into(),
            ));
        }
        encoded.push(b'\n');

        self.append_record(&encoded)?;
        *state = next;
        if let Err(error) = SessionAuthoritySnapshot::from_state(state)
            .and_then(|snapshot| write_snapshot(&self.snapshot_path, &snapshot))
        {
            tracing::warn!(%error, "session authority snapshot cache update failed after durable append");
        }
        Ok(true)
    }

    pub(crate) fn recover(&self, recorded_at: &str) -> Result<SessionAuthorityState> {
        let _guard = crate::filelock::acquire_lock(&self.log_path)
            .map_err(|error| AuthorityError::Invalid(error.to_string()))?;
        let mut state = reconstruct(&read_facts(&self.log_path)?)?;
        self.validate_state_content(&state)?;
        for fact in recovery_facts(&state, recorded_at)? {
            let mut next = state.clone();
            next.apply(&fact)?;
            let mut encoded = fact.encode()?;
            if encoded.len() > MAX_RECORD_BYTES {
                return Err(AuthorityError::Invalid(
                    "authority record exceeds 1 MiB".into(),
                ));
            }
            encoded.push(b'\n');
            self.append_record(&encoded)?;
            state = next;
        }
        if state.last_sequence > 0
            && let Err(error) = SessionAuthoritySnapshot::from_state(&state)
                .and_then(|snapshot| write_snapshot(&self.snapshot_path, &snapshot))
        {
            tracing::warn!(%error, "session authority recovery snapshot cache update failed");
        }
        Ok(state)
    }

    fn stage_attachment(&self, source: &Path) -> Result<AttachmentRef> {
        let mut source_file = File::open(source)?;
        let metadata = source_file.metadata()?;
        if !metadata.is_file() {
            return Err(AuthorityError::Invalid(
                "prompt attachment must be a regular file".into(),
            ));
        }
        if metadata.len() > MAX_ATTACHMENT_BYTES {
            return Err(AuthorityError::Invalid(format!(
                "prompt attachment exceeds {} MiB",
                MAX_ATTACHMENT_BYTES / (1024 * 1024)
            )));
        }

        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        std::io::Read::read_to_end(&mut source_file, &mut bytes)?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        fs::create_dir_all(&self.attachment_dir)?;
        let stored = self.attachment_dir.join(&digest);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stored)
        {
            Ok(mut file) => {
                file.write_all(&bytes)?;
                file.flush()?;
                file.sync_all()?;
                sync_parent(&stored)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if fs::read(&stored)? != bytes {
                    return Err(AuthorityError::Invalid(
                        "stored attachment digest collision".into(),
                    ));
                }
            }
            Err(error) => return Err(error.into()),
        }

        Ok(AttachmentRef {
            digest,
            media_type: attachment_media_type(source).to_string(),
            byte_length: metadata.len(),
            storage_ref: stored.to_string_lossy().into_owned(),
        })
    }

    pub(crate) fn validate_attachment(&self, attachment: &AttachmentRef) -> Result<PathBuf> {
        let path = PathBuf::from(&attachment.storage_ref);
        if path.parent() != Some(self.attachment_dir.as_path())
            || path.file_name().and_then(|name| name.to_str()) != Some(attachment.digest.as_str())
        {
            return Err(AuthorityError::Invalid(
                "authority attachment is outside content-addressed storage".into(),
            ));
        }
        let metadata = fs::metadata(&path)?;
        if !metadata.is_file() || metadata.len() != attachment.byte_length {
            return Err(AuthorityError::Invalid(
                "authority attachment size or type changed".into(),
            ));
        }
        let bytes = fs::read(&path)?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        if digest != attachment.digest {
            return Err(AuthorityError::Invalid(
                "authority attachment digest changed".into(),
            ));
        }
        Ok(path)
    }

    fn append_record(&self, encoded: &[u8]) -> Result<()> {
        if let Some(parent) = self.log_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let created = !self.log_path.exists();
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&self.log_path)?;
        file.write_all(encoded)?;
        file.flush()?;
        file.sync_all()?;
        if created {
            sync_parent(&self.log_path)?;
        }
        Ok(())
    }

    fn read_snapshot(&self) -> Result<SessionAuthoritySnapshot> {
        let bytes = fs::read(&self.snapshot_path)?;
        let snapshot: SessionAuthoritySnapshot = serde_json::from_slice(&bytes)?;
        snapshot.validate()?;
        Ok(snapshot)
    }
}

fn attachment_media_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    }
}

fn read_facts(path: &Path) -> Result<Vec<SessionFact>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    decode_facts(&bytes)
}

fn read_facts_stable(path: &Path) -> Result<Vec<SessionFact>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let before = file.metadata()?;
    if before.len() > usize::MAX as u64 {
        return Err(AuthorityError::Invalid(
            "authority stream is too large for this reader".into(),
        ));
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    Read::take(&mut file, before.len()).read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    if after.len() != before.len() || bytes.len() as u64 != before.len() {
        return Err(AuthorityError::Invalid(
            "authority stream moved while replay was reading it".into(),
        ));
    }
    decode_facts(&bytes)
}

fn decode_facts(bytes: &[u8]) -> Result<Vec<SessionFact>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if !bytes.ends_with(b"\n") {
        return Err(AuthorityError::Invalid(
            "authority stream has a truncated final record".into(),
        ));
    }
    let mut facts = Vec::new();
    for (index, line) in bytes[..bytes.len() - 1]
        .split(|byte| *byte == b'\n')
        .enumerate()
    {
        if line.is_empty() {
            return Err(AuthorityError::Invalid(format!(
                "authority stream contains blank line {}",
                index + 1
            )));
        }
        if line.len() > MAX_RECORD_BYTES {
            return Err(AuthorityError::Invalid(format!(
                "authority record {} exceeds 1 MiB",
                index + 1
            )));
        }
        facts.push(SessionFact::decode(line)?);
    }
    Ok(facts)
}

fn write_snapshot(path: &Path, snapshot: &SessionAuthoritySnapshot) -> Result<()> {
    let bytes = serde_json::to_vec(snapshot)?;
    let parent = path
        .parent()
        .ok_or_else(|| AuthorityError::Invalid("snapshot path has no parent".into()))?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("authority"),
        std::process::id()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    file.write_all(&bytes)?;
    file.flush()?;
    file.sync_all()?;
    fs::rename(&tmp, path)?;
    sync_parent(path)
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> Result<()> {
    Ok(())
}

fn latest_response_attempt(state: &SessionAuthorityState, request_id: Uuid) -> u32 {
    state
        .response_attempt_failures
        .get(&request_id)
        .into_iter()
        .flat_map(|failures| failures.keys().copied())
        .chain(
            state
                .assistant_chunks
                .get(&request_id)
                .into_iter()
                .flatten()
                .map(|chunk| chunk.response_attempt_ordinal),
        )
        .chain(
            state
                .provider_continuity
                .values()
                .filter(|continuity| continuity.request_id == request_id)
                .map(|continuity| continuity.response_attempt_ordinal),
        )
        .chain(
            state
                .request_message_commits
                .get(&request_id)
                .and_then(|message_id| state.assistant_messages.get(message_id))
                .map(|commit| commit.response_attempt_ordinal),
        )
        .max()
        .unwrap_or(0)
}

pub(crate) fn recovery_facts(
    state: &SessionAuthorityState,
    recorded_at: &str,
) -> Result<Vec<SessionFact>> {
    let mut working = state.clone();
    let mut facts = compaction_recovery_facts(&working, recorded_at)?;
    for fact in &facts {
        working.apply(fact)?;
    }
    facts.extend(recovery_facts_after_compaction(&working, recorded_at)?);
    Ok(facts)
}

fn compaction_recovery_facts(
    state: &SessionAuthorityState,
    recorded_at: &str,
) -> Result<Vec<SessionFact>> {
    let Some(compaction_id) = state.active_compaction else {
        return Ok(Vec::new());
    };
    let session_id = state
        .session_id
        .as_deref()
        .ok_or_else(|| AuthorityError::Invalid("recoverable compaction has no session".into()))?;
    let stream_id = state
        .stream_id
        .ok_or_else(|| AuthorityError::Invalid("recoverable compaction has no stream".into()))?;
    let mut working = state.clone();
    let mut facts = Vec::new();
    let latest_request = working
        .compaction_requests
        .values()
        .filter(|request| request.preparation().compaction_id == compaction_id)
        .max_by_key(|request| request.preparation().request_ordinal)
        .cloned();
    if let Some(summary_id) = working
        .compaction_summary_by_operation
        .get(&compaction_id)
        .copied()
    {
        let summary = working.compaction_summaries[&summary_id].clone();
        if matches!(
            working
                .compaction_requests
                .get(&summary.compaction_request_id),
            Some(CompactionRequestState::Open { .. })
        ) {
            let fact = recovery_fact(
                session_id,
                stream_id,
                working.last_sequence + 1,
                recorded_at,
                "compaction.request_closed",
                summary.compaction_request_id,
                SessionFactPayload::CompactionRequestClosed(CompactionRequestClosed {
                    compaction_request_id: summary.compaction_request_id,
                    compaction_id,
                    response_attempt_ordinal: summary.response_attempt_ordinal,
                    outcome: CompactionRequestOutcome::SummaryCommitted,
                    reason_code: "recovered_committed_summary".into(),
                    recovery_rule_version: Some(1),
                }),
            )?;
            working.apply(&fact)?;
            facts.push(fact);
        }
        let start = &working.compaction_starts[&compaction_id];
        let fact = recovery_fact(
            session_id,
            stream_id,
            working.last_sequence + 1,
            recorded_at,
            "compaction.applied",
            compaction_id,
            SessionFactPayload::CompactionApplied(CompactionApplied {
                compaction_id,
                compaction_summary_id: summary_id,
                source_context_revision: start.source_context_revision,
                target_context_revision: start.target_context_revision,
                replacement_manifest_id: summary.replacement_manifest_id,
                recovery_rule_version: Some(1),
            }),
        )?;
        facts.push(fact);
        return Ok(facts);
    }

    if let Some(CompactionRequestState::Open { preparation }) = latest_request.as_ref() {
        let attempt = working
            .compaction_attempt_failures
            .get(&preparation.compaction_request_id)
            .map_or(0, BTreeMap::len) as u32;
        let fact = recovery_fact(
            session_id,
            stream_id,
            working.last_sequence + 1,
            recorded_at,
            "compaction.request_closed",
            preparation.compaction_request_id,
            SessionFactPayload::CompactionRequestClosed(CompactionRequestClosed {
                compaction_request_id: preparation.compaction_request_id,
                compaction_id,
                response_attempt_ordinal: attempt,
                outcome: CompactionRequestOutcome::Abandoned,
                reason_code: "runtime_lost".into(),
                recovery_rule_version: Some(1),
            }),
        )?;
        working.apply(&fact)?;
        facts.push(fact);
    }
    let (last_request_id, last_attempt) = latest_request.map_or((None, None), |request| {
        let request_id = request.preparation().compaction_request_id;
        (
            Some(request_id),
            Some(
                working
                    .compaction_attempt_failures
                    .get(&request_id)
                    .map_or(0, BTreeMap::len) as u32,
            ),
        )
    });
    facts.push(recovery_fact(
        session_id,
        stream_id,
        working.last_sequence + 1,
        recorded_at,
        "compaction.abandoned",
        compaction_id,
        SessionFactPayload::CompactionAbandoned(CompactionAbandoned {
            compaction_id,
            reason_code: "runtime_lost".into(),
            last_compaction_request_id: last_request_id,
            last_response_attempt_ordinal: last_attempt,
            recovery_rule_version: 1,
        }),
    )?);
    Ok(facts)
}

fn recovery_facts_after_compaction(
    state: &SessionAuthorityState,
    recorded_at: &str,
) -> Result<Vec<SessionFact>> {
    let has_unsettled_durable = state.invocations.values().any(|invocation| {
        matches!(
            invocation,
            InvocationState::Dispatched { .. } | InvocationState::Acknowledged { .. }
        )
    });
    if state.active_turn.is_none() && !has_unsettled_durable {
        return Ok(Vec::new());
    }
    let session_id = state
        .session_id
        .as_ref()
        .ok_or_else(|| AuthorityError::Invalid("recoverable invocation has no session".into()))?;
    let stream_id = state
        .stream_id
        .ok_or_else(|| AuthorityError::Invalid("recoverable invocation has no stream".into()))?;
    let mut sequence = state.last_sequence;
    let mut facts = Vec::new();
    let classified_durable = state
        .invocations
        .values()
        .any(|invocation| match invocation {
            InvocationState::Dispatched { .. } | InvocationState::Acknowledged { .. } => true,
            InvocationState::DurableUnknown { classification, .. } => {
                classification.reason_code == "runtime_lost"
                    && classification.recovery_rule_version == 2
            }
            _ => false,
        });
    for invocation in state.invocations.values() {
        let (invocation_id, recovery_rule_version) = match invocation {
            InvocationState::Dispatched { preparation, .. }
            | InvocationState::Acknowledged { preparation, .. } => (preparation.invocation_id, 2),
            InvocationState::Registered { registration }
                if state
                    .active_turn
                    .as_ref()
                    .is_some_and(|active| active.turn_id == registration.turn_id) =>
            {
                (registration.invocation_id, 1)
            }
            _ => continue,
        };
        sequence += 1;
        facts.push(recovery_fact(
            session_id,
            stream_id,
            sequence,
            recorded_at,
            "invocation.classified_unknown",
            invocation_id,
            SessionFactPayload::InvocationClassifiedUnknown(InvocationClassifiedUnknown {
                invocation_id,
                reason_code: "runtime_lost".into(),
                recovery_rule_version,
            }),
        )?);
    }
    if let Some(active_step) = state.active_step.as_ref() {
        if let Some(request_id) = active_step.active_request_id {
            sequence += 1;
            facts.push(recovery_fact(
                session_id,
                stream_id,
                sequence,
                recorded_at,
                "model.request_closed",
                request_id,
                SessionFactPayload::ModelRequestClosed(ModelRequestClosed {
                    request_id,
                    step_id: active_step.start.step_id,
                    response_attempt_ordinal: latest_response_attempt(state, request_id),
                    outcome: ModelRequestOutcome::Abandoned,
                    reason_code: "runtime_lost".into(),
                    recovery_rule_version: Some(1),
                }),
            )?);
        }
        sequence += 1;
        facts.push(recovery_fact(
            session_id,
            stream_id,
            sequence,
            recorded_at,
            "step.abandoned",
            active_step.start.step_id,
            SessionFactPayload::StepAbandoned(StepAbandoned {
                step_id: active_step.start.step_id,
                turn_id: active_step.start.turn_id,
                reason_code: "runtime_lost".into(),
                recovery_rule_version: 1,
            }),
        )?);
    }
    if let Some(active) = state.active_turn.as_ref() {
        sequence += 1;
        facts.push(recovery_fact(
            session_id,
            stream_id,
            sequence,
            recorded_at,
            "turn.closed",
            active.turn_id,
            SessionFactPayload::TurnClosed(TurnClosed {
                turn_id: active.turn_id,
                outcome: TurnOutcome::Interrupted,
                reason_code: "runtime_lost".into(),
                recovery_rule_version: Some(if classified_durable { 2 } else { 1 }),
            }),
        )?);
    }
    Ok(facts)
}

fn recovery_fact(
    session_id: &str,
    stream_id: Uuid,
    sequence: u64,
    recorded_at: &str,
    kind: &str,
    subject_id: Uuid,
    payload: SessionFactPayload,
) -> Result<SessionFact> {
    let terminal_identity = match &payload {
        SessionFactPayload::ModelRequestClosed(closure) => Some((
            closure.reason_code.clone(),
            closure.recovery_rule_version.unwrap_or(1),
        )),
        SessionFactPayload::StepAbandoned(abandonment) => Some((
            abandonment.reason_code.clone(),
            abandonment.recovery_rule_version,
        )),
        SessionFactPayload::CompactionRequestClosed(closure) => Some((
            closure.reason_code.clone(),
            closure.recovery_rule_version.unwrap_or(1),
        )),
        SessionFactPayload::CompactionApplied(application) => Some((
            "committed_summary".into(),
            application.recovery_rule_version.unwrap_or(1),
        )),
        SessionFactPayload::CompactionAbandoned(abandonment) => Some((
            abandonment.reason_code.clone(),
            abandonment.recovery_rule_version,
        )),
        _ => None,
    };
    if let Some((reason_code, rule_version)) = terminal_identity {
        return deterministic_terminal_fact(
            session_id,
            stream_id,
            sequence,
            recorded_at,
            kind,
            subject_id,
            &reason_code,
            rule_version,
            payload,
        );
    }
    let identity = format!("{stream_id}:{subject_id}:{kind}:1");
    let event_id = Uuid::new_v5(&RECOVERY_NAMESPACE, identity.as_bytes());
    let command_id = Uuid::new_v5(
        &RECOVERY_NAMESPACE,
        format!("command:{identity}").as_bytes(),
    );
    let fingerprint = recovery_fingerprint(&identity);
    let mut fact = SessionFact::new(
        session_id,
        stream_id,
        sequence,
        command_id,
        fingerprint,
        recorded_at,
        payload,
    );
    fact.event_id = event_id;
    fact.validate_envelope()?;
    Ok(fact)
}

#[allow(clippy::too_many_arguments)]
fn deterministic_terminal_fact(
    session_id: &str,
    stream_id: Uuid,
    sequence: u64,
    recorded_at: &str,
    kind: &str,
    subject_id: Uuid,
    reason_code: &str,
    rule_version: u16,
    payload: SessionFactPayload,
) -> Result<SessionFact> {
    let identity = format!("{stream_id}:{subject_id}:{kind}:{reason_code}:{rule_version}");
    let event_id = Uuid::new_v5(&RECOVERY_NAMESPACE, identity.as_bytes());
    let command_id = Uuid::new_v5(
        &RECOVERY_NAMESPACE,
        format!("command:{identity}").as_bytes(),
    );
    let fingerprint = recovery_fingerprint(&identity);
    let mut fact = SessionFact::new(
        session_id,
        stream_id,
        sequence,
        command_id,
        fingerprint,
        recorded_at,
        payload,
    );
    fact.event_id = event_id;
    fact.validate_envelope()?;
    Ok(fact)
}

fn recovery_fingerprint(identity: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"omegon-session-recovery-v1\0");
    hasher.update(identity.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-08-19T18:00:00Z";

    fn fingerprint(label: &str) -> String {
        format!("{:x}", Sha256::digest(label.as_bytes()))
    }

    fn fact(
        session_id: &str,
        stream_id: Uuid,
        sequence: u64,
        payload: SessionFactPayload,
    ) -> SessionFact {
        SessionFact::new(
            session_id,
            stream_id,
            sequence,
            Uuid::new_v4(),
            fingerprint(&format!("command-{sequence}")),
            NOW,
            payload,
        )
    }

    fn created(session_id: &str, stream_id: Uuid) -> SessionFact {
        fact(
            session_id,
            stream_id,
            1,
            SessionFactPayload::SessionCreated(SessionCreated {
                workspace_identity: "workspace-key".into(),
                created_by: ActorIdentity {
                    principal: "operator".into(),
                    ingress: "tui".into(),
                },
                runtime_generation_id: "generation-1".into(),
            }),
        )
    }

    fn admitted(
        session_id: &str,
        stream_id: Uuid,
        sequence: u64,
        prompt_id: Uuid,
        text: &str,
    ) -> SessionFact {
        fact(
            session_id,
            stream_id,
            sequence,
            SessionFactPayload::PromptAdmitted(PromptAdmitted {
                submission_id: Uuid::new_v4(),
                prompt_id,
                principal: "operator".into(),
                ingress: "tui".into(),
                queue_mode: QueueMode::UntilReady,
                content: PromptContent {
                    text: text.into(),
                    attachments: Vec::new(),
                },
                metadata: serde_json::json!({}),
            }),
        )
    }

    fn started(
        session_id: &str,
        stream_id: Uuid,
        sequence: u64,
        prompt_id: Uuid,
        turn_id: Uuid,
    ) -> SessionFact {
        fact(
            session_id,
            stream_id,
            sequence,
            SessionFactPayload::TurnStarted(TurnStarted {
                turn_id,
                prompt_id,
                runtime_generation_id: "generation-1".into(),
            }),
        )
    }

    fn route_lease(turn_id: Uuid) -> RouteLeaseRecorded {
        RouteLeaseRecorded {
            lease_id: Uuid::new_v4(),
            request_id: Uuid::new_v4(),
            turn_id,
            selected_provider_id: "openai-codex".into(),
            selected_model_id: "gpt-5.5".into(),
            serving_provider_id: "openai".into(),
            serving_model_id: "gpt-5.5".into(),
            schema_dialect: "open_ai".into(),
            credential_source_class: "api_key".into(),
            fallback_reason: Some("selected_provider_unavailable".into()),
            contribution_generation_id: "provider:openai/builtin-v1".into(),
            route_policy: "declared_model_family_fallback_v1".into(),
        }
    }

    #[test]
    fn route_lease_is_reduced_only_for_active_owning_turn() {
        let session_id = "route-session";
        let stream_id = Uuid::new_v4();
        let prompt_id = Uuid::new_v4();
        let turn_id = Uuid::new_v4();
        let lease = route_lease(turn_id);
        let lease_id = lease.lease_id;
        let state = reconstruct(&[
            created(session_id, stream_id),
            admitted(session_id, stream_id, 2, prompt_id, "route me"),
            started(session_id, stream_id, 3, prompt_id, turn_id),
            fact(
                session_id,
                stream_id,
                4,
                SessionFactPayload::RouteLeaseRecorded(lease.clone()),
            ),
        ])
        .unwrap();

        assert_eq!(state.route_leases.get(&lease_id), Some(&lease));

        let mut stale = route_lease(Uuid::new_v4());
        stale.lease_id = Uuid::new_v4();
        let error = reconstruct(&[
            created(session_id, stream_id),
            admitted(session_id, stream_id, 2, prompt_id, "route me"),
            started(session_id, stream_id, 3, prompt_id, turn_id),
            fact(
                session_id,
                stream_id,
                4,
                SessionFactPayload::RouteLeaseRecorded(stale),
            ),
        ])
        .unwrap_err();
        assert!(error.to_string().contains("stale turn"));
    }

    #[test]
    fn route_lease_wire_round_trip_preserves_fallback_identity() {
        let stream_id = Uuid::new_v4();
        let lease = route_lease(Uuid::new_v4());
        let original = fact(
            "route-session",
            stream_id,
            1,
            SessionFactPayload::RouteLeaseRecorded(lease.clone()),
        );

        let decoded = SessionFact::decode(&original.encode().unwrap()).unwrap();

        assert_eq!(
            decoded.payload,
            SessionFactPayload::RouteLeaseRecorded(lease)
        );
    }

    #[test]
    fn endpoint_provenance_rejects_non_manifest_route_lease() {
        let session_id = "route-session";
        let stream_id = Uuid::new_v4();
        let prompt_id = Uuid::new_v4();
        let turn_id = Uuid::new_v4();
        let lease = route_lease(turn_id);
        let lease_id = lease.lease_id;

        let error = reconstruct(&[
            created(session_id, stream_id),
            admitted(session_id, stream_id, 2, prompt_id, "route me"),
            started(session_id, stream_id, 3, prompt_id, turn_id),
            fact(
                session_id,
                stream_id,
                4,
                SessionFactPayload::RouteLeaseRecorded(lease),
            ),
            fact(
                session_id,
                stream_id,
                5,
                SessionFactPayload::RouteEndpointProvenanceRecorded(
                    RouteEndpointProvenanceRecorded {
                        lease_id,
                        endpoint_id: "private-endpoint".into(),
                        adapter_id: "chat-completions".into(),
                        inventory_generation: 42,
                    },
                ),
            ),
        ])
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("requires a manifest route lease")
        );
    }

    fn prepared(turn_id: Uuid, call_id: &str) -> InvocationPrepared {
        let effects = vec![RuntimeEffect::FilesystemRead];
        InvocationPrepared {
            invocation_id: Uuid::new_v4(),
            lease_id: Uuid::new_v4(),
            turn_id,
            call_id: call_id.into(),
            deduplication_id: Some(call_id.into()),
            invocation_kind: RuntimeInvocationKind::Tool,
            invocation_name: "read".into(),
            capability_id: RuntimeCapabilityId::new("tool:read").unwrap(),
            contribution_id: RuntimeContributionId::new("feature:reader").unwrap(),
            owner_generation_id: RuntimeContributionGenerationId::new("contribution:reader-v1")
                .unwrap(),
            issue_generation_id: RuntimeCompositionGenerationId::new("composition:test").unwrap(),
            principal: "model".into(),
            principal_class: RuntimePrincipalClass::Model,
            surface: RuntimeSurface::Model,
            admitted_effects: effects,
            execution: RuntimeExecutionPolicy {
                principals: vec![RuntimePrincipalClass::Model],
                timeout_class: omegon_traits::RuntimeTimeoutClass::Interactive,
                retry_class: omegon_traits::RuntimeRetryClass::IdempotentFailure,
                idempotency: omegon_traits::RuntimeIdempotency::Idempotent,
                deduplication: omegon_traits::RuntimeDeduplication::OwnerEnforcedStableCallId,
                parallelism: omegon_traits::RuntimeParallelism::Serial,
                transaction: omegon_traits::RuntimeTransactionBehavior::None,
                mutation_fence: None,
                max_attempts: Some(2),
            },
            transition: RuntimeCapabilityTransitionPolicy {
                authority_narrowing: omegon_traits::RuntimeAuthorityNarrowing::CompleteExisting,
                active_call_timeout_ms: 30_000,
            },
            surfaces: vec![RuntimeSurface::Model],
        }
    }

    fn execution_binding(suffix: &str) -> ExecutionBindingGeneration {
        ExecutionBindingGeneration::new(
            format!("loop-driver:{suffix}"),
            format!("provider-route-service:{suffix}"),
        )
        .unwrap()
    }

    fn test_authority(directory: &tempfile::TempDir) -> SessionAuthority {
        SessionAuthority::open(
            &directory.path().join("session.json"),
            "session-binding",
            "workspace-binding",
            "composition:legacy",
            ActorIdentity {
                principal: "operator".into(),
                ingress: "test".into(),
            },
            NOW,
        )
        .unwrap()
    }

    #[test]
    fn parity_duplicate_prompt_command_survives_authority_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let command_id = Uuid::new_v4();
        let admission = PromptAdmitted {
            submission_id: Uuid::new_v4(),
            prompt_id: Uuid::new_v4(),
            principal: "operator".into(),
            ingress: "ipc".into(),
            queue_mode: QueueMode::UntilReady,
            content: PromptContent {
                text: "inspect".into(),
                attachments: Vec::new(),
            },
            metadata: serde_json::json!({}),
        };
        let sequence = {
            let mut authority = test_authority(&directory);
            assert!(
                authority
                    .admit_prompt(command_id, NOW, admission.clone())
                    .unwrap()
            );
            authority.state().last_sequence
        };
        let mut reopened = test_authority(&directory);
        assert!(
            !reopened
                .admit_prompt(command_id, NOW, admission.clone())
                .unwrap()
        );
        assert_eq!(reopened.state().last_sequence, sequence);
        assert_eq!(reopened.state().queued_prompts.len(), 1);
        let mut conflicting = admission;
        conflicting.content.text = "different side effect".into();
        assert!(reopened.admit_prompt(command_id, NOW, conflicting).is_err());
        assert_eq!(reopened.state().last_sequence, sequence);
        assert_eq!(reopened.state().queued_prompts.len(), 1);
    }

    fn begin_test_turn(authority: &mut SessionAuthority) -> (Uuid, Uuid) {
        let prompt_id = Uuid::new_v4();
        let turn_id = Uuid::new_v4();
        authority
            .admit_prompt(
                Uuid::new_v4(),
                NOW,
                PromptAdmitted {
                    submission_id: Uuid::new_v4(),
                    prompt_id,
                    principal: "operator".into(),
                    ingress: "test".into(),
                    queue_mode: QueueMode::UntilReady,
                    content: PromptContent {
                        text: "work".into(),
                        attachments: Vec::new(),
                    },
                    metadata: serde_json::json!({}),
                },
            )
            .unwrap();
        authority
            .start_turn(Uuid::new_v4(), NOW, turn_id, prompt_id)
            .unwrap();
        (prompt_id, turn_id)
    }

    fn model_request(
        authority: &SessionAuthority,
        step_id: Uuid,
        turn_id: Uuid,
        request_ordinal: u32,
        purpose: ModelRequestPurpose,
        replaces_request_id: Option<Uuid>,
    ) -> ModelRequestPrepared {
        let context_ref = authority
            .write_content(
                b"exact provider context",
                "text/plain",
                ProjectionClass::Default,
            )
            .unwrap();
        let schema_ref = authority
            .write_content(
                br#"{"name":"read","parameters":{"type":"object"}}"#,
                "application/json",
                ProjectionClass::Default,
            )
            .unwrap();
        let context_items = vec![ModelContextItem {
            ordinal: 0,
            role: ModelContextRole::System,
            content_ref: context_ref,
            provenance: ModelContextProvenance {
                source_kind: ModelContextSourceKind::SystemInstruction,
                source_event_id: None,
                source_identity: Some("legacy-instruction:base".into()),
                owner_id: Some("feature:system-prompt".into()),
                owner_generation_id: Some(
                    RuntimeContributionGenerationId::new("contribution:system-prompt-v1").unwrap(),
                ),
            },
        }];
        let schema_set = ModelSchemaSet {
            schema_set_version: 1,
            composition_generation_id: RuntimeCompositionGenerationId::new("composition:test")
                .unwrap(),
            normalizer_contribution_id: RuntimeContributionId::new("feature:schema-normalizer")
                .unwrap(),
            normalizer_generation_id: RuntimeContributionGenerationId::new(
                "contribution:schema-normalizer-v1",
            )
            .unwrap(),
            schemas: vec![ModelSchemaIdentity {
                ordinal: 0,
                capability_id: RuntimeCapabilityId::new("tool:read").unwrap(),
                contribution_id: RuntimeContributionId::new("feature:reader").unwrap(),
                owner_generation_id: RuntimeContributionGenerationId::new("contribution:reader-v1")
                    .unwrap(),
                schema_dialect: "open_ai".into(),
                schema_content_ref: schema_ref,
            }],
        };
        ModelRequestPrepared {
            request_id: Uuid::new_v4(),
            step_id,
            turn_id,
            request_ordinal,
            purpose,
            replaces_request_id,
            continuity_refs: Vec::new(),
            context_manifest_id: canonical_sha256(&context_items).unwrap(),
            context_items,
            schema_set_id: canonical_sha256(&schema_set).unwrap(),
            schema_set,
        }
    }

    fn begin_joined_model_request(
        authority: &mut SessionAuthority,
    ) -> (Uuid, Uuid, ModelRequestPrepared, RouteLeaseRecorded) {
        let (_, turn_id) = begin_test_turn(authority);
        let step_id = Uuid::new_v4();
        authority
            .start_step(
                Uuid::new_v4(),
                NOW,
                StepStarted {
                    step_id,
                    turn_id,
                    step_ordinal: 0,
                },
            )
            .unwrap();
        let request = model_request(
            authority,
            step_id,
            turn_id,
            0,
            ModelRequestPurpose::Initial,
            None,
        );
        authority
            .prepare_model_request(Uuid::new_v4(), NOW, request.clone())
            .unwrap();
        let mut lease = route_lease(turn_id);
        lease.request_id = request.request_id;
        authority.record_route_lease(NOW, lease.clone()).unwrap();
        authority
            .join_model_request_route(
                Uuid::new_v4(),
                NOW,
                ModelRequestRouteJoined {
                    request_id: request.request_id,
                    step_id,
                    turn_id,
                    lease_id: lease.lease_id,
                },
            )
            .unwrap();
        (turn_id, step_id, request, lease)
    }

    fn assistant_chunk(
        authority: &SessionAuthority,
        message_id: Uuid,
        request_id: Uuid,
        step_id: Uuid,
        content_kind: AssistantContentKind,
        chunk_ordinal: u32,
        bytes: &[u8],
    ) -> AssistantContentAppended {
        AssistantContentAppended {
            message_id,
            request_id,
            step_id,
            response_attempt_ordinal: 0,
            content_kind,
            chunk_ordinal,
            content_ref: authority
                .write_content(bytes, "text/plain", ProjectionClass::Default)
                .unwrap(),
        }
    }

    fn assistant_commit(
        message_id: Uuid,
        request_id: Uuid,
        step_id: Uuid,
        chunks: &[(&AssistantContentAppended, &[u8])],
    ) -> AssistantMessageCommitted {
        let mut content = Vec::new();
        for kind in [AssistantContentKind::Text, AssistantContentKind::Thinking] {
            let matching = chunks
                .iter()
                .filter(|(chunk, _)| chunk.content_kind == kind)
                .collect::<Vec<_>>();
            if matching.is_empty() {
                continue;
            }
            let mut hasher = Sha256::new();
            let mut chunk_refs = Vec::new();
            for (chunk, bytes) in matching {
                hasher.update(bytes);
                chunk_refs.push(chunk.content_ref.clone());
            }
            content.push(AssistantContentManifest {
                content_kind: kind,
                chunk_refs,
                content_digest: format!("{:x}", hasher.finalize()),
            });
        }
        AssistantMessageCommitted {
            message_id,
            request_id,
            step_id,
            response_attempt_ordinal: 0,
            completion_evidence: ProviderCompletionEvidence::ProviderDone,
            content,
            usage: Some(AssistantUsage {
                input_tokens: 12,
                output_tokens: 5,
            }),
            tool_call_count: 0,
        }
    }

    fn commit_response(
        authority: &mut SessionAuthority,
        request: &ModelRequestPrepared,
        text: &[u8],
        tool_call_count: u32,
    ) -> AssistantMessageCommitted {
        let message_id = Uuid::new_v4();
        let chunk = assistant_chunk(
            authority,
            message_id,
            request.request_id,
            request.step_id,
            AssistantContentKind::Text,
            0,
            text,
        );
        authority
            .append_assistant_content(Uuid::new_v4(), NOW, chunk.clone())
            .unwrap();
        let mut commit = assistant_commit(
            message_id,
            request.request_id,
            request.step_id,
            &[(&chunk, text)],
        );
        commit.tool_call_count = tool_call_count;
        authority
            .commit_assistant_message(Uuid::new_v4(), NOW, commit.clone())
            .unwrap();
        commit
    }

    fn tool_call(
        authority: &SessionAuthority,
        request: &ModelRequestPrepared,
        ordinal: u32,
        call_id: &str,
    ) -> ToolCallRecorded {
        ToolCallRecorded {
            tool_call_id: Uuid::new_v4(),
            request_id: request.request_id,
            step_id: request.step_id,
            call_ordinal: ordinal,
            call_id: call_id.into(),
            invocation_name: "read".into(),
            arguments_ref: authority
                .write_content(
                    br#"{"path":"README.md"}"#,
                    "application/json",
                    ProjectionClass::Default,
                )
                .unwrap(),
        }
    }

    fn tool_result(
        authority: &SessionAuthority,
        call: &ToolCallRecorded,
        disposition: ToolResultDisposition,
        invocation_id: Option<Uuid>,
        lease_id: Option<Uuid>,
        reason_code: Option<&str>,
        is_error: bool,
    ) -> ToolResultRecorded {
        ToolResultRecorded {
            tool_result_id: Uuid::new_v4(),
            tool_call_id: call.tool_call_id,
            step_id: call.step_id,
            result_ordinal: call.call_ordinal,
            call_id: call.call_id.clone(),
            disposition,
            invocation_id,
            lease_id,
            content_ref: authority
                .write_content(
                    b"final model-visible result",
                    "text/plain",
                    ProjectionClass::Default,
                )
                .unwrap(),
            is_error,
            reason_code: reason_code.map(str::to_owned),
        }
    }

    fn close_response_request(authority: &mut SessionAuthority, request: &ModelRequestPrepared) {
        authority
            .close_model_request(
                Uuid::new_v4(),
                NOW,
                ModelRequestClosed {
                    request_id: request.request_id,
                    step_id: request.step_id,
                    response_attempt_ordinal: 0,
                    outcome: ModelRequestOutcome::ResponseCompleted,
                    reason_code: "provider_done".into(),
                    recovery_rule_version: None,
                },
            )
            .unwrap();
    }

    #[test]
    fn text_only_step_closes_turn_and_tool_step_continues_to_next_ordinal() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (turn_id, step_id, request, _) = begin_joined_model_request(&mut authority);
        commit_response(&mut authority, &request, b"plain answer", 0);
        close_response_request(&mut authority, &request);
        authority
            .close_step(
                Uuid::new_v4(),
                NOW,
                StepClosed {
                    step_id,
                    turn_id,
                    outcome: StepOutcome::TurnCompleted,
                    reason_code: "assistant_complete".into(),
                },
            )
            .unwrap();
        assert!(authority.state().active_step.is_none());
        assert!(
            authority
                .start_step(
                    Uuid::new_v4(),
                    NOW,
                    StepStarted {
                        step_id: Uuid::new_v4(),
                        turn_id,
                        step_ordinal: 1,
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("does not permit continuation")
        );
        authority
            .close_turn(
                Uuid::new_v4(),
                NOW,
                TurnClosed {
                    turn_id,
                    outcome: TurnOutcome::Completed,
                    reason_code: "completed".into(),
                    recovery_rule_version: None,
                },
            )
            .unwrap();

        let (_, second_turn) = begin_test_turn(&mut authority);
        let second_step = Uuid::new_v4();
        authority
            .start_step(
                Uuid::new_v4(),
                NOW,
                StepStarted {
                    step_id: second_step,
                    turn_id: second_turn,
                    step_ordinal: 0,
                },
            )
            .unwrap();
    }

    #[test]
    fn denied_tool_call_has_no_lease_and_allows_contiguous_next_step() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (turn_id, step_id, request, _) = begin_joined_model_request(&mut authority);
        commit_response(&mut authority, &request, b"calling tool", 1);
        let call = tool_call(&authority, &request, 0, "provider-call-denied");
        authority
            .record_tool_call(Uuid::new_v4(), NOW, call.clone())
            .unwrap();
        close_response_request(&mut authority, &request);
        assert!(
            authority
                .close_step(
                    Uuid::new_v4(),
                    NOW,
                    StepClosed {
                        step_id,
                        turn_id,
                        outcome: StepOutcome::ContinueLoop,
                        reason_code: "result_missing".into(),
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("missing tool result")
        );
        let result = tool_result(
            &authority,
            &call,
            ToolResultDisposition::Denied,
            None,
            None,
            Some("permission:denied"),
            true,
        );
        authority
            .record_tool_result(Uuid::new_v4(), NOW, result.clone())
            .unwrap();
        authority
            .close_step(
                Uuid::new_v4(),
                NOW,
                StepClosed {
                    step_id,
                    turn_id,
                    outcome: StepOutcome::ContinueLoop,
                    reason_code: "tool_results_ready".into(),
                },
            )
            .unwrap();
        assert!(result.invocation_id.is_none() && result.lease_id.is_none());
        assert!(authority.state().invocations.is_empty());
        assert!(
            authority
                .close_turn(
                    Uuid::new_v4(),
                    NOW,
                    TurnClosed {
                        turn_id,
                        outcome: TurnOutcome::Completed,
                        reason_code: "invalid".into(),
                        recovery_rule_version: None,
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("continuation")
        );
        let next_step_id = Uuid::new_v4();
        authority
            .start_step(
                Uuid::new_v4(),
                NOW,
                StepStarted {
                    step_id: next_step_id,
                    turn_id,
                    step_ordinal: 1,
                },
            )
            .unwrap();
        let mut next_request = model_request(
            &authority,
            next_step_id,
            turn_id,
            0,
            ModelRequestPurpose::Initial,
            None,
        );
        next_request.context_items.push(ModelContextItem {
            ordinal: 1,
            role: ModelContextRole::Tool,
            content_ref: result.content_ref.clone(),
            provenance: ModelContextProvenance {
                source_kind: ModelContextSourceKind::ToolResult,
                source_event_id: Some(
                    authority.state().tool_result_source_events[&result.tool_result_id],
                ),
                source_identity: Some(result.tool_result_id.to_string()),
                owner_id: None,
                owner_generation_id: None,
            },
        });
        next_request.context_manifest_id = canonical_sha256(&next_request.context_items).unwrap();
        authority
            .prepare_model_request(Uuid::new_v4(), NOW, next_request)
            .unwrap();
    }

    #[test]
    fn settled_and_unknown_results_require_exact_invocation_and_lease_linkage() {
        for unknown in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let mut authority = test_authority(&directory);
            let (_, step_id, request, _) = begin_joined_model_request(&mut authority);
            commit_response(&mut authority, &request, b"calling tool", 1);
            let call = tool_call(
                &authority,
                &request,
                0,
                if unknown {
                    "unknown-call"
                } else {
                    "settled-call"
                },
            );
            authority
                .record_tool_call(Uuid::new_v4(), NOW, call.clone())
                .unwrap();
            let mut preparation = prepared(request.turn_id, &call.call_id);
            preparation.invocation_name = call.invocation_name.clone();
            authority
                .prepare_invocation(NOW, preparation.clone())
                .unwrap();
            close_response_request(&mut authority, &request);
            authority
                .mark_invocation_dispatched(
                    NOW,
                    InvocationDispatched {
                        invocation_id: preparation.invocation_id,
                        lease_id: preparation.lease_id,
                    },
                )
                .unwrap();
            if unknown {
                authority
                    .classify_invocation_unknown(
                        NOW,
                        InvocationClassifiedUnknown {
                            invocation_id: preparation.invocation_id,
                            reason_code: "transport_lost".into(),
                            recovery_rule_version: 2,
                        },
                    )
                    .unwrap();
            } else {
                authority
                    .acknowledge_invocation(
                        NOW,
                        InvocationAcknowledged {
                            invocation_id: preparation.invocation_id,
                            lease_id: preparation.lease_id,
                        },
                    )
                    .unwrap();
                authority
                    .settle_invocation(
                        NOW,
                        InvocationSettled {
                            invocation_id: preparation.invocation_id,
                            outcome: InvocationOutcome::Completed,
                            terminal_evidence_reference: None,
                        },
                    )
                    .unwrap();
            }
            let result = tool_result(
                &authority,
                &call,
                if unknown {
                    ToolResultDisposition::UnknownCompletion
                } else {
                    ToolResultDisposition::Settled
                },
                Some(preparation.invocation_id),
                Some(preparation.lease_id),
                unknown.then_some("transport_lost"),
                unknown,
            );
            let mut wrong = result.clone();
            wrong.lease_id = Some(Uuid::new_v4());
            assert!(
                authority
                    .record_tool_result(Uuid::new_v4(), NOW, wrong)
                    .unwrap_err()
                    .to_string()
                    .contains("contradicts")
            );
            authority
                .record_tool_result(Uuid::new_v4(), NOW, result)
                .unwrap();
            assert_eq!(authority.state().terminal_steps.len(), 0);
            assert_eq!(
                authority
                    .state()
                    .active_step
                    .as_ref()
                    .unwrap()
                    .start
                    .step_id,
                step_id
            );
        }
    }

    #[test]
    fn tool_and_step_cardinality_order_and_close_invariants_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (turn_id, step_id, request, _) = begin_joined_model_request(&mut authority);
        assert!(
            authority
                .record_tool_call(
                    Uuid::new_v4(),
                    NOW,
                    tool_call(&authority, &request, 0, "early")
                )
                .unwrap_err()
                .to_string()
                .contains("committed")
        );
        commit_response(&mut authority, &request, b"two calls", 2);
        let first = tool_call(&authority, &request, 0, "first");
        authority
            .record_tool_call(Uuid::new_v4(), NOW, first.clone())
            .unwrap();
        assert!(
            authority
                .record_tool_call(
                    Uuid::new_v4(),
                    NOW,
                    tool_call(&authority, &request, 0, "duplicate-ordinal")
                )
                .unwrap_err()
                .to_string()
                .contains("ordinal 1")
        );
        assert!(
            authority
                .close_step(
                    Uuid::new_v4(),
                    NOW,
                    StepClosed {
                        step_id,
                        turn_id,
                        outcome: StepOutcome::ContinueLoop,
                        reason_code: "too_early".into(),
                    }
                )
                .unwrap_err()
                .to_string()
                .contains("open model request")
        );
        close_response_request(&mut authority, &request);
        assert!(
            authority
                .close_step(
                    Uuid::new_v4(),
                    NOW,
                    StepClosed {
                        step_id,
                        turn_id,
                        outcome: StepOutcome::ContinueLoop,
                        reason_code: "missing_call".into(),
                    }
                )
                .unwrap_err()
                .to_string()
                .contains("not fully recorded")
        );
        assert!(
            authority
                .record_tool_result(
                    Uuid::new_v4(),
                    NOW,
                    tool_result(
                        &authority,
                        &ToolCallRecorded {
                            tool_call_id: Uuid::new_v4(),
                            ..first.clone()
                        },
                        ToolResultDisposition::Denied,
                        None,
                        None,
                        Some("permission:denied"),
                        true,
                    )
                )
                .unwrap_err()
                .to_string()
                .contains("unknown call")
        );
        let mut untransformed = tool_result(
            &authority,
            &first,
            ToolResultDisposition::Denied,
            None,
            None,
            Some("permission:denied"),
            true,
        );
        untransformed.content_ref = authority
            .write_content(
                b"restricted raw result",
                "text/plain",
                ProjectionClass::RestrictedContinuity,
            )
            .unwrap();
        assert!(
            authority
                .record_tool_result(Uuid::new_v4(), NOW, untransformed)
                .unwrap_err()
                .to_string()
                .contains("projection class")
        );
        let second = tool_call(&authority, &request, 1, "second");
        assert!(
            authority
                .record_tool_call(Uuid::new_v4(), NOW, second)
                .is_err()
        );
        assert!(
            authority
                .close_turn(
                    Uuid::new_v4(),
                    NOW,
                    TurnClosed {
                        turn_id,
                        outcome: TurnOutcome::Completed,
                        reason_code: "early".into(),
                        recovery_rule_version: None
                    }
                )
                .unwrap_err()
                .to_string()
                .contains("active step")
        );
    }

    #[test]
    fn tool_wire_append_blob_replay_and_strict_fields_are_atomic() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (_, _, request, _) = begin_joined_model_request(&mut authority);
        commit_response(&mut authority, &request, b"call", 1);
        let call = tool_call(&authority, &request, 0, "strict-call");
        let wire = fact(
            "session-binding",
            authority.stream_id,
            authority.state().last_sequence + 1,
            SessionFactPayload::ToolCallRecorded(call.clone()),
        );
        assert_eq!(SessionFact::decode(&wire.encode().unwrap()).unwrap(), wire);
        let mut unknown: Value = serde_json::from_slice(&wire.encode().unwrap()).unwrap();
        unknown["payload"]["admission"] = Value::String("forbidden".into());
        assert!(SessionFact::decode(&serde_json::to_vec(&unknown).unwrap()).is_err());

        let before = authority.state().clone();
        let log_path = authority.store.log_path.clone();
        let mut permissions = fs::metadata(&log_path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&log_path, permissions).unwrap();
        assert!(
            authority
                .record_tool_call(Uuid::new_v4(), NOW, call.clone())
                .is_err()
        );
        assert_eq!(authority.state(), &before);
        let mut permissions = fs::metadata(&log_path).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o600);
        }
        #[cfg(not(unix))]
        permissions.set_readonly(false);
        fs::set_permissions(&log_path, permissions).unwrap();
        authority
            .record_tool_call(Uuid::new_v4(), NOW, call.clone())
            .unwrap();
        assert_eq!(authority.store.load().unwrap(), authority.state().clone());
        let blob_path = directory
            .path()
            .join("session.authority.blobs")
            .join(call.arguments_ref.storage_reference().as_relative_path());
        fs::write(blob_path, b"tampered arguments").unwrap();
        assert!(
            authority
                .store
                .load()
                .unwrap_err()
                .to_string()
                .contains("blob")
        );
    }

    #[test]
    fn recovery_closes_request_then_abandons_step_without_inventing_results() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (turn_id, step_id, request, _) = begin_joined_model_request(&mut authority);
        commit_response(&mut authority, &request, b"call", 1);
        let call = tool_call(&authority, &request, 0, "crash-call");
        authority
            .record_tool_call(Uuid::new_v4(), NOW, call.clone())
            .unwrap();
        let facts = recovery_facts(authority.state(), NOW).unwrap();
        assert!(
            matches!(facts[0].payload, SessionFactPayload::ModelRequestClosed(ref closure) if closure.outcome == ModelRequestOutcome::Abandoned)
        );
        assert!(matches!(
            facts[1].payload,
            SessionFactPayload::StepAbandoned(_)
        ));
        assert!(matches!(
            facts[2].payload,
            SessionFactPayload::TurnClosed(_)
        ));
        let recovered = authority.store.recover(NOW).unwrap();
        assert!(recovered.active_step.is_none());
        assert!(recovered.active_turn.is_none());
        assert!(!recovered.call_results.contains_key(&call.tool_call_id));
        assert!(matches!(
            recovered.terminal_steps[&step_id],
            StepTerminalState::Abandoned { .. }
        ));
        assert_eq!(
            recovered.closed_turns[&turn_id].outcome,
            TurnOutcome::Interrupted
        );
        assert!(recovery_facts(&recovered, NOW).unwrap().is_empty());
    }

    #[test]
    fn every_recovery_prefix_resumes_with_the_identical_durable_suffix() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (_, turn_id) = begin_test_turn(&mut authority);
        let preparation = prepared(turn_id, "legacy-dispatch");
        authority
            .prepare_invocation(NOW, preparation.clone())
            .unwrap();
        authority
            .mark_invocation_dispatched(
                NOW,
                InvocationDispatched {
                    invocation_id: preparation.invocation_id,
                    lease_id: preparation.lease_id,
                },
            )
            .unwrap();
        let step_id = Uuid::new_v4();
        authority
            .start_step(
                Uuid::new_v4(),
                NOW,
                StepStarted {
                    step_id,
                    turn_id,
                    step_ordinal: 0,
                },
            )
            .unwrap();
        let request = model_request(
            &authority,
            step_id,
            turn_id,
            0,
            ModelRequestPurpose::Initial,
            None,
        );
        authority
            .prepare_model_request(Uuid::new_v4(), NOW, request.clone())
            .unwrap();
        let mut lease = route_lease(turn_id);
        lease.request_id = request.request_id;
        authority.record_route_lease(NOW, lease.clone()).unwrap();
        authority
            .join_model_request_route(
                Uuid::new_v4(),
                NOW,
                ModelRequestRouteJoined {
                    request_id: request.request_id,
                    step_id,
                    turn_id,
                    lease_id: lease.lease_id,
                },
            )
            .unwrap();

        let initial = authority.state().clone();
        let uninterrupted = recovery_facts(&initial, NOW).unwrap();
        assert_eq!(uninterrupted.len(), 4);
        assert!(matches!(
            uninterrupted[3].payload,
            SessionFactPayload::TurnClosed(TurnClosed {
                recovery_rule_version: Some(2),
                ..
            })
        ));

        for prefix_len in 0..=uninterrupted.len() {
            let mut crashed = initial.clone();
            for fact in &uninterrupted[..prefix_len] {
                crashed.apply(fact).unwrap();
            }
            assert_eq!(
                recovery_facts(&crashed, NOW).unwrap(),
                uninterrupted[prefix_len..],
                "recovery diverged after durable prefix {prefix_len}"
            );
        }
    }

    #[test]
    fn response_attempt_failures_gate_contiguous_lineage_and_zero_content_retry_commit() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (_, step_id, request, _) = begin_joined_model_request(&mut authority);
        let failure = ModelResponseAttemptFailed {
            request_id: request.request_id,
            step_id,
            response_attempt_ordinal: 0,
            failure: ModelResponseAttemptFailure::TransportLost,
            reason_code: "connection_reset".into(),
            retry_disposition: ModelResponseAttemptRetryDisposition::RetrySameRequest,
        };
        let wire = fact(
            "session-binding",
            authority.stream_id,
            authority.state().last_sequence + 1,
            SessionFactPayload::ModelResponseAttemptFailed(failure.clone()),
        );
        assert_eq!(SessionFact::decode(&wire.encode().unwrap()).unwrap(), wire);
        let mut unknown: Value = serde_json::from_slice(&wire.encode().unwrap()).unwrap();
        unknown["payload"]["unexpected"] = Value::Bool(true);
        assert!(SessionFact::decode(&serde_json::to_vec(&unknown).unwrap()).is_err());

        authority
            .fail_model_response_attempt(Uuid::new_v4(), NOW, failure.clone())
            .unwrap();
        let mut wrong_scope = failure.clone();
        wrong_scope.response_attempt_ordinal = 1;
        wrong_scope.step_id = Uuid::new_v4();
        assert!(
            authority
                .fail_model_response_attempt(Uuid::new_v4(), NOW, wrong_scope)
                .unwrap_err()
                .to_string()
                .contains("wrong request")
        );
        assert!(
            authority
                .fail_model_response_attempt(Uuid::new_v4(), NOW, failure)
                .unwrap_err()
                .to_string()
                .contains("current response attempt ordinal 1")
        );
        let gap = assistant_chunk(
            &authority,
            Uuid::new_v4(),
            request.request_id,
            step_id,
            AssistantContentKind::Text,
            0,
            b"gap",
        );
        let gap = AssistantContentAppended {
            response_attempt_ordinal: 2,
            ..gap
        };
        assert!(
            authority
                .append_assistant_content(Uuid::new_v4(), NOW, gap)
                .unwrap_err()
                .to_string()
                .contains("current response attempt ordinal 1")
        );

        let message_id = Uuid::new_v4();
        authority
            .commit_assistant_message(
                Uuid::new_v4(),
                NOW,
                AssistantMessageCommitted {
                    message_id,
                    request_id: request.request_id,
                    step_id,
                    response_attempt_ordinal: 1,
                    completion_evidence: ProviderCompletionEvidence::ProviderDone,
                    content: Vec::new(),
                    usage: Some(AssistantUsage {
                        input_tokens: 1,
                        output_tokens: 1,
                    }),
                    tool_call_count: 1,
                },
            )
            .unwrap();
        assert!(
            authority
                .fail_model_response_attempt(
                    Uuid::new_v4(),
                    NOW,
                    ModelResponseAttemptFailed {
                        request_id: request.request_id,
                        step_id,
                        response_attempt_ordinal: 1,
                        failure: ModelResponseAttemptFailure::ProviderError,
                        reason_code: "provider_overloaded".into(),
                        retry_disposition: ModelResponseAttemptRetryDisposition::RetrySameRequest,
                    }
                )
                .unwrap_err()
                .to_string()
                .contains("provider Done")
        );
        let recovery = recovery_facts(authority.state(), NOW).unwrap();
        assert!(matches!(
            recovery[0].payload,
            SessionFactPayload::ModelRequestClosed(ModelRequestClosed {
                response_attempt_ordinal: 1,
                ..
            })
        ));
        let mut closed = authority.state().clone();
        closed.apply(&recovery[0]).unwrap();
        let after_close = fact(
            "session-binding",
            authority.stream_id,
            closed.last_sequence + 1,
            SessionFactPayload::ModelResponseAttemptFailed(ModelResponseAttemptFailed {
                request_id: request.request_id,
                step_id,
                response_attempt_ordinal: 1,
                failure: ModelResponseAttemptFailure::ProviderError,
                reason_code: "provider_overloaded".into(),
                retry_disposition: ModelResponseAttemptRetryDisposition::RetrySameRequest,
            }),
        );
        assert!(
            closed
                .apply(&after_close)
                .unwrap_err()
                .to_string()
                .contains("wrong request")
        );
    }

    #[test]
    fn live_abnormal_terminalization_preserves_partial_text_and_allows_next_turn() {
        let directory = tempfile::tempdir().unwrap();
        let session_path = directory.path().join("session.json");
        let mut authority = test_authority(&directory);
        let (turn_id, _, request, _) = begin_joined_model_request(&mut authority);
        for response_attempt_ordinal in 0..3 {
            authority
                .fail_model_response_attempt(
                    Uuid::new_v4(),
                    NOW,
                    ModelResponseAttemptFailed {
                        request_id: request.request_id,
                        step_id: request.step_id,
                        response_attempt_ordinal,
                        failure: ModelResponseAttemptFailure::TransportLost,
                        reason_code: "connection_reset".into(),
                        retry_disposition: ModelResponseAttemptRetryDisposition::RetrySameRequest,
                    },
                )
                .unwrap();
        }
        let message_id = Uuid::new_v4();
        let content_ref = authority
            .write_content(b"durable partial", "text/plain", ProjectionClass::Default)
            .unwrap();
        authority
            .append_assistant_content(
                Uuid::new_v4(),
                NOW,
                AssistantContentAppended {
                    message_id,
                    request_id: request.request_id,
                    step_id: request.step_id,
                    response_attempt_ordinal: 3,
                    content_kind: AssistantContentKind::Text,
                    chunk_ordinal: 0,
                    content_ref,
                },
            )
            .unwrap();
        let second_prompt = Uuid::new_v4();
        authority
            .admit_prompt(
                Uuid::new_v4(),
                NOW,
                PromptAdmitted {
                    submission_id: Uuid::new_v4(),
                    prompt_id: second_prompt,
                    principal: "operator".into(),
                    ingress: "test".into(),
                    queue_mode: QueueMode::UntilReady,
                    content: PromptContent {
                        text: "continue after abandonment".into(),
                        attachments: Vec::new(),
                    },
                    metadata: serde_json::json!({}),
                },
            )
            .unwrap();

        assert!(
            authority
                .terminalize_active_semantic_step(
                    NOW,
                    SemanticTerminalization {
                        turn_id,
                        request_outcome: ModelRequestOutcome::Abandoned,
                        reason_code: "worker_join_failed".into(),
                        rule_version: 1,
                    },
                )
                .unwrap()
        );
        assert!(
            !authority
                .terminalize_active_semantic_step(
                    NOW,
                    SemanticTerminalization {
                        turn_id,
                        request_outcome: ModelRequestOutcome::Abandoned,
                        reason_code: "worker_join_failed".into(),
                        rule_version: 1,
                    },
                )
                .unwrap()
        );
        let state = authority.state();
        assert!(matches!(
            state.model_requests[&request.request_id],
            ModelRequestState::Closed {
                closure: ModelRequestClosed {
                    outcome: ModelRequestOutcome::Abandoned,
                    response_attempt_ordinal: 3,
                    ..
                },
                ..
            }
        ));
        assert!(matches!(
            state.terminal_steps[&request.step_id],
            StepTerminalState::Abandoned { .. }
        ));
        authority
            .close_turn(
                Uuid::new_v4(),
                NOW,
                TurnClosed {
                    turn_id,
                    outcome: TurnOutcome::Failed,
                    reason_code: "worker_join_failed".into(),
                    recovery_rule_version: None,
                },
            )
            .unwrap();
        let second_turn = Uuid::new_v4();
        authority
            .start_turn(Uuid::new_v4(), NOW, second_turn, second_prompt)
            .unwrap();
        drop(authority);

        let reopened = SessionAuthority::open(
            &session_path,
            "session-binding",
            "workspace-binding",
            "composition:legacy",
            ActorIdentity {
                principal: "operator".into(),
                ingress: "test".into(),
            },
            NOW,
        )
        .unwrap();
        assert_eq!(
            reopened
                .state()
                .active_turn
                .as_ref()
                .map(|turn| turn.turn_id),
            None,
            "recovery terminalizes only the deliberately unclosed second turn"
        );
        assert_eq!(
            reopened.state().closed_turns[&second_turn].outcome,
            TurnOutcome::Interrupted
        );
    }

    #[test]
    fn abnormal_terminalization_append_failure_leaves_step_open_and_blocks_turn_close() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (turn_id, _, request, _) = begin_joined_model_request(&mut authority);
        let authority = SessionAuthorityHandle::new(authority);
        authority.make_next_append_fail();

        assert!(
            authority
                .terminalize_active_semantic_step(
                    NOW,
                    SemanticTerminalization {
                        turn_id,
                        request_outcome: ModelRequestOutcome::ProviderFailed,
                        reason_code: "provider_failed".into(),
                        rule_version: 1,
                    },
                )
                .is_err()
        );
        assert!(matches!(
            authority.state().model_requests[&request.request_id],
            ModelRequestState::Open { .. }
        ));
        assert!(
            authority
                .close_turn(
                    Uuid::new_v4(),
                    NOW,
                    TurnClosed {
                        turn_id,
                        outcome: TurnOutcome::Failed,
                        reason_code: "provider_failed".into(),
                        recovery_rule_version: None,
                    },
                )
                .is_err()
        );
    }

    #[test]
    fn abandonment_is_recovery_only_and_requires_request_first() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (turn_id, step_id, request, _) = begin_joined_model_request(&mut authority);
        let late_chunk = assistant_chunk(
            &authority,
            Uuid::new_v4(),
            request.request_id,
            step_id,
            AssistantContentKind::Text,
            0,
            b"late",
        );
        let abandonment = StepAbandoned {
            step_id,
            turn_id,
            reason_code: "runtime_lost".into(),
            recovery_rule_version: 1,
        };
        let live = fact(
            "session-binding",
            authority.stream_id,
            authority.state().last_sequence + 1,
            SessionFactPayload::StepAbandoned(abandonment.clone()),
        );
        assert!(
            authority
                .state
                .apply(&live)
                .unwrap_err()
                .to_string()
                .contains("request closure first")
        );
        let recovered = recovery_facts(authority.state(), NOW).unwrap();
        let mut invalid = recovered[1].clone();
        invalid.event_id = Uuid::new_v4();
        let mut state_after_request = authority.state().clone();
        state_after_request.apply(&recovered[0]).unwrap();
        assert!(
            state_after_request
                .apply(&invalid)
                .unwrap_err()
                .to_string()
                .contains("deterministic recovery evidence")
        );
        assert!(matches!(
            recovered[1].payload,
            SessionFactPayload::StepAbandoned(ref value) if value == &abandonment
        ));
        state_after_request.apply(&recovered[1]).unwrap();
        let late = fact(
            "session-binding",
            authority.stream_id,
            state_after_request.last_sequence + 1,
            SessionFactPayload::AssistantContentAppended(late_chunk),
        );
        assert!(
            state_after_request
                .apply(&late)
                .unwrap_err()
                .to_string()
                .contains("no active step"),
            "assistant facts must be fenced after step abandonment"
        );
    }

    #[test]
    fn assistant_content_continuity_commit_close_and_store_reopen_happy_path() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (_, step_id, request, lease) = begin_joined_model_request(&mut authority);
        let message_id = Uuid::new_v4();
        let text = assistant_chunk(
            &authority,
            message_id,
            request.request_id,
            step_id,
            AssistantContentKind::Text,
            0,
            b"answer",
        );
        let thinking = assistant_chunk(
            &authority,
            message_id,
            request.request_id,
            step_id,
            AssistantContentKind::Thinking,
            0,
            b"ordinary disclosed reasoning",
        );
        authority
            .append_assistant_content(Uuid::new_v4(), NOW, text.clone())
            .unwrap();
        authority
            .append_assistant_content(Uuid::new_v4(), NOW, thinking.clone())
            .unwrap();

        let continuity_ref = authority
            .write_content(
                b"minimum provider continuation",
                "application/octet-stream",
                ProjectionClass::RestrictedContinuity,
            )
            .unwrap();
        let continuity = ProviderContinuityStored {
            continuity_id: Uuid::new_v4(),
            request_id: request.request_id,
            step_id,
            response_attempt_ordinal: 0,
            serving_provider_id: lease.serving_provider_id.clone(),
            serving_model_id: lease.serving_model_id.clone(),
            provider_contribution_generation_id: lease.contribution_generation_id.clone(),
            continuity_kind: ProviderContinuityKind::OpaqueProviderState,
            required_for: ProviderContinuityRequiredFor::NextRequest,
            restricted_required: RestrictedContinuityPolicy {
                allowed_kinds: vec![ProviderContinuityKind::OpaqueProviderState],
                max_blob_bytes: 1024,
            },
            content_ref: continuity_ref,
        };
        authority
            .store_provider_continuity(Uuid::new_v4(), NOW, continuity.clone())
            .unwrap();

        let commit = assistant_commit(
            message_id,
            request.request_id,
            step_id,
            &[
                (&text, b"answer"),
                (&thinking, b"ordinary disclosed reasoning"),
            ],
        );
        authority
            .commit_assistant_message(Uuid::new_v4(), NOW, commit.clone())
            .unwrap();
        authority
            .close_model_request(
                Uuid::new_v4(),
                NOW,
                ModelRequestClosed {
                    request_id: request.request_id,
                    step_id,
                    response_attempt_ordinal: 0,
                    outcome: ModelRequestOutcome::ResponseCompleted,
                    reason_code: "provider_done".into(),
                    recovery_rule_version: None,
                },
            )
            .unwrap();

        assert_eq!(authority.state().assistant_messages[&message_id], commit);
        assert_eq!(
            authority.state().provider_continuity[&continuity.continuity_id],
            continuity
        );
        assert!(
            authority
                .state()
                .assistant_chunks
                .values()
                .flatten()
                .all(|chunk| { chunk.content_ref.projection_class() == ProjectionClass::Default })
        );
        let expected = authority.state().clone();
        assert_eq!(authority.store.load().unwrap(), expected);
    }

    #[test]
    fn assistant_chunks_enforce_request_channel_order_size_utf8_and_projection() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (_, step_id, request, _) = begin_joined_model_request(&mut authority);
        let message_id = Uuid::new_v4();
        let before = authority.state().last_sequence;

        let out_of_order = assistant_chunk(
            &authority,
            message_id,
            request.request_id,
            step_id,
            AssistantContentKind::Text,
            1,
            b"late",
        );
        assert!(
            authority
                .append_assistant_content(Uuid::new_v4(), NOW, out_of_order)
                .unwrap_err()
                .to_string()
                .contains("ordinal 0")
        );

        let oversized = assistant_chunk(
            &authority,
            message_id,
            request.request_id,
            step_id,
            AssistantContentKind::Text,
            0,
            &vec![b'x'; MAX_ASSISTANT_CHUNK_BYTES as usize + 1],
        );
        assert!(
            authority
                .append_assistant_content(Uuid::new_v4(), NOW, oversized)
                .unwrap_err()
                .to_string()
                .contains("64 KiB")
        );

        let restricted = AssistantContentAppended {
            content_ref: authority
                .write_content(
                    b"not default",
                    "text/plain",
                    ProjectionClass::RestrictedContinuity,
                )
                .unwrap(),
            ..assistant_chunk(
                &authority,
                message_id,
                request.request_id,
                step_id,
                AssistantContentKind::Text,
                0,
                b"placeholder",
            )
        };
        assert!(
            authority
                .append_assistant_content(Uuid::new_v4(), NOW, restricted)
                .is_err()
        );

        let invalid_utf8 = AssistantContentAppended {
            content_ref: authority
                .write_content(&[0xff], "text/plain", ProjectionClass::Default)
                .unwrap(),
            ..assistant_chunk(
                &authority,
                message_id,
                request.request_id,
                step_id,
                AssistantContentKind::Thinking,
                0,
                b"placeholder",
            )
        };
        assert!(
            authority
                .append_assistant_content(Uuid::new_v4(), NOW, invalid_utf8)
                .unwrap_err()
                .to_string()
                .contains("UTF-8")
        );
        assert_eq!(authority.state().last_sequence, before);
    }

    #[test]
    fn eof_without_done_cannot_complete_and_commit_is_exact_and_bounded() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (_, step_id, request, _) = begin_joined_model_request(&mut authority);
        assert!(
            authority
                .close_model_request(
                    Uuid::new_v4(),
                    NOW,
                    ModelRequestClosed {
                        request_id: request.request_id,
                        step_id,
                        response_attempt_ordinal: 0,
                        outcome: ModelRequestOutcome::ResponseCompleted,
                        reason_code: "eof".into(),
                        recovery_rule_version: None,
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("provider completion evidence")
        );

        let message_id = Uuid::new_v4();
        let chunk = assistant_chunk(
            &authority,
            message_id,
            request.request_id,
            step_id,
            AssistantContentKind::Text,
            0,
            b"partial",
        );
        authority
            .append_assistant_content(Uuid::new_v4(), NOW, chunk.clone())
            .unwrap();
        let mut wrong = assistant_commit(
            message_id,
            request.request_id,
            step_id,
            &[(&chunk, b"partial")],
        );
        wrong.content[0].content_digest = fingerprint("wrong");
        assert!(
            authority
                .commit_assistant_message(Uuid::new_v4(), NOW, wrong)
                .unwrap_err()
                .to_string()
                .contains("digest")
        );
        let mut unbounded = assistant_commit(
            message_id,
            request.request_id,
            step_id,
            &[(&chunk, b"partial")],
        );
        unbounded.usage = Some(AssistantUsage {
            input_tokens: MAX_USAGE_TOKENS,
            output_tokens: 1,
        });
        assert!(
            authority
                .commit_assistant_message(Uuid::new_v4(), NOW, unbounded)
                .unwrap_err()
                .to_string()
                .contains("bounds")
        );
        let mut too_many_calls = assistant_commit(
            message_id,
            request.request_id,
            step_id,
            &[(&chunk, b"partial")],
        );
        too_many_calls.tool_call_count = MAX_MESSAGE_TOOL_CALLS + 1;
        assert!(
            authority
                .commit_assistant_message(Uuid::new_v4(), NOW, too_many_calls)
                .unwrap_err()
                .to_string()
                .contains("bounds")
        );

        authority
            .close_model_request(
                Uuid::new_v4(),
                NOW,
                ModelRequestClosed {
                    request_id: request.request_id,
                    step_id,
                    response_attempt_ordinal: 0,
                    outcome: ModelRequestOutcome::Eof,
                    reason_code: "transport_eof".into(),
                    recovery_rule_version: None,
                },
            )
            .unwrap();
        assert!(
            !authority
                .state()
                .request_message_commits
                .contains_key(&request.request_id)
        );
    }

    #[test]
    fn continuity_requires_restricted_policy_lineage_and_unique_request_kind() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (_, step_id, request, lease) = begin_joined_model_request(&mut authority);
        let restricted_ref = authority
            .write_content(
                b"continuation",
                "application/octet-stream",
                ProjectionClass::RestrictedContinuity,
            )
            .unwrap();
        let continuity = ProviderContinuityStored {
            continuity_id: Uuid::new_v4(),
            request_id: request.request_id,
            step_id,
            response_attempt_ordinal: 0,
            serving_provider_id: lease.serving_provider_id.clone(),
            serving_model_id: lease.serving_model_id.clone(),
            provider_contribution_generation_id: lease.contribution_generation_id.clone(),
            continuity_kind: ProviderContinuityKind::HiddenReasoning,
            required_for: ProviderContinuityRequiredFor::NextRequest,
            restricted_required: RestrictedContinuityPolicy {
                allowed_kinds: vec![ProviderContinuityKind::HiddenReasoning],
                max_blob_bytes: 64,
            },
            content_ref: restricted_ref,
        };

        let mut wrong_lineage = continuity.clone();
        wrong_lineage.serving_model_id = "another-model".into();
        assert!(
            authority
                .store_provider_continuity(Uuid::new_v4(), NOW, wrong_lineage)
                .unwrap_err()
                .to_string()
                .contains("lineage")
        );
        let mut undeclared = continuity.clone();
        undeclared.restricted_required.allowed_kinds = Vec::new();
        assert!(
            authority
                .store_provider_continuity(Uuid::new_v4(), NOW, undeclared)
                .unwrap_err()
                .to_string()
                .contains("restricted_required")
        );
        let mut default_ref = continuity.clone();
        default_ref.content_ref = authority
            .write_content(
                b"visible",
                "application/octet-stream",
                ProjectionClass::Default,
            )
            .unwrap();
        assert!(
            authority
                .store_provider_continuity(Uuid::new_v4(), NOW, default_ref)
                .is_err()
        );

        authority
            .store_provider_continuity(Uuid::new_v4(), NOW, continuity.clone())
            .unwrap();
        let mut duplicate_kind = continuity;
        duplicate_kind.continuity_id = Uuid::new_v4();
        assert!(
            authority
                .store_provider_continuity(Uuid::new_v4(), NOW, duplicate_kind)
                .unwrap_err()
                .to_string()
                .contains("already stored")
        );
    }

    #[test]
    fn repair_request_has_independent_chunk_ordinals_and_step_has_one_commit() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (turn_id, step_id, first, first_lease) = begin_joined_model_request(&mut authority);
        let first_message = Uuid::new_v4();
        let first_chunk = assistant_chunk(
            &authority,
            first_message,
            first.request_id,
            step_id,
            AssistantContentKind::Text,
            0,
            b"discarded partial",
        );
        authority
            .append_assistant_content(Uuid::new_v4(), NOW, first_chunk)
            .unwrap();
        let continuity_id = Uuid::new_v4();
        authority
            .store_provider_continuity(
                Uuid::new_v4(),
                NOW,
                ProviderContinuityStored {
                    continuity_id,
                    request_id: first.request_id,
                    step_id,
                    response_attempt_ordinal: 0,
                    serving_provider_id: first_lease.serving_provider_id,
                    serving_model_id: first_lease.serving_model_id,
                    provider_contribution_generation_id: first_lease.contribution_generation_id,
                    continuity_kind: ProviderContinuityKind::OpaqueProviderState,
                    required_for: ProviderContinuityRequiredFor::NextRequest,
                    restricted_required: RestrictedContinuityPolicy {
                        allowed_kinds: vec![ProviderContinuityKind::OpaqueProviderState],
                        max_blob_bytes: 64,
                    },
                    content_ref: authority
                        .write_content(
                            b"repair continuity",
                            "application/octet-stream",
                            ProjectionClass::RestrictedContinuity,
                        )
                        .unwrap(),
                },
            )
            .unwrap();
        authority
            .close_model_request(
                Uuid::new_v4(),
                NOW,
                ModelRequestClosed {
                    request_id: first.request_id,
                    step_id,
                    response_attempt_ordinal: 0,
                    outcome: ModelRequestOutcome::SupersededForHistoryRepair,
                    reason_code: "provider_history_repair".into(),
                    recovery_rule_version: None,
                },
            )
            .unwrap();
        let mut second = model_request(
            &authority,
            step_id,
            turn_id,
            1,
            ModelRequestPurpose::ProviderHistoryRepair,
            Some(first.request_id),
        );
        second.continuity_refs = vec![continuity_id];
        authority
            .prepare_model_request(Uuid::new_v4(), NOW, second.clone())
            .unwrap();
        let mut second_lease = route_lease(turn_id);
        second_lease.request_id = second.request_id;
        authority
            .record_route_lease(NOW, second_lease.clone())
            .unwrap();
        authority
            .join_model_request_route(
                Uuid::new_v4(),
                NOW,
                ModelRequestRouteJoined {
                    request_id: second.request_id,
                    step_id,
                    turn_id,
                    lease_id: second_lease.lease_id,
                },
            )
            .unwrap();

        let second_message = Uuid::new_v4();
        let second_chunk = assistant_chunk(
            &authority,
            second_message,
            second.request_id,
            step_id,
            AssistantContentKind::Text,
            0,
            b"repaired answer",
        );
        authority
            .append_assistant_content(Uuid::new_v4(), NOW, second_chunk.clone())
            .unwrap();
        let commit = assistant_commit(
            second_message,
            second.request_id,
            step_id,
            &[(&second_chunk, b"repaired answer")],
        );
        authority
            .commit_assistant_message(Uuid::new_v4(), NOW, commit.clone())
            .unwrap();
        assert!(
            authority
                .commit_assistant_message(Uuid::new_v4(), NOW, commit)
                .unwrap_err()
                .to_string()
                .contains("already committed")
        );
        assert_eq!(
            authority.state().assistant_chunks[&first.request_id][0].chunk_ordinal,
            0
        );
        assert_eq!(
            authority.state().assistant_chunks[&second.request_id][0].chunk_ordinal,
            0
        );
    }

    #[test]
    fn assistant_facts_are_strict_append_atomic_and_tampered_blobs_fail_replay() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (_, step_id, request, _) = begin_joined_model_request(&mut authority);
        let message_id = Uuid::new_v4();
        let chunk = assistant_chunk(
            &authority,
            message_id,
            request.request_id,
            step_id,
            AssistantContentKind::Text,
            0,
            b"durable answer",
        );
        let wire = fact(
            "session-binding",
            authority.stream_id,
            authority.state().last_sequence + 1,
            SessionFactPayload::AssistantContentAppended(chunk.clone()),
        );
        assert_eq!(SessionFact::decode(&wire.encode().unwrap()).unwrap(), wire);
        let mut unknown: Value = serde_json::from_slice(&wire.encode().unwrap()).unwrap();
        unknown["payload"]["raw_provider_payload"] = serde_json::json!({"forbidden": true});
        assert!(SessionFact::decode(&serde_json::to_vec(&unknown).unwrap()).is_err());
        let mut missing_done: Value = serde_json::to_value(assistant_commit(
            message_id,
            request.request_id,
            step_id,
            &[(&chunk, b"durable answer")],
        ))
        .unwrap();
        missing_done
            .as_object_mut()
            .unwrap()
            .remove("completion_evidence");
        assert!(serde_json::from_value::<AssistantMessageCommitted>(missing_done).is_err());

        let before = authority.state().clone();
        let log_path = authority.store.log_path.clone();
        let mut permissions = fs::metadata(&log_path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&log_path, permissions).unwrap();
        assert!(
            authority
                .append_assistant_content(Uuid::new_v4(), NOW, chunk.clone())
                .is_err()
        );
        assert_eq!(authority.state(), &before);
        let mut permissions = fs::metadata(&log_path).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o600);
        }
        #[cfg(not(unix))]
        permissions.set_readonly(false);
        fs::set_permissions(&log_path, permissions).unwrap();
        authority
            .append_assistant_content(Uuid::new_v4(), NOW, chunk.clone())
            .unwrap();

        let blob_path = directory
            .path()
            .join("session.authority.blobs")
            .join(chunk.content_ref.storage_reference().as_relative_path());
        fs::write(blob_path, b"tampered answer").unwrap();
        assert!(
            authority
                .store
                .load()
                .unwrap_err()
                .to_string()
                .contains("blob")
        );
    }

    #[test]
    fn model_request_happy_ordering_and_repair_stay_in_one_step() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (_, turn_id) = begin_test_turn(&mut authority);
        let step_id = Uuid::new_v4();
        let start = StepStarted {
            step_id,
            turn_id,
            step_ordinal: 0,
        };
        authority
            .start_step(Uuid::new_v4(), NOW, start.clone())
            .unwrap();
        let first = model_request(
            &authority,
            step_id,
            turn_id,
            0,
            ModelRequestPurpose::Initial,
            None,
        );
        let prepare_command = Uuid::new_v4();
        assert!(
            authority
                .prepare_model_request(prepare_command, NOW, first.clone())
                .unwrap()
        );
        assert!(
            !authority
                .prepare_model_request(prepare_command, NOW, first.clone())
                .unwrap()
        );
        let mut lease = route_lease(turn_id);
        lease.request_id = first.request_id;
        authority.record_route_lease(NOW, lease.clone()).unwrap();
        let join = ModelRequestRouteJoined {
            request_id: first.request_id,
            step_id,
            turn_id,
            lease_id: lease.lease_id,
        };
        authority
            .join_model_request_route(Uuid::new_v4(), NOW, join.clone())
            .unwrap();
        authority
            .close_model_request(
                Uuid::new_v4(),
                NOW,
                ModelRequestClosed {
                    request_id: first.request_id,
                    step_id,
                    response_attempt_ordinal: 0,
                    outcome: ModelRequestOutcome::SupersededForContextRepair,
                    reason_code: "context_overflow".into(),
                    recovery_rule_version: None,
                },
            )
            .unwrap();
        let second = model_request(
            &authority,
            step_id,
            turn_id,
            1,
            ModelRequestPurpose::ContextOverflowRepair,
            Some(first.request_id),
        );
        authority
            .prepare_model_request(Uuid::new_v4(), NOW, second.clone())
            .unwrap();
        authority
            .close_model_request(
                Uuid::new_v4(),
                NOW,
                ModelRequestClosed {
                    request_id: second.request_id,
                    step_id,
                    response_attempt_ordinal: 0,
                    outcome: ModelRequestOutcome::ProviderFailed,
                    reason_code: "provider_failed".into(),
                    recovery_rule_version: None,
                },
            )
            .unwrap();

        let state = authority.state();
        assert_eq!(state.steps[&step_id], start);
        assert_eq!(state.request_route_joins[&first.request_id], join);
        assert_eq!(state.joined_route_leases[&lease.lease_id], first.request_id);
        assert_eq!(
            state.model_requests[&second.request_id]
                .preparation()
                .schema_set,
            second.schema_set
        );
        assert_eq!(state.active_step.as_ref().unwrap().next_request_ordinal, 2);
        assert!(
            state
                .active_step
                .as_ref()
                .unwrap()
                .active_request_id
                .is_none()
        );
    }

    #[test]
    fn step_and_request_identity_ordinals_and_turn_scope_are_strict() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (_, turn_id) = begin_test_turn(&mut authority);
        let before = authority.state().last_sequence;
        let wrong_step = StepStarted {
            step_id: Uuid::new_v4(),
            turn_id,
            step_ordinal: 1,
        };
        assert!(
            authority
                .start_step(Uuid::new_v4(), NOW, wrong_step)
                .unwrap_err()
                .to_string()
                .contains("expected step ordinal 0")
        );
        assert_eq!(authority.state().last_sequence, before);

        let step_id = Uuid::new_v4();
        authority
            .start_step(
                Uuid::new_v4(),
                NOW,
                StepStarted {
                    step_id,
                    turn_id,
                    step_ordinal: 0,
                },
            )
            .unwrap();
        assert!(
            authority
                .start_step(
                    Uuid::new_v4(),
                    NOW,
                    StepStarted {
                        step_id: Uuid::new_v4(),
                        turn_id,
                        step_ordinal: 1,
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("already active")
        );
        let mut wrong_turn = model_request(
            &authority,
            step_id,
            turn_id,
            0,
            ModelRequestPurpose::Initial,
            None,
        );
        wrong_turn.turn_id = Uuid::new_v4();
        assert!(
            authority
                .prepare_model_request(Uuid::new_v4(), NOW, wrong_turn)
                .unwrap_err()
                .to_string()
                .contains("stale turn")
        );
        let mut wrong_ordinal = model_request(
            &authority,
            step_id,
            turn_id,
            1,
            ModelRequestPurpose::Initial,
            None,
        );
        assert!(
            authority
                .prepare_model_request(Uuid::new_v4(), NOW, wrong_ordinal.clone())
                .unwrap_err()
                .to_string()
                .contains("expected request ordinal 0")
        );
        wrong_ordinal.request_ordinal = 0;
        authority
            .prepare_model_request(Uuid::new_v4(), NOW, wrong_ordinal.clone())
            .unwrap();
        authority
            .close_model_request(
                Uuid::new_v4(),
                NOW,
                ModelRequestClosed {
                    request_id: wrong_ordinal.request_id,
                    step_id,
                    response_attempt_ordinal: 0,
                    outcome: ModelRequestOutcome::ProviderFailed,
                    reason_code: "failed".into(),
                    recovery_rule_version: None,
                },
            )
            .unwrap();
        let mut duplicate = model_request(
            &authority,
            step_id,
            turn_id,
            1,
            ModelRequestPurpose::ContextOverflowRepair,
            Some(wrong_ordinal.request_id),
        );
        duplicate.request_id = wrong_ordinal.request_id;
        assert!(
            authority
                .prepare_model_request(Uuid::new_v4(), NOW, duplicate)
                .unwrap_err()
                .to_string()
                .contains("already present")
        );
    }

    #[test]
    fn request_manifests_refs_and_route_join_identity_are_validated_before_append() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (_, turn_id) = begin_test_turn(&mut authority);
        let step_id = Uuid::new_v4();
        authority
            .start_step(
                Uuid::new_v4(),
                NOW,
                StepStarted {
                    step_id,
                    turn_id,
                    step_ordinal: 0,
                },
            )
            .unwrap();
        let baseline = authority.state().last_sequence;
        let mut wrong_manifest = model_request(
            &authority,
            step_id,
            turn_id,
            0,
            ModelRequestPurpose::Initial,
            None,
        );
        wrong_manifest.context_manifest_id = fingerprint("wrong");
        assert!(
            authority
                .prepare_model_request(Uuid::new_v4(), NOW, wrong_manifest)
                .unwrap_err()
                .to_string()
                .contains("context manifest")
        );
        assert_eq!(authority.state().last_sequence, baseline);

        let mut missing = model_request(
            &authority,
            step_id,
            turn_id,
            0,
            ModelRequestPurpose::Initial,
            None,
        );
        let mut missing_value =
            serde_json::to_value(&missing.context_items[0].content_ref).unwrap();
        missing_value["digest"] = Value::String(fingerprint("missing-blob"));
        missing.context_items[0].content_ref = serde_json::from_value(missing_value).unwrap();
        missing.context_manifest_id = canonical_sha256(&missing.context_items).unwrap();
        assert!(
            authority
                .prepare_model_request(Uuid::new_v4(), NOW, missing)
                .unwrap_err()
                .to_string()
                .contains("blob")
        );
        assert_eq!(authority.state().last_sequence, baseline);

        let mut restricted = model_request(
            &authority,
            step_id,
            turn_id,
            0,
            ModelRequestPurpose::Initial,
            None,
        );
        restricted.context_items[0].content_ref = authority
            .write_content(
                b"restricted",
                "text/plain",
                ProjectionClass::RestrictedContinuity,
            )
            .unwrap();
        restricted.context_manifest_id = canonical_sha256(&restricted.context_items).unwrap();
        assert!(
            authority
                .prepare_model_request(Uuid::new_v4(), NOW, restricted)
                .unwrap_err()
                .to_string()
                .contains("projection class")
        );

        let request = model_request(
            &authority,
            step_id,
            turn_id,
            0,
            ModelRequestPurpose::Initial,
            None,
        );
        authority
            .prepare_model_request(Uuid::new_v4(), NOW, request.clone())
            .unwrap();
        let mut lease = route_lease(turn_id);
        lease.request_id = Uuid::new_v4();
        authority.record_route_lease(NOW, lease.clone()).unwrap();
        assert!(
            authority
                .join_model_request_route(
                    Uuid::new_v4(),
                    NOW,
                    ModelRequestRouteJoined {
                        request_id: request.request_id,
                        step_id,
                        turn_id,
                        lease_id: lease.lease_id,
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("contradicts")
        );
    }

    #[test]
    fn request_close_outcomes_cardinality_and_turn_gate_are_strict() {
        for outcome in [
            ModelRequestOutcome::ProviderFailed,
            ModelRequestOutcome::Eof,
            ModelRequestOutcome::Cancelled,
            ModelRequestOutcome::TimedOut,
            ModelRequestOutcome::Revoked,
            ModelRequestOutcome::Unknown,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let mut authority = test_authority(&directory);
            let (_, turn_id) = begin_test_turn(&mut authority);
            let step_id = Uuid::new_v4();
            authority
                .start_step(
                    Uuid::new_v4(),
                    NOW,
                    StepStarted {
                        step_id,
                        turn_id,
                        step_ordinal: 0,
                    },
                )
                .unwrap();
            let request = model_request(
                &authority,
                step_id,
                turn_id,
                0,
                ModelRequestPurpose::Initial,
                None,
            );
            authority
                .prepare_model_request(Uuid::new_v4(), NOW, request.clone())
                .unwrap();
            let closure = ModelRequestClosed {
                request_id: request.request_id,
                step_id,
                response_attempt_ordinal: 0,
                outcome,
                reason_code: "classified".into(),
                recovery_rule_version: None,
            };
            authority
                .close_model_request(Uuid::new_v4(), NOW, closure.clone())
                .unwrap();
            assert!(
                authority
                    .close_model_request(Uuid::new_v4(), NOW, closure)
                    .is_err()
            );
            assert!(
                authority
                    .close_turn(
                        Uuid::new_v4(),
                        NOW,
                        TurnClosed {
                            turn_id,
                            outcome: TurnOutcome::Failed,
                            reason_code: "failed".into(),
                            recovery_rule_version: None,
                        },
                    )
                    .unwrap_err()
                    .to_string()
                    .contains("active step")
            );

            let mut constructed = authority.state().clone();
            constructed.terminalize_active_step_for_test();
            constructed
                .apply(&fact(
                    "session-binding",
                    authority.stream_id,
                    constructed.last_sequence + 1,
                    SessionFactPayload::TurnClosed(TurnClosed {
                        turn_id,
                        outcome: TurnOutcome::Failed,
                        reason_code: "failed".into(),
                        recovery_rule_version: None,
                    }),
                ))
                .unwrap();
        }

        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (_, turn_id) = begin_test_turn(&mut authority);
        let step_id = Uuid::new_v4();
        authority
            .start_step(
                Uuid::new_v4(),
                NOW,
                StepStarted {
                    step_id,
                    turn_id,
                    step_ordinal: 0,
                },
            )
            .unwrap();
        let request = model_request(
            &authority,
            step_id,
            turn_id,
            0,
            ModelRequestPurpose::Initial,
            None,
        );
        authority
            .prepare_model_request(Uuid::new_v4(), NOW, request.clone())
            .unwrap();
        assert!(
            authority
                .close_model_request(
                    Uuid::new_v4(),
                    NOW,
                    ModelRequestClosed {
                        request_id: request.request_id,
                        step_id,
                        response_attempt_ordinal: 0,
                        outcome: ModelRequestOutcome::ResponseCompleted,
                        reason_code: "completed".into(),
                        recovery_rule_version: None,
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("assistant message")
        );
        assert!(
            authority
                .close_model_request(
                    Uuid::new_v4(),
                    NOW,
                    ModelRequestClosed {
                        request_id: request.request_id,
                        step_id,
                        response_attempt_ordinal: 0,
                        outcome: ModelRequestOutcome::Abandoned,
                        reason_code: "runtime_lost".into(),
                        recovery_rule_version: Some(1),
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("live commands")
        );
    }

    #[test]
    fn model_request_wire_is_strict_and_append_failure_preserves_state() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let (_, turn_id) = begin_test_turn(&mut authority);
        let step_id = Uuid::new_v4();
        authority
            .start_step(
                Uuid::new_v4(),
                NOW,
                StepStarted {
                    step_id,
                    turn_id,
                    step_ordinal: 0,
                },
            )
            .unwrap();
        let request = model_request(
            &authority,
            step_id,
            turn_id,
            0,
            ModelRequestPurpose::Initial,
            None,
        );
        let wire = fact(
            "session-binding",
            authority.stream_id,
            authority.state().last_sequence + 1,
            SessionFactPayload::ModelRequestPrepared(request.clone()),
        );
        assert_eq!(SessionFact::decode(&wire.encode().unwrap()).unwrap(), wire);
        let mut unknown: Value = serde_json::from_slice(&wire.encode().unwrap()).unwrap();
        unknown["payload"]["unexpected"] = Value::Bool(true);
        assert!(SessionFact::decode(&serde_json::to_vec(&unknown).unwrap()).is_err());
        let mut noncanonical: Value = serde_json::from_slice(&wire.encode().unwrap()).unwrap();
        noncanonical["payload"]["request_id"] =
            Value::String(request.request_id.to_string().to_ascii_uppercase());
        assert!(SessionFact::decode(&serde_json::to_vec(&noncanonical).unwrap()).is_err());

        let before = authority.state().clone();
        let log_path = authority.store.log_path.clone();
        let mut permissions = fs::metadata(&log_path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&log_path, permissions).unwrap();
        assert!(
            authority
                .prepare_model_request(Uuid::new_v4(), NOW, request)
                .is_err()
        );
        assert_eq!(authority.state(), &before);
        let mut permissions = fs::metadata(&log_path).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o600);
        }
        #[cfg(not(unix))]
        permissions.set_readonly(false);
        fs::set_permissions(&log_path, permissions).unwrap();
    }

    #[test]
    fn model_request_blob_tampering_blocks_reopen_and_v3_cache_rebuilds_without_facts() {
        let directory = tempfile::tempdir().unwrap();
        let session_path = directory.path().join("session.json");
        let content_ref;
        {
            let mut authority = test_authority(&directory);
            let (_, turn_id) = begin_test_turn(&mut authority);
            let step_id = Uuid::new_v4();
            authority
                .start_step(
                    Uuid::new_v4(),
                    NOW,
                    StepStarted {
                        step_id,
                        turn_id,
                        step_ordinal: 0,
                    },
                )
                .unwrap();
            let request = model_request(
                &authority,
                step_id,
                turn_id,
                0,
                ModelRequestPurpose::Initial,
                None,
            );
            content_ref = request.context_items[0].content_ref.clone();
            authority
                .prepare_model_request(Uuid::new_v4(), NOW, request)
                .unwrap();
        }
        let blob_path = directory
            .path()
            .join("session.authority.blobs")
            .join(content_ref.storage_reference().as_relative_path());
        fs::write(blob_path, b"tampered provider context").unwrap();
        let error = SessionAuthority::open(
            &session_path,
            "session-binding",
            "workspace-binding",
            "composition:legacy",
            ActorIdentity {
                principal: "system".into(),
                ingress: "resume".into(),
            },
            NOW,
        )
        .unwrap_err();
        assert!(error.to_string().contains("blob"));

        let legacy = tempfile::tempdir().unwrap();
        let legacy_path = legacy.path().join("session.json");
        {
            let authority = test_authority(&legacy);
            assert_eq!(read_facts(&authority.store.log_path).unwrap().len(), 1);
        }
        let snapshot_path = legacy.path().join("session.authority.snapshot.json");
        let mut snapshot: Value =
            serde_json::from_slice(&fs::read(&snapshot_path).unwrap()).unwrap();
        snapshot["snapshot_version"] = Value::from(3);
        snapshot["reducer_version"] = Value::from(3);
        fs::write(&snapshot_path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        {
            let authority = SessionAuthority::open(
                &legacy_path,
                "session-binding",
                "workspace-binding",
                "composition:legacy",
                ActorIdentity {
                    principal: "system".into(),
                    ingress: "resume".into(),
                },
                NOW,
            )
            .unwrap();
            assert!(authority.state().steps.is_empty());
            assert!(authority.state().model_requests.is_empty());
            assert_eq!(read_facts(&authority.store.log_path).unwrap().len(), 1);
        }
        let rebuilt: Value = serde_json::from_slice(&fs::read(snapshot_path).unwrap()).unwrap();
        assert_eq!(rebuilt["snapshot_version"], 5);
        assert_eq!(rebuilt["reducer_version"], 5);
    }

    #[test]
    fn idle_execution_binding_migration_is_atomic_and_durable() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let from = execution_binding("v1");
        let target = execution_binding("v2");
        authority.bind_execution_at_boot(from.clone()).unwrap();

        assert!(
            authority
                .migrate_execution_binding(Uuid::new_v4(), NOW, from.clone(), target.clone())
                .unwrap()
        );
        assert_eq!(
            authority.state().execution_binding_generation.as_ref(),
            Some(&target)
        );
        assert_eq!(authority.boot_execution_binding.as_ref(), Some(&target));
        assert_eq!(authority.state().last_sequence, 2);

        let facts = read_facts(&authority.store.log_path).unwrap();
        assert!(matches!(
            &facts[1].payload,
            SessionFactPayload::ExecutionBindingMigrated(migration)
                if migration.from_generation == from && migration.target_generation == target
        ));
    }

    #[test]
    fn execution_binding_migration_rejects_an_active_turn() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let from = execution_binding("v1");
        authority.bind_execution_at_boot(from.clone()).unwrap();
        let prompt_id = Uuid::new_v4();
        authority
            .admit_prompt(
                Uuid::new_v4(),
                NOW,
                PromptAdmitted {
                    submission_id: Uuid::new_v4(),
                    prompt_id,
                    principal: "operator".into(),
                    ingress: "test".into(),
                    queue_mode: QueueMode::UntilReady,
                    content: PromptContent {
                        text: "work".into(),
                        attachments: Vec::new(),
                    },
                    metadata: serde_json::json!({}),
                },
            )
            .unwrap();
        authority
            .start_turn(Uuid::new_v4(), NOW, Uuid::new_v4(), prompt_id)
            .unwrap();
        let before = authority.state().last_sequence;

        let mut replayed = authority.state().clone();
        let replay_error = replayed
            .apply(&fact(
                "session-binding",
                authority.stream_id,
                before + 1,
                SessionFactPayload::ExecutionBindingMigrated(ExecutionBindingMigrated {
                    from_generation: from.clone(),
                    target_generation: execution_binding("v2"),
                }),
            ))
            .unwrap_err();
        assert!(replay_error.to_string().contains("active turn"));

        let error = authority
            .migrate_execution_binding(Uuid::new_v4(), NOW, from, execution_binding("v2"))
            .unwrap_err();
        assert!(error.to_string().contains("active turn"));
        assert_eq!(authority.state().last_sequence, before);
    }

    #[test]
    fn execution_binding_migration_rejects_every_unresolved_invocation_class() {
        let turn_id = Uuid::new_v4();
        let registration = InvocationRegistered {
            invocation_id: Uuid::new_v4(),
            turn_id,
            call_id: "legacy-call".into(),
            owner_generation_id: None,
        };
        let preparation = prepared(turn_id, "durable-call");
        let dispatch = InvocationDispatched {
            invocation_id: preparation.invocation_id,
            lease_id: preparation.lease_id,
        };
        let acknowledgement = InvocationAcknowledged {
            invocation_id: preparation.invocation_id,
            lease_id: preparation.lease_id,
        };
        let classification = InvocationClassifiedUnknown {
            invocation_id: preparation.invocation_id,
            reason_code: "runtime_lost".into(),
            recovery_rule_version: 2,
        };
        let states = vec![
            (
                "registered",
                registration.invocation_id,
                InvocationState::Registered {
                    registration: registration.clone(),
                },
            ),
            (
                "prepared",
                preparation.invocation_id,
                InvocationState::Prepared {
                    preparation: preparation.clone(),
                },
            ),
            (
                "dispatched",
                preparation.invocation_id,
                InvocationState::Dispatched {
                    preparation: preparation.clone(),
                    dispatch: dispatch.clone(),
                },
            ),
            (
                "acknowledged",
                preparation.invocation_id,
                InvocationState::Acknowledged {
                    preparation: preparation.clone(),
                    dispatch: dispatch.clone(),
                    acknowledgement: acknowledgement.clone(),
                },
            ),
            (
                "legacy_unknown",
                registration.invocation_id,
                InvocationState::Unknown {
                    registration: registration.clone(),
                    classification: InvocationClassifiedUnknown {
                        invocation_id: registration.invocation_id,
                        reason_code: "runtime_lost".into(),
                        recovery_rule_version: 1,
                    },
                },
            ),
            (
                "durable_unknown",
                preparation.invocation_id,
                InvocationState::DurableUnknown {
                    preparation: preparation.clone(),
                    dispatch,
                    acknowledgement: Some(acknowledgement),
                    classification,
                },
            ),
        ];

        for (name, invocation_id, state) in states {
            let directory = tempfile::tempdir().unwrap();
            let mut authority = test_authority(&directory);
            let from = execution_binding("v1");
            authority.bind_execution_at_boot(from.clone()).unwrap();
            authority.state.invocations.insert(invocation_id, state);

            let mut replayed = authority.state().clone();
            let replay_error = replayed
                .apply(&fact(
                    "session-binding",
                    authority.stream_id,
                    2,
                    SessionFactPayload::ExecutionBindingMigrated(ExecutionBindingMigrated {
                        from_generation: from.clone(),
                        target_generation: execution_binding("v2"),
                    }),
                ))
                .unwrap_err();
            assert!(
                replay_error.to_string().contains("unresolved invocation"),
                "{name} replay: {replay_error}"
            );

            let error = authority
                .migrate_execution_binding(Uuid::new_v4(), NOW, from, execution_binding("v2"))
                .unwrap_err();
            assert!(
                error.to_string().contains("unresolved invocation"),
                "{name}: {error}"
            );
            assert_eq!(authority.state().last_sequence, 1, "{name}");
        }
    }

    #[test]
    fn execution_binding_migration_rejects_stale_source() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let from = execution_binding("v1");
        let current = execution_binding("v2");
        authority.bind_execution_at_boot(from.clone()).unwrap();
        authority
            .migrate_execution_binding(Uuid::new_v4(), NOW, from.clone(), current)
            .unwrap();

        let error = authority
            .migrate_execution_binding(Uuid::new_v4(), NOW, from.clone(), execution_binding("v3"))
            .unwrap_err();
        assert!(error.to_string().contains("source is stale"));
        assert_eq!(authority.state().last_sequence, 2);

        let mut replayed = authority.state().clone();
        let replay_error = replayed
            .apply(&fact(
                "session-binding",
                authority.stream_id,
                3,
                SessionFactPayload::ExecutionBindingMigrated(ExecutionBindingMigrated {
                    from_generation: from,
                    target_generation: execution_binding("v3"),
                }),
            ))
            .unwrap_err();
        assert!(replay_error.to_string().contains("source is stale"));
    }

    #[test]
    fn execution_binding_migration_command_reuse_is_idempotent_or_conflicting() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        let from = execution_binding("v1");
        let target = execution_binding("v2");
        let command_id = Uuid::new_v4();
        authority.bind_execution_at_boot(from.clone()).unwrap();

        assert!(
            authority
                .migrate_execution_binding(command_id, NOW, from.clone(), target.clone(),)
                .unwrap()
        );
        assert!(
            !authority
                .migrate_execution_binding(command_id, NOW, from.clone(), target)
                .unwrap()
        );
        let error = authority
            .migrate_execution_binding(command_id, NOW, from, execution_binding("conflict"))
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("conflicting event or fingerprint")
        );
        assert_eq!(authority.state().last_sequence, 2);
    }

    #[test]
    fn execution_binding_migration_replays_and_boot_binding_does_not_append() {
        let directory = tempfile::tempdir().unwrap();
        let target = execution_binding("v2");
        {
            let mut authority = test_authority(&directory);
            let from = execution_binding("v1");
            authority.bind_execution_at_boot(from.clone()).unwrap();
            authority
                .migrate_execution_binding(Uuid::new_v4(), NOW, from, target.clone())
                .unwrap();
        }

        let mut reopened = SessionAuthority::open(
            &directory.path().join("session.json"),
            "session-binding",
            "workspace-binding",
            "composition:another-process",
            ActorIdentity {
                principal: "system".into(),
                ingress: "resume".into(),
            },
            "2026-08-19T19:00:00Z",
        )
        .unwrap();
        assert_eq!(
            reopened.state().execution_binding_generation.as_ref(),
            Some(&target)
        );
        assert_eq!(reopened.state().last_sequence, 2);
        reopened.bind_execution_at_boot(target).unwrap();
        assert_eq!(reopened.state().last_sequence, 2);
        assert_eq!(read_facts(&reopened.store.log_path).unwrap().len(), 2);
    }

    #[test]
    fn legacy_stream_has_no_fabricated_execution_migration() {
        let directory = tempfile::tempdir().unwrap();
        let mut authority = test_authority(&directory);
        assert!(authority.state().execution_binding_generation.is_none());
        assert_eq!(authority.state().last_sequence, 1);

        authority
            .bind_execution_at_boot(execution_binding("boot"))
            .unwrap();
        assert!(authority.state().execution_binding_generation.is_none());
        assert_eq!(authority.state().last_sequence, 1);
        assert_eq!(read_facts(&authority.store.log_path).unwrap().len(), 1);
    }

    #[test]
    fn execution_binding_generation_ids_are_strictly_decoded() {
        let stream_id = Uuid::new_v4();
        let original = fact(
            "session-binding",
            stream_id,
            2,
            SessionFactPayload::ExecutionBindingMigrated(ExecutionBindingMigrated {
                from_generation: execution_binding("v1"),
                target_generation: execution_binding("v2"),
            }),
        );
        let encoded = original.encode().unwrap();
        assert_eq!(SessionFact::decode(&encoded).unwrap(), original);

        let mut malformed: Value = serde_json::from_slice(&encoded).unwrap();
        malformed["payload"]["target_generation"]["driver_generation_id"] =
            Value::String("missing-namespace".into());
        assert!(SessionFact::decode(&serde_json::to_vec(&malformed).unwrap()).is_err());

        let mut incomplete: Value = serde_json::from_slice(&encoded).unwrap();
        incomplete["payload"]["target_generation"]
            .as_object_mut()
            .unwrap()
            .remove("provider_route_service_generation_id");
        assert!(SessionFact::decode(&serde_json::to_vec(&incomplete).unwrap()).is_err());
    }

    fn mutation_fence_evidence() -> InvocationMutationFenceEvidence {
        InvocationMutationFenceEvidence::new(InvocationMutationFenceEvidenceDraft {
            mutation_domain: RuntimeMutationDomainId::new("workspace:runtime").unwrap(),
            fence_key: RuntimeMutationFenceKey::new("capability:write").unwrap(),
            invocation_id: Uuid::new_v4(),
            call_id: "call-write".into(),
            capability_id: RuntimeCapabilityId::new("tool:write").unwrap(),
            owner_contribution_id: RuntimeContributionId::new("feature:writer").unwrap(),
            owner_generation_id: RuntimeContributionGenerationId::new("contribution:writer-v1")
                .unwrap(),
            issue_generation_id: RuntimeCompositionGenerationId::new("composition:test").unwrap(),
            lease_id: Uuid::new_v4(),
            session_id: "session-1".into(),
            turn_id: Uuid::new_v4(),
            failure_phase: InvocationFenceFailurePhase::TerminalSettlement,
            recorded_at: "2026-08-20T12:00:00Z".into(),
            failure_reason: "authority append failed".into(),
        })
        .unwrap()
    }

    #[test]
    fn emergency_mutation_fence_is_durable_idempotent_and_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionAuthorityStore::from_paths(
            directory.path().join("session.authority.jsonl"),
            directory.path().join("session.authority.snapshot.json"),
        );
        let evidence = mutation_fence_evidence();
        store.record_mutation_fence(&evidence).unwrap();
        store.record_mutation_fence(&evidence).unwrap();
        assert_eq!(
            store
                .active_mutation_fence(&evidence.mutation_domain, &evidence.fence_key)
                .unwrap(),
            Some(evidence.clone())
        );

        let reopened = SessionAuthorityStore::from_paths(
            directory.path().join("session.authority.jsonl"),
            directory.path().join("session.authority.snapshot.json"),
        );
        assert_eq!(
            reopened
                .active_mutation_fence(&evidence.mutation_domain, &evidence.fence_key)
                .unwrap(),
            Some(evidence.clone())
        );

        fs::write(
            reopened.emergency_fence_dir.join("malformed.json"),
            b"not-json",
        )
        .unwrap();
        assert!(
            reopened
                .active_mutation_fence(&evidence.mutation_domain, &evidence.fence_key)
                .is_err()
        );
    }

    #[test]
    fn failed_emergency_fence_write_poisons_runtime_mutation_admission() {
        let directory = tempfile::tempdir().unwrap();
        let authority = SessionAuthorityHandle::new(
            SessionAuthority::open(
                &directory.path().join("session.json"),
                "session-1",
                "workspace-1",
                "composition:test",
                ActorIdentity {
                    principal: "operator".into(),
                    ingress: "test".into(),
                },
                "2026-08-20T12:00:00Z",
            )
            .unwrap(),
        );
        let fence_dir = directory.path().join("invocation-mutation-fences");
        fs::remove_dir(&fence_dir).unwrap();
        fs::write(&fence_dir, b"not-a-directory").unwrap();
        let evidence = mutation_fence_evidence();

        assert!(authority.record_mutation_fence(&evidence).is_err());
        assert!(
            authority
                .active_mutation_fence(&evidence.mutation_domain, &evidence.fence_key)
                .is_err()
        );
    }

    #[test]
    fn unknown_retry_safety_uses_the_original_mutation_contract() {
        fn durable_unknown(preparation: InvocationPrepared) -> SessionAuthorityState {
            let invocation_id = preparation.invocation_id;
            let lease_id = preparation.lease_id;
            let mut state = SessionAuthorityState::default();
            state.invocations.insert(
                invocation_id,
                InvocationState::DurableUnknown {
                    preparation,
                    dispatch: InvocationDispatched {
                        invocation_id,
                        lease_id,
                    },
                    acknowledgement: None,
                    classification: InvocationClassifiedUnknown {
                        invocation_id,
                        reason_code: "runtime_loss_after_dispatch".into(),
                        recovery_rule_version: 2,
                    },
                },
            );
            state
        }

        let turn_id = Uuid::new_v4();
        let mut unsafe_preparation = prepared(turn_id, "stable-call");
        unsafe_preparation
            .admitted_effects
            .push(RuntimeEffect::FilesystemWrite);
        unsafe_preparation.execution.idempotency = omegon_traits::RuntimeIdempotency::NonIdempotent;
        unsafe_preparation.execution.deduplication =
            omegon_traits::RuntimeDeduplication::Unsupported;
        unsafe_preparation.deduplication_id = None;
        let unsafe_id = unsafe_preparation.invocation_id;
        assert_eq!(
            durable_unknown(unsafe_preparation.clone())
                .unknown_retry_disposition("stable-call")
                .unwrap(),
            UnknownRetryDisposition::Unsafe {
                invocation_id: unsafe_id
            }
        );

        let mut idempotent = unsafe_preparation.clone();
        idempotent.execution.idempotency = omegon_traits::RuntimeIdempotency::Idempotent;
        assert_eq!(
            durable_unknown(idempotent)
                .unknown_retry_disposition("stable-call")
                .unwrap(),
            UnknownRetryDisposition::Safe {
                invocation_id: unsafe_id
            }
        );

        let mut deduplicated = unsafe_preparation.clone();
        deduplicated.execution.deduplication =
            omegon_traits::RuntimeDeduplication::OwnerEnforcedStableCallId;
        deduplicated.deduplication_id = Some("stable-call".into());
        assert_eq!(
            durable_unknown(deduplicated)
                .unknown_retry_disposition("stable-call")
                .unwrap(),
            UnknownRetryDisposition::Safe {
                invocation_id: unsafe_id
            }
        );

        let mut mismatched = unsafe_preparation.clone();
        mismatched.execution.deduplication =
            omegon_traits::RuntimeDeduplication::OwnerEnforcedStableCallId;
        mismatched.deduplication_id = Some("another-call".into());
        assert!(matches!(
            durable_unknown(mismatched)
                .unknown_retry_disposition("stable-call")
                .unwrap(),
            UnknownRetryDisposition::Unsafe { .. }
        ));

        let read_only = prepared(turn_id, "stable-call");
        assert_eq!(
            durable_unknown(read_only)
                .unknown_retry_disposition("stable-call")
                .unwrap(),
            UnknownRetryDisposition::None
        );

        let legacy_id = Uuid::new_v4();
        let registration = InvocationRegistered {
            invocation_id: legacy_id,
            turn_id,
            call_id: "legacy-call".into(),
            owner_generation_id: None,
        };
        let mut legacy = SessionAuthorityState::default();
        legacy.invocations.insert(
            legacy_id,
            InvocationState::Unknown {
                registration,
                classification: InvocationClassifiedUnknown {
                    invocation_id: legacy_id,
                    reason_code: "runtime_loss".into(),
                    recovery_rule_version: 1,
                },
            },
        );
        assert_eq!(
            legacy.unknown_retry_disposition("legacy-call").unwrap(),
            UnknownRetryDisposition::Unsafe {
                invocation_id: legacy_id
            }
        );
    }

    #[test]
    fn fact_round_trip_is_strict_and_stable() {
        let fact = created("session-1", Uuid::new_v4());
        let encoded = fact.encode().unwrap();
        assert_eq!(SessionFact::decode(&encoded).unwrap(), fact);

        let mut value: Value = serde_json::from_slice(&encoded).unwrap();
        value["payload"]["unexpected"] = Value::Bool(true);
        let error = SessionFact::decode(&serde_json::to_vec(&value).unwrap()).unwrap_err();
        assert!(error.to_string().contains("unknown field"));

        value = serde_json::from_slice(&encoded).unwrap();
        value["event_version"] = Value::from(2);
        let error = SessionFact::decode(&serde_json::to_vec(&value).unwrap()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unsupported session.created event version")
        );
    }

    #[test]
    fn prepared_and_dispatched_facts_round_trip_and_reduce_in_order() {
        let session_id = "session-invocation";
        let stream_id = Uuid::new_v4();
        let prompt_id = Uuid::new_v4();
        let turn_id = Uuid::new_v4();
        let preparation = prepared(turn_id, "call-1");
        let dispatch = InvocationDispatched {
            invocation_id: preparation.invocation_id,
            lease_id: preparation.lease_id,
        };
        let prepared_fact = fact(
            session_id,
            stream_id,
            4,
            SessionFactPayload::InvocationPrepared(preparation.clone()),
        );
        let dispatched_fact = fact(
            session_id,
            stream_id,
            5,
            SessionFactPayload::InvocationDispatched(dispatch.clone()),
        );
        assert_eq!(
            SessionFact::decode(&prepared_fact.encode().unwrap()).unwrap(),
            prepared_fact
        );
        assert_eq!(
            SessionFact::decode(&dispatched_fact.encode().unwrap()).unwrap(),
            dispatched_fact
        );

        let state = reconstruct(&[
            created(session_id, stream_id),
            admitted(session_id, stream_id, 2, prompt_id, "run"),
            started(session_id, stream_id, 3, prompt_id, turn_id),
            prepared_fact,
            dispatched_fact,
            fact(
                session_id,
                stream_id,
                6,
                SessionFactPayload::TurnClosed(TurnClosed {
                    turn_id,
                    outcome: TurnOutcome::Completed,
                    reason_code: "worker_completed".into(),
                    recovery_rule_version: None,
                }),
            ),
        ])
        .unwrap();
        assert!(matches!(
            state.invocations.get(&preparation.invocation_id),
            Some(InvocationState::Dispatched {
                preparation: stored,
                dispatch: stored_dispatch,
            }) if stored == &preparation && stored_dispatch == &dispatch
        ));
        assert!(state.active_turn.is_none());
        let recovery = recovery_facts(&state, NOW).unwrap();
        assert_eq!(recovery.len(), 1);
        assert!(matches!(
            recovery[0].payload,
            SessionFactPayload::InvocationClassifiedUnknown(_)
        ));
    }

    #[test]
    fn dispatch_requires_matching_prepared_lease() {
        let session_id = "session-mismatch";
        let stream_id = Uuid::new_v4();
        let prompt_id = Uuid::new_v4();
        let turn_id = Uuid::new_v4();
        let preparation = prepared(turn_id, "call-1");
        let error = reconstruct(&[
            created(session_id, stream_id),
            admitted(session_id, stream_id, 2, prompt_id, "run"),
            started(session_id, stream_id, 3, prompt_id, turn_id),
            fact(
                session_id,
                stream_id,
                4,
                SessionFactPayload::InvocationPrepared(preparation.clone()),
            ),
            fact(
                session_id,
                stream_id,
                5,
                SessionFactPayload::InvocationDispatched(InvocationDispatched {
                    invocation_id: preparation.invocation_id,
                    lease_id: Uuid::new_v4(),
                }),
            ),
        ])
        .unwrap_err();
        assert!(error.to_string().contains("dispatch lease does not match"));
    }

    #[test]
    fn acknowledged_invocation_settles_before_turn_closure() {
        let session_id = "session-settlement";
        let stream_id = Uuid::new_v4();
        let prompt_id = Uuid::new_v4();
        let turn_id = Uuid::new_v4();
        let preparation = prepared(turn_id, "call-1");
        let acknowledgement = InvocationAcknowledged {
            invocation_id: preparation.invocation_id,
            lease_id: preparation.lease_id,
        };
        let settlement = InvocationSettled {
            invocation_id: preparation.invocation_id,
            outcome: InvocationOutcome::Completed,
            terminal_evidence_reference: None,
        };
        let state = reconstruct(&[
            created(session_id, stream_id),
            admitted(session_id, stream_id, 2, prompt_id, "run"),
            started(session_id, stream_id, 3, prompt_id, turn_id),
            fact(
                session_id,
                stream_id,
                4,
                SessionFactPayload::InvocationPrepared(preparation.clone()),
            ),
            fact(
                session_id,
                stream_id,
                5,
                SessionFactPayload::InvocationDispatched(InvocationDispatched {
                    invocation_id: preparation.invocation_id,
                    lease_id: preparation.lease_id,
                }),
            ),
            fact(
                session_id,
                stream_id,
                6,
                SessionFactPayload::InvocationAcknowledged(acknowledgement.clone()),
            ),
            fact(
                session_id,
                stream_id,
                7,
                SessionFactPayload::InvocationSettled(settlement.clone()),
            ),
            fact(
                session_id,
                stream_id,
                8,
                SessionFactPayload::TurnClosed(TurnClosed {
                    turn_id,
                    outcome: TurnOutcome::Completed,
                    reason_code: "completed".into(),
                    recovery_rule_version: None,
                }),
            ),
        ])
        .unwrap();
        assert!(matches!(
            state.invocations.get(&preparation.invocation_id),
            Some(InvocationState::DurableSettled {
                acknowledgement: stored_acknowledgement,
                settlement: stored_settlement,
                ..
            }) if stored_acknowledgement == &acknowledgement && stored_settlement == &settlement
        ));
        assert!(state.active_turn.is_none());
    }

    #[test]
    fn recovery_preserves_prepared_and_classifies_dispatched_unknown() {
        let session_id = "session-recovery-phases";
        let stream_id = Uuid::new_v4();
        let prompt_id = Uuid::new_v4();
        let turn_id = Uuid::new_v4();
        let first = prepared(turn_id, "prepared-call");
        let second = prepared(turn_id, "dispatched-call");
        let state = reconstruct(&[
            created(session_id, stream_id),
            admitted(session_id, stream_id, 2, prompt_id, "run"),
            started(session_id, stream_id, 3, prompt_id, turn_id),
            fact(
                session_id,
                stream_id,
                4,
                SessionFactPayload::InvocationPrepared(first.clone()),
            ),
            fact(
                session_id,
                stream_id,
                5,
                SessionFactPayload::InvocationPrepared(second.clone()),
            ),
            fact(
                session_id,
                stream_id,
                6,
                SessionFactPayload::InvocationDispatched(InvocationDispatched {
                    invocation_id: second.invocation_id,
                    lease_id: second.lease_id,
                }),
            ),
        ])
        .unwrap();
        let recovery = recovery_facts(&state, NOW).unwrap();
        assert_eq!(recovery.len(), 2);
        assert!(matches!(
            recovery[0].payload,
            SessionFactPayload::InvocationClassifiedUnknown(_)
        ));
        assert!(matches!(
            recovery[1].payload,
            SessionFactPayload::TurnClosed(_)
        ));
        let mut recovered = state;
        for fact in &recovery {
            recovered.apply(fact).unwrap();
        }
        assert!(matches!(
            recovered.invocations.get(&first.invocation_id),
            Some(InvocationState::Prepared { .. })
        ));
        assert!(matches!(
            recovered.invocations.get(&second.invocation_id),
            Some(InvocationState::DurableUnknown { .. })
        ));
    }

    #[test]
    fn minimum_vocabulary_round_trips() {
        let session = "session-1";
        let stream = Uuid::new_v4();
        let prompt = Uuid::new_v4();
        let turn = Uuid::new_v4();
        let invocation = Uuid::new_v4();
        let payloads = vec![
            SessionFactPayload::SessionCreated(SessionCreated {
                workspace_identity: "workspace".into(),
                created_by: ActorIdentity {
                    principal: "operator".into(),
                    ingress: "tui".into(),
                },
                runtime_generation_id: "generation".into(),
            }),
            SessionFactPayload::ExecutionBindingMigrated(ExecutionBindingMigrated {
                from_generation: execution_binding("v1"),
                target_generation: execution_binding("v2"),
            }),
            SessionFactPayload::PromptAdmitted(PromptAdmitted {
                submission_id: Uuid::new_v4(),
                prompt_id: prompt,
                principal: "operator".into(),
                ingress: "tui".into(),
                queue_mode: QueueMode::UntilReady,
                content: PromptContent {
                    text: "work".into(),
                    attachments: vec![AttachmentRef {
                        digest: fingerprint("attachment"),
                        media_type: "image/png".into(),
                        byte_length: 4,
                        storage_ref: "sha256/attachment".into(),
                    }],
                },
                metadata: serde_json::json!({"voice": false}),
            }),
            SessionFactPayload::PromptRejected(PromptRejected {
                submission_id: Uuid::new_v4(),
                principal: "operator".into(),
                ingress: "ipc".into(),
                reason_code: "session_closing".into(),
            }),
            SessionFactPayload::PromptRemoved(PromptRemoved {
                prompt_id: prompt,
                reason: PromptRemovalReason::Withdrawn,
            }),
            SessionFactPayload::TurnStarted(TurnStarted {
                turn_id: turn,
                prompt_id: prompt,
                runtime_generation_id: "generation".into(),
            }),
            SessionFactPayload::TurnInterruptionRequested(TurnInterruptionRequested {
                interruption_id: Uuid::new_v4(),
                turn_id: turn,
                kind: InterruptionKind::Cancel,
                principal: "operator".into(),
                ingress: "tui".into(),
                reason_code: "operator_cancelled".into(),
            }),
            SessionFactPayload::InvocationRegistered(InvocationRegistered {
                invocation_id: invocation,
                turn_id: turn,
                call_id: "call-1".into(),
                owner_generation_id: Some("tools-1".into()),
            }),
            SessionFactPayload::InvocationAcknowledged(InvocationAcknowledged {
                invocation_id: invocation,
                lease_id: Uuid::new_v4(),
            }),
            SessionFactPayload::InvocationClassifiedUnknown(InvocationClassifiedUnknown {
                invocation_id: invocation,
                reason_code: "runtime_lost".into(),
                recovery_rule_version: 1,
            }),
            SessionFactPayload::InvocationSettled(InvocationSettled {
                invocation_id: invocation,
                outcome: InvocationOutcome::Completed,
                terminal_evidence_reference: Some("owner:receipt-1".into()),
            }),
            SessionFactPayload::TurnClosed(TurnClosed {
                turn_id: turn,
                outcome: TurnOutcome::Cancelled,
                reason_code: "cancelled".into(),
                recovery_rule_version: None,
            }),
        ];
        for (index, payload) in payloads.into_iter().enumerate() {
            let fact = fact(session, stream, index as u64 + 1, payload);
            assert_eq!(SessionFact::decode(&fact.encode().unwrap()).unwrap(), fact);
        }
    }

    #[test]
    fn reducer_reconstructs_fifo_and_exact_terminal_state() {
        let session = "session-1";
        let stream = Uuid::new_v4();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let turn = Uuid::new_v4();
        let facts = vec![
            created(session, stream),
            admitted(session, stream, 2, first, "first"),
            admitted(session, stream, 3, second, "second"),
            started(session, stream, 4, first, turn),
            fact(
                session,
                stream,
                5,
                SessionFactPayload::TurnClosed(TurnClosed {
                    turn_id: turn,
                    outcome: TurnOutcome::Completed,
                    reason_code: "completed".into(),
                    recovery_rule_version: None,
                }),
            ),
        ];
        let state = reconstruct(&facts).unwrap();
        assert!(state.active_turn.is_none());
        assert_eq!(state.queued_prompts.len(), 1);
        assert_eq!(state.queued_prompts[0].prompt_id, second);
        assert_eq!(state.closed_turns[&turn].outcome, TurnOutcome::Completed);
        let first_submission = match &facts[1].payload {
            SessionFactPayload::PromptAdmitted(value) => value.submission_id,
            _ => unreachable!(),
        };
        assert_eq!(state.prompt_ids[&first], first_submission);
        assert_eq!(state.turn_starts[&turn].prompt_id, first);
    }

    #[test]
    fn reducer_retains_prompt_and_submission_identity_after_terminalization() {
        let session = "session-1";
        let stream = Uuid::new_v4();
        let prompt = Uuid::new_v4();
        let turn = Uuid::new_v4();
        let admission = admitted(session, stream, 2, prompt, "work");
        let submission = match &admission.payload {
            SessionFactPayload::PromptAdmitted(value) => value.submission_id,
            _ => unreachable!(),
        };
        let facts = vec![
            created(session, stream),
            admission,
            started(session, stream, 3, prompt, turn),
            fact(
                session,
                stream,
                4,
                SessionFactPayload::TurnClosed(TurnClosed {
                    turn_id: turn,
                    outcome: TurnOutcome::Completed,
                    reason_code: "completed".into(),
                    recovery_rule_version: None,
                }),
            ),
        ];
        let mut state = reconstruct(&facts).unwrap();
        let reused_prompt = PromptAdmitted {
            submission_id: Uuid::new_v4(),
            prompt_id: prompt,
            principal: "operator".into(),
            ingress: "tui".into(),
            queue_mode: QueueMode::UntilReady,
            content: PromptContent {
                text: "duplicate".into(),
                attachments: Vec::new(),
            },
            metadata: serde_json::json!({}),
        };
        assert!(
            state
                .apply(&fact(
                    session,
                    stream,
                    5,
                    SessionFactPayload::PromptAdmitted(reused_prompt),
                ))
                .unwrap_err()
                .to_string()
                .contains("already present")
        );
        let rejection = PromptRejected {
            submission_id: submission,
            principal: "operator".into(),
            ingress: "ipc".into(),
            reason_code: "late".into(),
        };
        assert!(
            state
                .apply(&fact(
                    session,
                    stream,
                    5,
                    SessionFactPayload::PromptRejected(rejection),
                ))
                .unwrap_err()
                .to_string()
                .contains("already has an outcome")
        );
    }

    #[test]
    fn reducer_rejects_gap_non_fifo_start_and_duplicate_terminal() {
        let session = "session-1";
        let stream = Uuid::new_v4();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let mut state = SessionAuthorityState::default();
        let creation = created(session, stream);
        state.apply(&creation).unwrap();
        let gap = admitted(session, stream, 3, first, "gap");
        assert!(
            state
                .apply(&gap)
                .unwrap_err()
                .to_string()
                .contains("expected sequence 2")
        );
        state
            .apply(&admitted(session, stream, 2, first, "first"))
            .unwrap();
        let mut reused_event = admitted(session, stream, 3, second, "reused-event");
        reused_event.event_id = creation.event_id;
        assert!(
            state
                .apply(&reused_event)
                .unwrap_err()
                .to_string()
                .contains("duplicate event ID")
        );
        state
            .apply(&admitted(session, stream, 3, second, "second"))
            .unwrap();
        let turn = Uuid::new_v4();
        let non_fifo = started(session, stream, 4, second, turn);
        assert!(
            state
                .apply(&non_fifo)
                .unwrap_err()
                .to_string()
                .contains("FIFO")
        );
        state
            .apply(&started(session, stream, 4, first, turn))
            .unwrap();
        state
            .apply(&fact(
                session,
                stream,
                5,
                SessionFactPayload::TurnClosed(TurnClosed {
                    turn_id: turn,
                    outcome: TurnOutcome::Completed,
                    reason_code: "completed".into(),
                    recovery_rule_version: None,
                }),
            ))
            .unwrap();
        let duplicate = fact(
            session,
            stream,
            6,
            SessionFactPayload::TurnClosed(TurnClosed {
                turn_id: turn,
                outcome: TurnOutcome::Failed,
                reason_code: "late".into(),
                recovery_rule_version: None,
            }),
        );
        assert!(
            state
                .apply(&duplicate)
                .unwrap_err()
                .to_string()
                .contains("no active turn")
        );
    }

    #[test]
    fn recovery_is_deterministic_and_classifies_unsettled_invocations() {
        let session = "session-1";
        let stream = Uuid::new_v4();
        let prompt = Uuid::new_v4();
        let turn = Uuid::new_v4();
        let invocation = Uuid::new_v4();
        let facts = vec![
            created(session, stream),
            admitted(session, stream, 2, prompt, "work"),
            started(session, stream, 3, prompt, turn),
            fact(
                session,
                stream,
                4,
                SessionFactPayload::InvocationRegistered(InvocationRegistered {
                    invocation_id: invocation,
                    turn_id: turn,
                    call_id: "call-1".into(),
                    owner_generation_id: Some("tools-1".into()),
                }),
            ),
        ];
        let mut state = reconstruct(&facts).unwrap();
        let recovery = recovery_facts(&state, NOW).unwrap();
        let repeated = recovery_facts(&state, NOW).unwrap();
        assert_eq!(recovery, repeated);
        assert_eq!(recovery.len(), 2);
        for fact in &recovery {
            state.apply(fact).unwrap();
        }
        assert!(state.active_turn.is_none());
        assert!(matches!(
            state.invocations[&invocation],
            InvocationState::Unknown { .. }
        ));
        assert_eq!(state.closed_turns[&turn].outcome, TurnOutcome::Interrupted);
        assert!(recovery_facts(&state, NOW).unwrap().is_empty());
    }

    #[test]
    fn store_appends_syncs_replays_tail_and_ignores_bad_cache() {
        let temp = tempfile::tempdir().unwrap();
        let session_path = temp.path().join("session-1.json");
        let store = SessionAuthorityStore::adjacent_to(&session_path).unwrap();
        let session = "session-1";
        let stream = Uuid::new_v4();
        let prompt = Uuid::new_v4();
        let mut state = SessionAuthorityState::default();
        assert!(store.append(&mut state, &created(session, stream)).unwrap());
        let old_snapshot = fs::read(&store.snapshot_path).unwrap();
        let admission = admitted(session, stream, 2, prompt, "work");
        assert!(store.append(&mut state, &admission).unwrap());
        assert_eq!(store.load().unwrap(), state);

        fs::write(&store.snapshot_path, old_snapshot).unwrap();
        assert_eq!(store.load().unwrap(), state);

        fs::write(&store.snapshot_path, b"not json").unwrap();
        assert_eq!(store.load().unwrap(), state);

        assert!(!store.append(&mut state, &admission).unwrap());
        assert_eq!(state.last_sequence, 2);

        let mut retry = admission.clone();
        retry.event_id = Uuid::new_v4();
        assert!(!store.append(&mut state, &retry).unwrap());

        let mut conflict = admission.clone();
        conflict.command_fingerprint = fingerprint("different-command");
        assert!(
            store
                .append(&mut state, &conflict)
                .unwrap_err()
                .to_string()
                .contains("conflicting event or fingerprint")
        );
    }

    #[test]
    fn store_discards_valid_cache_when_cursor_or_state_mismatches_stream() {
        let temp = tempfile::tempdir().unwrap();
        let store =
            SessionAuthorityStore::adjacent_to(&temp.path().join("session-1.json")).unwrap();
        let session = "session-1";
        let stream = Uuid::new_v4();
        let prompt = Uuid::new_v4();
        let mut expected = SessionAuthorityState::default();
        store
            .append(&mut expected, &created(session, stream))
            .unwrap();
        store
            .append(&mut expected, &admitted(session, stream, 2, prompt, "work"))
            .unwrap();
        let log_before = fs::read(&store.log_path).unwrap();
        let valid: SessionAuthoritySnapshot =
            serde_json::from_slice(&fs::read(&store.snapshot_path).unwrap()).unwrap();

        let mut wrong_event = valid.clone();
        wrong_event.last_event_id = Uuid::new_v4();
        wrong_event.state.last_event_id = Some(wrong_event.last_event_id);
        write_snapshot(&store.snapshot_path, &wrong_event).unwrap();
        assert_eq!(store.load().unwrap(), expected);

        let mut wrong_state = valid;
        wrong_state.state.queued_prompts.clear();
        write_snapshot(&store.snapshot_path, &wrong_state).unwrap();
        assert_eq!(store.load().unwrap(), expected);
        assert_eq!(fs::read(&store.log_path).unwrap(), log_before);
    }

    #[test]
    fn session_authority_commits_typed_transitions_and_reopens_at_same_cursor() {
        let temp = tempfile::tempdir().unwrap();
        let session_path = temp.path().join("session-1.json");
        let actor = ActorIdentity {
            principal: "operator".into(),
            ingress: "tui".into(),
        };
        let mut authority = SessionAuthority::open(
            &session_path,
            "session-1",
            "workspace-1",
            "generation-1",
            actor,
            NOW,
        )
        .unwrap();
        assert_eq!(authority.state().last_sequence, 1);

        let prompt_id = Uuid::new_v4();
        let admission_command = Uuid::new_v4();
        let admission = PromptAdmitted {
            submission_id: Uuid::new_v4(),
            prompt_id,
            principal: "operator".into(),
            ingress: "tui".into(),
            queue_mode: QueueMode::UntilReady,
            content: PromptContent {
                text: "inspect the workspace".into(),
                attachments: Vec::new(),
            },
            metadata: serde_json::json!({}),
        };
        assert!(
            authority
                .admit_prompt(admission_command, NOW, admission.clone())
                .unwrap()
        );
        assert!(
            !authority
                .admit_prompt(admission_command, NOW, admission)
                .unwrap()
        );

        let turn_id = Uuid::new_v4();
        assert!(
            authority
                .start_turn(Uuid::new_v4(), NOW, turn_id, prompt_id)
                .unwrap()
        );
        assert!(
            authority
                .request_interruption(
                    Uuid::new_v4(),
                    NOW,
                    TurnInterruptionRequested {
                        interruption_id: Uuid::new_v4(),
                        turn_id,
                        kind: InterruptionKind::Cancel,
                        principal: "operator".into(),
                        ingress: "ipc".into(),
                        reason_code: "operator_cancelled".into(),
                    },
                )
                .unwrap()
        );
        assert!(
            authority
                .close_turn(
                    Uuid::new_v4(),
                    NOW,
                    TurnClosed {
                        turn_id,
                        outcome: TurnOutcome::Revoked,
                        reason_code: "worker_revoked".into(),
                        recovery_rule_version: None,
                    },
                )
                .unwrap()
        );
        assert_eq!(authority.state().last_sequence, 5);
        assert!(authority.state().active_turn.is_none());
        drop(authority);

        let mut reopened = SessionAuthority::open(
            &session_path,
            "session-1",
            "workspace-1",
            "generation-2",
            ActorIdentity {
                principal: "system".into(),
                ingress: "resume".into(),
            },
            "2026-08-19T19:00:00Z",
        )
        .unwrap();
        assert_eq!(reopened.state().last_sequence, 5);
        assert_eq!(
            reopened.state().closed_turns[&turn_id].outcome,
            TurnOutcome::Revoked
        );

        let resumed_prompt_id = Uuid::new_v4();
        reopened
            .admit_prompt(
                Uuid::new_v4(),
                "2026-08-19T19:00:01Z",
                PromptAdmitted {
                    submission_id: Uuid::new_v4(),
                    prompt_id: resumed_prompt_id,
                    principal: "operator".into(),
                    ingress: "tui".into(),
                    queue_mode: QueueMode::UntilReady,
                    content: PromptContent {
                        text: "continue".into(),
                        attachments: Vec::new(),
                    },
                    metadata: serde_json::json!({}),
                },
            )
            .unwrap();
        let resumed_turn_id = Uuid::new_v4();
        reopened
            .start_turn(
                Uuid::new_v4(),
                "2026-08-19T19:00:02Z",
                resumed_turn_id,
                resumed_prompt_id,
            )
            .unwrap();
        assert_eq!(
            reopened.state().turn_starts[&resumed_turn_id].runtime_generation_id,
            "generation-1"
        );
    }

    #[test]
    fn open_refuses_concurrent_writer_without_recovering_live_turn() {
        let temp = tempfile::tempdir().unwrap();
        let session_path = temp.path().join("session-1.json");
        let mut authority = SessionAuthority::open(
            &session_path,
            "session-1",
            "workspace-1",
            "generation-1",
            ActorIdentity {
                principal: "operator".into(),
                ingress: "tui".into(),
            },
            NOW,
        )
        .unwrap();
        let prompt_id = Uuid::new_v4();
        authority
            .admit_prompt(
                Uuid::new_v4(),
                NOW,
                PromptAdmitted {
                    submission_id: Uuid::new_v4(),
                    prompt_id,
                    principal: "operator".into(),
                    ingress: "tui".into(),
                    queue_mode: QueueMode::UntilReady,
                    content: PromptContent {
                        text: "still running".into(),
                        attachments: Vec::new(),
                    },
                    metadata: serde_json::json!({}),
                },
            )
            .unwrap();
        authority
            .start_turn(Uuid::new_v4(), NOW, Uuid::new_v4(), prompt_id)
            .unwrap();
        let before = authority.state().clone();

        let error = SessionAuthority::open(
            &session_path,
            "session-1",
            "workspace-1",
            "generation-2",
            ActorIdentity {
                principal: "system".into(),
                ingress: "resume".into(),
            },
            "2026-08-19T18:01:00Z",
        )
        .unwrap_err();
        assert!(error.to_string().contains("active writer"));
        assert_eq!(authority.state(), &before);
        assert_eq!(
            SessionAuthorityStore::adjacent_to(&session_path)
                .unwrap()
                .load()
                .unwrap(),
            before
        );
    }

    #[test]
    fn open_validates_stream_identity_before_recovery_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let session_path = temp.path().join("session-1.json");
        let mut authority = SessionAuthority::open(
            &session_path,
            "session-1",
            "workspace-1",
            "generation-1",
            ActorIdentity {
                principal: "operator".into(),
                ingress: "tui".into(),
            },
            NOW,
        )
        .unwrap();
        let prompt_id = Uuid::new_v4();
        authority
            .admit_prompt(
                Uuid::new_v4(),
                NOW,
                PromptAdmitted {
                    submission_id: Uuid::new_v4(),
                    prompt_id,
                    principal: "operator".into(),
                    ingress: "tui".into(),
                    queue_mode: QueueMode::UntilReady,
                    content: PromptContent {
                        text: "unsettled".into(),
                        attachments: Vec::new(),
                    },
                    metadata: serde_json::json!({}),
                },
            )
            .unwrap();
        authority
            .start_turn(Uuid::new_v4(), NOW, Uuid::new_v4(), prompt_id)
            .unwrap();
        drop(authority);
        let store = SessionAuthorityStore::adjacent_to(&session_path).unwrap();
        let log_before = fs::read(&store.log_path).unwrap();

        let error = SessionAuthority::open(
            &session_path,
            "session-1",
            "wrong-workspace",
            "generation-2",
            ActorIdentity {
                principal: "system".into(),
                ingress: "resume".into(),
            },
            "2026-08-19T18:01:00Z",
        )
        .unwrap_err();
        assert!(error.to_string().contains("different workspace"));
        assert_eq!(fs::read(&store.log_path).unwrap(), log_before);
        assert!(store.load().unwrap().active_turn.is_some());
    }

    #[test]
    fn session_authority_stages_attachments_by_content_digest() {
        let temp = tempfile::tempdir().unwrap();
        let session_path = temp.path().join("session-1.json");
        let source = temp.path().join("capture.png");
        fs::write(&source, b"stable-image-bytes").unwrap();
        let authority = SessionAuthority::open(
            &session_path,
            "session-1",
            "workspace-1",
            "generation-1",
            ActorIdentity {
                principal: "operator".into(),
                ingress: "tui".into(),
            },
            NOW,
        )
        .unwrap();

        let first = authority.stage_attachment(&source).unwrap();
        let second = authority.stage_attachment(&source).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.media_type, "image/png");
        assert_eq!(first.byte_length, 18);
        assert_eq!(fs::read(&first.storage_ref).unwrap(), b"stable-image-bytes");
        assert!(
            Path::new(&first.storage_ref)
                .file_name()
                .is_some_and(|name| name == first.digest.as_str())
        );
    }

    #[test]
    fn session_authority_blob_api_survives_append_failure_and_reopen() {
        let temp = tempfile::tempdir().unwrap();
        let session_path = temp.path().join("session-blob.json");
        let authority = SessionAuthorityHandle::new(
            SessionAuthority::open(
                &session_path,
                "session-blob",
                "workspace-1",
                "generation-1",
                ActorIdentity {
                    principal: "operator".into(),
                    ingress: "test".into(),
                },
                NOW,
            )
            .unwrap(),
        );
        let content_ref = authority
            .write_content(
                b"durable before fact",
                "text/plain",
                ProjectionClass::Default,
            )
            .unwrap();
        let sequence_before = authority.state().last_sequence;
        authority.make_next_append_fail();
        let append = authority.admit_prompt(
            Uuid::new_v4(),
            NOW,
            PromptAdmitted {
                submission_id: Uuid::new_v4(),
                prompt_id: Uuid::new_v4(),
                principal: "operator".into(),
                ingress: "test".into(),
                queue_mode: QueueMode::UntilReady,
                content: PromptContent {
                    text: "must not commit".into(),
                    attachments: Vec::new(),
                },
                metadata: serde_json::json!({}),
            },
        );
        assert!(append.is_err());
        assert_eq!(authority.state().last_sequence, sequence_before);
        assert_eq!(
            authority
                .read_content(&content_ref, ProjectionClass::Default)
                .unwrap(),
            b"durable before fact"
        );

        let log_path = temp.path().join("session-blob.authority.jsonl");
        let mut permissions = fs::metadata(&log_path).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o600);
        }
        #[cfg(not(unix))]
        permissions.set_readonly(false);
        fs::set_permissions(&log_path, permissions).unwrap();
        drop(authority);

        let reopened = SessionAuthority::open(
            &session_path,
            "session-blob",
            "workspace-1",
            "generation-2",
            ActorIdentity {
                principal: "system".into(),
                ingress: "resume".into(),
            },
            "2026-08-21T12:01:00Z",
        )
        .unwrap();
        reopened
            .validate_content_ref(&content_ref, ProjectionClass::Default)
            .unwrap();
        assert_eq!(reopened.state().last_sequence, sequence_before);
    }

    #[test]
    fn store_recovery_holds_authority_and_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let store =
            SessionAuthorityStore::adjacent_to(&temp.path().join("session-1.json")).unwrap();
        let session = "session-1";
        let stream = Uuid::new_v4();
        let prompt = Uuid::new_v4();
        let turn = Uuid::new_v4();
        let invocation = Uuid::new_v4();
        let mut state = SessionAuthorityState::default();
        let initial = vec![
            created(session, stream),
            admitted(session, stream, 2, prompt, "work"),
            started(session, stream, 3, prompt, turn),
            fact(
                session,
                stream,
                4,
                SessionFactPayload::InvocationRegistered(InvocationRegistered {
                    invocation_id: invocation,
                    turn_id: turn,
                    call_id: "call-1".into(),
                    owner_generation_id: None,
                }),
            ),
        ];
        for fact in initial {
            store.append(&mut state, &fact).unwrap();
        }

        let recovered = store.recover(NOW).unwrap();
        assert!(recovered.active_turn.is_none());
        assert_eq!(recovered.last_sequence, 6);
        assert!(matches!(
            recovered.invocations[&invocation],
            InvocationState::Unknown { .. }
        ));
        assert_eq!(store.recover("2026-08-19T19:00:00Z").unwrap(), recovered);
        assert_eq!(read_facts(&store.log_path).unwrap().len(), 6);
    }

    #[test]
    fn store_rejects_truncated_log_and_preserves_state_on_append_failure() {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("authority.jsonl");
        let snapshot = temp.path().join("snapshot.json");
        let store = SessionAuthorityStore::from_paths(log.clone(), snapshot);
        fs::write(&log, b"{\"truncated\":true}").unwrap();
        assert!(store.load().unwrap_err().to_string().contains("truncated"));

        fs::remove_file(&log).unwrap();
        fs::create_dir(&log).unwrap();
        let mut state = SessionAuthorityState::default();
        let before = state.clone();
        let error = store
            .append(&mut state, &created("session-1", Uuid::new_v4()))
            .unwrap_err();
        assert!(matches!(error, AuthorityError::Io(_)));
        assert_eq!(state, before);
    }

    fn fake_default_ref(label: &str) -> ContentRef {
        let digest = format!("{:x}", Sha256::digest(label.as_bytes()));
        serde_json::from_value(serde_json::json!({
            "digest_algorithm": "sha256",
            "digest": digest,
            "media_type": "text/plain",
            "byte_length": label.len(),
            "storage_class": "session_blob_v1",
            "projection_class": "default"
        }))
        .unwrap()
    }

    fn compaction_start(
        state: &SessionAuthorityState,
        owner_scope: CompactionOwnerScope,
        trigger: CompactionTrigger,
        source_event_id: Uuid,
        compaction_id: Uuid,
    ) -> CompactionStarted {
        let mut start = CompactionStarted {
            compaction_id,
            owner_scope,
            trigger,
            source_frontier: AuthorityFrontierRef {
                sequence: state.last_sequence,
                event_id: state.last_event_id.unwrap(),
            },
            source_context_revision: state.context_revision,
            input_manifest_id: String::new(),
            input_items: vec![CompactionContextItem {
                ordinal: 0,
                source_event_id,
                source_identity: "source-0".into(),
                content_ref: fake_default_ref("input"),
            }],
            retained_items: vec![CompactionContextItem {
                ordinal: 0,
                source_event_id,
                source_identity: "source-1".into(),
                content_ref: fake_default_ref("retained"),
            }],
            target_context_revision: state.context_revision + 1,
        };
        start.input_manifest_id = compaction_input_manifest_id(&start).unwrap();
        start
    }

    fn committed_summary_fact(
        session: &str,
        stream: Uuid,
        sequence: u64,
        start: &CompactionStarted,
        request_id: Uuid,
        summary_id: Uuid,
    ) -> SessionFact {
        let event_id = Uuid::new_v4();
        let summary_ref = fake_default_ref("summary");
        let mut summary = CompactionSummaryCommitted {
            compaction_summary_id: summary_id,
            compaction_request_id: request_id,
            compaction_id: start.compaction_id,
            response_attempt_ordinal: 0,
            completion_evidence: ProviderCompletionEvidence::ProviderDone,
            summary_ref: summary_ref.clone(),
            summary_digest: summary_ref.digest().into(),
            replacement_manifest_id: String::new(),
            replacement_items: vec![
                CompactionReplacementItem {
                    ordinal: 0,
                    source_kind: CompactionReplacementSourceKind::CompactionSummary,
                    source_event_id: event_id,
                    source_identity: summary_id.to_string(),
                    content_ref: summary_ref,
                },
                CompactionReplacementItem {
                    ordinal: 1,
                    source_kind: CompactionReplacementSourceKind::Retained,
                    source_event_id: start.retained_items[0].source_event_id,
                    source_identity: start.retained_items[0].source_identity.clone(),
                    content_ref: start.retained_items[0].content_ref.clone(),
                },
            ],
            usage: None,
        };
        summary.replacement_manifest_id =
            compaction_replacement_manifest_id(&summary, start).unwrap();
        let mut fact = fact(
            session,
            stream,
            sequence,
            SessionFactPayload::CompactionSummaryCommitted(summary),
        );
        fact.event_id = event_id;
        fact
    }

    #[test]
    fn turn_compaction_commits_closes_then_applies_exactly_once() {
        let session = "turn-compaction";
        let stream = Uuid::new_v4();
        let prompt = Uuid::new_v4();
        let turn = Uuid::new_v4();
        let step = Uuid::new_v4();
        let compaction = Uuid::new_v4();
        let request = Uuid::new_v4();
        let summary = Uuid::new_v4();
        let mut facts = vec![
            created(session, stream),
            admitted(session, stream, 2, prompt, "compact"),
            started(session, stream, 3, prompt, turn),
            fact(
                session,
                stream,
                4,
                SessionFactPayload::StepStarted(StepStarted {
                    step_id: step,
                    turn_id: turn,
                    step_ordinal: 0,
                }),
            ),
        ];
        let source_event = facts[1].event_id;
        let state = reconstruct(&facts).unwrap();
        let start = compaction_start(
            &state,
            CompactionOwnerScope::Turn {
                turn_id: turn,
                step_id: step,
            },
            CompactionTrigger::ContextPressure,
            source_event,
            compaction,
        );
        facts.push(fact(
            session,
            stream,
            5,
            SessionFactPayload::CompactionStarted(start.clone()),
        ));
        let lease_id = Uuid::new_v4();
        facts.push(fact(
            session,
            stream,
            6,
            SessionFactPayload::RouteLeaseRecorded(RouteLeaseRecorded {
                lease_id,
                request_id: request,
                turn_id: turn,
                selected_provider_id: "openai".into(),
                selected_model_id: "gpt".into(),
                serving_provider_id: "openai".into(),
                serving_model_id: "gpt".into(),
                schema_dialect: "open_ai".into(),
                credential_source_class: "api_key".into(),
                fallback_reason: None,
                contribution_generation_id: "provider:openai/builtin-v1".into(),
                route_policy: "selected_provider_only_v1".into(),
            }),
        ));
        facts.push(fact(
            session,
            stream,
            7,
            SessionFactPayload::CompactionRequestPrepared(CompactionRequestPrepared {
                compaction_request_id: request,
                compaction_id: compaction,
                request_ordinal: 0,
                replaces_compaction_request_id: None,
                prompt_template: CompactionPromptTemplate {
                    owner_id: "kernel".into(),
                    owner_generation_id: "v1".into(),
                    content_ref: fake_default_ref("prompt"),
                },
                route: CompactionRoute::TurnLease { lease_id },
            }),
        ));
        facts.push(committed_summary_fact(
            session, stream, 8, &start, request, summary,
        ));
        let replacement_manifest_id = match &facts[7].payload {
            SessionFactPayload::CompactionSummaryCommitted(value) => {
                value.replacement_manifest_id.clone()
            }
            _ => unreachable!(),
        };
        facts.push(fact(
            session,
            stream,
            9,
            SessionFactPayload::CompactionRequestClosed(CompactionRequestClosed {
                compaction_request_id: request,
                compaction_id: compaction,
                response_attempt_ordinal: 0,
                outcome: CompactionRequestOutcome::SummaryCommitted,
                reason_code: "provider_done".into(),
                recovery_rule_version: None,
            }),
        ));
        facts.push(fact(
            session,
            stream,
            10,
            SessionFactPayload::CompactionApplied(CompactionApplied {
                compaction_id: compaction,
                compaction_summary_id: summary,
                source_context_revision: 0,
                target_context_revision: 1,
                replacement_manifest_id,
                recovery_rule_version: None,
            }),
        ));
        let state = reconstruct(&facts).unwrap();
        assert_eq!(state.context_revision, 1);
        assert!(state.active_compaction.is_none());
        assert!(matches!(
            state.compaction_terminals[&compaction],
            CompactionTerminalState::Applied { .. }
        ));
        let duplicate = fact(session, stream, 11, facts.last().unwrap().payload.clone());
        assert!(state.clone().apply(&duplicate).is_err());
    }

    #[test]
    fn idle_eof_compaction_abandons_without_advancing_context() {
        let session = "idle-compaction";
        let stream = Uuid::new_v4();
        let source = fact(
            session,
            stream,
            2,
            SessionFactPayload::PromptRejected(PromptRejected {
                submission_id: Uuid::new_v4(),
                principal: "operator".into(),
                ingress: "tui".into(),
                reason_code: "fixture".into(),
            }),
        );
        let mut facts = vec![created(session, stream), source.clone()];
        let state = reconstruct(&facts).unwrap();
        let compaction = Uuid::new_v4();
        let request = Uuid::new_v4();
        let start = compaction_start(
            &state,
            CompactionOwnerScope::SessionIdle,
            CompactionTrigger::ManualIdle,
            source.event_id,
            compaction,
        );
        facts.push(fact(
            session,
            stream,
            3,
            SessionFactPayload::CompactionStarted(start),
        ));
        facts.push(fact(
            session,
            stream,
            4,
            SessionFactPayload::CompactionRequestPrepared(CompactionRequestPrepared {
                compaction_request_id: request,
                compaction_id: compaction,
                request_ordinal: 0,
                replaces_compaction_request_id: None,
                prompt_template: CompactionPromptTemplate {
                    owner_id: "kernel".into(),
                    owner_generation_id: "v1".into(),
                    content_ref: fake_default_ref("prompt"),
                },
                route: CompactionRoute::SessionIdle {
                    selected_provider_id: "openai".into(),
                    selected_model_id: "gpt".into(),
                    serving_provider_id: "openai".into(),
                    serving_model_id: "gpt".into(),
                    schema_dialect: "open_ai".into(),
                    credential_source_class: "api_key".into(),
                    fallback_reason: None,
                    contribution_generation_id: "provider:openai/builtin-v1".into(),
                    route_policy: "selected_provider_only_v1".into(),
                },
            }),
        ));
        facts.push(fact(
            session,
            stream,
            5,
            SessionFactPayload::CompactionResponseAttemptFailed(CompactionResponseAttemptFailed {
                compaction_request_id: request,
                compaction_id: compaction,
                response_attempt_ordinal: 0,
                failure: CompactionResponseAttemptFailure::TransportLost,
                reason_code: "transport_lost".into(),
                retry_disposition: CompactionRetryDisposition::RetrySameRequest,
            }),
        ));
        facts.push(fact(
            session,
            stream,
            6,
            SessionFactPayload::CompactionRequestClosed(CompactionRequestClosed {
                compaction_request_id: request,
                compaction_id: compaction,
                response_attempt_ordinal: 1,
                outcome: CompactionRequestOutcome::Eof,
                reason_code: "provider_eof".into(),
                recovery_rule_version: None,
            }),
        ));
        facts.push(fact(
            session,
            stream,
            7,
            SessionFactPayload::CompactionAbandoned(CompactionAbandoned {
                compaction_id: compaction,
                reason_code: "provider_eof".into(),
                last_compaction_request_id: Some(request),
                last_response_attempt_ordinal: Some(1),
                recovery_rule_version: 1,
            }),
        ));
        let state = reconstruct(&facts).unwrap();
        assert_eq!(state.context_revision, 0);
        assert!(state.active_turn.is_none());
        assert!(state.route_leases.is_empty());
        assert!(matches!(
            state.compaction_terminals[&compaction],
            CompactionTerminalState::Abandoned { .. }
        ));
    }

    #[test]
    fn recovery_applies_committed_summary_before_turn_recovery() {
        let session = "compaction-recovery";
        let stream = Uuid::new_v4();
        let source = fact(
            session,
            stream,
            2,
            SessionFactPayload::PromptRejected(PromptRejected {
                submission_id: Uuid::new_v4(),
                principal: "operator".into(),
                ingress: "tui".into(),
                reason_code: "fixture".into(),
            }),
        );
        let mut facts = vec![created(session, stream), source.clone()];
        let state = reconstruct(&facts).unwrap();
        let compaction = Uuid::new_v4();
        let request = Uuid::new_v4();
        let summary = Uuid::new_v4();
        let start = compaction_start(
            &state,
            CompactionOwnerScope::SessionIdle,
            CompactionTrigger::ManualIdle,
            source.event_id,
            compaction,
        );
        facts.push(fact(
            session,
            stream,
            3,
            SessionFactPayload::CompactionStarted(start.clone()),
        ));
        facts.push(fact(
            session,
            stream,
            4,
            SessionFactPayload::CompactionRequestPrepared(CompactionRequestPrepared {
                compaction_request_id: request,
                compaction_id: compaction,
                request_ordinal: 0,
                replaces_compaction_request_id: None,
                prompt_template: CompactionPromptTemplate {
                    owner_id: "kernel".into(),
                    owner_generation_id: "v1".into(),
                    content_ref: fake_default_ref("prompt"),
                },
                route: CompactionRoute::SessionIdle {
                    selected_provider_id: "openai".into(),
                    selected_model_id: "gpt".into(),
                    serving_provider_id: "openai".into(),
                    serving_model_id: "gpt".into(),
                    schema_dialect: "open_ai".into(),
                    credential_source_class: "api_key".into(),
                    fallback_reason: None,
                    contribution_generation_id: "provider:openai/builtin-v1".into(),
                    route_policy: "selected_provider_only_v1".into(),
                },
            }),
        ));
        facts.push(committed_summary_fact(
            session, stream, 5, &start, request, summary,
        ));
        let state = reconstruct(&facts).unwrap();
        let recovery = recovery_facts(&state, NOW).unwrap();
        assert_eq!(
            recovery
                .iter()
                .map(|fact| fact.payload.event_type())
                .collect::<Vec<_>>(),
            ["compaction.request_closed", "compaction.applied"]
        );
        let recovered =
            reconstruct(&facts.into_iter().chain(recovery).collect::<Vec<_>>()).unwrap();
        assert_eq!(recovered.context_revision, 1);
        assert!(recovery_facts(&recovered, NOW).unwrap().is_empty());
    }

    #[test]
    fn legacy_pair_is_materialized_once_before_full_spine_boundary() {
        let directory = tempfile::tempdir().unwrap();
        let session_id = "2026-08-23T06-30-00_deadbeef";
        let snapshot = directory.path().join(format!("{session_id}.json"));
        let mut conversation = crate::conversation::ConversationState::new();
        conversation.push_user("legacy prompt".into());
        conversation.save_session(&snapshot).unwrap();
        fs::write(
            snapshot.with_extension("meta.json"),
            serde_json::to_vec(&crate::session::SessionMeta {
                session_id: session_id.into(),
                cwd: directory.path().to_string_lossy().into_owned(),
                created_at: NOW.into(),
                turns: 1,
                tool_calls: 0,
                description: String::new(),
                friendly_name: String::new(),
                last_prompt_snippet: "legacy prompt".into(),
            })
            .unwrap(),
        )
        .unwrap();

        let mut authority = SessionAuthority::open(
            &snapshot,
            session_id,
            "workspace",
            "generation",
            ActorIdentity {
                principal: "test".into(),
                ingress: "test".into(),
            },
            NOW,
        )
        .unwrap();
        let compatibility = conversation.build_llm_view();
        assert!(
            authority
                .import_legacy_compatibility_base(&compatibility, NOW)
                .unwrap()
        );
        assert!(
            !authority
                .import_legacy_compatibility_base(&compatibility, NOW)
                .unwrap()
        );
        assert_eq!(authority.state.lineage_level, AuthorityLineageLevel::Mixed);
        let source = authority
            .state
            .materialized_context_sources
            .values()
            .next()
            .unwrap();
        assert!(is_legacy_compatibility_source(source));
        let mut unrelated = source.clone();
        unrelated.source_identity = "legacy-unrelated-context".into();
        assert!(!is_legacy_compatibility_source(&unrelated));
        assert_eq!(
            authority
                .state
                .materialized_context_sources
                .values()
                .filter(|source| source.source_identity == "legacy-compatibility-base-v1")
                .count(),
            1
        );
        drop(authority);

        fs::remove_file(&snapshot).unwrap();
        fs::remove_file(snapshot.with_extension("meta.json")).unwrap();
        let reopened = SessionAuthority::open(
            &snapshot,
            session_id,
            "workspace",
            "generation",
            ActorIdentity {
                principal: "test".into(),
                ingress: "test".into(),
            },
            NOW,
        )
        .unwrap();
        assert_eq!(reopened.state.lineage_level, AuthorityLineageLevel::Mixed);
        assert_eq!(
            reopened
                .state
                .materialized_context_sources
                .values()
                .filter(|source| source.source_identity == "legacy-compatibility-base-v1")
                .count(),
            1
        );
    }

    #[test]
    fn legacy_import_excludes_existing_semantic_suffix() {
        let directory = tempfile::tempdir().unwrap();
        let session_id = "2026-08-23T06-32-00_aabbccdd";
        let snapshot = directory.path().join(format!("{session_id}.json"));
        let mut authority = SessionAuthority::open(
            &snapshot,
            session_id,
            "workspace",
            "generation",
            ActorIdentity {
                principal: "test".into(),
                ingress: "test".into(),
            },
            NOW,
        )
        .unwrap();
        authority
            .admit_prompt(
                Uuid::new_v4(),
                NOW,
                PromptAdmitted {
                    submission_id: Uuid::new_v4(),
                    prompt_id: Uuid::new_v4(),
                    principal: "operator".into(),
                    ingress: "test".into(),
                    queue_mode: QueueMode::UntilReady,
                    content: PromptContent {
                        text: "semantic prompt".into(),
                        attachments: Vec::new(),
                    },
                    metadata: serde_json::json!({}),
                },
            )
            .unwrap();
        let legacy = crate::bridge::LlmMessage::User {
            content: "legacy prompt".into(),
            images: Vec::new(),
        };
        let compatibility = vec![
            legacy.clone(),
            crate::bridge::LlmMessage::User {
                content: "semantic prompt".into(),
                images: Vec::new(),
            },
            crate::bridge::LlmMessage::Assistant {
                text: vec!["semantic response".into()],
                thinking: Vec::new(),
                tool_calls: Vec::new(),
                raw: None,
            },
        ];

        assert!(
            authority
                .import_legacy_compatibility_base(&compatibility, NOW)
                .unwrap()
        );
        let source = authority
            .state
            .materialized_context_sources
            .values()
            .find(|source| is_legacy_compatibility_source(source))
            .unwrap();
        assert_eq!(
            authority
                .read_content(&source.content_ref, ProjectionClass::Default)
                .unwrap(),
            legacy_compatibility_base_bytes(&[legacy]).unwrap()
        );
    }
}
