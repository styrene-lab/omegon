//! Release-coupled session policy used by the agent loop compatibility adapter.

use crate::behavior::ToolCapabilityCatalog;
use crate::conversation::{ConversationState, IntentDocument, ToolCall, ToolResultEntry};
use omegon_traits::{ContentBlock, PlanSurfaceProjection};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

const CONTEXT_CAPTURE_OWNER: &str = "loop-context:capture";
const CONTEXT_CAPTURE_GENERATION: &str = "loop-context:capture/builtin-v1";

pub(crate) struct LoopModelRequestCapture<'a> {
    pub(crate) step: &'a crate::loop_driver::LoopStepIdentity,
    pub(crate) purpose: crate::loop_driver::LoopModelRequestPurpose,
    pub(crate) replaces: Option<&'a crate::loop_driver::LoopModelRequestIdentity>,
    pub(crate) system_prompt: &'a str,
    pub(crate) messages: &'a [crate::bridge::LlmMessage],
    pub(crate) tools: &'a [omegon_traits::ToolDefinition],
    pub(crate) tool_lineage: &'a crate::loop_driver::LoopToolSchemaLineage,
    pub(crate) route: &'a crate::loop_driver::LoopRoute,
}

/// Flow adapter shared by initial requests and both history-repair paths.
/// Selection, authority manifests, and invocation permission keep their owners.
pub(crate) struct TurnRequestPreparation<'a> {
    pub(crate) step: Option<&'a crate::loop_driver::LoopStepIdentity>,
    pub(crate) compatibility_step_id: uuid::Uuid,
    pub(crate) system_prompt: &'a str,
    pub(crate) tools: &'a [omegon_traits::ToolDefinition],
    pub(crate) route: &'a crate::loop_driver::LoopRoute,
    pub(crate) scope: &'a crate::invocation_service::InvocationScope,
    pub(crate) events: &'a tokio::sync::broadcast::Sender<omegon_traits::AgentEvent>,
    pub(crate) config: &'a crate::r#loop::LoopConfig,
}

pub(crate) struct CapturedTurnRequest {
    messages: Vec<crate::bridge::LlmMessage>,
    pub(crate) identity: Option<crate::loop_driver::LoopModelRequestIdentity>,
}

/// Compare the final transport inputs to the existing durable manifest. This is
/// read-only: a changed input must obtain a new request, never recapture this ID.
pub(crate) fn validate_dispatch_capture(
    authority: &crate::session_authority::SessionAuthorityHandle,
    identity: &crate::loop_driver::LoopModelRequestIdentity,
    inputs: &crate::model_request::RequestInputs<'_>,
    route: &crate::provider_route_service::LoopRoute,
) -> anyhow::Result<()> {
    use crate::session_authority::{ModelRequestState, ProjectionClass};
    let state = authority.state();
    let Some(ModelRequestState::Open { preparation, .. }) =
        state.model_requests.get(&identity.request_id)
    else {
        anyhow::bail!("dispatch requires an open prepared request");
    };
    anyhow::ensure!(
        preparation.step_id == identity.step_id
            && preparation.turn_id == identity.turn_id
            && preparation.request_ordinal == identity.request_ordinal
            && state
                .active_step
                .as_ref()
                .is_some_and(|step| step.active_request_id == Some(identity.request_id)),
        "dispatch identity does not match the active prepared request"
    );
    anyhow::ensure!(
        preparation.context_items.len() == inputs.messages.len() + 1
            && preparation.schema_set.schemas.len() == inputs.tools.len(),
        "dispatch input count differs from the prepared manifest"
    );
    let read = |reference: &crate::session_authority::ContentRef| {
        authority.read_content(reference, ProjectionClass::Default)
    };
    anyhow::ensure!(
        read(&preparation.context_items[0].content_ref)? == inputs.system.as_bytes(),
        "dispatch system differs from the prepared manifest"
    );
    for (item, message) in preparation
        .context_items
        .iter()
        .skip(1)
        .zip(inputs.messages)
    {
        anyhow::ensure!(
            read(&item.content_ref)? == canonical_json_bytes(message)?,
            "dispatch message differs from the prepared manifest"
        );
    }
    anyhow::ensure!(
        preparation.schema_set.normalizer_contribution_id == route.normalizer_contribution_id
            && preparation.schema_set.normalizer_generation_id == route.normalizer_generation_id,
        "dispatch schema normalizer differs from the prepared manifest"
    );
    for (schema, tool) in preparation.schema_set.schemas.iter().zip(inputs.tools) {
        anyhow::ensure!(
            schema.schema_dialect == route.schema_dialect
                && read(&schema.schema_content_ref)? == canonical_json_bytes(tool)?,
            "dispatch tool differs from the prepared manifest"
        );
    }
    Ok(())
}

impl TurnRequestPreparation<'_> {
    pub(crate) fn capture(
        &self,
        facts: &mut dyn LoopSemanticFactContract,
        invocation: &dyn crate::loop_driver::LoopInvocationContract,
        compatibility: &[crate::bridge::LlmMessage],
        purpose: crate::loop_driver::LoopModelRequestPurpose,
        replaces: Option<&crate::loop_driver::LoopModelRequestIdentity>,
    ) -> anyhow::Result<CapturedTurnRequest> {
        if let Some(previous) = replaces {
            facts.supersede_for_repair(previous, purpose)?;
        }
        let messages = facts.current_context_messages(compatibility)?;
        let identity = if facts.enabled() {
            let tool_lineage = invocation.tool_schema_lineage(self.tools)?;
            facts.prepare_model_request(LoopModelRequestCapture {
                step: self
                    .step
                    .ok_or_else(|| anyhow::anyhow!("semantic emission produced no step"))?,
                purpose,
                replaces,
                system_prompt: self.system_prompt,
                messages: &messages,
                tools: self.tools,
                tool_lineage: &tool_lineage,
                route: self.route,
            })?
        } else {
            None
        };
        Ok(CapturedTurnRequest { messages, identity })
    }

    pub(crate) fn dispatch<'a>(
        &'a self,
        captured: &'a CapturedTurnRequest,
        facts: &'a dyn LoopSemanticFactContract,
    ) -> crate::loop_driver::LoopRouteRequest<'a> {
        crate::loop_driver::LoopRouteRequest {
            route: self.route,
            system_prompt: self.system_prompt,
            messages: &captured.messages,
            tools: self.tools,
            events: self.events,
            max_retries: self.config.max_retries,
            retry_delay_ms: self.config.retry_delay_ms,
            cancel_keeps_prompt: self.config.cancel_keeps_prompt.as_ref(),
            scope: self.scope,
            step_id: self
                .step
                .map_or(self.compatibility_step_id, |step| step.step_id),
            semantic_request: captured.identity.as_ref(),
            response_facts: captured
                .identity
                .as_ref()
                .map(|_| facts as &dyn crate::loop_driver::LoopResponseFactContract),
        }
    }
}

pub(crate) trait LoopSemanticFactContract:
    crate::loop_driver::LoopResponseFactContract + Send
{
    fn enabled(&self) -> bool;
    fn start_step(&mut self) -> anyhow::Result<Option<crate::loop_driver::LoopStepIdentity>>;
    fn current_context_messages(
        &mut self,
        compatibility: &[crate::bridge::LlmMessage],
    ) -> anyhow::Result<Vec<crate::bridge::LlmMessage>>;
    fn prepare_model_request(
        &mut self,
        capture: LoopModelRequestCapture<'_>,
    ) -> anyhow::Result<Option<crate::loop_driver::LoopModelRequestIdentity>>;

    fn supersede_for_repair(
        &mut self,
        request: &crate::loop_driver::LoopModelRequestIdentity,
        purpose: crate::loop_driver::LoopModelRequestPurpose,
    ) -> anyhow::Result<()>;

    fn record_tool_calls(
        &mut self,
        request: &crate::loop_driver::LoopModelRequestIdentity,
        calls: &[ToolCall],
    ) -> anyhow::Result<Vec<crate::loop_driver::LoopToolCallReceipt>>;

    fn record_tool_results(
        &mut self,
        step: &crate::loop_driver::LoopStepIdentity,
        calls: &[crate::loop_driver::LoopToolCallReceipt],
        results: &[ToolResultEntry],
        terminals: &[crate::loop_driver::LoopInvocationTerminal],
    ) -> anyhow::Result<()>;

    fn close_step(
        &mut self,
        step: &crate::loop_driver::LoopStepIdentity,
        outcome: crate::loop_driver::LoopStepOutcome,
        reason_code: &str,
    ) -> anyhow::Result<()>;
}

impl crate::loop_driver::LoopResponseFactContract for LoopSemanticFactAdapter {
    fn fail_attempt(
        &self,
        request: &crate::loop_driver::LoopModelRequestIdentity,
        response_attempt_ordinal: u32,
        failure: crate::loop_driver::LoopResponseAttemptFailure,
        reason_code: &str,
    ) -> anyhow::Result<()> {
        self.authority()?.fail_model_response_attempt(
            uuid::Uuid::new_v4(),
            &recorded_at_now(),
            crate::session_authority::ModelResponseAttemptFailed {
                request_id: request.request_id,
                step_id: request.step_id,
                response_attempt_ordinal,
                failure: match failure {
                    crate::loop_driver::LoopResponseAttemptFailure::ProviderError => {
                        crate::session_authority::ModelResponseAttemptFailure::ProviderError
                    }
                    crate::loop_driver::LoopResponseAttemptFailure::Eof => {
                        crate::session_authority::ModelResponseAttemptFailure::Eof
                    }
                    crate::loop_driver::LoopResponseAttemptFailure::TimedOut => {
                        crate::session_authority::ModelResponseAttemptFailure::TimedOut
                    }
                    crate::loop_driver::LoopResponseAttemptFailure::TransportLost => {
                        crate::session_authority::ModelResponseAttemptFailure::TransportLost
                    }
                },
                reason_code: reason_code.into(),
                retry_disposition:
                    crate::session_authority::ModelResponseAttemptRetryDisposition::RetrySameRequest,
            },
        )?;
        Ok(())
    }

    fn append_content(
        &self,
        request: &crate::loop_driver::LoopModelRequestIdentity,
        message_id: uuid::Uuid,
        response_attempt_ordinal: u32,
        content_kind: crate::loop_driver::LoopResponseContentKind,
        chunk_ordinal: u32,
        bytes: &[u8],
    ) -> anyhow::Result<crate::loop_driver::LoopResponseChunkReceipt> {
        let authority = self.authority()?;
        let content_ref = authority.write_content(
            bytes,
            "text/plain",
            crate::session_authority::ProjectionClass::Default,
        )?;
        authority.append_assistant_content(
            uuid::Uuid::new_v4(),
            &recorded_at_now(),
            crate::session_authority::AssistantContentAppended {
                message_id,
                request_id: request.request_id,
                step_id: request.step_id,
                response_attempt_ordinal,
                content_kind: match content_kind {
                    crate::loop_driver::LoopResponseContentKind::Text => {
                        crate::session_authority::AssistantContentKind::Text
                    }
                    crate::loop_driver::LoopResponseContentKind::Thinking => {
                        crate::session_authority::AssistantContentKind::Thinking
                    }
                },
                chunk_ordinal,
                content_ref: content_ref.clone(),
            },
        )?;
        Ok(crate::loop_driver::LoopResponseChunkReceipt {
            content_kind,
            content_ref,
        })
    }

    fn store_continuity(
        &self,
        request: &crate::loop_driver::LoopModelRequestIdentity,
        response_attempt_ordinal: u32,
        serving_provider_id: &str,
        serving_model_id: &str,
        provider_contribution_generation_id: &str,
        kind: crate::loop_driver::LoopProviderContinuityKind,
        allowed_kinds: &[crate::loop_driver::LoopProviderContinuityKind],
        max_blob_bytes: u64,
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        let authority = self.authority()?;
        let map_kind = |kind| match kind {
            crate::loop_driver::LoopProviderContinuityKind::HiddenReasoning => {
                crate::session_authority::ProviderContinuityKind::HiddenReasoning
            }
            crate::loop_driver::LoopProviderContinuityKind::OpaqueProviderState => {
                crate::session_authority::ProviderContinuityKind::OpaqueProviderState
            }
        };
        let content_ref = authority.write_content(
            bytes,
            "application/octet-stream",
            crate::session_authority::ProjectionClass::RestrictedContinuity,
        )?;
        authority.store_provider_continuity(
            uuid::Uuid::new_v4(),
            &recorded_at_now(),
            crate::session_authority::ProviderContinuityStored {
                continuity_id: uuid::Uuid::new_v4(),
                request_id: request.request_id,
                step_id: request.step_id,
                response_attempt_ordinal,
                serving_provider_id: serving_provider_id.into(),
                serving_model_id: serving_model_id.into(),
                provider_contribution_generation_id: provider_contribution_generation_id.into(),
                continuity_kind: map_kind(kind),
                required_for: crate::session_authority::ProviderContinuityRequiredFor::NextRequest,
                restricted_required: crate::session_authority::RestrictedContinuityPolicy {
                    allowed_kinds: allowed_kinds.iter().copied().map(map_kind).collect(),
                    max_blob_bytes,
                },
                content_ref,
            },
        )?;
        Ok(())
    }

    fn commit_message(
        &self,
        request: &crate::loop_driver::LoopModelRequestIdentity,
        message_id: uuid::Uuid,
        response_attempt_ordinal: u32,
        chunks: &[crate::loop_driver::LoopResponseChunkReceipt],
        usage: Option<(u64, u64)>,
        tool_call_count: u32,
    ) -> anyhow::Result<()> {
        let authority = self.authority()?;
        let mut content = Vec::new();
        for kind in [
            crate::loop_driver::LoopResponseContentKind::Text,
            crate::loop_driver::LoopResponseContentKind::Thinking,
        ] {
            let matching = chunks
                .iter()
                .filter(|chunk| chunk.content_kind == kind)
                .collect::<Vec<_>>();
            if matching.is_empty() {
                continue;
            }
            let mut hasher = Sha256::new();
            let mut chunk_refs = Vec::new();
            for chunk in matching {
                hasher.update(authority.read_content(
                    &chunk.content_ref,
                    crate::session_authority::ProjectionClass::Default,
                )?);
                chunk_refs.push(chunk.content_ref.clone());
            }
            content.push(crate::session_authority::AssistantContentManifest {
                content_kind: match kind {
                    crate::loop_driver::LoopResponseContentKind::Text => {
                        crate::session_authority::AssistantContentKind::Text
                    }
                    crate::loop_driver::LoopResponseContentKind::Thinking => {
                        crate::session_authority::AssistantContentKind::Thinking
                    }
                },
                chunk_refs,
                content_digest: format!("{:x}", hasher.finalize()),
            });
        }
        authority.commit_assistant_message(
            uuid::Uuid::new_v4(),
            &recorded_at_now(),
            crate::session_authority::AssistantMessageCommitted {
                message_id,
                request_id: request.request_id,
                step_id: request.step_id,
                response_attempt_ordinal,
                completion_evidence:
                    crate::session_authority::ProviderCompletionEvidence::ProviderDone,
                content,
                usage: usage.map(|(input_tokens, output_tokens)| {
                    crate::session_authority::AssistantUsage {
                        input_tokens,
                        output_tokens,
                    }
                }),
                tool_call_count,
            },
        )?;
        Ok(())
    }

    fn close_request(
        &self,
        request: &crate::loop_driver::LoopModelRequestIdentity,
        response_attempt_ordinal: u32,
        terminal: crate::loop_driver::LoopRequestTerminal,
        reason_code: &str,
    ) -> anyhow::Result<()> {
        let authority = self.authority()?;
        let outcome = match terminal {
            crate::loop_driver::LoopRequestTerminal::ResponseCompleted => {
                crate::session_authority::ModelRequestOutcome::ResponseCompleted
            }
            crate::loop_driver::LoopRequestTerminal::ProviderFailed => {
                crate::session_authority::ModelRequestOutcome::ProviderFailed
            }
            crate::loop_driver::LoopRequestTerminal::Eof => {
                crate::session_authority::ModelRequestOutcome::Eof
            }
            crate::loop_driver::LoopRequestTerminal::Cancelled => {
                crate::session_authority::ModelRequestOutcome::Cancelled
            }
            crate::loop_driver::LoopRequestTerminal::TimedOut => {
                crate::session_authority::ModelRequestOutcome::TimedOut
            }
            crate::loop_driver::LoopRequestTerminal::Unknown => {
                crate::session_authority::ModelRequestOutcome::Unknown
            }
        };
        authority.close_model_request(
            uuid::Uuid::new_v4(),
            &recorded_at_now(),
            crate::session_authority::ModelRequestClosed {
                request_id: request.request_id,
                step_id: request.step_id,
                response_attempt_ordinal,
                outcome,
                reason_code: reason_code.into(),
                recovery_rule_version: None,
            },
        )?;
        Ok(())
    }
}

pub(crate) enum LoopSemanticFactAdapter {
    Disabled,
    Authority {
        authority: crate::session_authority::SessionAuthorityHandle,
        pending_context: Option<crate::session_current_context::CurrentContextDraftV1>,
    },
    Invalid(&'static str),
}

impl LoopSemanticFactAdapter {
    pub(crate) fn new(scope: &crate::invocation_service::InvocationScope) -> Self {
        match (
            scope.session_id.as_deref(),
            scope.turn_id,
            scope.authority.as_ref(),
        ) {
            (None, None, None) => Self::Disabled,
            (Some(session_id), Some(turn_id), Some(authority))
                if authority.session_id() == session_id
                    && authority.state().active_turn.map(|turn| turn.turn_id) == Some(turn_id) =>
            {
                Self::Authority {
                    authority: authority.clone(),
                    pending_context: None,
                }
            }
            _ => Self::Invalid("semantic emission requires one complete active session authority"),
        }
    }

    fn authority(&self) -> anyhow::Result<&crate::session_authority::SessionAuthorityHandle> {
        match self {
            Self::Authority { authority, .. } => Ok(authority),
            Self::Disabled => anyhow::bail!("disabled semantic adapter has no session authority"),
            Self::Invalid(reason) => anyhow::bail!(*reason),
        }
    }
}

fn replay_authority_frontier(
    authority: &crate::session_authority::SessionAuthorityHandle,
) -> anyhow::Result<crate::session_replay::SessionReplay> {
    let state = authority.state();
    let event_id = state
        .last_event_id
        .ok_or_else(|| anyhow::anyhow!("session authority has no replay frontier"))?;
    let descriptor = authority.projection_worker_descriptor();
    Ok(crate::session_replay::SessionReplay::replay_prefix(
        &descriptor.session_snapshot,
        &descriptor.session_id,
        descriptor.stream_id,
        crate::session_replay::ReplayEnd::Event(event_id),
    )?)
}

fn materialize_legacy_compatibility_base(
    authority: &crate::session_authority::SessionAuthorityHandle,
    replay: &crate::session_replay::SessionReplay,
    compatibility: &[crate::bridge::LlmMessage],
) -> anyhow::Result<()> {
    let latest_prompt = replay.records().iter().rev().find_map(|record| {
        let crate::session_authority::SessionFactPayload::PromptAdmitted(prompt) = record.payload()
        else {
            return None;
        };
        Some(&prompt.content.text)
    });
    let semantic_start = latest_prompt.and_then(|prompt| {
        compatibility.iter().rposition(|message| {
            matches!(message, crate::bridge::LlmMessage::User { content, .. } if content == prompt)
        })
    });
    let legacy = semantic_start.map_or(compatibility, |index| &compatibility[..index]);
    if legacy.is_empty() {
        anyhow::bail!("mixed session has no legacy compatibility base to materialize");
    }
    let message = crate::bridge::LlmMessage::User {
        content: format!(
            "[Legacy compatibility context - frozen at full-spine cutover]\n{}\n[End legacy compatibility context]",
            serde_json::to_string(legacy)?
        ),
        images: Vec::new(),
    };
    let content_ref = authority.write_content(
        &canonical_json_bytes(&message)?,
        "application/json",
        crate::session_authority::ProjectionClass::Default,
    )?;
    authority.materialize_context_source(
        uuid::Uuid::new_v4(),
        &recorded_at_now(),
        crate::session_authority::ContextSourceMaterialized {
            context_source_id: uuid::Uuid::new_v4(),
            source_kind: crate::session_authority::ContextSourceKind::ContributionContext,
            source_identity: "legacy-compatibility-base-v1".into(),
            owner_id: "compatibility:session-resume".into(),
            owner_generation_id: omegon_traits::RuntimeContributionGenerationId::new(
                "session-resume:legacy-base-v1",
            )
            .map_err(anyhow::Error::msg)?,
            content_ref,
        },
    )?;
    Ok(())
}

fn compatible_continuity_refs(
    authority: &crate::session_authority::SessionAuthorityHandle,
    context: &crate::session_current_context::CurrentContextDraftV1,
    route: &crate::loop_driver::LoopRoute,
) -> anyhow::Result<Vec<uuid::Uuid>> {
    let generation = route.contribution_generation_id.clone();
    let model_id = crate::providers::model_id_from_spec(&route.serving_model);
    let state = authority.state();
    Ok(context
        .continuity_refs
        .iter()
        .copied()
        .filter(|continuity_id| {
            state
                .provider_continuity
                .get(continuity_id)
                .is_some_and(|value| {
                    value.serving_provider_id == route.provider_id
                        && value.serving_model_id == model_id
                        && value.provider_contribution_generation_id == generation
                })
        })
        .collect())
}

impl LoopSemanticFactContract for LoopSemanticFactAdapter {
    fn enabled(&self) -> bool {
        matches!(self, Self::Authority { .. })
    }

    fn start_step(&mut self) -> anyhow::Result<Option<crate::loop_driver::LoopStepIdentity>> {
        if let Self::Invalid(reason) = self {
            anyhow::bail!(*reason);
        }
        if let Self::Authority { authority, .. } = self {
            let state = authority.state();
            let turn_id = state
                .active_turn
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("semantic step capture has no active turn"))?
                .turn_id;
            let step_ordinal = state.next_step_ordinals.get(&turn_id).copied().unwrap_or(0);
            let identity = crate::loop_driver::LoopStepIdentity {
                step_id: uuid::Uuid::new_v4(),
                turn_id,
                step_ordinal,
            };
            authority.start_step(
                uuid::Uuid::new_v4(),
                &recorded_at_now(),
                crate::session_authority::StepStarted {
                    step_id: identity.step_id,
                    turn_id,
                    step_ordinal,
                },
            )?;
            return Ok(Some(identity));
        }
        Ok(None)
    }

    fn current_context_messages(
        &mut self,
        compatibility: &[crate::bridge::LlmMessage],
    ) -> anyhow::Result<Vec<crate::bridge::LlmMessage>> {
        let Self::Authority {
            authority,
            pending_context,
        } = self
        else {
            return Ok(compatibility.to_vec());
        };
        let mut replay = replay_authority_frontier(authority)?;
        if replay.lineage_level() == crate::session_authority::AuthorityLineageLevel::Mixed
            && !replay.records().iter().any(|record| {
                matches!(record.payload(), crate::session_authority::SessionFactPayload::ContextSourceMaterialized(source)
                    if source.source_kind == crate::session_authority::ContextSourceKind::ContributionContext
                        && source.source_identity.starts_with("legacy-"))
            })
        {
            materialize_legacy_compatibility_base(authority, &replay, compatibility)?;
            replay = replay_authority_frontier(authority)?;
        }
        let context = crate::session_current_context::CurrentContextDraftV1::derive(&replay)?;
        let messages = context.messages();
        *pending_context = Some(context);
        Ok(messages)
    }

    fn prepare_model_request(
        &mut self,
        capture: LoopModelRequestCapture<'_>,
    ) -> anyhow::Result<Option<crate::loop_driver::LoopModelRequestIdentity>> {
        if let Self::Authority {
            authority,
            pending_context,
        } = self
        {
            use crate::session_authority::{
                ContextSourceKind, ContextSourceMaterialized, ModelContextItem,
                ModelContextProvenance, ModelContextRole, ModelContextSourceKind,
                ModelRequestPrepared, ModelSchemaIdentity, ModelSchemaSet, ProjectionClass,
            };

            if capture.tools.len() != capture.tool_lineage.tools.len() {
                anyhow::bail!("tool schema capture lineage does not match dispatched tools");
            }
            let context = match pending_context.take() {
                Some(context) => context,
                None => crate::session_current_context::CurrentContextDraftV1::derive(
                    &replay_authority_frontier(authority)?,
                )?,
            };
            let state = authority.state();
            if state.last_sequence != context.frontier.sequence()
                || state.last_event_id != Some(context.frontier.event_id())
            {
                anyhow::bail!("authority frontier moved after current-context derivation");
            }
            if canonical_json_bytes(&context.messages())?
                != canonical_json_bytes(&capture.messages)?
            {
                anyhow::bail!("provider dispatch messages do not byte-match current context");
            }
            let request_ordinal = state
                .active_step
                .as_ref()
                .filter(|active| active.start.step_id == capture.step.step_id)
                .ok_or_else(|| anyhow::anyhow!("semantic request capture targets no active step"))?
                .next_request_ordinal;
            let generation = |value: &str| {
                omegon_traits::RuntimeContributionGenerationId::new(value)
                    .map_err(anyhow::Error::msg)
            };
            let system_ref = authority.write_content(
                capture.system_prompt.as_bytes(),
                "text/plain",
                ProjectionClass::Default,
            )?;
            let system_source_id = uuid::Uuid::new_v4();
            let system_source_event_id = authority.materialize_context_source(
                uuid::Uuid::new_v4(),
                &recorded_at_now(),
                ContextSourceMaterialized {
                    context_source_id: system_source_id,
                    source_kind: ContextSourceKind::SystemInstruction,
                    source_identity: "provider-visible-system-prompt".into(),
                    owner_id: CONTEXT_CAPTURE_OWNER.into(),
                    owner_generation_id: generation(CONTEXT_CAPTURE_GENERATION)?,
                    content_ref: system_ref.clone(),
                },
            )?;
            let mut context_items = vec![ModelContextItem {
                ordinal: 0,
                role: ModelContextRole::System,
                content_ref: system_ref,
                provenance: ModelContextProvenance {
                    source_kind: ModelContextSourceKind::SystemInstruction,
                    source_event_id: Some(system_source_event_id),
                    source_identity: Some(system_source_id.to_string()),
                    owner_id: Some(CONTEXT_CAPTURE_OWNER.into()),
                    owner_generation_id: Some(generation(CONTEXT_CAPTURE_GENERATION)?),
                },
            }];
            for (index, item) in context.items.iter().enumerate() {
                let message = &item.message;
                let role = match message {
                    crate::bridge::LlmMessage::User { .. } => ModelContextRole::User,
                    crate::bridge::LlmMessage::Assistant { .. } => ModelContextRole::Assistant,
                    crate::bridge::LlmMessage::ToolResult { .. } => ModelContextRole::Tool,
                };
                context_items.push(ModelContextItem {
                    ordinal: u32::try_from(index + 1)?,
                    role,
                    content_ref: authority.write_content(
                        &canonical_json_bytes(message)?,
                        "application/json",
                        ProjectionClass::Default,
                    )?,
                    provenance: item.provenance.clone(),
                });
            }
            let schemas = capture
                .tools
                .iter()
                .zip(&capture.tool_lineage.tools)
                .enumerate()
                .map(|(ordinal, (tool, owner))| {
                    let schema_content_ref = authority.write_content(
                        &canonical_json_bytes(tool)?,
                        "application/json",
                        ProjectionClass::Default,
                    )?;
                    Ok(ModelSchemaIdentity {
                        ordinal: ordinal as u32,
                        capability_id: owner.capability_id.clone(),
                        contribution_id: owner.contribution_id.clone(),
                        owner_generation_id: owner.owner_generation_id.clone(),
                        schema_dialect: capture.route.schema_dialect.clone(),
                        schema_content_ref,
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            let schema_set = ModelSchemaSet {
                schema_set_version: 1,
                composition_generation_id: capture.tool_lineage.composition_generation_id.clone(),
                normalizer_contribution_id: capture.route.normalizer_contribution_id.clone(),
                normalizer_generation_id: capture.route.normalizer_generation_id.clone(),
                schemas,
            };
            let identity = crate::loop_driver::LoopModelRequestIdentity {
                request_id: uuid::Uuid::new_v4(),
                step_id: capture.step.step_id,
                turn_id: capture.step.turn_id,
                request_ordinal,
            };
            let preparation = ModelRequestPrepared {
                request_id: identity.request_id,
                step_id: identity.step_id,
                turn_id: identity.turn_id,
                request_ordinal,
                purpose: capture.purpose.into(),
                replaces_request_id: capture.replaces.map(|request| request.request_id),
                continuity_refs: compatible_continuity_refs(authority, &context, capture.route)?,
                context_manifest_id: canonical_sha256(&context_items)?,
                context_items,
                schema_set_id: canonical_sha256(&schema_set)?,
                schema_set,
            };
            authority.prepare_model_request(
                uuid::Uuid::new_v4(),
                &recorded_at_now(),
                preparation.clone(),
            )?;
            let replay = replay_authority_frontier(authority)?;
            let view =
                crate::session_current_context::CurrentContextViewV1::compare_prepared_capture(
                    &replay,
                    &preparation,
                )?;
            let provider_generation = capture.route.contribution_generation_id.clone();
            for continuity_id in &preparation.continuity_refs {
                view.authorize_continuity(
                    &replay,
                    *continuity_id,
                    &capture.route.provider_id,
                    crate::providers::model_id_from_spec(&capture.route.serving_model),
                    &provider_generation,
                )?;
            }
            if canonical_json_bytes(
                &view
                    .items
                    .iter()
                    .skip(1)
                    .map(|item| {
                        serde_json::from_slice::<crate::bridge::LlmMessage>(
                            &item.model_visible_bytes,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )? != canonical_json_bytes(&capture.messages)?
            {
                anyhow::bail!("durable request capture does not byte-match provider dispatch");
            }
            return Ok(Some(identity));
        }
        let _ = capture;
        Ok(None)
    }

    fn supersede_for_repair(
        &mut self,
        request: &crate::loop_driver::LoopModelRequestIdentity,
        purpose: crate::loop_driver::LoopModelRequestPurpose,
    ) -> anyhow::Result<()> {
        {
            let Self::Authority { authority, .. } = self else {
                return Ok(());
            };
            let outcome = match purpose {
                crate::loop_driver::LoopModelRequestPurpose::ContextOverflowRepair => {
                    crate::session_authority::ModelRequestOutcome::SupersededForContextRepair
                }
                crate::loop_driver::LoopModelRequestPurpose::ProviderHistoryRepair => {
                    crate::session_authority::ModelRequestOutcome::SupersededForHistoryRepair
                }
                crate::loop_driver::LoopModelRequestPurpose::Initial => {
                    anyhow::bail!("initial request cannot supersede a request")
                }
            };
            authority.close_model_request(
                uuid::Uuid::new_v4(),
                &recorded_at_now(),
                crate::session_authority::ModelRequestClosed {
                    request_id: request.request_id,
                    step_id: request.step_id,
                    response_attempt_ordinal: 0,
                    outcome,
                    reason_code: "context_repair_boundary".into(),
                    recovery_rule_version: None,
                },
            )?;
            Ok(())
        }
    }

    fn record_tool_calls(
        &mut self,
        request: &crate::loop_driver::LoopModelRequestIdentity,
        calls: &[ToolCall],
    ) -> anyhow::Result<Vec<crate::loop_driver::LoopToolCallReceipt>> {
        {
            let Self::Authority { authority, .. } = self else {
                return Ok(Vec::new());
            };
            calls
                .iter()
                .enumerate()
                .map(|(ordinal, call)| {
                    let arguments_ref = authority.write_content(
                        &canonical_json_bytes(&call.arguments)?,
                        "application/json",
                        crate::session_authority::ProjectionClass::Default,
                    )?;
                    let receipt = crate::loop_driver::LoopToolCallReceipt {
                        tool_call_id: uuid::Uuid::new_v4(),
                        call_id: call.id.clone(),
                        call_ordinal: u32::try_from(ordinal)?,
                    };
                    authority.record_tool_call(
                        uuid::Uuid::new_v4(),
                        &recorded_at_now(),
                        crate::session_authority::ToolCallRecorded {
                            tool_call_id: receipt.tool_call_id,
                            request_id: request.request_id,
                            step_id: request.step_id,
                            call_ordinal: receipt.call_ordinal,
                            call_id: call.id.clone(),
                            invocation_name: call.name.clone(),
                            arguments_ref,
                        },
                    )?;
                    Ok(receipt)
                })
                .collect()
        }
    }

    fn record_tool_results(
        &mut self,
        step: &crate::loop_driver::LoopStepIdentity,
        calls: &[crate::loop_driver::LoopToolCallReceipt],
        results: &[ToolResultEntry],
        terminals: &[crate::loop_driver::LoopInvocationTerminal],
    ) -> anyhow::Result<()> {
        {
            let Self::Authority { authority, .. } = self else {
                return Ok(());
            };
            if calls.len() != results.len() || calls.len() != terminals.len() {
                anyhow::bail!("tool call/result terminal cardinality mismatch");
            }
            for ((call, result), terminal) in calls.iter().zip(results).zip(terminals) {
                if call.call_id != result.call_id {
                    anyhow::bail!("tool result order contradicts provider call order");
                }
                let (disposition, invocation_id, lease_id, reason_code) = match terminal {
                    crate::loop_driver::LoopInvocationTerminal::Denied { reason_code } => (
                        crate::session_authority::ToolResultDisposition::Denied,
                        None,
                        None,
                        Some(reason_code.clone()),
                    ),
                    crate::loop_driver::LoopInvocationTerminal::NotDispatched { reason_code } => (
                        crate::session_authority::ToolResultDisposition::NotDispatched,
                        None,
                        None,
                        Some(reason_code.clone()),
                    ),
                    crate::loop_driver::LoopInvocationTerminal::AuthorityLinked => {
                        let state = authority.state();
                        let linked =
                            state
                                .invocations
                                .iter()
                                .find(|(_, invocation)| match invocation {
                                    crate::session_authority::InvocationState::DurableSettled {
                                        preparation,
                                        ..
                                    }
                                    | crate::session_authority::InvocationState::DurableUnknown {
                                        preparation,
                                        ..
                                    } => preparation.call_id == call.call_id,
                                    _ => false,
                                });
                        match linked {
                            Some((
                                invocation_id,
                                crate::session_authority::InvocationState::DurableSettled {
                                    preparation,
                                    ..
                                },
                            )) => (
                                crate::session_authority::ToolResultDisposition::Settled,
                                Some(*invocation_id),
                                Some(preparation.lease_id),
                                None,
                            ),
                            Some((
                                invocation_id,
                                crate::session_authority::InvocationState::DurableUnknown {
                                    preparation,
                                    classification,
                                    ..
                                },
                            )) => (
                                crate::session_authority::ToolResultDisposition::UnknownCompletion,
                                Some(*invocation_id),
                                Some(preparation.lease_id),
                                Some(classification.reason_code.clone()),
                            ),
                            _ => anyhow::bail!(
                                "tool result has no terminal authoritative invocation"
                            ),
                        }
                    }
                };
                let content_ref = authority.write_content(
                    &canonical_json_bytes(&result.content)?,
                    "application/json",
                    crate::session_authority::ProjectionClass::Default,
                )?;
                authority.record_tool_result(
                    uuid::Uuid::new_v4(),
                    &recorded_at_now(),
                    crate::session_authority::ToolResultRecorded {
                        tool_result_id: uuid::Uuid::new_v4(),
                        tool_call_id: call.tool_call_id,
                        step_id: step.step_id,
                        result_ordinal: call.call_ordinal,
                        call_id: call.call_id.clone(),
                        disposition,
                        invocation_id,
                        lease_id,
                        content_ref,
                        is_error: result.is_error,
                        reason_code,
                    },
                )?;
            }
            Ok(())
        }
    }

    fn close_step(
        &mut self,
        step: &crate::loop_driver::LoopStepIdentity,
        outcome: crate::loop_driver::LoopStepOutcome,
        reason_code: &str,
    ) -> anyhow::Result<()> {
        {
            let Self::Authority { authority, .. } = self else {
                return Ok(());
            };
            authority.close_step(
                uuid::Uuid::new_v4(),
                &recorded_at_now(),
                crate::session_authority::StepClosed {
                    step_id: step.step_id,
                    turn_id: step.turn_id,
                    outcome: match outcome {
                        crate::loop_driver::LoopStepOutcome::Continue => {
                            crate::session_authority::StepOutcome::ContinueLoop
                        }
                        crate::loop_driver::LoopStepOutcome::Finish => {
                            crate::session_authority::StepOutcome::TurnCompleted
                        }
                    },
                    reason_code: reason_code.into(),
                },
            )?;
            Ok(())
        }
    }
}

impl From<crate::loop_driver::LoopModelRequestPurpose>
    for crate::session_authority::ModelRequestPurpose
{
    fn from(value: crate::loop_driver::LoopModelRequestPurpose) -> Self {
        match value {
            crate::loop_driver::LoopModelRequestPurpose::Initial => Self::Initial,
            crate::loop_driver::LoopModelRequestPurpose::ContextOverflowRepair => {
                Self::ContextOverflowRepair
            }
            crate::loop_driver::LoopModelRequestPurpose::ProviderHistoryRepair => {
                Self::ProviderHistoryRepair
            }
        }
    }
}

fn canonical_json_bytes(value: &(impl serde::Serialize + ?Sized)) -> anyhow::Result<Vec<u8>> {
    fn sort(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => serde_json::Value::Object(
                map.into_iter()
                    .map(|(key, value)| (key, sort(value)))
                    .collect::<std::collections::BTreeMap<_, _>>()
                    .into_iter()
                    .collect(),
            ),
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.into_iter().map(sort).collect())
            }
            value => value,
        }
    }
    Ok(serde_json::to_vec(&sort(serde_json::to_value(value)?))?)
}

fn canonical_sha256(value: &impl serde::Serialize) -> anyhow::Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(canonical_json_bytes(value)?)
    ))
}

fn recorded_at_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

const MAX_PLAN_RECONCILIATION_NUDGES: u8 = 3;

pub(crate) struct LoopPlanToolOutcome {
    pub(crate) notification: Option<String>,
    pub(crate) projection: Option<PlanSurfaceProjection>,
    pub(crate) reconciled: bool,
    pub(crate) requires_continuation: bool,
}

pub(crate) struct LoopFinalizationSummary {
    pub(crate) tool_calls: u32,
    pub(crate) initial_prompt: Option<String>,
    pub(crate) outcome_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompletionPhaseObligation {
    pub(crate) number: String,
    pub(crate) label: String,
}

impl From<crate::skills::SkillPhaseInfo> for CompletionPhaseObligation {
    fn from(phase: crate::skills::SkillPhaseInfo) -> Self {
        Self {
            number: phase.final_phase_number,
            label: phase.final_phase_label,
        }
    }
}

pub(crate) struct LoopCompletionAdvisory {
    pub(crate) drift_kind: omegon_traits::DriftKind,
    pub(crate) progress_nudge_reason: omegon_traits::ProgressNudgeReason,
}

pub(crate) struct LoopCompletionDirective {
    pub(crate) guidance: String,
    pub(crate) advisory: Option<LoopCompletionAdvisory>,
}

pub(crate) struct LoopRecoveryDirective {
    pub(crate) guidance: Option<String>,
}

pub(crate) struct LoopStuckDirective {
    pub(crate) guidance: String,
}

pub(crate) trait LoopSessionPolicyContract: Send {
    fn operator_correction_recovery(&mut self) -> String;

    fn stuck_recovery(
        &mut self,
        tool_catalog: &ToolCapabilityCatalog,
    ) -> Option<LoopStuckDirective>;

    fn meta_recovery(&mut self, assistant_text: &str, turn: u32, max_turns: u32) -> Option<String>;

    fn text_only_recovery(
        &mut self,
        conversation: &ConversationState,
        assistant_text: &str,
        turn: u32,
        config: &crate::r#loop::LoopConfig,
    ) -> Option<LoopRecoveryDirective>;

    fn observe_assistant_tool_calls(&mut self, calls: &[ToolCall]);

    fn record_tool_outcomes(
        &mut self,
        tool_catalog: &ToolCapabilityCatalog,
        calls: &[ToolCall],
        results: &[ToolResultEntry],
        observations: &[crate::observation::ObservationEvent],
    );

    fn capture_ambient(
        &mut self,
        conversation: &mut ConversationState,
        assistant_text: &str,
    ) -> usize;

    fn finalization_summary(&self, conversation: &ConversationState) -> LoopFinalizationSummary;

    fn pending_continuation(
        &mut self,
        conversation: &mut ConversationState,
        cwd: &std::path::Path,
    ) -> Option<String>;

    fn completion_directive(
        &mut self,
        conversation: &mut ConversationState,
        assistant_text: &str,
        turn: u32,
        config: &crate::r#loop::LoopConfig,
    ) -> Option<LoopCompletionDirective>;

    fn visible_plan_snapshot(
        &self,
        conversation: &ConversationState,
        cwd: &std::path::Path,
    ) -> serde_json::Value;

    fn reconcile_plan_tools(
        &mut self,
        conversation: &ConversationState,
        cwd: &std::path::Path,
        snapshot_before: &serde_json::Value,
        calls: &[ToolCall],
        results: &mut [ToolResultEntry],
    ) -> LoopPlanToolOutcome;

    fn realtime_completion_reminder(
        &self,
        conversation: &ConversationState,
        tool_catalog: &ToolCapabilityCatalog,
        calls: &[ToolCall],
        results: &[ToolResultEntry],
    ) -> Option<&'static str>;
}

pub(crate) struct LoopSessionCompatibilityAdapter {
    stuck_detector: StuckDetector,
    dead_mouse_nudges: u8,
    meta_recovery_nudges: u8,
    dead_mouse_nudge_injected: bool,
    work_snapshot: Option<std::sync::Arc<styrene_work_runtime::WorkSnapshot>>,
    behavior_policy: Option<crate::behavior::BehaviorPolicyBinding>,
}

impl Default for LoopSessionCompatibilityAdapter {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl LoopSessionCompatibilityAdapter {
    pub(crate) fn new(
        work_snapshot: Option<std::sync::Arc<styrene_work_runtime::WorkSnapshot>>,
        behavior_policy: Option<crate::behavior::BehaviorPolicyBinding>,
    ) -> Self {
        Self {
            stuck_detector: StuckDetector::new(),
            dead_mouse_nudges: 0,
            meta_recovery_nudges: 0,
            dead_mouse_nudge_injected: false,
            work_snapshot,
            behavior_policy,
        }
    }
}

impl LoopSessionPolicyContract for LoopSessionCompatibilityAdapter {
    fn operator_correction_recovery(&mut self) -> String {
        tracing::info!("Operator correction detected — entering recovery mode");
        self.dead_mouse_nudges = 0;
        self.meta_recovery_nudges = 0;
        crate::behavior::operator_correction_recovery_message()
    }

    fn stuck_recovery(
        &mut self,
        tool_catalog: &ToolCapabilityCatalog,
    ) -> Option<LoopStuckDirective> {
        let warning = self.stuck_detector.check(tool_catalog)?;
        tracing::info!(
            consecutive = warning.consecutive,
            "Stuck detector: {}",
            warning.message
        );
        let guidance = if warning.consecutive >= 3 {
            tracing::warn!(
                "Stuck detector escalation — injecting recovery guidance after {} consecutive warnings",
                warning.consecutive
            );
            self.stuck_detector.reset_after_escalation();
            "[System: Repetition pressure — several recent turns repeated similar tool calls without producing new evidence. If you already have what you need, produce the deliverable now. Otherwise take one concrete, different next action. If no concrete action is possible, state the blocker plainly and stop.]".to_string()
        } else {
            format!("[System: {}]", warning.message)
        };
        Some(LoopStuckDirective { guidance })
    }

    fn meta_recovery(&mut self, assistant_text: &str, turn: u32, max_turns: u32) -> Option<String> {
        let policy = self.behavior_policy.as_ref()?;
        if !policy
            .service
            .assess_text(assistant_text)
            .pathological_meta_response
            || turn >= max_turns
            || self.meta_recovery_nudges >= 2
        {
            return None;
        }
        self.meta_recovery_nudges += 1;
        tracing::info!(
            nudges = self.meta_recovery_nudges,
            "Pathological meta response — forcing concrete recovery retry"
        );
        Some(
            policy
                .service
                .message(crate::behavior::BehaviorMessageKind::MetaRetry),
        )
    }

    fn text_only_recovery(
        &mut self,
        conversation: &ConversationState,
        assistant_text: &str,
        turn: u32,
        config: &crate::r#loop::LoopConfig,
    ) -> Option<LoopRecoveryDirective> {
        let below_turn_limit = config.max_turns == 0 || turn < config.max_turns;
        let automation_level = config
            .settings
            .as_ref()
            .and_then(|settings| {
                settings
                    .lock()
                    .ok()
                    .map(|settings| settings.automation_level)
            })
            .unwrap_or_default();
        if below_turn_limit
            && self.dead_mouse_nudges < 3
            && should_continue_text_only_turn(
                automation_level,
                conversation.last_user_prompt(),
                assistant_text,
                conversation.intent.stats.tool_calls > 0,
            )
        {
            self.dead_mouse_nudges += 1;
            tracing::info!(
                nudge = self.dead_mouse_nudges,
                "Text-only turn ended before action — auto-continuing"
            );
            self.dead_mouse_nudge_injected = true;
            return Some(LoopRecoveryDirective {
                guidance: Some(
                    "[System: The operator already asked you to proceed. Do not ask for confirmation or describe work you will do next. Take the next concrete action now with the available tools, or give a final answer only if the requested work is actually complete.]".to_string(),
                ),
            });
        }

        let in_task_mode = conversation.intent.stats.tool_calls > 0;
        let user_asked_question =
            conversation.intent.task_mode == crate::conversation::TaskMode::Research;
        let last_assistant_substantial = conversation
            .last_assistant_text()
            .map(|text| text.trim().len() >= 200)
            .unwrap_or(false);
        if !has_mutations(conversation)
            && turn > 1
            && in_task_mode
            && !user_asked_question
            && !last_assistant_substantial
            && below_turn_limit
            && self.dead_mouse_nudges < 3
        {
            self.dead_mouse_nudges += 1;
            if self.dead_mouse_nudges < 2 {
                return Some(LoopRecoveryDirective { guidance: None });
            }
            let guidance = if self.dead_mouse_nudges == 2 {
                "[System: You responded with text but did not advance the task. If the user asked for a file change, use the appropriate tool. If the user asked a question, your text answer may be sufficient — but make sure it actually answers what they asked.]"
            } else {
                "[System: Multiple turns without task progress. Either answer the user's question completely, or use tools to make the changes they requested. Do not invent file-writing work the user did not ask for.]"
            };
            tracing::info!(
                nudge = self.dead_mouse_nudges,
                "Dead-mouse detection — model responded without acting"
            );
            self.dead_mouse_nudge_injected = true;
            return Some(LoopRecoveryDirective {
                guidance: Some(guidance.to_string()),
            });
        }
        None
    }

    fn observe_assistant_tool_calls(&mut self, calls: &[ToolCall]) {
        self.meta_recovery_nudges = 0;
        if self.dead_mouse_nudge_injected {
            if calls.iter().any(counts_as_real_work_for_dead_mouse) {
                self.dead_mouse_nudges = 0;
                self.dead_mouse_nudge_injected = false;
            }
        } else {
            self.dead_mouse_nudges = 0;
        }
    }

    fn record_tool_outcomes(
        &mut self,
        tool_catalog: &ToolCapabilityCatalog,
        calls: &[ToolCall],
        results: &[ToolResultEntry],
        observations: &[crate::observation::ObservationEvent],
    ) {
        for event in observations {
            self.stuck_detector.record_observation(event);
        }
        for call in calls {
            let is_error = results
                .iter()
                .find(|result| result.call_id == call.id)
                .is_some_and(|result| result.is_error);
            if is_error {
                self.stuck_detector.record(tool_catalog, call, true);
            }
        }
    }

    fn capture_ambient(
        &mut self,
        conversation: &mut ConversationState,
        assistant_text: &str,
    ) -> usize {
        let captured = crate::lifecycle::capture::parse_ambient_blocks(assistant_text);
        let constraint_captures = captured
            .iter()
            .filter(|capture| {
                matches!(
                    capture,
                    crate::lifecycle::capture::AmbientCapture::Constraint(_)
                )
            })
            .count();
        if !captured.is_empty() {
            conversation.apply_ambient_captures(&captured);
        }
        constraint_captures
    }

    fn finalization_summary(&self, conversation: &ConversationState) -> LoopFinalizationSummary {
        LoopFinalizationSummary {
            tool_calls: conversation.intent.stats.tool_calls,
            initial_prompt: conversation
                .first_user_text()
                .map(|text| text.chars().take(200).collect()),
            outcome_summary: conversation
                .last_assistant_text()
                .map(|text| text.chars().take(300).collect()),
        }
    }

    fn pending_continuation(
        &mut self,
        conversation: &mut ConversationState,
        cwd: &std::path::Path,
    ) -> Option<String> {
        if !crate::conversation::is_continuance_approval(conversation.last_user_prompt()) {
            return None;
        }
        let (repo_root, branch) = current_repo_identity(cwd);
        let resolution = conversation
            .intent
            .resolve_pending_action(repo_root.as_deref(), branch.as_deref());
        Some(continuation_resolution_message(resolution))
    }

    fn completion_directive(
        &mut self,
        conversation: &mut ConversationState,
        assistant_text: &str,
        turn: u32,
        config: &crate::r#loop::LoopConfig,
    ) -> Option<LoopCompletionDirective> {
        let near_budget = turn + 6 >= config.max_turns;
        let response_looks_done = looks_like_completion(assistant_text);
        if config.allow_commit_nudge
            && !conversation.intent.commit_nudged
            && has_mutations(conversation)
            && turn < config.max_turns
            && (near_budget || response_looks_done)
        {
            conversation.intent.commit_nudged = true;
            tracing::info!(
                near_budget,
                response_looks_done,
                "Agent finishing without committing — nudging"
            );
            return Some(LoopCompletionDirective {
                guidance:
                    "[System: You have uncommitted file changes. Commit your work before finishing.]"
                        .into(),
                advisory: Some(LoopCompletionAdvisory {
                    drift_kind: omegon_traits::DriftKind::ClosureStall,
                    progress_nudge_reason: omegon_traits::ProgressNudgeReason::CommitHygiene,
                }),
            });
        }

        let intent = &mut conversation.intent;
        if turn < config.max_turns && should_nudge_plan_reconciliation(intent) {
            let fingerprint = plan_open_fingerprint(intent);
            if intent.plan_reconciliation_fingerprint == Some(fingerprint) {
                intent.plan_reconciliation_nudges =
                    intent.plan_reconciliation_nudges.saturating_add(1);
            } else {
                intent.plan_reconciliation_fingerprint = Some(fingerprint);
                intent.plan_reconciliation_nudges = 1;
            }
            return Some(LoopCompletionDirective {
                guidance: "[System: The visible Workbench plan still has active/todo items. Before ending the turn, reconcile it with the `plan` tool: use `plan advance`/`plan complete` for finished items, `plan skip` for deliberately bypassed items, or `plan clear` only if the plan gate is no longer useful. If work truly remains, leave the plan active and state the remaining work explicitly.]".into(),
                advisory: Some(LoopCompletionAdvisory {
                    drift_kind: omegon_traits::DriftKind::ClosureStall,
                    progress_nudge_reason: omegon_traits::ProgressNudgeReason::PlanReconciliation,
                }),
            });
        }

        if !config.skill_phases.is_empty()
            && !intent.skill_completion_nudged
            && turn < config.max_turns
        {
            let response_text = assistant_text.to_lowercase();
            let incomplete = config
                .skill_phases
                .iter()
                .filter(|phase| {
                    !response_text.contains(&format!("phase {}", phase.number))
                        && !response_text.contains(&phase.label.to_lowercase())
                })
                .map(|phase| phase.label.as_str())
                .collect::<Vec<_>>();
            if !incomplete.is_empty() {
                intent.skill_completion_nudged = true;
                tracing::info!(incomplete = ?incomplete, "agent stopped before completing all skill phases — nudging");
                let labels = incomplete
                    .iter()
                    .map(|label| format!("  - {label}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                return Some(LoopCompletionDirective {
                    guidance: format!(
                        "[System: You have not completed all phases of the active skill. The following phase(s) still need to be executed:\n{labels}\n\nPlease continue and complete the remaining phases before finishing.]"
                    ),
                    advisory: None,
                });
            }
        }

        None
    }

    fn visible_plan_snapshot(
        &self,
        conversation: &ConversationState,
        _cwd: &std::path::Path,
    ) -> serde_json::Value {
        conversation
            .intent
            .work_plan_snapshot_json_for_repo(self.work_snapshot.as_deref())
    }

    fn reconcile_plan_tools(
        &mut self,
        conversation: &ConversationState,
        cwd: &std::path::Path,
        snapshot_before: &serde_json::Value,
        calls: &[ToolCall],
        results: &mut [ToolResultEntry],
    ) -> LoopPlanToolOutcome {
        enrich_plan_list_tool_results(
            results,
            calls,
            &conversation.intent,
            self.work_snapshot.as_deref(),
        );
        let snapshot_after = self.visible_plan_snapshot(conversation, cwd);
        let projection = (snapshot_before != &snapshot_after).then(|| {
            conversation
                .intent
                .plan_surface_projection_for_repo(self.work_snapshot.as_deref())
        });
        let action = calls.iter().rev().find_map(plan_action);
        let notification =
            action.map(|action| plan_status_notification(action, &conversation.intent));
        let reconciliation_action = calls
            .iter()
            .filter_map(plan_action)
            .any(|action| matches!(action, "advance" | "complete" | "skip" | "clear"));
        let continuation_action = calls
            .iter()
            .filter_map(plan_action)
            .any(|action| matches!(action, "advance" | "complete" | "skip"));
        let was_nudged = conversation.intent.plan_reconciliation_nudges > 0;
        let has_open_items = !plan_open_items(&conversation.intent).is_empty();

        LoopPlanToolOutcome {
            notification,
            projection,
            reconciled: was_nudged && reconciliation_action && !has_open_items,
            requires_continuation: was_nudged && continuation_action && has_open_items,
        }
    }

    fn realtime_completion_reminder(
        &self,
        conversation: &ConversationState,
        tool_catalog: &ToolCapabilityCatalog,
        calls: &[ToolCall],
        results: &[ToolResultEntry],
    ) -> Option<&'static str> {
        if plan_open_items(&conversation.intent).is_empty()
            || calls.iter().any(|call| plan_action(call).is_some())
        {
            return None;
        }
        calls
            .iter()
            .zip(results)
            .any(|(call, result)| {
                !result.is_error
                    && crate::behavior::is_progress_boundary_tool(tool_catalog, &call.name)
            })
            .then(|| {
                tracing::info!(
                    "Material progress boundary crossed with a stale completion obligation — injecting reminder"
                );
                "[System: Workbench progress may be stale. If the active item just finished, update it now with `plan advance`, `plan complete`, or `plan skip` before moving on.]"
            })
    }
}

pub(crate) fn counts_as_real_work_for_dead_mouse(call: &ToolCall) -> bool {
    matches!(call.name.as_str(), "bash" | "read" | "codebase_search")
        || (matches!(call.name.as_str(), "write" | "edit")
            && !call
                .arguments
                .get("path")
                .and_then(serde_json::Value::as_str)
                .map(is_session_noise_path)
                .unwrap_or(false))
}

fn has_mutations(conversation: &ConversationState) -> bool {
    !conversation.intent.files_modified.is_empty()
}

pub(crate) fn should_continue_text_only_turn(
    automation_level: crate::settings::AutomationLevel,
    user_prompt: &str,
    assistant_text: &str,
    prior_tool_activity: bool,
) -> bool {
    if matches!(automation_level, crate::settings::AutomationLevel::Ask) {
        return false;
    }
    let assistant = assistant_text.trim();
    if assistant.is_empty() {
        return prior_tool_activity
            || user_prompt_is_continue_or_proceed(user_prompt)
            || user_prompt_expects_concrete_action(user_prompt);
    }
    if looks_like_blocked_response(assistant) || looks_like_completion(assistant) {
        return false;
    }
    if looks_like_incomplete_structured_answer(assistant) {
        return matches!(
            automation_level,
            crate::settings::AutomationLevel::Flow | crate::settings::AutomationLevel::Autonomous
        ) || user_prompt_is_continue_or_proceed(user_prompt);
    }
    if looks_like_continuation_request(assistant) {
        return match automation_level {
            crate::settings::AutomationLevel::Flow
            | crate::settings::AutomationLevel::Autonomous => true,
            _ => {
                user_prompt_is_continue_or_proceed(user_prompt)
                    || user_prompt_expects_concrete_action(user_prompt)
            }
        };
    }
    if matches!(automation_level, crate::settings::AutomationLevel::Guarded) {
        return user_prompt_is_continue_or_proceed(user_prompt)
            && looks_like_plan_or_future_action(assistant);
    }
    if user_prompt_is_continue_or_proceed(user_prompt) {
        return looks_like_plan_or_future_action(assistant) || !prior_tool_activity;
    }
    user_prompt_expects_concrete_action(user_prompt) && looks_like_plan_or_future_action(assistant)
}

pub(crate) fn looks_like_incomplete_structured_answer(text: &str) -> bool {
    let trimmed = text.trim();
    let fence_count = trimmed
        .lines()
        .filter(|line| line.trim_start().starts_with("```"))
        .count();
    if fence_count % 2 == 1 {
        return true;
    }
    if trimmed.len() < 120 {
        return false;
    }

    let nonempty = trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let Some(last) = nonempty.last().copied() else {
        return false;
    };
    let lower = trimmed.to_ascii_lowercase();
    let last_lower = last.to_ascii_lowercase();
    let last_is_list_item = last_lower.starts_with("- ")
        || last_lower.starts_with("* ")
        || last_lower
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_digit())
            && last_lower.contains(". ");
    let last_has_terminal_punctuation = last.ends_with('.')
        || last.ends_with('!')
        || last.ends_with('?')
        || last.ends_with(')')
        || last.ends_with(']')
        || last.ends_with('`');

    last_is_list_item
        && !last_has_terminal_punctuation
        && (lower.contains("phase 1") || lower.contains("roadmap") || lower.contains("plan"))
        && !lower.contains("phase 2")
}

fn looks_like_continuation_request(text: &str) -> bool {
    let tail = text
        .chars()
        .rev()
        .take(300)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>()
        .to_ascii_lowercase();
    tail.contains("shall i")
        || tail.contains("should i")
        || tail.contains("would you like")
        || tail.contains("do you want me to")
        || tail.contains("ready to proceed")
        || tail.contains("want me to proceed")
        || tail.contains("want me to continue")
        || tail.contains("let me know if you want me to")
        || tail.contains("let me know and i")
        || tail.ends_with('?')
            && (tail.contains("proceed")
                || tail.contains("continue")
                || tail.contains("implement")
                || tail.contains("make the change")
                || tail.contains("go ahead"))
}

fn user_prompt_is_continue_or_proceed(text: &str) -> bool {
    crate::conversation::is_continuance_approval(text)
}

fn user_prompt_expects_concrete_action(text: &str) -> bool {
    let lower = text.trim().to_ascii_lowercase();
    let trimmed = lower.trim_start();
    let action_prefixes = [
        "fix ",
        "get ",
        "implement ",
        "make ",
        "build ",
        "wire ",
        "add ",
        "update ",
        "remove ",
        "delete ",
        "clean ",
        "cleanup ",
        "install ",
        "link ",
        "commit ",
        "push ",
        "publish ",
        "cut ",
        "release ",
        "run ",
        "test ",
        "validate ",
        "proceed",
        "continue",
    ];
    action_prefixes
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
        || lower.contains("make it so")
        || lower.contains("get it done")
        || lower.contains("go fix")
        || lower.contains("go clean")
        || lower.contains("go ahead")
}

fn looks_like_plan_or_future_action(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let planning_markers = [
        "i'll ",
        "i will ",
        "i’m going to ",
        "i'm going to ",
        "i can ",
        "i would ",
        "i should ",
        "next i",
        "the next step",
        "my plan",
        "plan:",
        "approach:",
        "i’ll start",
        "i'll start",
        "i’ll inspect",
        "i'll inspect",
        "i’ll update",
        "i'll update",
        "i’ll implement",
        "i'll implement",
        "i’ll make",
        "i'll make",
    ];
    planning_markers.iter().any(|marker| lower.contains(marker))
}

fn looks_like_blocked_response(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("blocked")
        || lower.contains("i need clarification")
        || lower.contains("need clarification")
        || lower.contains("i need you to")
        || lower.contains("cannot proceed")
        || lower.contains("can't proceed")
        || lower.contains("unable to proceed")
        || lower.contains("permission")
}

pub(crate) fn looks_like_completion(text: &str) -> bool {
    if text.len() < 20 {
        return false;
    }
    let lower = text.to_lowercase();
    let completion_phrases = [
        "all done",
        "that's done",
        "that's everything",
        "that's all",
        "all changes",
        "have been made",
        "have been applied",
        "have been updated",
        "all set",
        "let me know if",
        "let me know what",
        "anything else",
        "to summarize",
        "in summary",
        "here's a summary",
        "here is a summary",
        "summary of",
        "the changes are",
        "changes are complete",
        "implementation is complete",
        "task is complete",
        "done!",
        "not committed yet",
    ];
    completion_phrases
        .iter()
        .any(|phrase| lower.contains(phrase))
}

pub(crate) fn is_session_noise_path(path: &str) -> bool {
    let noise_dirs = ["ai/session/", ".omegon/", "ai/lifecycle/"];
    if noise_dirs.iter().any(|directory| path.contains(directory)) {
        return true;
    }
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let noise_fragments = [
        "warning",
        "compliance",
        "-ack",
        "ack-",
        "tool-output",
        "session-note",
        "system-note",
        "marker",
    ];
    noise_fragments
        .iter()
        .any(|fragment| stem.contains(fragment))
}

pub(crate) struct StuckWarning {
    pub(crate) message: String,
    pub(crate) consecutive: u32,
}

impl std::fmt::Display for StuckWarning {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

pub(crate) struct StuckDetector {
    recent: Vec<(String, u64, bool)>,
    recent_file_accesses: Vec<String>,
    window: usize,
    consecutive_warnings: u32,
}

impl StuckDetector {
    pub(crate) fn new() -> Self {
        Self {
            recent: Vec::new(),
            recent_file_accesses: Vec::new(),
            window: 10,
            consecutive_warnings: 0,
        }
    }

    pub(crate) fn reset_after_escalation(&mut self) {
        self.recent.clear();
        self.recent_file_accesses.clear();
        self.consecutive_warnings = 0;
    }

    pub(crate) fn record(
        &mut self,
        catalog: &ToolCapabilityCatalog,
        call: &ToolCall,
        is_error: bool,
    ) {
        let args_hash = if crate::behavior::is_repo_inspection_tool(catalog, &call.name) {
            call.arguments
                .get("path")
                .map(hash_value)
                .unwrap_or_else(|| hash_value(&call.arguments))
        } else {
            hash_value(&call.arguments)
        };
        self.recent.push((call.name.clone(), args_hash, is_error));
        if self.recent.len() > self.window * 2 {
            self.recent.drain(..self.window);
        }

        if crate::behavior::is_mutation_tool_name(catalog, &call.name) {
            if let Some(path) = call
                .arguments
                .get("path")
                .and_then(serde_json::Value::as_str)
            {
                self.recent_file_accesses.retain(|recent| recent != path);
            }
        } else if crate::behavior::is_repo_inspection_tool(catalog, &call.name)
            && let Some(path) = call
                .arguments
                .get("path")
                .and_then(serde_json::Value::as_str)
        {
            self.recent_file_accesses.push(path.to_string());
            if self.recent_file_accesses.len() > self.window * 2 {
                self.recent_file_accesses.drain(..self.window);
            }
        }
    }

    pub(crate) fn record_observation(&mut self, event: &crate::observation::ObservationEvent) {
        match event {
            crate::observation::ObservationEvent::FileRead { source_tool, path } => {
                let tool_name = source_tool
                    .strip_prefix("bash:")
                    .unwrap_or(source_tool)
                    .to_string();
                self.recent.push((tool_name, hash_str_path(path), false));
                self.recent_file_accesses.push(path.display().to_string());
            }
            crate::observation::ObservationEvent::SearchPerformed {
                source_tool,
                query,
                roots,
            } => {
                let tool_name = source_tool
                    .strip_prefix("bash:")
                    .unwrap_or(source_tool)
                    .to_string();
                let mut fingerprint = String::from("<search>");
                if let Some(query) = query {
                    fingerprint.push('\u{1f}');
                    fingerprint.push_str(query);
                }
                for root in roots {
                    fingerprint.push('\u{1f}');
                    fingerprint.push_str(&root.display().to_string());
                }
                self.recent.push((tool_name, hash_str(&fingerprint), false));
            }
            crate::observation::ObservationEvent::FileMutated { source_tool, path } => {
                self.recent
                    .push((source_tool.clone(), hash_str_path(path), false));
                let rendered = path.display().to_string();
                self.recent_file_accesses
                    .retain(|recent| recent != &rendered);
            }
            crate::observation::ObservationEvent::ValidationRun { source_tool } => {
                let tool_name = if source_tool == "bash" {
                    crate::tool_registry::core::VALIDATE.to_string()
                } else {
                    source_tool.clone()
                };
                self.recent
                    .push((tool_name, hash_str("<validation>"), false));
                self.recent_file_accesses.clear();
            }
            crate::observation::ObservationEvent::ProgressBoundary { source_tool, .. } => {
                let tool_name = if source_tool == "bash" {
                    crate::tool_registry::core::COMMIT.to_string()
                } else {
                    source_tool.clone()
                };
                self.recent.push((tool_name, hash_str("<progress>"), false));
            }
        }
        if self.recent.len() > self.window * 2 {
            self.recent.drain(..self.window);
        }
        if self.recent_file_accesses.len() > self.window * 2 {
            self.recent_file_accesses.drain(..self.window);
        }
    }

    pub(crate) fn check(&mut self, catalog: &ToolCapabilityCatalog) -> Option<StuckWarning> {
        let len = self.recent.len();
        if len < 3 {
            self.consecutive_warnings = 0;
            return None;
        }
        let window = &self.recent[len.saturating_sub(self.window)..];
        let has_mutation_or_validation = window.iter().any(|(name, _, _)| {
            crate::behavior::is_mutation_tool_name(catalog, name)
                || crate::behavior::is_validation_tool_name(catalog, name)
        });
        let reads = window
            .iter()
            .filter(|(name, _, _)| crate::behavior::is_repo_inspection_tool(catalog, name))
            .collect::<Vec<_>>();
        if !has_mutation_or_validation && reads.len() >= 5 {
            let mut hash_counts: HashMap<u64, u32> = HashMap::new();
            for (_, hash, _) in &reads {
                *hash_counts.entry(*hash).or_default() += 1;
            }
            if hash_counts.values().any(|&count| count >= 5) {
                self.consecutive_warnings += 1;
                return Some(StuckWarning {
                    message: "You've inspected the same target multiple times without modifying it. Stop re-reading and either edit, validate, or summarize the blocker plainly.".into(),
                    consecutive: self.consecutive_warnings,
                });
            }
        }
        if let Some(repeated) = self.find_repeated_call(catalog, window, 3) {
            self.consecutive_warnings += 1;
            return Some(StuckWarning {
                message: format!(
                    "You've called `{}` with the same arguments {} times. If it's not producing the result you need, try a different approach.",
                    repeated.0, repeated.1
                ),
                consecutive: self.consecutive_warnings,
            });
        }
        let recent_errors = window
            .iter()
            .filter(|(_, _, is_error)| *is_error)
            .collect::<Vec<_>>();
        if recent_errors.len() >= 3 {
            let names = recent_errors
                .iter()
                .map(|(name, _, _)| name.as_str())
                .collect::<Vec<_>>();
            if names
                .windows(3)
                .any(|names| names[0] == names[1] && names[1] == names[2])
            {
                self.consecutive_warnings += 1;
                return Some(StuckWarning {
                    message: format!(
                        "Your last several `{}` calls returned errors. Consider reading the current file state before retrying.",
                        recent_errors.last().unwrap().0
                    ),
                    consecutive: self.consecutive_warnings,
                });
            }
        }
        if self.recent_file_accesses.len() >= 4 {
            let access_window = &self.recent_file_accesses
                [self.recent_file_accesses.len().saturating_sub(self.window)..];
            let mut path_counts: HashMap<&str, u32> = HashMap::new();
            for path in access_window {
                *path_counts.entry(path.as_str()).or_default() += 1;
            }
            if let Some((path, count)) = path_counts.iter().find(|&(_, &count)| count >= 4) {
                self.consecutive_warnings += 1;
                return Some(StuckWarning {
                    message: format!(
                        "You've accessed `{}` {} times across different tools without modifying it. Stop inspecting and either edit it, run a validation, or state the blocker.",
                        path, count
                    ),
                    consecutive: self.consecutive_warnings,
                });
            }
        }
        self.consecutive_warnings = 0;
        None
    }

    fn find_repeated_call(
        &self,
        catalog: &ToolCapabilityCatalog,
        window: &[(String, u64, bool)],
        threshold: usize,
    ) -> Option<(String, usize)> {
        let validation_marker = hash_str("<validation>");
        let progress_marker = hash_str("<progress>");
        let mut counts: HashMap<(String, u64), usize> = HashMap::new();
        for (name, hash, _) in window {
            if *hash == validation_marker || *hash == progress_marker {
                continue;
            }
            if crate::behavior::is_repo_inspection_tool(catalog, name)
                || crate::observation::is_read_program(name)
            {
                continue;
            }
            if crate::behavior::is_mutation_tool_name(catalog, name) || name.starts_with("bash:") {
                continue;
            }
            *counts.entry((name.clone(), *hash)).or_default() += 1;
        }
        counts
            .into_iter()
            .find(|(_, count)| *count >= threshold)
            .map(|((name, _), count)| (name, count))
    }
}

fn hash_value(value: &serde_json::Value) -> u64 {
    hash_str(&value.to_string())
}

fn hash_str(value: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn hash_str_path(path: &std::path::Path) -> u64 {
    hash_str(&path.display().to_string())
}

fn plan_action(call: &ToolCall) -> Option<&str> {
    (call.name == crate::tool_registry::core::PLAN).then(|| {
        call.arguments
            .get("action")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("status")
    })
}

fn enrich_plan_list_tool_results(
    results: &mut [ToolResultEntry],
    calls: &[ToolCall],
    intent: &IntentDocument,
    work_snapshot: Option<&styrene_work_runtime::WorkSnapshot>,
) {
    for (call, result) in calls.iter().zip(results.iter_mut()) {
        if plan_action(call) != Some("list") {
            continue;
        }
        let mut text = crate::plan::render_plan_list_text(intent, work_snapshot);
        text.push('\n');
        text.push_str(
            &result
                .content
                .iter()
                .filter_map(ContentBlock::as_text)
                .collect::<Vec<_>>()
                .join("\n"),
        );
        result.content = vec![ContentBlock::Text { text }];
    }
}

fn plan_status_notification(action: &str, intent: &IntentDocument) -> String {
    let heading = if intent.work_plan.is_empty()
        && matches!(action, "advance" | "complete" | "skip" | "clear")
    {
        "Plan cleared"
    } else {
        match action {
            "set" => "Plan set",
            "advance" | "complete" => "Plan progress",
            "skip" => "Plan item skipped",
            "approve" => "Plan approved",
            "execute" => "Plan executing",
            "clear" => "Plan cleared",
            "status" => "Plan status",
            _ => "Plan updated",
        }
    };
    format!("{heading}\n{}", intent.render_work_plan())
}

fn plan_open_items(
    intent: &IntentDocument,
) -> Vec<(usize, crate::conversation::WorkItemStatus, &str)> {
    let items = intent
        .visible_plan
        .as_ref()
        .map(|plan| plan.items.as_slice())
        .unwrap_or(intent.work_plan.as_slice());
    items
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| {
            matches!(
                item.status,
                crate::conversation::WorkItemStatus::Pending
                    | crate::conversation::WorkItemStatus::Active
            )
            .then_some((idx, item.status, item.description.as_str()))
        })
        .collect()
}

fn plan_open_fingerprint(intent: &IntentDocument) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    fn feed(hash: &mut u64, bytes: &[u8]) {
        for byte in bytes {
            *hash ^= u64::from(*byte);
            *hash = hash.wrapping_mul(FNV_PRIME);
        }
        *hash ^= 0xff;
        *hash = hash.wrapping_mul(FNV_PRIME);
    }

    let mut hash = FNV_OFFSET;
    if let Some(plan) = intent.visible_plan.as_ref() {
        feed(&mut hash, plan.plan_id.as_bytes());
        feed(&mut hash, plan.scope.label().as_bytes());
    } else {
        feed(&mut hash, b"legacy-work-plan");
    }
    for (idx, status, description) in plan_open_items(intent) {
        feed(&mut hash, idx.to_string().as_bytes());
        feed(&mut hash, format!("{status:?}").as_bytes());
        feed(&mut hash, description.as_bytes());
    }
    hash
}

fn should_nudge_plan_reconciliation(intent: &IntentDocument) -> bool {
    if plan_open_items(intent).is_empty() {
        return false;
    }
    if intent.plan_reconciliation_fingerprint != Some(plan_open_fingerprint(intent)) {
        return true;
    }
    intent.plan_reconciliation_nudges < MAX_PLAN_RECONCILIATION_NUDGES
}

fn continuation_resolution_message(
    resolution: crate::conversation::PendingActionResolution,
) -> String {
    match resolution {
        crate::conversation::PendingActionResolution::Ready(action) => format!(
            "[System: Operator continuance approval is bound to pending_action_id={} from turn {}: {}. Execute that bound action now; do not resume any older Workbench plan item unless it matches this pending action.]",
            action.id, action.source_turn, action.summary
        ),
        crate::conversation::PendingActionResolution::Missing =>
            "[System: The operator used continuance approval language, but there is no bound pending_action_id. Resolve the latest explicit operator directive from the live conversation; do not resume stale Workbench plan state.]"
                .to_string(),
        crate::conversation::PendingActionResolution::BranchMismatch {
            action,
            current_branch,
        } => format!(
            "[System: Continuance approval rejected for pending_action_id={}: branch mismatch. Pending action was bound to branch {:?}, current branch is {:?}. Do not mutate until the action is explicitly rebound or the checkout is reconciled.]",
            action.id, action.branch, current_branch
        ),
        crate::conversation::PendingActionResolution::RepoMismatch {
            action,
            current_repo_root,
        } => format!(
            "[System: Continuance approval rejected for pending_action_id={}: repository mismatch. Pending action was bound to repo {:?}, current repo is {:?}. Do not mutate until the action is explicitly rebound or the workspace is reconciled.]",
            action.id, action.repo_root, current_repo_root
        ),
    }
}

fn current_repo_identity(cwd: &std::path::Path) -> (Option<std::path::PathBuf>, Option<String>) {
    let Ok(repo) = git2::Repository::discover(cwd) else {
        return (None, None);
    };
    let repo_root = repo.workdir().map(std::path::Path::to_path_buf);
    let branch = repo.head().ok().and_then(|head| {
        head.is_branch()
            .then(|| head.shorthand().map(str::to_string))
            .flatten()
    });
    (repo_root, branch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::{
        PlanBinding, PlanMode, PlanScope, PlanSource, VisiblePlanState, WorkItem, WorkItemStatus,
    };
    use crate::loop_driver::LoopResponseFactContract;

    fn behavior_policy_binding() -> crate::behavior::BehaviorPolicyBinding {
        crate::behavior::BehaviorPolicyBinding {
            capability_id: crate::features::behavior_policy::behavior_policy_capability_id(),
            owner: omegon_traits::RuntimeContributionId::new("feature:behavior-policy").unwrap(),
            generation_id: omegon_traits::RuntimeContributionGenerationId::new(
                "behavior-policy:v1",
            )
            .unwrap(),
            service: std::sync::Arc::new(crate::behavior::DefaultBehaviorPolicy),
        }
    }

    fn authority_scope() -> (
        tempfile::TempDir,
        crate::session_authority::SessionAuthorityHandle,
        crate::invocation_service::InvocationScope,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let now = "2026-08-21T12:00:00Z";
        let mut authority = crate::session_authority::SessionAuthority::open(
            &directory.path().join("session.json"),
            "semantic-capture",
            "workspace",
            "composition:test",
            crate::session_authority::ActorIdentity {
                principal: "operator".into(),
                ingress: "test".into(),
            },
            now,
        )
        .unwrap();
        let prompt_id = uuid::Uuid::new_v4();
        authority
            .admit_prompt(
                uuid::Uuid::new_v4(),
                now,
                crate::session_authority::PromptAdmitted {
                    submission_id: uuid::Uuid::new_v4(),
                    prompt_id,
                    principal: "operator".into(),
                    ingress: "test".into(),
                    queue_mode: crate::session_authority::QueueMode::UntilReady,
                    content: crate::session_authority::PromptContent {
                        text: "capture".into(),
                        attachments: Vec::new(),
                    },
                    metadata: serde_json::json!({}),
                },
            )
            .unwrap();
        let turn_id = uuid::Uuid::new_v4();
        authority
            .start_turn(uuid::Uuid::new_v4(), now, turn_id, prompt_id)
            .unwrap();
        let authority = crate::session_authority::SessionAuthorityHandle::new(authority);
        let scope = crate::invocation_service::InvocationScope {
            session_id: Some("semantic-capture".into()),
            turn_id: Some(turn_id),
            authority: Some(authority.clone()),
            ..Default::default()
        };
        (directory, authority, scope)
    }

    fn route() -> crate::loop_driver::LoopRoute {
        let contribution = crate::provider_contributions::registry()
            .get("anthropic")
            .unwrap();
        crate::loop_driver::LoopRoute {
            selected_model: "anthropic:claude-sonnet-4-6".into(),
            serving_model: "anthropic:claude-sonnet-4-6".into(),
            provider_id: "anthropic".into(),
            schema_dialect: contribution.tools.dialect_name().into(),
            contribution_generation_id: contribution.owner_generation_id.as_str().into(),
            normalizer_contribution_id: omegon_traits::RuntimeContributionId::new(
                "system:tool-schema-normalizer",
            )
            .unwrap(),
            normalizer_generation_id: omegon_traits::RuntimeContributionGenerationId::new(
                "tool-schema-normalizer:builtin-v1",
            )
            .unwrap(),
        }
    }

    fn tool(parameters: serde_json::Value) -> omegon_traits::ToolDefinition {
        omegon_traits::ToolDefinition {
            name: "read".into(),
            label: "Read".into(),
            description: "Read a file".into(),
            parameters,
            capabilities: Vec::new(),
        }
    }

    fn lineage(count: usize) -> crate::loop_driver::LoopToolSchemaLineage {
        crate::loop_driver::LoopToolSchemaLineage {
            composition_generation_id: omegon_traits::RuntimeCompositionGenerationId::new(
                "composition:test",
            )
            .unwrap(),
            tools: (0..count)
                .map(|ordinal| crate::loop_driver::LoopToolOwnerLineage {
                    capability_id: omegon_traits::RuntimeCapabilityId::tool(if ordinal == 0 {
                        "read"
                    } else {
                        "write"
                    }),
                    contribution_id: omegon_traits::RuntimeContributionId::new("feature:tools")
                        .unwrap(),
                    owner_generation_id: omegon_traits::RuntimeContributionGenerationId::new(
                        "feature:tools/builtin-v1",
                    )
                    .unwrap(),
                })
                .collect(),
        }
    }

    fn capture_request(
        adapter: &mut LoopSemanticFactAdapter,
        system: &str,
        messages: &[crate::bridge::LlmMessage],
        tools: &[omegon_traits::ToolDefinition],
    ) -> crate::loop_driver::LoopModelRequestIdentity {
        let step = adapter.start_step().unwrap().unwrap();
        let messages = adapter.current_context_messages(messages).unwrap();
        adapter
            .prepare_model_request(LoopModelRequestCapture {
                step: &step,
                purpose: crate::loop_driver::LoopModelRequestPurpose::Initial,
                replaces: None,
                system_prompt: system,
                messages: &messages,
                tools,
                tool_lineage: &lineage(tools.len()),
                route: &route(),
            })
            .unwrap()
            .unwrap()
    }

    fn assert_current_context_matches_capture(
        directory: &tempfile::TempDir,
        authority: &crate::session_authority::SessionAuthorityHandle,
        request: &crate::loop_driver::LoopModelRequestIdentity,
    ) {
        let state = authority.state();
        let stream_id = state.stream_id.unwrap();
        let prepared = state.model_requests[&request.request_id]
            .preparation()
            .clone();
        drop(state);
        let replay = crate::session_replay::SessionReplay::replay_prefix(
            &directory.path().join("session.json"),
            "semantic-capture",
            stream_id,
            crate::session_replay::ReplayEnd::EndOfStream,
        )
        .unwrap();
        let view = crate::session_current_context::CurrentContextViewV1::compare_prepared_capture(
            &replay, &prepared,
        )
        .unwrap();
        assert!(matches!(
            crate::session_current_context::CurrentContextViewV1::derive(&replay).unwrap(),
            crate::session_current_context::CurrentContextReadV1::ExactFull(_)
                | crate::session_current_context::CurrentContextReadV1::ExactSuffix(_)
        ));
        assert_eq!(view.request_id, request.request_id);
        assert_eq!(view.context_manifest_id, prepared.context_manifest_id);
        assert_eq!(view.items.len(), prepared.context_items.len());
    }

    fn commit_request(
        adapter: &LoopSemanticFactAdapter,
        scope: &crate::invocation_service::InvocationScope,
        request: &crate::loop_driver::LoopModelRequestIdentity,
        text: Option<&str>,
        tool_call_count: u32,
    ) {
        crate::provider_route_service::record_loop_route_lease_for_test(
            scope,
            request.step_id,
            &route().selected_model,
            &route().serving_model,
            request,
        )
        .unwrap();
        let message_id = uuid::Uuid::new_v4();
        let chunks = text.map_or_else(Vec::new, |text| {
            vec![
                adapter
                    .append_content(
                        request,
                        message_id,
                        0,
                        crate::loop_driver::LoopResponseContentKind::Text,
                        0,
                        text.as_bytes(),
                    )
                    .unwrap(),
            ]
        });
        adapter
            .commit_message(
                request,
                message_id,
                0,
                &chunks,
                Some((1, 1)),
                tool_call_count,
            )
            .unwrap();
    }

    #[tokio::test]
    async fn token_retention_preserves_tool_results_after_last_prepared_request() {
        let (_directory, authority, scope) = authority_scope();
        let mut adapter = LoopSemanticFactAdapter::new(&scope);
        let request = capture_request(&mut adapter, "system", &[], &[]);
        commit_request(&adapter, &scope, &request, None, 2);
        let calls = vec![
            ToolCall {
                id: "call-1".into(),
                name: "read".into(),
                arguments: serde_json::json!({"z": 1, "a": "exact"}),
            },
            ToolCall {
                id: "call-2".into(),
                name: "write".into(),
                arguments: serde_json::json!({"path": "denied"}),
            },
        ];
        let receipts = adapter.record_tool_calls(&request, &calls).unwrap();
        adapter
            .close_request(
                &request,
                0,
                crate::loop_driver::LoopRequestTerminal::ResponseCompleted,
                "provider_done",
            )
            .unwrap();
        let results = vec![
            ToolResultEntry {
                call_id: "call-1".into(),
                tool_name: "read".into(),
                content: vec![ContentBlock::Text {
                    text: "[REDACTED] final enriched".into(),
                }],
                is_error: true,
                args_summary: None,
            },
            ToolResultEntry {
                call_id: "call-2".into(),
                tool_name: "write".into(),
                content: vec![ContentBlock::Text {
                    text: "not dispatched".into(),
                }],
                is_error: true,
                args_summary: None,
            },
        ];
        adapter
            .record_tool_results(
                &crate::loop_driver::LoopStepIdentity {
                    step_id: request.step_id,
                    turn_id: request.turn_id,
                    step_ordinal: 0,
                },
                &receipts,
                &results,
                &[
                    crate::loop_driver::LoopInvocationTerminal::Denied {
                        reason_code: "permission_denied".into(),
                    },
                    crate::loop_driver::LoopInvocationTerminal::NotDispatched {
                        reason_code: "tool_execution_limit".into(),
                    },
                ],
            )
            .unwrap();
        let mut conversation = ConversationState::new();
        conversation.push_user("capture".into());
        conversation.intent.stats.turns = 1;
        conversation.push_assistant(crate::conversation::AssistantMessage {
            tool_calls: calls.clone(),
            ..Default::default()
        });
        for result in results {
            conversation.push_tool_result(result);
        }
        conversation.intent.stats.turns = 2;
        let plan = crate::context_compaction_service::ContextCompactionBinding::direct_for_test()
            .plan(
                conversation
                    .context_compaction_snapshot()
                    .with_retained_token_budget(0),
                crate::context_compaction_service::ContextCompactionModeV1::Pressure,
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(plan.evict_count, 1);
        let compaction = crate::session_compaction::SessionCompaction::begin_turn(
            authority.clone(),
            request.turn_id,
            request.step_id,
            crate::session_authority::CompactionTrigger::ContextPressure,
            &plan,
        )
        .unwrap()
        .unwrap();
        assert!(compaction.provider_payload().contains("capture"));
        assert!(!compaction.provider_payload().contains("final enriched"));
        let lease_id = uuid::Uuid::new_v4();
        authority
            .record_route_lease(
                "2026-08-21T12:00:02Z",
                crate::session_authority::RouteLeaseRecorded {
                    lease_id,
                    request_id: compaction.compaction_request_id(),
                    turn_id: request.turn_id,
                    selected_provider_id: "anthropic".into(),
                    selected_model_id: route().selected_model,
                    serving_provider_id: "anthropic".into(),
                    serving_model_id: route().serving_model,
                    schema_dialect: route().schema_dialect,
                    credential_source_class: "test".into(),
                    fallback_reason: None,
                    contribution_generation_id: "provider:anthropic/v1".into(),
                    route_policy: "direct".into(),
                },
            )
            .unwrap();
        compaction
            .prepare(crate::session_authority::CompactionRoute::TurnLease { lease_id })
            .unwrap();
        compaction.commit_done("compacted summary", None).unwrap();
        plan.apply(&mut conversation, "compacted summary".into());
        assert_eq!(conversation.message_count(), 3);
        let descriptor = authority.projection_worker_descriptor();
        let replay = crate::session_replay::SessionReplay::replay_prefix(
            &descriptor.session_snapshot,
            &descriptor.session_id,
            descriptor.stream_id,
            crate::session_replay::ReplayEnd::Event(authority.state().last_event_id.unwrap()),
        )
        .unwrap();
        let current =
            crate::session_current_context::CurrentContextDraftV1::derive(&replay).unwrap();
        assert_eq!(current.items.len(), 4);
        assert!(matches!(&current.items[1].message,
            crate::bridge::LlmMessage::Assistant { tool_calls, .. } if tool_calls.len() == 2));
        assert!(matches!(&current.items[2].message,
            crate::bridge::LlmMessage::ToolResult { call_id, content, .. }
                if call_id == "call-1" && content.contains("final enriched")));
        assert!(matches!(&current.items[3].message,
            crate::bridge::LlmMessage::ToolResult { call_id, .. } if call_id == "call-2"));
        let mut next = LoopSemanticFactAdapter::new(&scope);
        assert_eq!(next.current_context_messages(&[]).unwrap().len(), 4);
    }

    #[test]
    fn tool_facts_preserve_provider_order_cardinality_and_final_content() {
        let (directory, authority, scope) = authority_scope();
        let mut adapter = LoopSemanticFactAdapter::new(&scope);
        let request = capture_request(&mut adapter, "system", &[], &[]);
        commit_request(&adapter, &scope, &request, None, 2);
        let calls = vec![
            ToolCall {
                id: "call-1".into(),
                name: "read".into(),
                arguments: serde_json::json!({"z": 1, "a": "exact"}),
            },
            ToolCall {
                id: "call-2".into(),
                name: "write".into(),
                arguments: serde_json::json!({"path": "denied"}),
            },
        ];
        let receipts = adapter.record_tool_calls(&request, &calls).unwrap();
        adapter
            .close_request(
                &request,
                0,
                crate::loop_driver::LoopRequestTerminal::ResponseCompleted,
                "provider_done",
            )
            .unwrap();
        let results = vec![
            ToolResultEntry {
                call_id: "call-1".into(),
                tool_name: "read".into(),
                content: vec![ContentBlock::Text {
                    text: "[REDACTED] final enriched".into(),
                }],
                is_error: true,
                args_summary: None,
            },
            ToolResultEntry {
                call_id: "call-2".into(),
                tool_name: "write".into(),
                content: vec![ContentBlock::Text {
                    text: "not dispatched".into(),
                }],
                is_error: true,
                args_summary: None,
            },
        ];
        adapter
            .record_tool_results(
                &crate::loop_driver::LoopStepIdentity {
                    step_id: request.step_id,
                    turn_id: request.turn_id,
                    step_ordinal: 0,
                },
                &receipts,
                &results,
                &[
                    crate::loop_driver::LoopInvocationTerminal::Denied {
                        reason_code: "permission_denied".into(),
                    },
                    crate::loop_driver::LoopInvocationTerminal::NotDispatched {
                        reason_code: "tool_execution_limit".into(),
                    },
                ],
            )
            .unwrap();
        let state = authority.state();
        let mut recorded_calls = state.tool_calls.values().collect::<Vec<_>>();
        recorded_calls.sort_by_key(|call| call.call_ordinal);
        let mut recorded_results = state.tool_results.values().collect::<Vec<_>>();
        recorded_results.sort_by_key(|result| result.result_ordinal);
        assert_eq!(
            recorded_calls
                .iter()
                .map(|call| call.call_id.as_str())
                .collect::<Vec<_>>(),
            ["call-1", "call-2"]
        );
        assert_eq!(
            recorded_results
                .iter()
                .map(|result| result.call_id.as_str())
                .collect::<Vec<_>>(),
            ["call-1", "call-2"]
        );
        assert!(
            recorded_results
                .iter()
                .all(|result| result.invocation_id.is_none() && result.lease_id.is_none())
        );
        assert_eq!(
            authority
                .read_content(
                    &recorded_calls[0].arguments_ref,
                    crate::session_authority::ProjectionClass::Default
                )
                .unwrap(),
            canonical_json_bytes(&calls[0].arguments).unwrap()
        );
        assert_eq!(
            authority
                .read_content(
                    &recorded_results[0].content_ref,
                    crate::session_authority::ProjectionClass::Default
                )
                .unwrap(),
            canonical_json_bytes(&results[0].content).unwrap()
        );
        drop(state);

        adapter
            .close_step(
                &crate::loop_driver::LoopStepIdentity {
                    step_id: request.step_id,
                    turn_id: request.turn_id,
                    step_ordinal: 0,
                },
                crate::loop_driver::LoopStepOutcome::Continue,
                "tool_results_committed",
            )
            .unwrap();
        let messages = vec![
            crate::bridge::LlmMessage::Assistant {
                text: Vec::new(),
                thinking: Vec::new(),
                tool_calls: calls
                    .iter()
                    .map(|call| crate::bridge::WireToolCall {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        arguments: call.arguments.clone(),
                    })
                    .collect(),
                raw: None,
            },
            crate::bridge::LlmMessage::ToolResult {
                call_id: "call-1".into(),
                tool_name: "read".into(),
                content: "final enriched".into(),
                images: Vec::new(),
                is_error: true,
                args_summary: None,
            },
            crate::bridge::LlmMessage::ToolResult {
                call_id: "call-2".into(),
                tool_name: "write".into(),
                content: "not dispatched".into(),
                images: Vec::new(),
                is_error: true,
                args_summary: None,
            },
        ];
        let next = capture_request(&mut adapter, "system", &messages, &[]);
        assert_current_context_matches_capture(&directory, &authority, &next);
    }

    #[test]
    fn text_and_policy_steps_close_with_truthful_continuations() {
        for (outcome, reason, expected) in [
            (
                crate::loop_driver::LoopStepOutcome::Finish,
                "assistant_complete",
                crate::session_authority::StepOutcome::TurnCompleted,
            ),
            (
                crate::loop_driver::LoopStepOutcome::Continue,
                "text_policy_continuation",
                crate::session_authority::StepOutcome::ContinueLoop,
            ),
        ] {
            let (_directory, authority, scope) = authority_scope();
            let mut adapter = LoopSemanticFactAdapter::new(&scope);
            let request = capture_request(&mut adapter, "system", &[], &[]);
            commit_request(&adapter, &scope, &request, Some("final text"), 0);
            adapter
                .close_request(
                    &request,
                    0,
                    crate::loop_driver::LoopRequestTerminal::ResponseCompleted,
                    "provider_done",
                )
                .unwrap();
            let step = crate::loop_driver::LoopStepIdentity {
                step_id: request.step_id,
                turn_id: request.turn_id,
                step_ordinal: 0,
            };
            adapter.close_step(&step, outcome, reason).unwrap();
            assert!(
                matches!(authority.state().terminal_steps[&step.step_id], crate::session_authority::StepTerminalState::Closed { ref closure } if closure.outcome == expected)
            );
        }
    }

    #[test]
    fn result_append_failure_blocks_projection_and_next_step() {
        let (_directory, authority, scope) = authority_scope();
        let mut adapter = LoopSemanticFactAdapter::new(&scope);
        let request = capture_request(&mut adapter, "system", &[], &[]);
        commit_request(&adapter, &scope, &request, None, 1);
        let calls = vec![ToolCall {
            id: "denied-call".into(),
            name: "write".into(),
            arguments: serde_json::json!({"path": "blocked"}),
        }];
        let receipts = adapter.record_tool_calls(&request, &calls).unwrap();
        adapter
            .close_request(
                &request,
                0,
                crate::loop_driver::LoopRequestTerminal::ResponseCompleted,
                "provider_done",
            )
            .unwrap();
        let mut conversation = ConversationState::new();
        let before = conversation.replay_messages().len();
        authority.make_next_append_fail();
        let result = ToolResultEntry {
            call_id: "denied-call".into(),
            tool_name: "write".into(),
            content: vec![ContentBlock::Text {
                text: "permission denied".into(),
            }],
            is_error: true,
            args_summary: None,
        };
        let step = crate::loop_driver::LoopStepIdentity {
            step_id: request.step_id,
            turn_id: request.turn_id,
            step_ordinal: 0,
        };
        assert!(
            adapter
                .record_tool_results(
                    &step,
                    &receipts,
                    std::slice::from_ref(&result),
                    &[crate::loop_driver::LoopInvocationTerminal::Denied {
                        reason_code: "permission_denied".into(),
                    }],
                )
                .is_err()
        );
        assert_eq!(conversation.replay_messages().len(), before);
        assert!(adapter.start_step().is_err());
        assert!(!authority.state().terminal_steps.contains_key(&step.step_id));
        // The production loop performs this push only after the append and close succeed.
        conversation.push_tool_result(result);
        assert_eq!(conversation.replay_messages().len(), before + 1);
    }

    #[test]
    fn capture_preserves_exact_dispatch_inputs_and_event_backed_provenance() {
        let (directory, authority, scope) = authority_scope();
        let mut adapter = LoopSemanticFactAdapter::new(&scope);
        let messages = vec![crate::bridge::LlmMessage::User {
            content: "capture".into(),
            images: Vec::new(),
        }];
        let tools = vec![tool(serde_json::json!({
            "type": "object",
            "properties": {"path": {"type": "string"}}
        }))];
        let request = capture_request(&mut adapter, "exact system", &messages, &tools);
        let state = authority.state();
        let preparation = state.model_requests[&request.request_id].preparation();

        assert_eq!(
            authority
                .read_content(
                    &preparation.context_items[0].content_ref,
                    crate::session_authority::ProjectionClass::Default,
                )
                .unwrap(),
            b"exact system"
        );
        assert_eq!(
            authority
                .read_content(
                    &preparation.context_items[1].content_ref,
                    crate::session_authority::ProjectionClass::Default,
                )
                .unwrap(),
            canonical_json_bytes(&messages[0]).unwrap()
        );
        assert_eq!(
            authority
                .read_content(
                    &preparation.schema_set.schemas[0].schema_content_ref,
                    crate::session_authority::ProjectionClass::Default,
                )
                .unwrap(),
            canonical_json_bytes(&tools[0]).unwrap()
        );
        assert_eq!(
            preparation.context_items[1].provenance.source_kind,
            crate::session_authority::ModelContextSourceKind::Prompt
        );
        assert!(
            preparation.context_items[1]
                .provenance
                .source_event_id
                .is_some()
        );
        assert_eq!(
            preparation.schema_set.composition_generation_id.as_str(),
            "composition:test"
        );
        assert_eq!(
            preparation.schema_set.schemas[0]
                .owner_generation_id
                .as_str(),
            "feature:tools/builtin-v1"
        );
        assert_eq!(
            preparation.schema_set.normalizer_generation_id,
            route().normalizer_generation_id
        );
        drop(state);
        assert_current_context_matches_capture(&directory, &authority, &request);
    }

    #[test]
    fn captured_host_policy_replays_after_instruction_sources_change_and_disappear() {
        let workspace = tempfile::tempdir().unwrap();
        let global = workspace.path().join("global.md");
        let project = workspace.path().join("AGENTS.md");
        std::fs::write(&global, "ORIGINAL OPERATOR 界").unwrap();
        std::fs::write(&project, "ORIGINAL PROJECT POLICY").unwrap();
        let assembled = crate::prompt::assemble_host_prompt(workspace.path(), &global).unwrap();
        let (directory, authority, scope) = authority_scope();
        let mut adapter = LoopSemanticFactAdapter::new(&scope);
        let messages = vec![crate::bridge::LlmMessage::User {
            content: "capture".into(),
            images: vec![],
        }];
        let request = capture_request(&mut adapter, &assembled.prompt, &messages, &[]);
        let preparation = authority.state().model_requests[&request.request_id]
            .preparation()
            .clone();
        let source = &preparation.context_items[0];
        assert!(source.provenance.source_event_id.is_some());
        for remove in [false, true] {
            if remove {
                std::fs::remove_file(&global).unwrap();
                std::fs::remove_file(&project).unwrap();
            } else {
                std::fs::write(&global, "REPLACED OPERATOR").unwrap();
                std::fs::write(&project, "REPLACED PROJECT").unwrap();
            }
            let current = crate::prompt::assemble_host_prompt(workspace.path(), &global).unwrap();
            assert!(
                current
                    .prompt
                    .starts_with(&crate::prompt::core_directives())
            );
            assert_ne!(current.prompt, assembled.prompt);
            assert_current_context_matches_capture(&directory, &authority, &request);
            assert_eq!(
                authority
                    .read_content(
                        &source.content_ref,
                        crate::session_authority::ProjectionClass::Default
                    )
                    .unwrap(),
                assembled.prompt.as_bytes()
            );
        }
        // A captured historical family is evidence, not a template to migrate.
        let (history_dir, history_authority, history_scope) = authority_scope();
        let mut history_adapter = LoopSemanticFactAdapter::new(&history_scope);
        let historical = "Historical Full/Slim system bytes — Commit when done";
        let history_request = capture_request(&mut history_adapter, historical, &messages, &[]);
        assert_current_context_matches_capture(&history_dir, &history_authority, &history_request);
        let stored = history_authority.state().model_requests[&history_request.request_id]
            .preparation()
            .context_items[0]
            .content_ref
            .clone();
        assert_eq!(
            history_authority
                .read_content(&stored, crate::session_authority::ProjectionClass::Default)
                .unwrap(),
            historical.as_bytes()
        );
    }

    #[test]
    fn full_spine_capture_rejects_unattributed_legacy_transcript_content() {
        let (_directory, authority, scope) = authority_scope();
        let mut adapter = LoopSemanticFactAdapter::new(&scope);
        let step = adapter.start_step().unwrap().unwrap();
        let messages = vec![crate::bridge::LlmMessage::User {
            content: "legacy snapshot content".into(),
            images: Vec::new(),
        }];

        let error = adapter
            .prepare_model_request(LoopModelRequestCapture {
                step: &step,
                purpose: crate::loop_driver::LoopModelRequestPurpose::Initial,
                replaces: None,
                system_prompt: "system",
                messages: &messages,
                tools: &[],
                tool_lineage: &lineage(0),
                route: &route(),
            })
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("provider dispatch messages do not byte-match current context")
        );
        assert!(authority.state().model_requests.is_empty());
    }

    #[test]
    fn schema_identity_is_canonical_and_changes_with_enabled_tools() {
        fn schema_id(tools: Vec<omegon_traits::ToolDefinition>) -> String {
            let (_directory, authority, scope) = authority_scope();
            let mut adapter = LoopSemanticFactAdapter::new(&scope);
            let request = capture_request(&mut adapter, "system", &[], &tools);
            authority.state().model_requests[&request.request_id]
                .preparation()
                .schema_set_id
                .clone()
        }

        let first = tool(
            serde_json::from_str(
                r#"{"type":"object","properties":{"b":{"type":"number"},"a":{"type":"string"}}}"#,
            )
            .unwrap(),
        );
        let reordered = tool(
            serde_json::from_str(
                r#"{"properties":{"a":{"type":"string"},"b":{"type":"number"}},"type":"object"}"#,
            )
            .unwrap(),
        );
        assert_eq!(schema_id(vec![first.clone()]), schema_id(vec![reordered]));
        assert_ne!(
            schema_id(vec![first.clone()]),
            schema_id(vec![first, tool(serde_json::json!({"type": "object"}))])
        );
    }

    #[test]
    fn dispatch_capture_rejects_changed_schema_without_reusing_request_identity() {
        let (_directory, authority, scope) = authority_scope();
        let mut facts = LoopSemanticFactAdapter::new(&scope);
        let tools = [tool(serde_json::json!({"type":"object"}))];
        let identity = capture_request(&mut facts, "system", &[], &tools);
        let messages = facts.current_context_messages(&[]).unwrap();
        let route = route();
        let route = crate::provider_route_service::LoopRoute {
            selected_model: route.selected_model,
            serving_model: route.serving_model,
            provider_id: route.provider_id,
            schema_dialect: route.schema_dialect,
            contribution_generation_id: route.contribution_generation_id,
            normalizer_contribution_id: route.normalizer_contribution_id,
            normalizer_generation_id: route.normalizer_generation_id,
            options: crate::bridge::StreamOptions::default(),
        };
        let mut inputs = crate::model_request::RequestInputs {
            system: "system",
            messages: &messages,
            tools: &tools,
            options: &route.options,
            policy: crate::model_request::RequestPolicy::Turn,
        };
        validate_dispatch_capture(&authority, &identity, &inputs, &route).unwrap();
        let changed = [tool(
            serde_json::json!({"type":"object","required":["new_argument"]}),
        )];
        inputs.tools = &changed;
        assert!(validate_dispatch_capture(&authority, &identity, &inputs, &route).is_err());
        assert_eq!(authority.state().model_requests.len(), 1);
        assert!(authority.state().route_leases.is_empty());
    }

    #[test]
    fn repair_allocates_next_request_in_same_step() {
        let (directory, authority, scope) = authority_scope();
        let mut adapter = LoopSemanticFactAdapter::new(&scope);
        let step = adapter.start_step().unwrap().unwrap();
        let initial_messages = adapter.current_context_messages(&[]).unwrap();
        let initial = adapter
            .prepare_model_request(LoopModelRequestCapture {
                step: &step,
                purpose: crate::loop_driver::LoopModelRequestPurpose::Initial,
                replaces: None,
                system_prompt: "system",
                messages: &initial_messages,
                tools: &[],
                tool_lineage: &lineage(0),
                route: &route(),
            })
            .unwrap()
            .unwrap();
        adapter
            .supersede_for_repair(
                &initial,
                crate::loop_driver::LoopModelRequestPurpose::ProviderHistoryRepair,
            )
            .unwrap();
        let repair_messages = adapter.current_context_messages(&[]).unwrap();
        let repair = adapter
            .prepare_model_request(LoopModelRequestCapture {
                step: &step,
                purpose: crate::loop_driver::LoopModelRequestPurpose::ProviderHistoryRepair,
                replaces: Some(&initial),
                system_prompt: "system",
                messages: &repair_messages,
                tools: &[],
                tool_lineage: &lineage(0),
                route: &route(),
            })
            .unwrap()
            .unwrap();

        assert_eq!(repair.step_id, initial.step_id);
        assert_eq!(repair.request_ordinal, initial.request_ordinal + 1);
        assert_current_context_matches_capture(&directory, &authority, &repair);
        assert_eq!(
            authority.state().active_step.unwrap().start.step_id,
            step.step_id
        );
    }

    #[test]
    fn turn_preparation_adapter_preserves_both_repair_identities_and_manifests() {
        use crate::loop_driver::LoopModelRequestPurpose;
        for purpose in [
            LoopModelRequestPurpose::ContextOverflowRepair,
            LoopModelRequestPurpose::ProviderHistoryRepair,
        ] {
            let (directory, authority, scope) = authority_scope();
            let mut facts = LoopSemanticFactAdapter::new(&scope);
            let step = facts.start_step().unwrap().unwrap();
            let route = route();
            let (events, _) = tokio::sync::broadcast::channel(4);
            let config = crate::r#loop::LoopConfig {
                max_retries: 3,
                retry_delay_ms: 42,
                ..Default::default()
            };
            let mut runtime = crate::bus::EventBus::new();
            runtime.finalize();
            let invocation = crate::loop_driver::LoopInvocationPort::new(&mut runtime);
            let preparation = TurnRequestPreparation {
                step: Some(&step),
                compatibility_step_id: uuid::Uuid::new_v4(),
                system_prompt: "system",
                tools: &[],
                route: &route,
                scope: &scope,
                events: &events,
                config: &config,
            };
            let initial = preparation
                .capture(
                    &mut facts,
                    &invocation,
                    &[],
                    LoopModelRequestPurpose::Initial,
                    None,
                )
                .unwrap();
            let initial_id = initial.identity.as_ref().unwrap();
            crate::provider_route_service::record_loop_route_lease_for_test(
                &scope,
                step.step_id,
                &route.selected_model,
                &route.serving_model,
                initial_id,
            )
            .unwrap();
            let repair = preparation
                .capture(&mut facts, &invocation, &[], purpose, Some(initial_id))
                .unwrap();
            let repair_id = repair.identity.as_ref().unwrap();
            assert_current_context_matches_capture(&directory, &authority, repair_id);
            crate::provider_route_service::record_loop_route_lease_for_test(
                &scope,
                step.step_id,
                &route.selected_model,
                &route.serving_model,
                repair_id,
            )
            .unwrap();
            let dispatch = preparation.dispatch(&repair, &facts);
            assert_eq!(dispatch.step_id, step.step_id);
            assert_eq!(dispatch.max_retries, 3);
            assert_eq!(dispatch.retry_delay_ms, 42);
            assert!(dispatch.tools.is_empty());
            assert!(dispatch.response_facts.is_some());
            assert_ne!(repair_id.request_id, initial_id.request_id);
            assert_eq!(repair_id.request_ordinal, initial_id.request_ordinal + 1);
            let state = authority.state();
            let prepared = state.model_requests[&repair_id.request_id].preparation();
            assert_eq!(prepared.replaces_request_id, Some(initial_id.request_id));
            assert_eq!(prepared.purpose, purpose.into());
            assert_eq!(state.route_leases.len(), 2);
            assert_ne!(
                state.request_route_joins[&repair_id.request_id].lease_id,
                state.request_route_joins[&initial_id.request_id].lease_id
            );
            assert_eq!(
                authority
                    .read_content(
                        &prepared.context_items[0].content_ref,
                        crate::session_authority::ProjectionClass::Default
                    )
                    .unwrap(),
                dispatch.system_prompt.as_bytes()
            );
            for (item, message) in prepared.context_items.iter().skip(1).zip(dispatch.messages) {
                assert_eq!(
                    authority
                        .read_content(
                            &item.content_ref,
                            crate::session_authority::ProjectionClass::Default
                        )
                        .unwrap(),
                    canonical_json_bytes(message).unwrap()
                );
            }
        }
    }

    #[tokio::test]
    async fn applied_compaction_context_matches_next_prepared_capture() {
        let (directory, authority, scope) = authority_scope();
        let mut adapter = LoopSemanticFactAdapter::new(&scope);
        authority
            .admit_prompt(
                uuid::Uuid::new_v4(),
                "2026-08-21T12:00:01Z",
                crate::session_authority::PromptAdmitted {
                    submission_id: uuid::Uuid::new_v4(),
                    prompt_id: uuid::Uuid::new_v4(),
                    principal: "operator".into(),
                    ingress: "test".into(),
                    queue_mode: crate::session_authority::QueueMode::UntilReady,
                    content: crate::session_authority::PromptContent {
                        text: "second context".into(),
                        attachments: Vec::new(),
                    },
                    metadata: serde_json::json!({}),
                },
            )
            .unwrap();
        let history = vec![
            crate::bridge::LlmMessage::User {
                content: "capture".into(),
                images: Vec::new(),
            },
            crate::bridge::LlmMessage::User {
                content: "second context".into(),
                images: Vec::new(),
            },
        ];
        let second = capture_request(&mut adapter, "system", &history, &[]);
        adapter
            .supersede_for_repair(
                &second,
                crate::loop_driver::LoopModelRequestPurpose::ContextOverflowRepair,
            )
            .unwrap();
        let compaction = crate::session_compaction::SessionCompaction::begin_turn(
            authority.clone(),
            second.turn_id,
            second.step_id,
            crate::session_authority::CompactionTrigger::ContextOverflow,
            &crate::context_compaction_service::ContextCompactionPlanV1 {
                payload: String::new(),
                evict_count: 1,
                source_is_prefix: true,
                reason: None,
                application: crate::context_compaction_service::ContextCompactionApplicationV1::KeepRecent(0),
                source_messages: history.clone(),
                previous_summary: None,
            },
        )
        .unwrap()
        .unwrap();
        let bridge = crate::bridge::MockBridge {
            events: vec![
                crate::bridge::LlmEvent::TextDelta {
                    delta: "compacted summary".into(),
                },
                crate::bridge::LlmEvent::Done {
                    message: serde_json::json!({}),
                    input_tokens: 1,
                    output_tokens: 1,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                    provider_telemetry: None,
                },
            ],
        };
        let summary = crate::provider_route_service::compact_loop_route(
            &bridge,
            crate::provider_route_service::LoopCompactionRequest {
                payload: "compatibility must not replace authority",
                options: &crate::bridge::StreamOptions::default(),
                selected_model: &route().selected_model,
                scope: &scope,
                step_id: second.step_id,
                authority: Some(&compaction),
            },
        )
        .await
        .unwrap();
        assert_eq!(summary, "compacted summary");
        assert!(
            authority
                .state()
                .route_leases
                .values()
                .any(
                    |lease| lease.request_id == compaction.compaction_request_id()
                        && lease.turn_id == second.turn_id
                )
        );
        let compatibility_compacted = [crate::bridge::LlmMessage::User {
            content: "compacted summary".into(),
            images: Vec::new(),
        }];
        let compacted = adapter
            .current_context_messages(&compatibility_compacted)
            .unwrap();
        assert_eq!(compacted.len(), 2);
        assert!(
            matches!(&compacted[0], crate::bridge::LlmMessage::User { content, .. }
            if content.contains("compacted summary"))
        );
        assert!(
            matches!(&compacted[1], crate::bridge::LlmMessage::User { content, .. }
            if content == "second context")
        );
        let repair = adapter
            .prepare_model_request(LoopModelRequestCapture {
                step: &crate::loop_driver::LoopStepIdentity {
                    step_id: second.step_id,
                    turn_id: second.turn_id,
                    step_ordinal: 1,
                },
                purpose: crate::loop_driver::LoopModelRequestPurpose::ContextOverflowRepair,
                replaces: Some(&second),
                system_prompt: "system",
                messages: &compacted,
                tools: &[],
                tool_lineage: &lineage(0),
                route: &route(),
            })
            .unwrap()
            .unwrap();
        assert_current_context_matches_capture(&directory, &authority, &repair);
    }

    #[test]
    fn sessionless_execution_does_not_fabricate_steps() {
        let mut adapter =
            LoopSemanticFactAdapter::new(&crate::invocation_service::InvocationScope::default());
        assert!(!adapter.enabled());
        assert!(adapter.start_step().unwrap().is_none());
        let compatibility = [crate::bridge::LlmMessage::User {
            content: "typed compatibility".into(),
            images: Vec::new(),
        }];
        assert_eq!(
            canonical_json_bytes(&adapter.current_context_messages(&compatibility).unwrap())
                .unwrap(),
            canonical_json_bytes(&compatibility).unwrap()
        );
    }

    #[test]
    fn partial_authority_scope_fails_closed_instead_of_downgrading_to_sessionless() {
        let mut adapter =
            LoopSemanticFactAdapter::new(&crate::invocation_service::InvocationScope {
                session_id: Some("partial".into()),
                ..Default::default()
            });
        assert!(!adapter.enabled());
        assert!(adapter.start_step().is_err());
    }

    fn plan_call(action: &str) -> ToolCall {
        ToolCall {
            id: format!("plan-{action}"),
            name: crate::tool_registry::core::PLAN.into(),
            arguments: serde_json::json!({"action": action}),
        }
    }

    fn visible_plan(items: Vec<(&str, WorkItemStatus)>) -> VisiblePlanState {
        VisiblePlanState {
            plan_id: "repo:example".into(),
            scope: PlanScope::Repo,
            source: PlanSource::OpenSpec,
            binding: PlanBinding::default(),
            mode: PlanMode::Executing,
            items: items
                .into_iter()
                .map(|(description, status)| WorkItem {
                    description: description.into(),
                    status,
                    intent: None,
                    completion_policy: Default::default(),
                    evidence: Vec::new(),
                })
                .collect(),
        }
    }

    fn completion_config() -> crate::r#loop::LoopConfig {
        crate::r#loop::LoopConfig {
            max_turns: 50,
            allow_commit_nudge: false,
            ..Default::default()
        }
    }

    #[test]
    fn ambient_capture_adapter_applies_blocks_and_reports_constraint_count() {
        let mut conversation = ConversationState::new();
        let count = LoopSessionCompatibilityAdapter::default().capture_ambient(
            &mut conversation,
            "<omg:constraint>Keep finalization bounded</omg:constraint>",
        );

        assert_eq!(count, 1);
        assert!(
            conversation
                .intent
                .constraints_discovered
                .iter()
                .any(|constraint| constraint.contains("Keep finalization bounded"))
        );
    }

    #[test]
    fn reconciliation_budget_is_bounded_and_rearms_on_visible_progress() {
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .set_work_plan(vec!["Inspect".into(), "Patch".into()]);
        let mut adapter = LoopSessionCompatibilityAdapter::default();
        let config = completion_config();

        for _ in 0..MAX_PLAN_RECONCILIATION_NUDGES {
            assert!(
                adapter
                    .completion_directive(&mut conversation, "Done", 1, &config)
                    .is_some()
            );
        }
        assert!(
            adapter
                .completion_directive(&mut conversation, "Done", 1, &config)
                .is_none()
        );
        conversation.intent.advance_work_plan();
        assert!(
            adapter
                .completion_directive(&mut conversation, "Done", 1, &config)
                .is_some()
        );
    }

    #[test]
    fn commit_completion_directive_preserves_guidance_advisory_and_one_shot_state() {
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_modified
            .insert(std::path::PathBuf::from("src/lib.rs"));
        let config = crate::r#loop::LoopConfig {
            allow_commit_nudge: true,
            max_turns: 50,
            ..Default::default()
        };
        let mut adapter = LoopSessionCompatibilityAdapter::default();

        let directive = adapter
            .completion_directive(
                &mut conversation,
                "All changes have been applied and validated.",
                2,
                &config,
            )
            .expect("completion with mutations should produce guidance");

        assert_eq!(
            directive.guidance,
            "[System: You have uncommitted file changes. Commit your work before finishing.]"
        );
        let advisory = directive
            .advisory
            .expect("commit guidance emits an advisory");
        assert_eq!(advisory.drift_kind, omegon_traits::DriftKind::ClosureStall);
        assert_eq!(
            advisory.progress_nudge_reason,
            omegon_traits::ProgressNudgeReason::CommitHygiene
        );
        assert!(conversation.intent.commit_nudged);
        assert!(
            adapter
                .completion_directive(
                    &mut conversation,
                    "All changes have been applied and validated.",
                    2,
                    &config,
                )
                .is_none()
        );
    }

    #[test]
    fn phase_completion_matches_number_or_label_case_insensitively() {
        let mut adapter = LoopSessionCompatibilityAdapter::default();
        for response in ["Completed Phase 10.", "export TO FILE is complete."] {
            let mut conversation = ConversationState::new();
            let config = crate::r#loop::LoopConfig {
                skill_phases: vec![CompletionPhaseObligation {
                    number: "10".into(),
                    label: "Export to File".into(),
                }],
                ..completion_config()
            };

            assert!(
                adapter
                    .completion_directive(&mut conversation, response, 2, &config)
                    .is_none(),
                "response should satisfy the final phase: {response}"
            );
        }
    }

    #[test]
    fn incomplete_phase_directive_preserves_labels_guidance_and_one_shot_state() {
        let mut conversation = ConversationState::new();
        let config = crate::r#loop::LoopConfig {
            skill_phases: vec![
                CompletionPhaseObligation {
                    number: "3".into(),
                    label: "Validate Output".into(),
                },
                CompletionPhaseObligation {
                    number: "4".into(),
                    label: "Export to File".into(),
                },
            ],
            ..completion_config()
        };
        let mut adapter = LoopSessionCompatibilityAdapter::default();

        let directive = adapter
            .completion_directive(&mut conversation, "Phase 3 is complete.", 2, &config)
            .expect("missing final phase should produce guidance");

        assert_eq!(
            directive.guidance,
            "[System: You have not completed all phases of the active skill. The following phase(s) still need to be executed:\n  - Export to File\n\nPlease continue and complete the remaining phases before finishing.]"
        );
        assert!(directive.advisory.is_none());
        assert!(conversation.intent.skill_completion_nudged);
        assert!(
            adapter
                .completion_directive(&mut conversation, "Still working.", 2, &config)
                .is_none()
        );
    }

    #[test]
    fn visible_plan_drives_fingerprint_and_completion_obligation() {
        let mut conversation = ConversationState::new();
        conversation.intent.visible_plan = Some(visible_plan(vec![
            ("Repo A", WorkItemStatus::Active),
            ("Repo B", WorkItemStatus::Pending),
        ]));
        let before = plan_open_fingerprint(&conversation.intent);
        conversation.intent.visible_plan.as_mut().unwrap().items[0].status = WorkItemStatus::Done;
        conversation.intent.visible_plan.as_mut().unwrap().items[1].status = WorkItemStatus::Active;
        assert_ne!(before, plan_open_fingerprint(&conversation.intent));
    }

    #[test]
    fn reconciled_advance_continues_until_visible_work_closes() {
        let mut conversation = ConversationState::new();
        conversation.intent.plan_reconciliation_nudges = 1;
        conversation
            .intent
            .set_work_plan(vec!["first".into(), "second".into()]);
        conversation.intent.execute_work_plan();
        let adapter = &mut LoopSessionCompatibilityAdapter::default();
        let before = adapter.visible_plan_snapshot(&conversation, std::path::Path::new("."));
        conversation.intent.advance_work_plan();
        let outcome = adapter.reconcile_plan_tools(
            &conversation,
            std::path::Path::new("."),
            &before,
            &[plan_call("advance")],
            &mut [],
        );
        assert!(outcome.requires_continuation);
        assert!(!outcome.reconciled);
        assert!(
            outcome
                .notification
                .as_deref()
                .is_some_and(|message| message.starts_with("Plan progress"))
        );
        assert!(outcome.projection.is_some());

        conversation.intent.advance_work_plan();
        let outcome = adapter.reconcile_plan_tools(
            &conversation,
            std::path::Path::new("."),
            &before,
            &[plan_call("complete")],
            &mut [],
        );
        assert!(!outcome.requires_continuation);
        assert!(outcome.reconciled);
    }

    #[test]
    fn plan_list_result_is_enriched_without_changing_result_shape() {
        let mut conversation = ConversationState::new();
        conversation.intent.set_work_plan(vec!["Inspect".into()]);
        let mut results = vec![ToolResultEntry {
            call_id: "plan-list".into(),
            tool_name: crate::tool_registry::core::PLAN.into(),
            content: vec![ContentBlock::Text {
                text: "owner result".into(),
            }],
            is_error: false,
            args_summary: None,
        }];
        enrich_plan_list_tool_results(
            &mut results,
            &[plan_call("list")],
            &conversation.intent,
            None,
        );
        let text = results[0].content[0].as_text().unwrap();
        assert!(text.contains("Inspect"));
        assert!(text.contains("owner result"));
        assert!(!results[0].is_error);
    }

    #[test]
    fn continuance_approval_resolves_only_the_bound_pending_action() {
        let mut conversation = ConversationState::new();
        conversation.intent.pending_action = Some(crate::conversation::PendingAction {
            id: "pending-action-4-abcd".into(),
            source_turn: 4,
            directive_digest: "abcd".into(),
            summary: "Open PR #167".into(),
            repo_root: None,
            branch: None,
            created_at_ms: 1,
            kind: crate::conversation::PendingActionKind::Continuation,
        });
        conversation.push_user("continue".into());

        let message = LoopSessionCompatibilityAdapter::default()
            .pending_continuation(&mut conversation, std::path::Path::new("."))
            .expect("continuance approval should resolve");

        assert!(message.contains("pending_action_id=pending-action-4-abcd"));
        assert!(message.contains("Open PR #167"));
        assert!(message.contains("do not resume any older Workbench plan item"));
    }

    #[test]
    fn realtime_reminder_requires_successful_progress_and_no_plan_call() {
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .set_work_plan(vec!["Implement behavior".into()]);
        let catalog = ToolCapabilityCatalog::from_tool_defs(&[omegon_traits::ToolDefinition {
            name: "boundary".into(),
            label: "boundary".into(),
            description: String::new(),
            parameters: serde_json::Value::Null,
            capabilities: vec![omegon_traits::ToolCapability::ProgressBoundary],
        }]);
        let boundary = ToolCall {
            id: "boundary-1".into(),
            name: "boundary".into(),
            arguments: serde_json::Value::Null,
        };
        let success = ToolResultEntry {
            call_id: "boundary-1".into(),
            tool_name: "boundary".into(),
            content: vec![],
            is_error: false,
            args_summary: None,
        };
        let adapter = LoopSessionCompatibilityAdapter::default();

        assert!(
            adapter
                .realtime_completion_reminder(
                    &conversation,
                    &catalog,
                    std::slice::from_ref(&boundary),
                    std::slice::from_ref(&success),
                )
                .is_some()
        );
        assert!(
            adapter
                .realtime_completion_reminder(
                    &conversation,
                    &catalog,
                    &[boundary.clone(), plan_call("advance")],
                    &[success.clone(), success.clone()],
                )
                .is_none()
        );
        let mut failed = success;
        failed.is_error = true;
        assert!(
            adapter
                .realtime_completion_reminder(&conversation, &catalog, &[boundary], &[failed])
                .is_none()
        );
    }

    #[test]
    fn meta_recovery_is_bounded_and_preserves_guidance() {
        let mut adapter =
            LoopSessionCompatibilityAdapter::new(None, Some(behavior_policy_binding()));
        let response = "I'm wasting time and should stop exploring.";
        assert!(crate::behavior::is_pathological_meta_response(response));

        for _ in 0..2 {
            assert_eq!(
                adapter.meta_recovery(response, 1, 50),
                Some(crate::behavior::meta_recovery_retry_message())
            );
        }
        assert_eq!(adapter.meta_recovery(response, 1, 50), None);
        assert_eq!(adapter.meta_recovery(response, 50, 50), None);
    }

    #[test]
    fn absent_behavior_policy_holds_meta_recovery_state() {
        let mut adapter = LoopSessionCompatibilityAdapter::new(None, None);
        let response = "I'm wasting time and should stop exploring.";

        assert_eq!(adapter.meta_recovery(response, 1, 50), None);
        assert_eq!(adapter.meta_recovery_nudges, 0);
    }

    #[test]
    fn authorized_text_only_recovery_preserves_guidance_and_counter_reset_rules() {
        let mut adapter = LoopSessionCompatibilityAdapter::default();
        let mut conversation = ConversationState::new();
        conversation.push_user("fix the release flow".into());
        let config = crate::r#loop::LoopConfig::default();

        let directive = adapter
            .text_only_recovery(
                &conversation,
                "I can make that change. Should I proceed?",
                2,
                &config,
            )
            .expect("authorized action should continue");
        assert_eq!(
            directive.guidance.as_deref(),
            Some(
                "[System: The operator already asked you to proceed. Do not ask for confirmation or describe work you will do next. Take the next concrete action now with the available tools, or give a final answer only if the requested work is actually complete.]"
            )
        );

        let noise = ToolCall {
            id: "noise".into(),
            name: "write".into(),
            arguments: serde_json::json!({"path": "ai/session/system-warning-note.md"}),
        };
        adapter.observe_assistant_tool_calls(&[noise]);
        assert_eq!(adapter.dead_mouse_nudges, 1);
        let work = ToolCall {
            id: "work".into(),
            name: "edit".into(),
            arguments: serde_json::json!({"path": "src/lib.rs"}),
        };
        adapter.observe_assistant_tool_calls(&[work]);
        assert_eq!(adapter.dead_mouse_nudges, 0);
    }

    #[test]
    fn stuck_recovery_preserves_warning_and_escalation_copy() {
        let mut adapter = LoopSessionCompatibilityAdapter::default();
        let catalog = ToolCapabilityCatalog::from_tool_defs(&[omegon_traits::ToolDefinition {
            name: "custom".into(),
            label: "custom".into(),
            description: String::new(),
            parameters: serde_json::Value::Null,
            capabilities: vec![],
        }]);
        let call = ToolCall {
            id: "repeat".into(),
            name: "custom".into(),
            arguments: serde_json::json!({"same": true}),
        };
        for _ in 0..3 {
            adapter.stuck_detector.record(&catalog, &call, false);
        }

        let first = adapter.stuck_recovery(&catalog).expect("repeat warning");
        assert_eq!(
            first.guidance,
            "[System: You've called `custom` with the same arguments 3 times. If it's not producing the result you need, try a different approach.]"
        );
        assert!(adapter.stuck_recovery(&catalog).is_some());
        let escalation = adapter.stuck_recovery(&catalog).expect("escalation");
        assert_eq!(
            escalation.guidance,
            "[System: Repetition pressure — several recent turns repeated similar tool calls without producing new evidence. If you already have what you need, produce the deliverable now. Otherwise take one concrete, different next action. If no concrete action is possible, state the blocker plainly and stop.]"
        );
        assert!(adapter.stuck_recovery(&catalog).is_none());
    }

    fn production_loop_without_test_policy() -> String {
        let source = include_str!("loop.rs");
        let (prefix, recovery_and_tail) = source
            .split_once("#[cfg(test)]\nmod legacy_session_recovery_policy_tests")
            .expect("session recovery test-policy boundary");
        let (_, tail) = recovery_and_tail
            .split_once("#[cfg(test)]\nuse crate::loop_session::{")
            .expect("session recovery test-policy end boundary");
        let (_, tail) = tail
            .split_once("};")
            .expect("session recovery test import end boundary");
        let source = format!("{prefix}{tail}");
        let (prefix, route_and_tail) = source
            .split_once("#[cfg(test)]\nmod legacy_route_policy_tests")
            .expect("legacy route test-policy boundary");
        let (_, tail) = route_and_tail
            .split_once("#[cfg(test)]\nuse legacy_route_policy_tests::*;")
            .expect("legacy route test-policy end boundary");
        let production_and_tests = format!("{prefix}{tail}");
        production_and_tests
            .split_once("#[cfg(test)]\nmod tests")
            .map_or(production_and_tests.clone(), |(production, _)| {
                production.to_string()
            })
    }

    #[test]
    fn production_loop_cannot_regain_concrete_recovery_policy() {
        let production = production_loop_without_test_policy();
        for forbidden in [
            "struct StuckDetector",
            "fn should_continue_text_only_turn",
            "fn looks_like_incomplete_structured_answer",
            "fn looks_like_continuation_request",
            "fn user_prompt_expects_concrete_action",
            "fn looks_like_plan_or_future_action",
            "fn looks_like_blocked_response",
            "fn looks_like_completion",
            "fn is_session_noise_path",
            "is_pathological_meta_response",
            "You've inspected the same target multiple times",
            "Repetition pressure",
            "responded with text but did not advance the task",
            "Multiple turns without task progress",
            "operator already asked you to proceed",
            "Material progress boundary crossed",
        ] {
            assert!(
                !production.contains(forbidden),
                "production loop.rs regained recovery policy marker {forbidden:?}"
            );
        }
    }

    #[test]
    fn production_loop_cannot_regain_concrete_plan_policy() {
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
        let production = format!("{prefix}{production_tail}");
        for forbidden in [
            "tool_registry::core::PLAN",
            "plan_reconciliation_fingerprint",
            "plan_reconciliation_nudges",
            "WorkItemStatus::Pending",
            "WorkItemStatus::Active",
            "visible Workbench plan",
            "`plan advance`",
            "`plan complete`",
            "`plan skip`",
            "`plan clear`",
            "Some(\"advance\" | \"complete\" | \"skip\")",
        ] {
            assert!(
                !production.contains(forbidden),
                "production loop.rs regained plan policy marker {forbidden:?}"
            );
        }
    }

    #[test]
    fn production_loop_cannot_regain_concrete_commit_or_skill_policy() {
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
        let production = format!("{prefix}{production_tail}");
        for forbidden in [
            "SkillPhaseInfo",
            "CommitHygiene",
            "commit_nudged",
            "skill_completion_nudged",
            "final_phase_number",
            "final_phase_label",
            "format!(\"phase {}\"",
            "uncommitted file changes",
            "not completed all phases",
        ] {
            assert!(
                !production.contains(forbidden),
                "production loop.rs regained completion policy marker {forbidden:?}"
            );
        }
    }
}
