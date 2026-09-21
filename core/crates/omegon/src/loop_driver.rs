//! Release-coupled agent loop driver boundary.
//!
//! These ports make session projection, leased inference routing, context
//! assembly, and privileged invocation explicit at turn construction. Their
//! compatibility implementations remain in the integration crate for Slice 4;
//! concrete policy stays behind these boundaries.

use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::bridge::LlmBridge;
use crate::context::ContextManager;
use crate::conversation::ConversationState;
use crate::r#loop::LoopConfig;

#[derive(Clone)]
pub(crate) struct LoopCompatibilityBindings {
    pub(crate) route_setup: LoopRouteSetup,
    pub(crate) invocation_frontend: crate::loop_permission::LoopInvocationFrontend,
    pub(crate) secrets: Option<std::sync::Arc<omegon_secrets::SecretsManager>>,
    pub(crate) permission_policy: Option<crate::permissions::LayeredPermissionPolicy>,
    pub(crate) permission_role: Option<styrene_rbac::Role>,
    pub(crate) invocation_scope: crate::invocation_service::InvocationScope,
    pub(crate) route_step_id: uuid::Uuid,
    pub(crate) drain_late_requests: bool,
    pub(crate) work_snapshot: Option<std::sync::Arc<styrene_work_runtime::WorkSnapshot>>,
    pub(crate) behavior_policy: Option<crate::behavior::BehaviorPolicyBinding>,
    pub(crate) memory_binding: crate::memory_service::MemoryBinding,
    pub(crate) context_compaction: crate::context_compaction_service::ContextCompactionBinding,
}

impl Default for LoopCompatibilityBindings {
    fn default() -> Self {
        Self {
            route_setup: LoopRouteSetup::default(),
            invocation_frontend: crate::loop_permission::LoopInvocationFrontend::default(),
            secrets: None,
            permission_policy: None,
            permission_role: None,
            invocation_scope: crate::invocation_service::InvocationScope::default(),
            route_step_id: uuid::Uuid::new_v4(),
            drain_late_requests: true,
            work_snapshot: None,
            behavior_policy: None,
            memory_binding: Default::default(),
            context_compaction: Default::default(),
        }
    }
}

pub(crate) struct LoopSessionParts<'a> {
    pub(crate) projection: &'a mut ConversationState,
    pub(crate) policy: &'a mut dyn crate::loop_session::LoopSessionPolicyContract,
    pub(crate) advisory_events: &'a broadcast::Sender<omegon_traits::AgentEvent>,
    pub(crate) cancellation: CancellationToken,
    pub(crate) invocation_scope: crate::invocation_service::InvocationScope,
    pub(crate) route_step_id: uuid::Uuid,
    pub(crate) semantic_facts: &'a mut dyn crate::loop_session::LoopSemanticFactContract,
}

impl LoopSessionParts<'_> {
    fn validate_scope(&self) -> anyhow::Result<()> {
        validate_invocation_scope(&self.invocation_scope)
    }
}

fn validate_invocation_scope(
    scope: &crate::invocation_service::InvocationScope,
) -> anyhow::Result<()> {
    if scope.principal.trim().is_empty() {
        anyhow::bail!("loop session contract has no principal identity");
    }
    let session_bound = scope.session_id.is_some();
    let turn_bound = scope.turn_id.is_some();
    let authority_bound = scope.authority.is_some();
    if session_bound || turn_bound || authority_bound {
        if !(session_bound && turn_bound && authority_bound) {
            anyhow::bail!("loop session contract has incomplete durable authority");
        }
        let authority = scope
            .authority
            .as_ref()
            .expect("authority presence checked");
        if authority.session_id() != scope.session_id.as_deref().unwrap_or_default() {
            anyhow::bail!("loop session contract authority belongs to another session");
        }
        if authority.state().active_turn.map(|active| active.turn_id) != scope.turn_id {
            anyhow::bail!("loop session contract targets a stale authority turn");
        }
    }
    Ok(())
}

pub(crate) trait LoopSessionContract: Send {
    fn parts(&mut self) -> LoopSessionParts<'_>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopModelRequestPurpose {
    Initial,
    ContextOverflowRepair,
    ProviderHistoryRepair,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopStepIdentity {
    pub(crate) step_id: uuid::Uuid,
    pub(crate) turn_id: uuid::Uuid,
    pub(crate) step_ordinal: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopModelRequestIdentity {
    pub(crate) request_id: uuid::Uuid,
    pub(crate) step_id: uuid::Uuid,
    pub(crate) turn_id: uuid::Uuid,
    pub(crate) request_ordinal: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopToolCallReceipt {
    pub(crate) tool_call_id: uuid::Uuid,
    pub(crate) call_id: String,
    pub(crate) call_ordinal: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopStepOutcome {
    Continue,
    Finish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopResponseContentKind {
    Text,
    Thinking,
}

#[derive(Debug, Clone)]
pub(crate) struct LoopResponseChunkReceipt {
    pub(crate) content_kind: LoopResponseContentKind,
    pub(crate) content_ref: crate::session_authority::ContentRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopProviderContinuityKind {
    HiddenReasoning,
    OpaqueProviderState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopRequestTerminal {
    ResponseCompleted,
    ProviderFailed,
    Eof,
    Cancelled,
    TimedOut,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopResponseAttemptFailure {
    ProviderError,
    Eof,
    TimedOut,
    TransportLost,
}

pub(crate) trait LoopResponseFactContract: Send + Sync {
    fn fail_attempt(
        &self,
        request: &LoopModelRequestIdentity,
        response_attempt_ordinal: u32,
        failure: LoopResponseAttemptFailure,
        reason_code: &str,
    ) -> anyhow::Result<()>;

    fn append_content(
        &self,
        request: &LoopModelRequestIdentity,
        message_id: uuid::Uuid,
        response_attempt_ordinal: u32,
        content_kind: LoopResponseContentKind,
        chunk_ordinal: u32,
        bytes: &[u8],
    ) -> anyhow::Result<LoopResponseChunkReceipt>;

    #[allow(clippy::too_many_arguments)]
    fn store_continuity(
        &self,
        request: &LoopModelRequestIdentity,
        response_attempt_ordinal: u32,
        serving_provider_id: &str,
        serving_model_id: &str,
        provider_contribution_generation_id: &str,
        kind: LoopProviderContinuityKind,
        allowed_kinds: &[LoopProviderContinuityKind],
        max_blob_bytes: u64,
        bytes: &[u8],
    ) -> anyhow::Result<()>;

    fn commit_message(
        &self,
        request: &LoopModelRequestIdentity,
        message_id: uuid::Uuid,
        response_attempt_ordinal: u32,
        chunks: &[LoopResponseChunkReceipt],
        usage: Option<(u64, u64)>,
        tool_call_count: u32,
    ) -> anyhow::Result<()>;

    fn close_request(
        &self,
        request: &LoopModelRequestIdentity,
        response_attempt_ordinal: u32,
        terminal: LoopRequestTerminal,
        reason_code: &str,
    ) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopToolSchemaLineage {
    pub(crate) composition_generation_id: omegon_traits::RuntimeCompositionGenerationId,
    pub(crate) tools: Vec<LoopToolOwnerLineage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopToolOwnerLineage {
    pub(crate) capability_id: omegon_traits::RuntimeCapabilityId,
    pub(crate) contribution_id: omegon_traits::RuntimeContributionId,
    pub(crate) owner_generation_id: omegon_traits::RuntimeContributionGenerationId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopRoute {
    pub(crate) selected_model: String,
    pub(crate) serving_model: String,
    pub(crate) provider_id: String,
    pub(crate) schema_dialect: String,
    pub(crate) contribution_generation_id: String,
    pub(crate) normalizer_contribution_id: omegon_traits::RuntimeContributionId,
    pub(crate) normalizer_generation_id: omegon_traits::RuntimeContributionGenerationId,
}

#[async_trait::async_trait]
pub(crate) trait LoopRouteSetupContract: Send + Sync {
    async fn serving_model(&self) -> Option<String>;
    async fn invalidate(&self, _model: &str, _detail: &str) -> bool {
        false
    }
    async fn prepare(
        &self,
        route: &LoopRoute,
        events: &broadcast::Sender<omegon_traits::AgentEvent>,
    );
}

#[derive(Clone, Default)]
pub(crate) struct LoopRouteSetup {
    adapter: Option<std::sync::Arc<dyn LoopRouteSetupContract>>,
}

impl LoopRouteSetup {
    pub(crate) fn new(adapter: std::sync::Arc<dyn LoopRouteSetupContract>) -> Self {
        Self {
            adapter: Some(adapter),
        }
    }

    pub(crate) async fn serving_model(&self) -> Option<String> {
        match self.adapter.as_ref() {
            Some(adapter) => adapter.serving_model().await,
            None => None,
        }
    }

    pub(crate) async fn invalidate(&self, model: &str, detail: &str) -> bool {
        match self.adapter.as_ref() {
            Some(adapter) => adapter.invalidate(model, detail).await,
            None => false,
        }
    }

    pub(crate) async fn prepare(
        &self,
        route: &LoopRoute,
        events: &broadcast::Sender<omegon_traits::AgentEvent>,
    ) -> bool {
        let Some(adapter) = self.adapter.as_ref() else {
            return false;
        };
        adapter.prepare(route, events).await;
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopRouteFailure {
    ContextOverflow,
    MalformedHistory,
    Exhausted,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopRouteRepair {
    CompactOverflow,
    RepairMalformedHistory,
}

pub(crate) fn route_repair(failure: LoopRouteFailure) -> Option<LoopRouteRepair> {
    match failure {
        LoopRouteFailure::ContextOverflow => Some(LoopRouteRepair::CompactOverflow),
        LoopRouteFailure::MalformedHistory => Some(LoopRouteRepair::RepairMalformedHistory),
        LoopRouteFailure::Exhausted | LoopRouteFailure::Other => None,
    }
}

pub(crate) struct LoopRouteRequest<'a> {
    pub(crate) route: &'a LoopRoute,
    pub(crate) system_prompt: &'a str,
    pub(crate) messages: &'a [crate::bridge::LlmMessage],
    pub(crate) tools: &'a [omegon_traits::ToolDefinition],
    pub(crate) events: &'a broadcast::Sender<omegon_traits::AgentEvent>,
    pub(crate) max_retries: u32,
    pub(crate) retry_delay_ms: u64,
    pub(crate) cancel_keeps_prompt: Option<&'a std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub(crate) scope: &'a crate::invocation_service::InvocationScope,
    pub(crate) step_id: uuid::Uuid,
    pub(crate) semantic_request: Option<&'a LoopModelRequestIdentity>,
    pub(crate) response_facts: Option<&'a dyn LoopResponseFactContract>,
}

pub(crate) struct LoopCompactionRequest<'a> {
    pub(crate) payload: &'a str,
    pub(crate) selected_model: &'a str,
    pub(crate) scope: &'a crate::invocation_service::InvocationScope,
    pub(crate) step_id: uuid::Uuid,
    pub(crate) authority: &'a dyn LoopCompactionAuthority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopCompactionTrigger {
    ContextPressure,
    ContextOverflow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopCompactionRouteEvidence {
    pub(crate) lease_id: Option<uuid::Uuid>,
    pub(crate) selected_provider_id: String,
    pub(crate) selected_model_id: String,
    pub(crate) serving_provider_id: String,
    pub(crate) serving_model_id: String,
    pub(crate) schema_dialect: String,
    pub(crate) credential_source_class: String,
    pub(crate) fallback_reason: Option<String>,
    pub(crate) contribution_generation_id: String,
    pub(crate) route_policy: String,
    pub(crate) endpoint_id: Option<String>,
    pub(crate) adapter_id: Option<String>,
    pub(crate) inventory_generation: Option<u64>,
}

pub(crate) trait LoopCompactionAuthority: Send + Sync {
    fn provider_payload<'a>(&'a self, fallback: &'a str) -> &'a str;
    fn compaction_request_id(&self) -> Option<uuid::Uuid>;
    fn is_idle(&self) -> bool;
    fn prepare(&self, evidence: LoopCompactionRouteEvidence) -> anyhow::Result<()>;
    fn commit_done(&self, summary: &str) -> anyhow::Result<()>;
    fn fail(
        &self,
        outcome: crate::session_authority::CompactionRequestOutcome,
        reason: &str,
    ) -> anyhow::Result<()>;
}

struct SessionlessLoopCompactionAuthority;

impl LoopCompactionAuthority for SessionlessLoopCompactionAuthority {
    fn provider_payload<'a>(&'a self, fallback: &'a str) -> &'a str {
        fallback
    }
    fn compaction_request_id(&self) -> Option<uuid::Uuid> {
        None
    }
    fn is_idle(&self) -> bool {
        false
    }
    fn prepare(&self, _evidence: LoopCompactionRouteEvidence) -> anyhow::Result<()> {
        Ok(())
    }
    fn commit_done(&self, _summary: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn fail(
        &self,
        _outcome: crate::session_authority::CompactionRequestOutcome,
        _reason: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopRouteNotice {
    pub(crate) provider: String,
    pub(crate) reason: String,
    pub(crate) message: String,
}

pub(crate) struct LoopRouteDispatch {
    pub(crate) message: crate::conversation::AssistantMessage,
    pub(crate) stop_notice: Option<LoopRouteNotice>,
    pub(crate) durable_route: Option<LoopDurableRouteIdentity>,
    pub(crate) completed_request: Option<LoopModelRequestIdentity>,
    pub(crate) response_attempt_ordinal: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopDurableRouteIdentity {
    pub(crate) lease_id: uuid::Uuid,
    pub(crate) request_id: uuid::Uuid,
}

#[async_trait::async_trait]
pub(crate) trait LoopRouteContract: Sync {
    async fn startup_route(&self) -> LoopRoute;
    async fn turn_route(&self) -> LoopRoute;
    async fn prepare(
        &self,
        route: &LoopRoute,
        events: &broadcast::Sender<omegon_traits::AgentEvent>,
    );
    fn validate_request_capabilities(
        &self,
        tools: &[omegon_traits::ToolDefinition],
    ) -> anyhow::Result<()>;
    async fn dispatch(&self, request: LoopRouteRequest<'_>) -> anyhow::Result<LoopRouteDispatch>;
    async fn compact(&self, request: LoopCompactionRequest<'_>) -> anyhow::Result<String>;
    fn failure_kind(&self, error: &anyhow::Error) -> LoopRouteFailure;
    fn provider_id(&self, model: &str) -> String;
}

#[async_trait::async_trait]
pub(crate) trait LoopContextContract: Send {
    fn resolve_windows(&mut self, config: &LoopConfig) -> crate::loop_context::LoopContextWindows;
    async fn prepare_turn(
        &mut self,
        conversation: &mut ConversationState,
        runtime: &mut crate::bus::EventBus,
        turn: u32,
        tools: &[omegon_traits::ToolDefinition],
        context_window: usize,
    ) -> crate::loop_context::LoopContextAssembly;
    fn compose(
        &mut self,
        conversation: &ConversationState,
        tools: &[omegon_traits::ToolDefinition],
        context_window: usize,
    ) -> crate::loop_context::LoopContextAssembly;
    fn messages(&self, conversation: &ConversationState) -> Vec<crate::bridge::LlmMessage>;
    fn default_composition(&self, context_window: usize) -> omegon_traits::ContextComposition;
    fn record_activity(&mut self, calls: &[crate::conversation::ToolCall]);
    async fn pressure_compaction_plan(
        &self,
        snapshot: crate::context_compaction_service::ContextCompactionSnapshotV1,
        cancellation: CancellationToken,
    ) -> anyhow::Result<Option<crate::loop_context::LoopCompactionPlan>>;
    async fn overflow_compaction_plan(
        &self,
        snapshot: crate::context_compaction_service::ContextCompactionSnapshotV1,
        cancellation: CancellationToken,
    ) -> anyhow::Result<Option<crate::loop_context::LoopCompactionPlan>>;
    fn begin_compaction(
        &self,
        plan: &crate::loop_context::LoopCompactionPlan,
        scope: &crate::invocation_service::InvocationScope,
        step_id: uuid::Uuid,
        trigger: LoopCompactionTrigger,
    ) -> anyhow::Result<Box<dyn LoopCompactionAuthority>> {
        let _ = (plan, scope, step_id, trigger);
        Ok(Box::new(SessionlessLoopCompactionAuthority))
    }
    fn apply_compaction(
        &self,
        conversation: &mut ConversationState,
        plan: crate::loop_context::LoopCompactionPlan,
        summary: String,
    );
    fn decay_failed_compaction(
        &self,
        conversation: &mut ConversationState,
        plan: &crate::loop_context::LoopCompactionPlan,
    );
    fn repair_overflow_without_plan(&self, conversation: &mut ConversationState) -> usize;
    fn repair_malformed_history(&self, conversation: &mut ConversationState);
    fn tighten_decay(&self, conversation: &mut ConversationState);
    fn context_update(
        &self,
        config: &LoopConfig,
        conversation: &ConversationState,
        context_window: usize,
    ) -> crate::loop_context::LoopContextUpdate;
}

pub(crate) struct LoopToolSurfaceRequest<'a> {
    pub(crate) turn: u32,
    pub(crate) used_tools: &'a std::collections::HashSet<String>,
    pub(crate) final_response_turn: bool,
    pub(crate) constrained: bool,
}

pub(crate) struct LoopInvocationBatchRequest<'a> {
    pub(crate) calls: &'a [crate::conversation::ToolCall],
    pub(crate) tool_surface: &'a [omegon_traits::ToolDefinition],
    pub(crate) events: &'a broadcast::Sender<omegon_traits::AgentEvent>,
    pub(crate) cancellation: CancellationToken,
    pub(crate) dispatch_allowed: bool,
}

pub(crate) struct LoopInvocationBatchOutcome {
    pub(crate) results: Vec<crate::conversation::ToolResultEntry>,
    pub(crate) terminals: Vec<LoopInvocationTerminal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoopInvocationTerminal {
    Denied { reason_code: String },
    NotDispatched { reason_code: String },
    AuthorityLinked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopInvocationDeclaration {
    pub(crate) effects: Vec<omegon_traits::RuntimeEffect>,
    pub(crate) parallel_safe: bool,
    pub(crate) best_effort_rollback: bool,
}

pub(crate) struct LoopToolOwnerRequest<'a> {
    pub(crate) lease: &'a crate::invocation_service::ExecutionLease,
    pub(crate) execution_tool_name: &'a str,
    pub(crate) visible_call_id: &'a str,
    pub(crate) execution_args: serde_json::Value,
    pub(crate) cancel: CancellationToken,
    pub(crate) sink: omegon_traits::ToolProgressSink,
    pub(crate) context: omegon_traits::ToolExecutionContext,
}

pub(crate) struct LoopToolApprovalRequest<'a> {
    pub(crate) pending: crate::invocation_service::PendingInvocationApproval,
    pub(crate) visible_call_id: &'a str,
    pub(crate) visible_tool_name: &'a str,
    pub(crate) events: &'a broadcast::Sender<omegon_traits::AgentEvent>,
    pub(crate) cancel: CancellationToken,
    pub(crate) permission_log: &'a mut Vec<crate::loop_permission::PermissionRecord>,
}

pub(crate) struct LoopToolPresentationRequest<'a> {
    pub(crate) result: anyhow::Result<omegon_traits::ToolResult>,
    pub(crate) lease: &'a crate::invocation_service::ExecutionLease,
    pub(crate) visible_call_id: &'a str,
    pub(crate) visible_tool_name: &'a str,
    pub(crate) execution_tool_name: &'a str,
    pub(crate) execution_args: serde_json::Value,
    pub(crate) events: &'a broadcast::Sender<omegon_traits::AgentEvent>,
    pub(crate) cancel: CancellationToken,
    pub(crate) sink: omegon_traits::ToolProgressSink,
    pub(crate) context: omegon_traits::ToolExecutionContext,
    pub(crate) permission_log: &'a mut Vec<crate::loop_permission::PermissionRecord>,
    pub(crate) invocation_scope: &'a crate::invocation_service::InvocationScope,
}

pub(crate) enum LoopToolPresentation {
    Resolved(omegon_traits::ToolResult, bool),
    Unhandled(anyhow::Error),
}

pub(crate) struct LoopToolOwnerRetryRequest<'a> {
    pub(crate) lease: &'a crate::invocation_service::ExecutionLease,
    pub(crate) execution_tool_name: &'a str,
    pub(crate) visible_call_id: &'a str,
    pub(crate) execution_args: serde_json::Value,
    pub(crate) cancel: CancellationToken,
    pub(crate) sink: omegon_traits::ToolProgressSink,
    pub(crate) context: omegon_traits::ToolExecutionContext,
}

pub(crate) struct LoopInternalInvocationRequest<'a> {
    pub(crate) name: &'a str,
    pub(crate) call_id: &'a str,
    pub(crate) args: serde_json::Value,
    pub(crate) cancel: CancellationToken,
    pub(crate) principal: &'a str,
    pub(crate) authority_scope: Option<&'a crate::invocation_service::InvocationScope>,
}

pub(crate) enum LoopTurnAdvisory {
    Notify {
        message: String,
        level: omegon_traits::NotifyLevel,
    },
    InjectSystemMessage {
        content: String,
    },
    EmitAgentEvent {
        event: Box<omegon_traits::AgentEvent>,
    },
}

pub(crate) struct LoopLifecycleRequest<'a> {
    pub(crate) conversation: &'a mut ConversationState,
    pub(crate) route: &'a dyn LoopRouteContract,
    pub(crate) context: &'a mut dyn LoopContextContract,
    pub(crate) events: &'a broadcast::Sender<omegon_traits::AgentEvent>,
    pub(crate) cancellation: CancellationToken,
    pub(crate) active_route: &'a LoopRoute,
    pub(crate) invocation_scope: &'a crate::invocation_service::InvocationScope,
    pub(crate) route_step_id: uuid::Uuid,
}

pub(crate) struct LoopFinalizationRequest<'a> {
    pub(crate) events: &'a broadcast::Sender<omegon_traits::AgentEvent>,
    pub(crate) cancellation: CancellationToken,
    pub(crate) turns: u32,
    pub(crate) tool_calls: u32,
    pub(crate) elapsed: std::time::Duration,
    pub(crate) initial_prompt: Option<String>,
    pub(crate) outcome_summary: Option<String>,
}

pub(crate) enum LoopToolOwnerHandoff {
    Delegated(anyhow::Result<omegon_traits::ToolResult>),
    Local(anyhow::Result<omegon_traits::ToolResult>),
}

#[async_trait::async_trait]
pub(crate) trait LoopInvocationContract: Send + Sync {
    fn drain_late_requests(&self) -> bool;
    fn tool_definitions(
        &self,
        request: LoopToolSurfaceRequest<'_>,
    ) -> Vec<omegon_traits::ToolDefinition>;
    fn tool_schema_lineage(
        &self,
        _tools: &[omegon_traits::ToolDefinition],
    ) -> anyhow::Result<LoopToolSchemaLineage> {
        anyhow::bail!("loop invocation contract has no tool schema lineage")
    }
    async fn dispatch_batch(
        &mut self,
        request: LoopInvocationBatchRequest<'_>,
    ) -> LoopInvocationBatchOutcome;
    fn runtime(&mut self) -> &mut crate::bus::EventBus;
    fn runtime_ref(&self) -> &crate::bus::EventBus;
    fn memory_binding(&self) -> Option<&crate::memory_service::MemoryBinding> {
        None
    }
    fn tool_declaration(&self, name: &str) -> Option<LoopInvocationDeclaration>;
    fn admit_tool(
        &self,
        execution_tool_name: &str,
        request: crate::invocation_service::InvocationAdmissionRequest<'_>,
    ) -> crate::invocation_service::InvocationAdmission;
    fn persist_tool_dispatch(
        &self,
        lease: &crate::invocation_service::ExecutionLease,
        call_id: &str,
        invocation_name: &str,
    ) -> Result<(), crate::invocation_service::InvocationDenial>;
    async fn acquire_tool_approval(
        &self,
        request: LoopToolApprovalRequest<'_>,
    ) -> Result<
        crate::invocation_service::ExecutionLease,
        crate::invocation_service::InvocationDenial,
    >;
    fn tool_execution_context(&self) -> omegon_traits::ToolExecutionContext;
    async fn handoff_tool_owner(&self, request: LoopToolOwnerRequest<'_>) -> LoopToolOwnerHandoff;
    async fn present_tool_owner_result(
        &self,
        request: LoopToolPresentationRequest<'_>,
    ) -> LoopToolPresentation;
    async fn retry_tool_owner(
        &self,
        request: LoopToolOwnerRetryRequest<'_>,
    ) -> anyhow::Result<omegon_traits::ToolResult>;
    async fn dispatch_internal(
        &self,
        request: LoopInternalInvocationRequest<'_>,
    ) -> anyhow::Result<omegon_traits::ToolResult>;
    async fn process_lifecycle_requests(
        &mut self,
        request: LoopLifecycleRequest<'_>,
    ) -> Vec<LoopTurnAdvisory>;
    async fn finalize_session(&mut self, request: LoopFinalizationRequest<'_>);
    fn settle_tool_owner(
        &self,
        lease: &crate::invocation_service::ExecutionLease,
        result: &omegon_traits::ToolResult,
        is_error: bool,
        cancelled: bool,
    ) -> Result<(), crate::invocation_service::InvocationDenial>;
    fn classify_tool_owner_completion(
        &self,
        lease: &crate::invocation_service::ExecutionLease,
        error: &anyhow::Error,
    ) -> Result<bool, crate::invocation_service::InvocationDenial>;
}

struct LoopSessionPort<'a> {
    projection: &'a mut ConversationState,
    policy: crate::loop_session::LoopSessionCompatibilityAdapter,
    advisory_events: &'a broadcast::Sender<omegon_traits::AgentEvent>,
    cancellation: CancellationToken,
    invocation_scope: crate::invocation_service::InvocationScope,
    route_step_id: uuid::Uuid,
    semantic_facts: crate::loop_session::LoopSemanticFactAdapter,
}

impl LoopSessionContract for LoopSessionPort<'_> {
    fn parts(&mut self) -> LoopSessionParts<'_> {
        LoopSessionParts {
            projection: self.projection,
            policy: &mut self.policy,
            advisory_events: self.advisory_events,
            cancellation: self.cancellation.clone(),
            invocation_scope: self.invocation_scope.clone(),
            route_step_id: self.route_step_id,
            semantic_facts: &mut self.semantic_facts,
        }
    }
}

struct LoopRoutePort<'a> {
    leased_bridge: &'a dyn LlmBridge,
    service: std::sync::Arc<dyn crate::provider_route_service::ProviderRouteServiceContract>,
    setup: LoopRouteSetup,
    policy: crate::provider_route_service::LoopRoutePolicy,
    baseline_options: std::sync::Mutex<Option<crate::bridge::StreamOptions>>,
    active_options: std::sync::Mutex<Option<crate::bridge::StreamOptions>>,
}

impl From<crate::provider_route_service::LoopRoute> for LoopRoute {
    fn from(route: crate::provider_route_service::LoopRoute) -> Self {
        Self {
            selected_model: route.selected_model,
            serving_model: route.serving_model,
            provider_id: route.provider_id,
            schema_dialect: route.schema_dialect,
            contribution_generation_id: route.contribution_generation_id,
            normalizer_contribution_id: route.normalizer_contribution_id,
            normalizer_generation_id: route.normalizer_generation_id,
        }
    }
}

impl From<crate::provider_route_service::ProviderStopNotice> for LoopRouteNotice {
    fn from(notice: crate::provider_route_service::ProviderStopNotice) -> Self {
        Self {
            provider: notice.provider,
            reason: notice.reason,
            message: notice.message,
        }
    }
}

#[async_trait::async_trait]
impl LoopRouteContract for LoopRoutePort<'_> {
    async fn startup_route(&self) -> LoopRoute {
        let route = self
            .service
            .startup_route(self.leased_bridge, &self.setup, &self.policy)
            .await;
        *self.baseline_options.lock().expect("route options lock") = Some(route.options.clone());
        *self.active_options.lock().expect("route options lock") = Some(route.options.clone());
        route.into()
    }

    async fn turn_route(&self) -> LoopRoute {
        let base = self
            .baseline_options
            .lock()
            .expect("route options lock")
            .clone()
            .expect("startup route must precede turn route");
        let route = self
            .service
            .turn_route(self.leased_bridge, &self.setup, &self.policy, &base)
            .await;
        *self.active_options.lock().expect("route options lock") = Some(route.options.clone());
        route.into()
    }

    async fn prepare(
        &self,
        route: &LoopRoute,
        events: &broadcast::Sender<omegon_traits::AgentEvent>,
    ) {
        let route = self.provider_route(route);
        self.service.prepare(&route, &self.setup, events).await;
    }

    fn validate_request_capabilities(
        &self,
        tools: &[omegon_traits::ToolDefinition],
    ) -> anyhow::Result<()> {
        let options = self
            .active_options
            .lock()
            .expect("route options lock")
            .clone()
            .expect("turn route must precede request validation");
        self.leased_bridge
            .validate_request_capabilities(tools, &options)
    }

    async fn dispatch(&self, request: LoopRouteRequest<'_>) -> anyhow::Result<LoopRouteDispatch> {
        let provider_route = self.provider_route(request.route);
        let dispatch = self
            .service
            .dispatch(
                self.leased_bridge,
                crate::provider_route_service::LoopRouteRequest {
                    route: &provider_route,
                    system_prompt: request.system_prompt,
                    messages: request.messages,
                    tools: request.tools,
                    events: request.events,
                    max_retries: request.max_retries,
                    retry_delay_ms: request.retry_delay_ms,
                    cancel_keeps_prompt: request.cancel_keeps_prompt,
                    scope: request.scope,
                    step_id: request.step_id,
                    semantic_request: request.semantic_request,
                    response_facts: request.response_facts,
                },
            )
            .await;
        let dispatch = match dispatch {
            Ok(dispatch) => dispatch,
            Err(error) => {
                self.reconcile_route_failure(&error).await;
                return Err(error);
            }
        };
        let stop_notice = self
            .service
            .stop_notice(&provider_route, &dispatch.message.raw)
            .map(Into::into);
        Ok(LoopRouteDispatch {
            message: dispatch.message,
            stop_notice,
            durable_route: dispatch
                .durable_route
                .map(|identity| LoopDurableRouteIdentity {
                    lease_id: identity.lease_id,
                    request_id: identity.request_id,
                }),
            completed_request: request.semantic_request.cloned(),
            response_attempt_ordinal: dispatch.response_attempt_ordinal,
        })
    }

    async fn compact(&self, request: LoopCompactionRequest<'_>) -> anyhow::Result<String> {
        let options = self
            .baseline_options
            .lock()
            .expect("route options lock")
            .clone()
            .expect("startup route must precede compaction");
        self.service
            .compact(
                self.leased_bridge,
                crate::provider_route_service::LoopCompactionRequest {
                    payload: request.payload,
                    options: &options,
                    selected_model: request.selected_model,
                    scope: request.scope,
                    step_id: request.step_id,
                    authority: Some(request.authority),
                },
            )
            .await
    }

    fn failure_kind(&self, error: &anyhow::Error) -> LoopRouteFailure {
        match self.service.failure_kind(error) {
            crate::provider_route_service::LoopRouteFailure::ContextOverflow => {
                LoopRouteFailure::ContextOverflow
            }
            crate::provider_route_service::LoopRouteFailure::MalformedHistory => {
                LoopRouteFailure::MalformedHistory
            }
            crate::provider_route_service::LoopRouteFailure::Exhausted => {
                LoopRouteFailure::Exhausted
            }
            crate::provider_route_service::LoopRouteFailure::Other => LoopRouteFailure::Other,
        }
    }

    fn provider_id(&self, model: &str) -> String {
        self.service.provider_id(model)
    }
}

impl LoopRoutePort<'_> {
    async fn reconcile_route_failure(&self, error: &anyhow::Error) {
        let Some(unavailable) = error.downcast_ref::<crate::bridge::ProviderRouteUnavailable>()
        else {
            return;
        };
        if self
            .setup
            .invalidate(&unavailable.model, &unavailable.message)
            .await
            && let Some(settings) = &self.policy.settings
            && let Ok(mut settings) = settings.lock()
            && settings.model == unavailable.model
        {
            settings.provider_connected = false;
        }
    }

    fn provider_route(&self, route: &LoopRoute) -> crate::provider_route_service::LoopRoute {
        let options = self
            .active_options
            .lock()
            .expect("route options lock")
            .clone()
            .expect("route options must be initialized");
        crate::provider_route_service::LoopRoute {
            selected_model: route.selected_model.clone(),
            serving_model: route.serving_model.clone(),
            provider_id: route.provider_id.clone(),
            schema_dialect: route.schema_dialect.clone(),
            contribution_generation_id: route.contribution_generation_id.clone(),
            normalizer_contribution_id: route.normalizer_contribution_id.clone(),
            normalizer_generation_id: route.normalizer_generation_id.clone(),
            options,
        }
    }

    async fn validate(&self) -> anyhow::Result<()> {
        let expected = self.setup.serving_model().await;
        let Some(actual) = self.leased_bridge.serving_model_hint() else {
            if expected.is_none() && self.leased_bridge.route_is_disconnected() {
                return Ok(());
            }
            anyhow::bail!("loop route contract bridge has no serving identity");
        };
        let Some(expected) = expected.as_deref() else {
            return Ok(());
        };
        if self.service.canonical_model_spec(actual) != self.service.canonical_model_spec(expected)
        {
            anyhow::bail!(
                "loop route contract bridge identity {actual} disagrees with serving route {expected}"
            );
        }
        Ok(())
    }
}

struct LoopContextPort<'a> {
    adapter: crate::loop_context::LoopContextCompatibilityAdapter<'a>,
}

#[async_trait::async_trait]
impl LoopContextContract for LoopContextPort<'_> {
    fn resolve_windows(&mut self, config: &LoopConfig) -> crate::loop_context::LoopContextWindows {
        self.adapter.resolve_windows(config)
    }

    async fn prepare_turn(
        &mut self,
        conversation: &mut ConversationState,
        runtime: &mut crate::bus::EventBus,
        turn: u32,
        tools: &[omegon_traits::ToolDefinition],
        context_window: usize,
    ) -> crate::loop_context::LoopContextAssembly {
        self.adapter
            .prepare_turn(conversation, runtime, turn, tools, context_window)
            .await
    }

    fn compose(
        &mut self,
        conversation: &ConversationState,
        tools: &[omegon_traits::ToolDefinition],
        context_window: usize,
    ) -> crate::loop_context::LoopContextAssembly {
        self.adapter.compose(conversation, tools, context_window)
    }

    fn messages(&self, conversation: &ConversationState) -> Vec<crate::bridge::LlmMessage> {
        self.adapter.messages(conversation)
    }

    fn default_composition(&self, context_window: usize) -> omegon_traits::ContextComposition {
        crate::loop_context::default_context_composition(context_window)
    }

    fn record_activity(&mut self, calls: &[crate::conversation::ToolCall]) {
        self.adapter.record_activity(calls);
    }

    async fn pressure_compaction_plan(
        &self,
        snapshot: crate::context_compaction_service::ContextCompactionSnapshotV1,
        cancellation: CancellationToken,
    ) -> anyhow::Result<Option<crate::loop_context::LoopCompactionPlan>> {
        self.adapter
            .pressure_compaction_plan(snapshot, cancellation)
            .await
            .map_err(|error| anyhow::anyhow!("{error:?}"))
    }

    async fn overflow_compaction_plan(
        &self,
        snapshot: crate::context_compaction_service::ContextCompactionSnapshotV1,
        cancellation: CancellationToken,
    ) -> anyhow::Result<Option<crate::loop_context::LoopCompactionPlan>> {
        self.adapter
            .overflow_compaction_plan(snapshot, cancellation)
            .await
            .map_err(|error| anyhow::anyhow!("{error:?}"))
    }

    fn begin_compaction(
        &self,
        plan: &crate::loop_context::LoopCompactionPlan,
        scope: &crate::invocation_service::InvocationScope,
        step_id: uuid::Uuid,
        trigger: LoopCompactionTrigger,
    ) -> anyhow::Result<Box<dyn LoopCompactionAuthority>> {
        let Some(authority) = scope.authority.clone() else {
            if scope.session_id.is_some() || scope.turn_id.is_some() {
                anyhow::bail!("partial session scope cannot compact");
            }
            return Ok(Box::new(SessionlessLoopCompactionAuthority));
        };
        let turn_id = scope
            .turn_id
            .ok_or_else(|| anyhow::anyhow!("turn compaction has no turn identity"))?;
        let trigger = match trigger {
            LoopCompactionTrigger::ContextPressure => {
                crate::session_authority::CompactionTrigger::ContextPressure
            }
            LoopCompactionTrigger::ContextOverflow => {
                crate::session_authority::CompactionTrigger::ContextOverflow
            }
        };
        let compaction = crate::session_compaction::SessionCompaction::begin_turn(
            authority, turn_id, step_id, trigger, plan,
        )?
        .ok_or_else(|| anyhow::anyhow!("exact authority compaction input is unavailable"))?;
        Ok(Box::new(compaction))
    }

    fn apply_compaction(
        &self,
        conversation: &mut ConversationState,
        plan: crate::loop_context::LoopCompactionPlan,
        summary: String,
    ) {
        self.adapter.apply_compaction(conversation, plan, summary);
    }

    fn decay_failed_compaction(
        &self,
        conversation: &mut ConversationState,
        plan: &crate::loop_context::LoopCompactionPlan,
    ) {
        self.adapter.decay_failed_compaction(conversation, plan);
    }

    fn repair_overflow_without_plan(&self, conversation: &mut ConversationState) -> usize {
        self.adapter.repair_overflow_without_plan(conversation)
    }

    fn repair_malformed_history(&self, conversation: &mut ConversationState) {
        self.adapter.repair_malformed_history(conversation);
    }

    fn tighten_decay(&self, conversation: &mut ConversationState) {
        self.adapter.tighten_decay(conversation);
    }

    fn context_update(
        &self,
        config: &LoopConfig,
        conversation: &ConversationState,
        context_window: usize,
    ) -> crate::loop_context::LoopContextUpdate {
        self.adapter
            .context_update(config, conversation, context_window)
    }
}

pub(crate) struct LoopInvocationPort<'a> {
    runtime: &'a mut crate::bus::EventBus,
    frontend: crate::loop_permission::LoopInvocationFrontend,
    cwd: std::path::PathBuf,
    secrets: Option<std::sync::Arc<omegon_secrets::SecretsManager>>,
    settings: Option<crate::settings::SharedSettings>,
    permission_policy: Option<crate::permissions::LayeredPermissionPolicy>,
    permission_role: Option<styrene_rbac::Role>,
    invocation_scope: crate::invocation_service::InvocationScope,
    drain_late_requests: bool,
    memory_binding: crate::memory_service::MemoryBinding,
}

impl<'a> LoopInvocationPort<'a> {
    pub(crate) fn new(runtime: &'a mut crate::bus::EventBus) -> Self {
        Self {
            runtime,
            frontend: crate::loop_permission::LoopInvocationFrontend::default(),
            cwd: std::env::current_dir().unwrap_or_default(),
            secrets: None,
            settings: None,
            permission_policy: None,
            permission_role: None,
            invocation_scope: crate::invocation_service::InvocationScope::default(),
            drain_late_requests: true,
            memory_binding: Default::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_drain_late_requests(&mut self, enabled: bool) {
        self.drain_late_requests = enabled;
    }

    fn for_loop(runtime: &'a mut crate::bus::EventBus, config: &LoopConfig) -> Self {
        Self {
            runtime,
            frontend: config.compatibility.invocation_frontend.clone(),
            cwd: config.cwd.clone(),
            secrets: config.compatibility.secrets.clone(),
            settings: config.settings.clone(),
            permission_policy: config.compatibility.permission_policy.clone(),
            permission_role: config.compatibility.permission_role,
            invocation_scope: config.compatibility.invocation_scope.clone(),
            drain_late_requests: config.compatibility.drain_late_requests,
            memory_binding: config.compatibility.memory_binding.clone(),
        }
    }
}

#[async_trait::async_trait]
impl LoopInvocationContract for LoopInvocationPort<'_> {
    fn drain_late_requests(&self) -> bool {
        self.drain_late_requests
    }

    fn tool_definitions(
        &self,
        request: LoopToolSurfaceRequest<'_>,
    ) -> Vec<omegon_traits::ToolDefinition> {
        if request.final_response_turn {
            Vec::new()
        } else if request.constrained {
            self.runtime
                .tool_definitions_lean(request.turn, request.used_tools)
        } else {
            self.runtime
                .tool_definitions_lazy(true, request.turn, request.used_tools)
        }
    }

    fn tool_schema_lineage(
        &self,
        tools: &[omegon_traits::ToolDefinition],
    ) -> anyhow::Result<LoopToolSchemaLineage> {
        let composition_generation_id = self
            .runtime
            .composition_generation_id()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("tool schema capture has no composition generation"))?;
        let tools = tools
            .iter()
            .map(|tool| {
                let owner = self
                    .runtime
                    .resolve_invocation(omegon_traits::RuntimeInvocationKind::Tool, &tool.name)?;
                Ok(LoopToolOwnerLineage {
                    capability_id: owner.capability_id,
                    contribution_id: owner.contribution_id,
                    owner_generation_id: owner.owner_generation_id,
                })
            })
            .collect::<Result<Vec<_>, crate::invocation_service::InvocationDenial>>()
            .map_err(|denial| anyhow::anyhow!(denial.message))?;
        Ok(LoopToolSchemaLineage {
            composition_generation_id,
            tools,
        })
    }

    async fn dispatch_batch(
        &mut self,
        request: LoopInvocationBatchRequest<'_>,
    ) -> LoopInvocationBatchOutcome {
        if !request.dispatch_allowed {
            return LoopInvocationBatchOutcome {
                results: request
                    .calls
                    .iter()
                    .map(|call| crate::conversation::ToolResultEntry {
                        call_id: call.id.clone(),
                        tool_name: call.name.clone(),
                        content: vec![omegon_traits::ContentBlock::Text {
                            text: format!(
                                "Tool `{}` was not dispatched because the tool-execution limit was reached.",
                                call.name
                            ),
                        }],
                        is_error: true,
                        args_summary: crate::invocation_batch::summarize_tool_args(
                            &call.name,
                            &call.arguments,
                        ),
                    })
                    .collect(),
                terminals: request
                    .calls
                    .iter()
                    .map(|_| LoopInvocationTerminal::NotDispatched {
                        reason_code: "tool_execution_limit".into(),
                    })
                    .collect(),
            };
        }
        for call in request.calls {
            let capabilities = request
                .tool_surface
                .iter()
                .find(|tool| tool.name == call.name)
                .map(|tool| tool.capabilities.clone())
                .unwrap_or_default();
            self.runtime.emit(&omegon_traits::BusEvent::ToolStart {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.arguments.clone(),
                capabilities,
            });
        }

        let settings_permission_snapshot = self.settings.as_ref().and_then(|settings| {
            settings.lock().ok().map(|settings| {
                (
                    crate::permissions::layered_policy_from_settings(&settings),
                    crate::permissions::styrene_role_from_settings(&settings),
                )
            })
        });
        let (permission_policy, permission_role) = settings_permission_snapshot
            .as_ref()
            .map(|(policy, role)| (Some(policy), *role))
            .unwrap_or((self.permission_policy.as_ref(), self.permission_role));
        let dispatch = crate::invocation_batch::dispatch_tools(
            self,
            request.calls,
            request.events,
            request.cancellation,
            &self.cwd,
            self.secrets.as_deref(),
            permission_policy,
            permission_role,
            &self.invocation_scope,
        )
        .await;

        for permission in dispatch.permission_decisions {
            self.runtime
                .emit(&omegon_traits::BusEvent::PermissionDecision {
                    tool_name: permission.tool_name,
                    path: permission.path,
                    decision: permission.decision,
                    kind: permission.kind,
                    persistence: permission.persistence,
                    grant_path: permission.grant_path,
                });
        }
        let terminals = request
            .calls
            .iter()
            .zip(&dispatch.results)
            .map(|(call, result)| {
                let linked =
                    self.invocation_scope
                        .authority
                        .as_ref()
                        .is_some_and(|authority| {
                            authority.state().invocations.values().any(
                                |invocation| match invocation {
                                    crate::session_authority::InvocationState::Prepared {
                                        preparation,
                                    }
                                    | crate::session_authority::InvocationState::Dispatched {
                                        preparation,
                                        ..
                                    }
                                    | crate::session_authority::InvocationState::Acknowledged {
                                        preparation,
                                        ..
                                    }
                                    | crate::session_authority::InvocationState::DurableUnknown {
                                        preparation,
                                        ..
                                    }
                                    | crate::session_authority::InvocationState::DurableSettled {
                                        preparation,
                                        ..
                                    } => preparation.call_id == call.id,
                                    crate::session_authority::InvocationState::Registered {
                                        registration,
                                    }
                                    | crate::session_authority::InvocationState::Unknown {
                                        registration,
                                        ..
                                    }
                                    | crate::session_authority::InvocationState::Settled {
                                        registration,
                                        ..
                                    } => registration.call_id == call.id,
                                },
                            )
                        });
                if linked {
                    LoopInvocationTerminal::AuthorityLinked
                } else if result
                    .content
                    .iter()
                    .filter_map(omegon_traits::ContentBlock::as_text)
                    .any(|text| text.starts_with("Skipped "))
                {
                    LoopInvocationTerminal::NotDispatched {
                        reason_code: "batch_rollback".into(),
                    }
                } else {
                    LoopInvocationTerminal::Denied {
                        reason_code: "admission_denied".into(),
                    }
                }
            })
            .collect();
        LoopInvocationBatchOutcome {
            results: dispatch.results,
            terminals,
        }
    }

    fn runtime(&mut self) -> &mut crate::bus::EventBus {
        self.runtime
    }

    fn runtime_ref(&self) -> &crate::bus::EventBus {
        self.runtime
    }

    fn memory_binding(&self) -> Option<&crate::memory_service::MemoryBinding> {
        Some(&self.memory_binding)
    }

    fn tool_declaration(&self, name: &str) -> Option<LoopInvocationDeclaration> {
        self.runtime
            .resolve_invocation(omegon_traits::RuntimeInvocationKind::Tool, name)
            .ok()
            .map(|resolved| LoopInvocationDeclaration {
                effects: resolved.effects,
                parallel_safe: resolved.execution.parallelism
                    == omegon_traits::RuntimeParallelism::ParallelSafe
                    && resolved.execution.transaction
                        == omegon_traits::RuntimeTransactionBehavior::None,
                best_effort_rollback: resolved.execution.transaction
                    == omegon_traits::RuntimeTransactionBehavior::BestEffortRollback,
            })
    }

    fn admit_tool(
        &self,
        execution_tool_name: &str,
        request: crate::invocation_service::InvocationAdmissionRequest<'_>,
    ) -> crate::invocation_service::InvocationAdmission {
        crate::invocation_service::InvocationService::admit_tool(
            self.runtime,
            execution_tool_name,
            request,
        )
    }

    fn persist_tool_dispatch(
        &self,
        lease: &crate::invocation_service::ExecutionLease,
        call_id: &str,
        invocation_name: &str,
    ) -> Result<(), crate::invocation_service::InvocationDenial> {
        lease.claim_dispatch(call_id, invocation_name)?;
        self.runtime.validate_execution_lease(
            lease,
            call_id,
            omegon_traits::RuntimeInvocationKind::Tool,
            invocation_name,
        )?;
        lease.persist_dispatched()
    }

    async fn acquire_tool_approval(
        &self,
        request: LoopToolApprovalRequest<'_>,
    ) -> Result<
        crate::invocation_service::ExecutionLease,
        crate::invocation_service::InvocationDenial,
    > {
        self.frontend.acquire_tool_approval(request).await
    }

    fn tool_execution_context(&self) -> omegon_traits::ToolExecutionContext {
        self.frontend.tool_execution_context()
    }

    async fn handoff_tool_owner(&self, request: LoopToolOwnerRequest<'_>) -> LoopToolOwnerHandoff {
        if let Some(context) = self.frontend.host_context() {
            let timeout = request.lease.execution_timeout(&request.execution_args);
            let delegated = tokio::time::timeout(
                timeout,
                crate::host_context::try_delegate_to_host(
                    context,
                    request.execution_tool_name,
                    &request.execution_args,
                    &request.lease.dispatch_metadata(),
                    &request.lease.invocation_control(),
                ),
            )
            .await;
            match delegated {
                Ok(Some(result)) => return LoopToolOwnerHandoff::Delegated(result),
                Ok(None) => {}
                Err(_) => {
                    return LoopToolOwnerHandoff::Delegated(Err(
                        crate::invocation_service::UnknownCompletionError {
                            reason: format!(
                                "host-delegated tool '{}' timed out after {} seconds",
                                request.execution_tool_name,
                                timeout.as_secs()
                            ),
                        }
                        .into(),
                    ));
                }
            }
        }

        LoopToolOwnerHandoff::Local(
            self.retry_tool_owner(LoopToolOwnerRetryRequest {
                lease: request.lease,
                execution_tool_name: request.execution_tool_name,
                visible_call_id: request.visible_call_id,
                execution_args: request.execution_args,
                cancel: request.cancel,
                sink: request.sink,
                context: request.context,
            })
            .await,
        )
    }

    async fn present_tool_owner_result(
        &self,
        request: LoopToolPresentationRequest<'_>,
    ) -> LoopToolPresentation {
        self.frontend.present_tool_owner_result(self, request).await
    }

    async fn retry_tool_owner(
        &self,
        request: LoopToolOwnerRetryRequest<'_>,
    ) -> anyhow::Result<omegon_traits::ToolResult> {
        self.runtime
            .execute_tool_with_lease(
                request.lease,
                request.execution_tool_name,
                request.visible_call_id,
                request.execution_args,
                request.cancel,
                request.sink,
                request.context,
            )
            .await
    }

    async fn dispatch_internal(
        &self,
        request: LoopInternalInvocationRequest<'_>,
    ) -> anyhow::Result<omegon_traits::ToolResult> {
        let (session_id, turn_id, authority) = request.authority_scope.map_or_else(
            || (None, None, None),
            |scope| {
                (
                    scope.session_id.clone(),
                    scope.turn_id,
                    scope.authority.clone(),
                )
            },
        );
        let scope = crate::invocation_service::InvocationScope {
            principal: request.principal.into(),
            principal_class: omegon_traits::RuntimePrincipalClass::Internal,
            surface: omegon_traits::RuntimeSurface::Internal,
            session_id,
            turn_id,
            authority,
            tool_budget: None,
        };
        self.runtime
            .invoke_internal(
                request.name,
                request.call_id,
                request.args,
                request.cancel,
                scope,
            )
            .await
    }

    async fn process_lifecycle_requests(
        &mut self,
        request: LoopLifecycleRequest<'_>,
    ) -> Vec<LoopTurnAdvisory> {
        crate::loop_lifecycle::process_turn_requests(self, request).await
    }

    async fn finalize_session(&mut self, request: LoopFinalizationRequest<'_>) {
        crate::loop_lifecycle::finalize_session(self, request).await;
    }

    fn settle_tool_owner(
        &self,
        lease: &crate::invocation_service::ExecutionLease,
        result: &omegon_traits::ToolResult,
        is_error: bool,
        cancelled: bool,
    ) -> Result<(), crate::invocation_service::InvocationDenial> {
        let outcome = invocation_outcome(result, is_error, cancelled);
        let terminal = if is_error {
            crate::invocation_service::LeaseTerminal::Failed
        } else {
            crate::invocation_service::LeaseTerminal::Completed
        };
        lease.persist_settlement(outcome)?;
        lease.close(terminal);
        Ok(())
    }

    fn classify_tool_owner_completion(
        &self,
        lease: &crate::invocation_service::ExecutionLease,
        error: &anyhow::Error,
    ) -> Result<bool, crate::invocation_service::InvocationDenial> {
        if error
            .downcast_ref::<crate::invocation_service::UnknownCompletionError>()
            .is_none()
        {
            return Ok(false);
        }
        lease.persist_unknown("owner_completion_unknown")?;
        lease.revoke();
        Ok(true)
    }
}

fn invocation_outcome(
    result: &omegon_traits::ToolResult,
    is_error: bool,
    cancelled: bool,
) -> crate::session_authority::InvocationOutcome {
    if cancelled {
        return crate::session_authority::InvocationOutcome::Cancelled;
    }
    match result
        .details
        .get("status")
        .and_then(serde_json::Value::as_str)
    {
        Some("timed_out") => crate::session_authority::InvocationOutcome::TimedOut,
        Some("cancelled") => crate::session_authority::InvocationOutcome::Cancelled,
        Some("revoked") => crate::session_authority::InvocationOutcome::Revoked,
        _ if is_error => crate::session_authority::InvocationOutcome::Failed,
        _ => crate::session_authority::InvocationOutcome::Completed,
    }
}

pub(crate) struct LoopDriverTurn<'a> {
    session: LoopSessionPort<'a>,
    route: LoopRoutePort<'a>,
    context: LoopContextPort<'a>,
    invocations: LoopInvocationPort<'a>,
    config: &'a LoopConfig,
}

impl<'a> LoopDriverTurn<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        bridge: &'a dyn LlmBridge,
        bus: &'a mut crate::bus::EventBus,
        context: &'a mut ContextManager,
        conversation: &'a mut ConversationState,
        events: &'a broadcast::Sender<omegon_traits::AgentEvent>,
        cancellation: CancellationToken,
        config: &'a LoopConfig,
        route_service: std::sync::Arc<
            dyn crate::provider_route_service::ProviderRouteServiceContract,
        >,
    ) -> Self {
        let semantic_facts = crate::loop_session::LoopSemanticFactAdapter::new(
            &config.compatibility.invocation_scope,
        );
        Self {
            session: LoopSessionPort {
                projection: conversation,
                policy: crate::loop_session::LoopSessionCompatibilityAdapter::new(
                    config.compatibility.work_snapshot.clone(),
                    config.compatibility.behavior_policy.clone(),
                ),
                advisory_events: events,
                cancellation,
                invocation_scope: config.compatibility.invocation_scope.clone(),
                route_step_id: config.compatibility.route_step_id,
                semantic_facts,
            },
            route: LoopRoutePort {
                leased_bridge: bridge,
                service: route_service,
                setup: config.compatibility.route_setup.clone(),
                policy: crate::provider_route_service::LoopRoutePolicy {
                    selected_model: config.model.clone(),
                    bridge_model: config.bridge_model.clone(),
                    extended_context: config.extended_context,
                    settings: config.settings.clone(),
                },
                baseline_options: std::sync::Mutex::new(None),
                active_options: std::sync::Mutex::new(None),
            },
            context: LoopContextPort {
                adapter: crate::loop_context::LoopContextCompatibilityAdapter::new(
                    context,
                    config.compatibility.context_compaction.clone(),
                ),
            },
            invocations: LoopInvocationPort::for_loop(bus, config),
            config,
        }
    }

    pub(crate) fn route(&self) -> &dyn LoopRouteContract {
        &self.route
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ReleaseCoupledLoopDriver;

#[async_trait::async_trait]
pub(crate) trait LoopDriverContract: Send + Sync {
    async fn run(&self, turn: LoopDriverTurn<'_>) -> LoopDriverExecution;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopTerminalProposal {
    pub(crate) outcome: crate::runtime_turn::RuntimeTurnOutcome,
    pub(crate) reason_code: &'static str,
}

impl LoopTerminalProposal {
    pub(crate) fn into_intent(
        self,
        identity: crate::runtime_turn::RuntimeTurnIdentity,
    ) -> crate::runtime_turn::LoopTerminalIntent {
        crate::runtime_turn::LoopTerminalIntent {
            identity,
            outcome: self.outcome,
            reason_code: self.reason_code.into(),
        }
    }
}

pub(crate) struct LoopDriverExecution {
    pub(crate) result: anyhow::Result<()>,
    pub(crate) terminal: LoopTerminalProposal,
}

impl ReleaseCoupledLoopDriver {
    pub(crate) async fn run(self, turn: LoopDriverTurn<'_>) -> LoopDriverExecution {
        let LoopDriverTurn {
            mut session,
            route,
            mut context,
            mut invocations,
            config,
        } = turn;
        let result = async {
            session.parts().validate_scope()?;
            route.validate().await?;
            crate::r#loop::run_release_coupled(
                &mut session,
                &route,
                &mut context,
                &mut invocations,
                config,
            )
            .await
        }
        .await;
        let failure_kind = result.as_ref().err().map(|error| route.failure_kind(error));
        let terminal = match &result {
            Ok(()) => LoopTerminalProposal {
                outcome: crate::runtime_turn::RuntimeTurnOutcome::Completed,
                reason_code: "loop_completed",
            },
            Err(_) if failure_kind == Some(LoopRouteFailure::Exhausted) => LoopTerminalProposal {
                outcome: crate::runtime_turn::RuntimeTurnOutcome::Failed,
                reason_code: "provider_exhausted",
            },
            Err(_) => LoopTerminalProposal {
                outcome: crate::runtime_turn::RuntimeTurnOutcome::Failed,
                reason_code: "loop_failed",
            },
        };
        LoopDriverExecution { result, terminal }
    }
}

#[async_trait::async_trait]
impl LoopDriverContract for ReleaseCoupledLoopDriver {
    async fn run(&self, turn: LoopDriverTurn<'_>) -> LoopDriverExecution {
        ReleaseCoupledLoopDriver.run(turn).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SurfaceTools;

    #[async_trait::async_trait]
    impl omegon_traits::ToolProvider for SurfaceTools {
        fn tools(&self) -> Vec<omegon_traits::ToolDefinition> {
            [
                crate::tool_registry::core::BASH,
                crate::tool_registry::web_search::WEB_SEARCH,
            ]
            .into_iter()
            .map(|name| omegon_traits::ToolDefinition {
                name: name.into(),
                label: name.into(),
                description: name.into(),
                parameters: serde_json::json!({"type": "object"}),
                capabilities: Vec::new(),
            })
            .collect()
        }

        async fn execute(
            &self,
            _tool_name: &str,
            _call_id: &str,
            _args: serde_json::Value,
            _cancel: CancellationToken,
        ) -> anyhow::Result<omegon_traits::ToolResult> {
            unreachable!("surface selection does not execute tools")
        }
    }

    struct AdmittedTool;

    #[async_trait::async_trait]
    impl omegon_traits::ToolProvider for AdmittedTool {
        fn tools(&self) -> Vec<omegon_traits::ToolDefinition> {
            vec![omegon_traits::ToolDefinition {
                name: "remote_read".into(),
                label: "remote_read".into(),
                description: "read a remote resource".into(),
                parameters: serde_json::json!({"type": "object"}),
                capabilities: Vec::new(),
            }]
        }

        fn runtime_tool_policy(
            &self,
            _tool_name: &str,
        ) -> Option<omegon_traits::RuntimeToolPolicy> {
            Some(omegon_traits::RuntimeToolPolicy {
                effects: vec![omegon_traits::RuntimeEffect::NetworkAccess],
                execution: omegon_traits::RuntimeExecutionPolicy {
                    principals: vec![omegon_traits::RuntimePrincipalClass::Model],
                    timeout_class: omegon_traits::RuntimeTimeoutClass::Immediate,
                    retry_class: omegon_traits::RuntimeRetryClass::Never,
                    idempotency: omegon_traits::RuntimeIdempotency::Idempotent,
                    deduplication: omegon_traits::RuntimeDeduplication::Unsupported,
                    parallelism: omegon_traits::RuntimeParallelism::ParallelSafe,
                    transaction: omegon_traits::RuntimeTransactionBehavior::None,
                    mutation_fence: None,
                    max_attempts: None,
                },
            })
        }

        async fn execute(
            &self,
            _tool_name: &str,
            _call_id: &str,
            _args: serde_json::Value,
            _cancel: CancellationToken,
        ) -> anyhow::Result<omegon_traits::ToolResult> {
            unreachable!("admission contract test does not execute the owner")
        }
    }

    struct UnknownOwner;

    #[async_trait::async_trait]
    impl omegon_traits::ToolProvider for UnknownOwner {
        fn tools(&self) -> Vec<omegon_traits::ToolDefinition> {
            vec![omegon_traits::ToolDefinition {
                name: "unknown_owner".into(),
                label: "unknown_owner".into(),
                description: "returns an ambiguous owner completion".into(),
                parameters: serde_json::json!({"type": "object"}),
                capabilities: vec![omegon_traits::ToolCapability::RepoInspection],
            }]
        }

        async fn execute(
            &self,
            _tool_name: &str,
            _call_id: &str,
            _args: serde_json::Value,
            _cancel: CancellationToken,
        ) -> anyhow::Result<omegon_traits::ToolResult> {
            Err(crate::invocation_service::UnknownCompletionError {
                reason: "owner transport closed after acknowledgement".into(),
            }
            .into())
        }
    }

    #[derive(Clone)]
    struct InternalInvocationProbe {
        observed: std::sync::Arc<std::sync::Mutex<Option<(String, serde_json::Value, bool)>>>,
    }

    #[async_trait::async_trait]
    impl omegon_traits::ToolProvider for InternalInvocationProbe {
        fn tools(&self) -> Vec<omegon_traits::ToolDefinition> {
            vec![omegon_traits::ToolDefinition {
                name: "internal_probe".into(),
                label: "internal_probe".into(),
                description: "records an internal invocation".into(),
                parameters: serde_json::json!({"type": "object"}),
                capabilities: Vec::new(),
            }]
        }

        async fn execute(
            &self,
            _tool_name: &str,
            call_id: &str,
            args: serde_json::Value,
            cancel: CancellationToken,
        ) -> anyhow::Result<omegon_traits::ToolResult> {
            *self.observed.lock().unwrap() = Some((call_id.into(), args, cancel.is_cancelled()));
            Ok(omegon_traits::ToolResult {
                content: vec![omegon_traits::ContentBlock::Text {
                    text: "internal result".into(),
                }],
                details: serde_json::json!({"status": "ok"}),
            })
        }
    }

    fn durable_invocation_scope() -> (
        tempfile::TempDir,
        crate::session_authority::SessionAuthorityHandle,
        crate::invocation_service::InvocationScope,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let recorded_at = "2026-08-21T12:00:00Z";
        let mut authority = crate::session_authority::SessionAuthority::open(
            &directory.path().join("session.json"),
            "session-loop-driver",
            "workspace-loop-driver",
            "composition:test",
            crate::session_authority::ActorIdentity {
                principal: "operator".into(),
                ingress: "test".into(),
            },
            recorded_at,
        )
        .unwrap();
        let prompt_id = uuid::Uuid::new_v4();
        authority
            .admit_prompt(
                uuid::Uuid::new_v4(),
                recorded_at,
                crate::session_authority::PromptAdmitted {
                    submission_id: uuid::Uuid::new_v4(),
                    prompt_id,
                    principal: "operator".into(),
                    ingress: "test".into(),
                    queue_mode: crate::session_authority::QueueMode::UntilReady,
                    content: crate::session_authority::PromptContent {
                        text: "run".into(),
                        attachments: vec![],
                    },
                    metadata: serde_json::json!({}),
                },
            )
            .unwrap();
        let turn_id = uuid::Uuid::new_v4();
        authority
            .start_turn(uuid::Uuid::new_v4(), recorded_at, turn_id, prompt_id)
            .unwrap();
        let authority = crate::session_authority::SessionAuthorityHandle::new(authority);
        let scope = crate::invocation_service::InvocationScope {
            session_id: Some("session-loop-driver".into()),
            turn_id: Some(turn_id),
            authority: Some(authority.clone()),
            ..Default::default()
        };
        (directory, authority, scope)
    }

    fn invocation_port_with_surface_tools() -> crate::bus::EventBus {
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::adapter::ToolAdapter::new(
            "surface-tools",
            Box::new(SurfaceTools),
        )));
        bus.finalize();
        bus
    }

    #[test]
    fn release_coupled_driver_is_the_only_constructible_driver() {
        let driver = ReleaseCoupledLoopDriver;
        assert_eq!(format!("{driver:?}"), "ReleaseCoupledLoopDriver");
    }

    #[test]
    fn driver_turn_requires_all_four_ports() {
        fn assert_contracts<Session, Route, Context, Invocations>()
        where
            Session: LoopSessionContract,
            Route: LoopRouteContract,
            Context: LoopContextContract,
            Invocations: LoopInvocationContract,
        {
        }

        assert_contracts::<
            LoopSessionPort<'static>,
            LoopRoutePort<'static>,
            LoopContextPort<'static>,
            LoopInvocationPort<'static>,
        >();
    }

    #[test]
    fn invocation_contract_selects_the_turn_tool_surface() {
        let mut bus = invocation_port_with_surface_tools();
        let port = LoopInvocationPort::new(&mut bus);
        let mut used_tools = std::collections::HashSet::new();
        let names = |defs: Vec<omegon_traits::ToolDefinition>| {
            defs.into_iter().map(|tool| tool.name).collect::<Vec<_>>()
        };

        let full = names(port.tool_definitions(LoopToolSurfaceRequest {
            turn: 1,
            used_tools: &used_tools,
            final_response_turn: false,
            constrained: false,
        }));
        assert!(full.contains(&crate::tool_registry::core::BASH.into()));
        assert!(full.contains(&crate::tool_registry::web_search::WEB_SEARCH.into()));

        let constrained = names(port.tool_definitions(LoopToolSurfaceRequest {
            turn: 1,
            used_tools: &used_tools,
            final_response_turn: false,
            constrained: true,
        }));
        assert!(constrained.contains(&crate::tool_registry::core::BASH.into()));
        assert!(!constrained.contains(&crate::tool_registry::web_search::WEB_SEARCH.into()));

        used_tools.insert(crate::tool_registry::web_search::WEB_SEARCH.into());
        let used = names(port.tool_definitions(LoopToolSurfaceRequest {
            turn: 2,
            used_tools: &used_tools,
            final_response_turn: false,
            constrained: false,
        }));
        assert!(used.contains(&crate::tool_registry::web_search::WEB_SEARCH.into()));

        assert!(
            port.tool_definitions(LoopToolSurfaceRequest {
                turn: 2,
                used_tools: &used_tools,
                final_response_turn: true,
                constrained: false,
            })
            .is_empty()
        );
    }

    #[test]
    fn invocation_contract_owns_declaration_admission_and_dispatch_persistence() {
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::adapter::ToolAdapter::new(
            "admitted-tool",
            Box::new(AdmittedTool),
        )));
        bus.finalize();
        let port = LoopInvocationPort::new(&mut bus);

        let declaration = port
            .tool_declaration("remote_read")
            .expect("accepted declaration");
        assert_eq!(
            declaration.effects,
            vec![omegon_traits::RuntimeEffect::NetworkAccess]
        );
        assert!(declaration.parallel_safe);
        assert!(!declaration.best_effort_rollback);

        let args = serde_json::json!({});
        let admission = port.admit_tool(
            "remote_read",
            crate::invocation_service::InvocationAdmissionRequest {
                call_id: "call-1",
                visible_tool_name: "remote_read",
                args: &args,
                scope: crate::invocation_service::InvocationScope::default(),
                permission_policy: None,
                permission_role: None,
            },
        );
        let crate::invocation_service::InvocationAdmission::Lease(lease) = admission else {
            panic!("accepted tool should receive an execution lease")
        };
        port.persist_tool_dispatch(&lease, "call-1", "remote_read")
            .expect("current accepted declaration should validate");
        let denial = port
            .persist_tool_dispatch(&lease, "call-1", "remote_read")
            .expect_err("a lease can only be claimed once");
        assert_eq!(
            denial.code,
            crate::invocation_service::InvocationDenialCode::LeaseClosed
        );
    }

    #[tokio::test]
    async fn owner_unknown_completion_is_acknowledged_classified_and_terminalized() {
        let (_directory, authority, scope) = durable_invocation_scope();
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::adapter::ToolAdapter::new(
            "unknown-owner",
            Box::new(UnknownOwner),
        )));
        bus.finalize();
        let port = LoopInvocationPort::new(&mut bus);
        let args = serde_json::json!({});
        let crate::invocation_service::InvocationAdmission::Lease(lease) = port.admit_tool(
            "unknown_owner",
            crate::invocation_service::InvocationAdmissionRequest {
                call_id: "unknown-call",
                visible_tool_name: "unknown_owner",
                args: &args,
                scope,
                permission_policy: None,
                permission_role: None,
            },
        ) else {
            panic!("unknown owner should be admitted");
        };

        port.persist_tool_dispatch(&lease, "unknown-call", "unknown_owner")
            .unwrap();
        let handoff = port
            .handoff_tool_owner(LoopToolOwnerRequest {
                lease: &lease,
                execution_tool_name: "unknown_owner",
                visible_call_id: "unknown-call",
                execution_args: args,
                cancel: CancellationToken::new(),
                sink: omegon_traits::ToolProgressSink::noop(),
                context: omegon_traits::ToolExecutionContext::default(),
            })
            .await;
        let LoopToolOwnerHandoff::Local(Err(error)) = handoff else {
            panic!("owner should return an unknown local completion");
        };
        assert!(authority.state().invocations.values().any(|state| {
            matches!(state, crate::session_authority::InvocationState::Acknowledged { preparation, .. }
                if preparation.call_id == "unknown-call")
        }));

        assert!(port.classify_tool_owner_completion(&lease, &error).unwrap());
        assert_eq!(
            lease.terminal(),
            crate::invocation_service::LeaseTerminal::Revoked
        );
        assert!(authority.state().invocations.values().any(|state| {
            matches!(state, crate::session_authority::InvocationState::DurableUnknown {
                preparation,
                acknowledgement: Some(_),
                classification,
                ..
            } if preparation.call_id == "unknown-call"
                && classification.reason_code == "owner_completion_unknown")
        }));
    }

    #[tokio::test]
    async fn internal_invocation_contract_preserves_call_cancellation_scope_and_result() {
        let (_directory, authority, scope) = durable_invocation_scope();
        let observed = std::sync::Arc::new(std::sync::Mutex::new(None));
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::adapter::ToolAdapter::new(
            "internal-probe",
            Box::new(InternalInvocationProbe {
                observed: observed.clone(),
            }),
        )));
        bus.register_internal_tool("internal_probe", "internal-probe");
        bus.finalize();
        let port = LoopInvocationPort::new(&mut bus);
        let cancel = CancellationToken::new();
        cancel.cancel();

        let result = port
            .dispatch_internal(LoopInternalInvocationRequest {
                name: "internal_probe",
                call_id: "late-call-17",
                args: serde_json::json!({"payload": 42}),
                cancel,
                principal: "kernel:late-probe",
                authority_scope: Some(&scope),
            })
            .await
            .unwrap();

        assert_eq!(result.content[0].as_text(), Some("internal result"));
        assert_eq!(
            observed.lock().unwrap().as_ref(),
            Some(&(
                "late-call-17".into(),
                serde_json::json!({"payload": 42}),
                true,
            ))
        );
        assert!(authority.state().invocations.values().any(|state| {
            matches!(state, crate::session_authority::InvocationState::DurableSettled {
                preparation,
                settlement,
                ..
            } if preparation.call_id == "late-call-17"
                && preparation.principal == "kernel:late-probe"
                && preparation.principal_class == omegon_traits::RuntimePrincipalClass::Internal
                && preparation.surface == omegon_traits::RuntimeSurface::Internal
                && settlement.outcome == crate::session_authority::InvocationOutcome::Completed)
        }));
    }

    #[test]
    fn every_compiled_host_uses_an_owner_captured_execution_binding() {
        for (name, source, required_capture) in [
            (
                "main daemon/interactive/headless/bounded",
                include_str!("main.rs"),
                "active_execution_capture",
            ),
            (
                "interactive worker",
                include_str!("interactive_coordinator.rs"),
                "execution_capture.execute(",
            ),
            (
                "ACP",
                include_str!("acp_worker.rs"),
                "active_execution_capture",
            ),
            (
                "Sentry",
                include_str!("sentry/executor.rs"),
                "SessionExecutionOwner::immutable_at_boot",
            ),
        ] {
            assert!(
                source.contains(required_capture),
                "{name} does not retain and use its required execution capture"
            );
            assert!(
                source.contains(".execute("),
                "{name} does not execute through its retained capture"
            );
            for forbidden in [
                "ReleaseCoupledLoopDriver",
                "LoopDriverTurn::new",
                "r#loop::run(",
                "run_release_coupled(",
                "ProviderRouteService",
                ".start_turn(",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "{name} contains execution-owner bypass {forbidden:?}"
                );
            }
        }

        let supervisor = include_str!("runtime_supervisor.rs");
        assert!(supervisor.contains("start_turn_and_capture("));
        assert!(supervisor.contains("active_execution = Some(start.capture)"));
        assert!(!supervisor.contains("authority.start_turn("));
        let supervisor_production = supervisor
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("runtime supervisor production source");
        assert!(
            !supervisor_production.contains("commit_pending_at_quiescence"),
            "runtime supervisor must not auto-commit pending replacement on closure or start"
        );

        for (name, source, required_binding) in [
            (
                "quick completion",
                include_str!("providers.rs"),
                "boot_execution_binding()",
            ),
            (
                "smoke",
                include_str!("smoke.rs"),
                "boot_execution_binding()",
            ),
            (
                "sessionless compaction",
                include_str!("control_runtime.rs"),
                "boot_execution_binding()",
            ),
        ] {
            assert!(
                source.contains(required_binding),
                "{name} does not use the immutable boot binding"
            );
            assert!(
                !source.contains("ProviderRouteService"),
                "{name} independently constructs a provider route service"
            );
        }
    }

    #[test]
    fn production_loop_contains_no_concrete_provider_or_transport_policy() {
        let production = production_loop_source();
        let production = production.to_lowercase();

        for forbidden in [
            "anthropic",
            "openai",
            "ollama",
            "gemini",
            "llmevent",
            "classify_upstream_error_for_provider",
            ".stream(",
            "provider_stop_reason",
            ".tool_definitions_lean(",
            ".tool_definitions_lazy(",
            ".resolve_invocation(",
            "invocationservice::admit",
            ".validate_execution_lease(",
            ".claim_dispatch(",
            ".execute_tool_with_lease(",
            ".persist_dispatched(",
            ".persist_settlement(",
            ".persist_unknown(",
            "lease.close(",
            "lease.revoke(",
            "try_delegate_to_host(",
        ] {
            assert!(
                !production.contains(forbidden),
                "production loop.rs contains route policy marker {forbidden:?}"
            );
        }
    }

    #[test]
    fn production_loop_route_boundary_exposes_only_neutral_contracts() {
        let production = production_loop_source();
        let route_loop = production
            .split_once("pub(crate) async fn run_release_coupled")
            .expect("release-coupled loop")
            .1;

        for forbidden in [
            "provider_route_service",
            "StreamOptions",
            "LlmBridge",
            "RouteController",
            ".raw",
            "provider_stop_reason",
        ] {
            assert!(
                !route_loop.contains(forbidden),
                "production loop route boundary exposes {forbidden:?}"
            );
        }
    }

    #[test]
    fn slice_5_loop_emission_uses_only_typed_neutral_ports() {
        let production = production_loop_source();
        let route_loop = production
            .split_once("pub(crate) async fn run_release_coupled")
            .expect("release-coupled loop")
            .1;

        assert!(route_loop.contains("semantic_facts.start_step()?"));
        assert!(route_loop.contains("semantic_facts.prepare_model_request"));
        assert!(route_loop.contains("semantic_facts.record_tool_calls"));
        assert!(route_loop.contains("semantic_facts.record_tool_results"));
        assert!(route_loop.contains("semantic_facts.close_step"));
        assert!(route_loop.contains("semantic_request: semantic_request.as_ref()"));
        for concrete in [
            "SessionAuthority",
            "StepStarted",
            "ModelRequestPrepared",
            "ModelRequestRouteJoined",
        ] {
            assert!(
                !route_loop.contains(concrete),
                "production loop imports concrete authority type {concrete}"
            );
        }
    }

    #[test]
    fn slice_5_tool_fact_order_cannot_be_bypassed() {
        let production = production_loop_source();
        let call = production.find("semantic_facts.record_tool_calls").unwrap();
        let close = production[call..]
            .find("semantic_facts.close_request")
            .unwrap()
            + call;
        let dispatch = production[close..].find(".dispatch_batch(").unwrap() + close;
        let result = production[dispatch..]
            .find("semantic_facts.record_tool_results")
            .unwrap()
            + dispatch;
        let projection = production[result..]
            .find("conversation.push_tool_result")
            .unwrap()
            + result;
        let step = production[result..]
            .find("semantic_facts.close_step")
            .unwrap()
            + result;
        assert!(call < close && close < dispatch && dispatch < result && result < projection);
        assert!(result < step && step < projection);
    }

    #[test]
    fn semantic_emission_is_production_active_only_for_complete_authority() {
        let source = include_str!("loop_driver.rs");
        assert!(!source.contains(&["staged", "semantic", "emission"].join("_")));
        let session_source = include_str!("loop_session.rs");
        assert!(session_source.contains("(None, None, None) => Self::Disabled"));
        assert!(session_source.contains("Self::Invalid("));
        assert!(!session_source.contains("production semantic emission remains staged off"));
    }

    #[test]
    fn production_loop_config_contains_no_concrete_route_compatibility_types() {
        let production = production_loop_source();

        for forbidden in [
            "provider_route_service",
            "RouteController",
            "RouteWarmupHandle",
            "LlmBridge",
            "StreamOptions",
            "default_loop_model",
            "LoopRouteSetup",
            "LoopInvocationFrontend",
            "LayeredPermissionPolicy",
            "SecretsManager",
            "permission_policy",
            "permission_role",
            "invocation_frontend",
            "drain_post_loop_requests",
        ] {
            assert!(
                !production.contains(forbidden),
                "production loop.rs contains concrete route compatibility marker {forbidden:?}"
            );
        }
    }

    #[test]
    fn route_failure_arbitration_repairs_only_context_failures() {
        assert_eq!(
            route_repair(LoopRouteFailure::ContextOverflow),
            Some(LoopRouteRepair::CompactOverflow)
        );
        assert_eq!(
            route_repair(LoopRouteFailure::MalformedHistory),
            Some(LoopRouteRepair::RepairMalformedHistory)
        );
        assert_eq!(route_repair(LoopRouteFailure::Exhausted), None);
        assert_eq!(route_repair(LoopRouteFailure::Other), None);
    }

    fn production_loop_source() -> String {
        let source = include_str!("loop.rs");
        let (prefix, rest) = source
            .split_once("#[cfg(test)]\nmod legacy_route_policy_tests")
            .expect("legacy test-policy boundary");
        let (_, production_and_tests) = rest
            .split_once("#[cfg(test)]\nuse legacy_route_policy_tests::*;")
            .expect("legacy test-policy end boundary");
        let production_tail = production_and_tests
            .split_once("#[cfg(test)]\nmod tests")
            .map_or(production_and_tests, |(production, _)| production);
        format!("{prefix}{production_tail}")
    }

    #[test]
    fn production_loop_uses_contract_for_owner_dispatch_lifecycle() {
        let production = production_loop_source();

        for forbidden in [
            ".invoke_internal(",
            "RuntimePrincipalClass::Internal",
            "RuntimeSurface::Internal",
            "execute_tool_with_lease(",
            "try_delegate_to_host(",
            "lease.invocation_control()",
            "lease.persist_dispatched(",
            "lease.persist_settlement(",
            "lease.persist_unknown(",
            "lease.close(",
            "lease.revoke(",
        ] {
            assert!(
                !production.contains(forbidden),
                "production loop.rs bypasses the invocation contract with {forbidden:?}"
            );
        }
    }

    #[test]
    fn production_loop_contains_no_frontend_permission_presentation_policy() {
        let production = production_loop_source().to_lowercase();
        for forbidden in [
            "acp",
            "tui",
            "flynt",
            "host_context",
            "agentevent::permissionrequest",
            "agentevent::operatorwaitrequest",
            "pathpermissionerror",
            "operatorwaitrequired",
            "trust_directory",
            "wait_for_permission_response",
            "request_permission(",
            "std::sync::mpsc::",
        ] {
            assert!(
                !production.contains(forbidden),
                "production loop.rs regained frontend permission policy marker {forbidden:?}"
            );
        }
    }

    #[test]
    fn production_loop_contains_no_invocation_batching_or_rollback_helpers() {
        let production = production_loop_source();
        for forbidden in [
            "fn dispatch_tools(",
            "fn dispatch_single_tool(",
            "fn dispatch_edit_batch(",
            "fn declaration_allows_parallel(",
            "fn declaration_allows_rollback(",
            "fn extract_mutation_path(",
            "fn normalize_tool_result_content(",
            ".buffer_unordered(",
            "Auto-rollback:",
        ] {
            assert!(
                !production.contains(forbidden),
                "production loop.rs regained invocation batching policy {forbidden:?}"
            );
        }
    }

    #[test]
    fn invocation_batch_does_not_callback_into_loop_orchestration() {
        let source = include_str!("invocation_batch.rs");
        assert!(!source.contains("crate::r#loop::"));
    }

    #[test]
    fn route_adapter_does_not_callback_into_loop_policy() {
        let source = include_str!("provider_route_service.rs");
        assert!(!source.contains("crate::r#loop::"));
        assert!(!source.contains("LoopConfig"));
    }

    #[test]
    fn production_loop_dispatches_only_through_the_semantic_batch_contract() {
        let production = production_loop_source();
        for forbidden in [
            "execute_tool_invocation",
            "invocation_batch::dispatch_tools",
            "BusEvent::ToolStart",
            "BusEvent::PermissionDecision",
            ".runtime_ref()",
            "fn summarize_tool_args",
            "\"memory_recall\" | \"memory_store\" | \"memory_query\"",
        ] {
            assert!(
                !production.contains(forbidden),
                "production loop.rs bypasses semantic batch dispatch with {forbidden:?}"
            );
        }
        assert!(production.contains(".dispatch_batch("));
    }

    #[test]
    fn production_loop_uses_context_contract_instead_of_concrete_context_policy() {
        let production = production_loop_source();
        for forbidden in [
            "ContextManager",
            ".build_system_prompt(",
            ".last_prompt_telemetry(",
            ".prepare_embeddings(",
            ".set_selector_policy(",
            ".set_context_window(",
            ".signals_data(",
            ".inject_intent(",
            ".inject_external(",
            ".record_tool_call(",
            ".record_file_access(",
            ".update_phase_from_activity(",
            ".render_attachment_context_injection(",
            ".build_llm_view(",
            ".collect_context(",
            "ContextSignals",
            "ContextInjection",
            "PromptTelemetry",
            "compute_context_composition",
            "estimate_tool_schema_tokens",
            "conversation.build_compaction_payload(",
            "conversation.build_compaction_payload_keeping_recent(",
            "conversation.apply_compaction(",
            "conversation.apply_compaction_keeping_recent(",
            "conversation.decay_oldest(",
            "conversation.tighten_decay(",
            "starts_with(\"memory_",
            "strip_prefix(\"memory_",
            "contains(\"memory_",
            "ends_with(\"memory_",
        ] {
            assert!(
                !production.contains(forbidden),
                "production loop.rs bypasses the context contract with {forbidden:?}"
            );
        }
    }

    #[test]
    fn partial_session_authority_is_rejected() {
        let scope = crate::invocation_service::InvocationScope {
            session_id: Some("session-1".into()),
            ..Default::default()
        };

        let error = validate_invocation_scope(&scope).unwrap_err();

        assert!(error.to_string().contains("incomplete durable authority"));
    }

    #[tokio::test]
    async fn serving_route_requires_bridge_identity() {
        let controller = std::sync::Arc::new(crate::route::RouteController::new(
            crate::route::ProviderRoute::Serving {
                model: "anthropic:claude-sonnet-4-6".into(),
            },
            Box::new(crate::bridge::NullBridge),
            None,
        ));
        let bridge = crate::bridge::NullBridge;
        let route = LoopRoutePort {
            leased_bridge: &bridge,
            service: crate::session_execution::boot_execution_binding().route_service(),
            setup: crate::provider_route_service::loop_route_setup(Some(controller), None),
            policy: crate::provider_route_service::LoopRoutePolicy {
                selected_model: "anthropic:claude-sonnet-4-6".into(),
                bridge_model: None,
                extended_context: false,
                settings: None,
            },
            baseline_options: std::sync::Mutex::new(None),
            active_options: std::sync::Mutex::new(None),
        };

        let error = route.validate().await.unwrap_err();

        assert!(error.to_string().contains("no serving identity"));
    }
    #[tokio::test]
    async fn zen_withdrawal_dispatch_disconnects_authority_and_settings() {
        struct Withdrawn;
        #[async_trait::async_trait]
        impl LlmBridge for Withdrawn {
            async fn stream(
                &self,
                _: &str,
                _: &[crate::bridge::LlmMessage],
                _: &[omegon_traits::ToolDefinition],
                _: &crate::bridge::StreamOptions,
            ) -> anyhow::Result<tokio::sync::mpsc::Receiver<crate::bridge::LlmEvent>> {
                Err(crate::bridge::ProviderRouteUnavailable {
                    model: "opencode-zen:big-pickle".into(),
                    message: "Free route withdrawn".into(),
                }
                .into())
            }
        }
        let model = "opencode-zen:big-pickle";
        let (events, mut rx) = broadcast::channel(16);
        let controller = std::sync::Arc::new(crate::route::RouteController::new(
            crate::route::ProviderRoute::Serving {
                model: model.into(),
            },
            Box::new(crate::bridge::NullBridge),
            Some(events.clone()),
        ));
        let settings = crate::settings::shared(model);
        settings.lock().unwrap().provider_connected = true;
        let port = LoopRoutePort {
            leased_bridge: &Withdrawn,
            service: crate::session_execution::boot_execution_binding().route_service(),
            setup: crate::provider_route_service::loop_route_setup(Some(controller.clone()), None),
            policy: crate::provider_route_service::LoopRoutePolicy {
                selected_model: model.into(),
                bridge_model: None,
                extended_context: false,
                settings: Some(settings.clone()),
            },
            baseline_options: std::sync::Mutex::new(None),
            active_options: std::sync::Mutex::new(None),
        };
        // Similar prose or a transient error must not invalidate a route.
        port.reconcile_route_failure(&anyhow::anyhow!("Free route withdrawn 429"))
            .await;
        assert!(settings.lock().unwrap().provider_connected);
        assert_eq!(controller.snapshot().await.serving_model(), Some(model));
        let route = port.startup_route().await;
        let scope = crate::invocation_service::InvocationScope::default();
        let error = port
            .dispatch(LoopRouteRequest {
                route: &route,
                system_prompt: "fixture",
                messages: &[],
                tools: &[],
                events: &events,
                max_retries: 3,
                retry_delay_ms: 1,
                cancel_keeps_prompt: None,
                scope: &scope,
                step_id: uuid::Uuid::new_v4(),
                semantic_request: None,
                response_facts: None,
            })
            .await
            .err()
            .unwrap();
        assert!(error.is::<crate::bridge::ProviderRouteUnavailable>());
        assert!(controller.snapshot().await.serving_model().is_none());
        assert!(!settings.lock().unwrap().provider_connected);
        assert_eq!(settings.lock().unwrap().model, model);
        let mut disconnected = false;
        while let Ok(event) = rx.try_recv() {
            if let omegon_traits::AgentEvent::RouteChanged { state, serving, .. } = event {
                disconnected |= state == "disconnected" && serving.is_none();
            }
        }
        assert!(
            disconnected,
            "authoritative route invalidation must notify frontends"
        );
    }

    #[tokio::test]
    async fn zen_late_withdrawal_cannot_disconnect_replacement_route() {
        let controller = crate::route::RouteController::new(
            crate::route::ProviderRoute::Serving {
                model: "openai:gpt-5.6".into(),
            },
            Box::new(crate::bridge::NullBridge),
            None,
        );
        assert!(
            !controller
                .invalidate_serving_model("opencode-zen:big-pickle", "Withdrawn")
                .await
        );
        assert_eq!(
            controller.snapshot().await.serving_model(),
            Some("openai:gpt-5.6")
        );
    }
}
