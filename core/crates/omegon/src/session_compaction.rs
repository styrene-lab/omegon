use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::session_authority::{
    AuthorityError, AuthorityFrontierRef, CompactionAbandoned, CompactionApplied,
    CompactionContextItem, CompactionOwnerScope, CompactionPromptTemplate,
    CompactionReplacementItem, CompactionReplacementSourceKind, CompactionRequestClosed,
    CompactionRequestOutcome, CompactionRequestPrepared, CompactionResponseAttemptFailed,
    CompactionResponseAttemptFailure, CompactionRetryDisposition, CompactionRoute,
    CompactionStarted, CompactionSummaryCommitted, CompactionTrigger, CompactionUsage,
    ModelContextSourceKind, ProjectionClass, ProviderCompletionEvidence, SessionAuthorityHandle,
    compaction_input_manifest_id,
};

const SUMMARY_PROMPT_PATH: &str = "prompts/session-compaction.md";

fn same_semantic_message(
    left: &crate::bridge::LlmMessage,
    right: &crate::bridge::LlmMessage,
) -> Result<bool, AuthorityError> {
    fn normalize(message: &crate::bridge::LlmMessage) -> crate::bridge::LlmMessage {
        let mut message = message.clone();
        match &mut message {
            crate::bridge::LlmMessage::User { images, .. } => {
                for image in images {
                    image.source_path = None;
                }
            }
            crate::bridge::LlmMessage::Assistant {
                text,
                thinking,
                raw,
                ..
            } => {
                for blocks in [text, thinking] {
                    let content = blocks.concat();
                    *blocks = if content.is_empty() {
                        Vec::new()
                    } else {
                        vec![content]
                    };
                }
                *raw = None;
            }
            crate::bridge::LlmMessage::ToolResult {
                images,
                args_summary,
                ..
            } => {
                for image in images {
                    image.source_path = None;
                }
                *args_summary = None;
            }
        }
        message
    }
    Ok(serde_json::to_value(normalize(left))? == serde_json::to_value(normalize(right))?)
}

pub(crate) fn summary_prompt() -> anyhow::Result<(String, String, String)> {
    let pack = crate::content_pack::boot_pack();
    summary_prompt_from_pack(pack.as_deref())
}

fn summary_prompt_from_pack(
    pack: Option<&crate::content_pack::ContentPack>,
) -> anyhow::Result<(String, String, String)> {
    let pack = pack.ok_or_else(|| anyhow::anyhow!("shipped compaction prompt is unavailable"))?;
    let body = pack
        .text(SUMMARY_PROMPT_PATH)
        .map_err(|error| anyhow::anyhow!("shipped compaction prompt is unavailable: {error}"))?;
    Ok((
        format!("content-pack:{}", pack.manifest.id),
        pack.generation.clone(),
        body.to_string(),
    ))
}

#[derive(Debug, Clone)]
pub(crate) struct SessionCompaction {
    authority: SessionAuthorityHandle,
    compaction_id: Uuid,
    compaction_request_id: Uuid,
    owner_scope: CompactionOwnerScope,
    source_context_revision: u64,
    target_context_revision: u64,
    retained_items: Vec<CompactionContextItem>,
    provider_payload: String,
    prompt_owner: String,
    prompt_generation: String,
    summary_prompt: String,
    response_attempt_ordinal: std::sync::Arc<std::sync::atomic::AtomicU32>,
}

impl SessionCompaction {
    pub(crate) fn begin_turn(
        authority: SessionAuthorityHandle,
        turn_id: Uuid,
        step_id: Uuid,
        trigger: CompactionTrigger,
        plan: &crate::context_compaction_service::ContextCompactionPlanV1,
    ) -> Result<Option<Self>, AuthorityError> {
        Self::begin(
            authority,
            CompactionOwnerScope::Turn { turn_id, step_id },
            trigger,
            plan,
        )
    }

    pub(crate) fn begin_idle(
        authority: SessionAuthorityHandle,
        plan: &crate::context_compaction_service::ContextCompactionPlanV1,
    ) -> Result<Option<Self>, AuthorityError> {
        Self::begin(
            authority,
            CompactionOwnerScope::SessionIdle,
            CompactionTrigger::ManualIdle,
            plan,
        )
    }

    fn begin(
        authority: SessionAuthorityHandle,
        owner_scope: CompactionOwnerScope,
        trigger: CompactionTrigger,
        plan: &crate::context_compaction_service::ContextCompactionPlanV1,
    ) -> Result<Option<Self>, AuthorityError> {
        let evict_count = plan.evict_count;
        let state = authority.state();
        if state.lineage_level != crate::session_authority::AuthorityLineageLevel::FullSpine {
            return Err(AuthorityError::Invalid(
                "compaction requires aligned full semantic lineage; legacy or mixed context is preserved".into(),
            ));
        }
        if evict_count == 0 {
            return Ok(None);
        }
        if !plan.source_is_prefix {
            return Err(AuthorityError::Invalid(
                "compaction selection is not a chronological source prefix".into(),
            ));
        }
        if evict_count > plan.source_messages.len() {
            return Err(AuthorityError::Invalid(
                "compaction boundary exceeds canonical source".into(),
            ));
        }
        let descriptor = authority.projection_worker_descriptor();
        let replay = crate::session_replay::SessionReplay::replay_prefix(
            &descriptor.session_snapshot,
            &descriptor.session_id,
            descriptor.stream_id,
            crate::session_replay::ReplayEnd::Event(state.last_event_id.ok_or_else(|| {
                AuthorityError::Invalid("compaction source frontier is empty".into())
            })?),
        )
        .map_err(|error| AuthorityError::Invalid(error.to_string()))?;
        let draft = crate::session_current_context::CurrentContextDraftV1::derive(&replay)
            .map_err(|error| AuthorityError::Invalid(error.to_string()))?;
        let mut canonical_index = 0;
        let mut summary_count = 0;
        let mut conversation_items = Vec::with_capacity(draft.items.len());
        for (index, item) in draft.items.into_iter().enumerate() {
            let event_id = item.provenance.source_event_id.ok_or_else(|| {
                AuthorityError::Invalid("compaction source event is absent".into())
            })?;
            let identity = item.provenance.source_identity.as_deref().ok_or_else(|| {
                AuthorityError::Invalid("compaction source identity is absent".into())
            })?;
            let (message, provenance) = crate::session_current_context::compaction_source_message(
                &replay, event_id, identity,
            )
            .map_err(|error| AuthorityError::Invalid(error.to_string()))?;
            if provenance.source_kind != item.provenance.source_kind
                || !same_semantic_message(&message, &item.message)?
            {
                return Err(AuthorityError::Invalid(
                    "compaction draft differs from its semantic source".into(),
                ));
            }
            if provenance.source_kind == ModelContextSourceKind::CompactionSummary {
                let previous = plan.previous_summary.as_deref().ok_or_else(|| {
                    AuthorityError::Invalid("compaction canonical summary is missing".into())
                })?;
                let expected = crate::bridge::LlmMessage::User {
                    content: format!(
                        "[Previous conversation summary]\n{previous}\n[End summary - continue from here]"
                    ),
                    images: Vec::new(),
                };
                if index != 0 || summary_count != 0 || !same_semantic_message(&message, &expected)?
                {
                    return Err(AuthorityError::Invalid(
                        "compaction previous summary does not align with authority".into(),
                    ));
                }
                summary_count += 1;
            } else {
                let expected = plan.source_messages.get(canonical_index).ok_or_else(|| {
                    AuthorityError::Invalid(
                        "compaction authority has additional canonical messages".into(),
                    )
                })?;
                if !same_semantic_message(&message, expected)? {
                    return Err(AuthorityError::Invalid(format!(
                        "compaction canonical message {canonical_index} does not align with authority"
                    )));
                }
                canonical_index += 1;
            }
            conversation_items.push((message, event_id, identity.to_string()));
        }
        if canonical_index != plan.source_messages.len()
            || usize::from(plan.previous_summary.is_some()) != summary_count
        {
            return Err(AuthorityError::Invalid(
                "compaction canonical source does not cover the exact authority draft".into(),
            ));
        }
        // Validate the full projection before writing any content or authority
        // facts. The cut now counts actual authority items, including summary.
        let cut = evict_count + summary_count;
        let (prompt_owner, prompt_generation, summary_prompt) =
            summary_prompt().map_err(|error| AuthorityError::Invalid(error.to_string()))?;
        let make_item = |ordinal: usize,
                         item: &(crate::bridge::LlmMessage, Uuid, String)|
         -> Result<CompactionContextItem, AuthorityError> {
            let bytes = crate::surfaces::session::canonical_json_bytes(&item.0)
                .map_err(|error| AuthorityError::Invalid(error.to_string()))?;
            Ok(CompactionContextItem {
                ordinal: u32::try_from(ordinal).map_err(|_| {
                    AuthorityError::Invalid("compaction item ordinal exceeds u32".into())
                })?,
                source_event_id: item.1,
                source_identity: item.2.clone(),
                content_ref: authority.write_content(
                    &bytes,
                    "application/json",
                    ProjectionClass::Default,
                )?,
            })
        };
        let input_items = conversation_items[..cut]
            .iter()
            .enumerate()
            .map(|(ordinal, item)| make_item(ordinal, item))
            .collect::<Result<Vec<_>, _>>()?;
        let retained_items = conversation_items[cut..]
            .iter()
            .enumerate()
            .map(|(ordinal, item)| make_item(ordinal, item))
            .collect::<Result<Vec<_>, _>>()?;
        let mut provider_payload = String::new();
        for item in &input_items {
            let bytes = authority.read_content(&item.content_ref, ProjectionClass::Default)?;
            let text = std::str::from_utf8(&bytes).map_err(|_| {
                AuthorityError::Invalid("compaction input is not valid UTF-8".into())
            })?;
            if !provider_payload.is_empty() {
                provider_payload.push('\n');
            }
            provider_payload.push_str(text);
        }

        let compaction_id = Uuid::new_v4();
        let compaction_request_id = Uuid::new_v4();
        let source_context_revision = state.context_revision;
        let target_context_revision = source_context_revision.checked_add(1).ok_or_else(|| {
            AuthorityError::Invalid("compaction context revision overflow".into())
        })?;
        let mut start = CompactionStarted {
            compaction_id,
            owner_scope: owner_scope.clone(),
            trigger,
            source_frontier: AuthorityFrontierRef {
                sequence: state.last_sequence,
                event_id: state.last_event_id.ok_or_else(|| {
                    AuthorityError::Invalid("compaction source frontier is empty".into())
                })?,
            },
            source_context_revision,
            input_manifest_id: String::new(),
            input_items,
            retained_items: retained_items.clone(),
            target_context_revision,
        };
        start.input_manifest_id = compaction_input_manifest_id(&start)?;
        authority.start_compaction(Uuid::new_v4(), &recorded_at_now(), start)?;
        Ok(Some(Self {
            authority,
            compaction_id,
            compaction_request_id,
            owner_scope,
            source_context_revision,
            target_context_revision,
            retained_items,
            provider_payload,
            prompt_owner,
            prompt_generation,
            summary_prompt,
            response_attempt_ordinal: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }))
    }

    pub(crate) fn compaction_request_id(&self) -> Uuid {
        self.compaction_request_id
    }

    pub(crate) fn owner_scope(&self) -> &CompactionOwnerScope {
        &self.owner_scope
    }

    pub(crate) fn provider_payload(&self) -> &str {
        &self.provider_payload
    }

    pub(crate) fn prepare(&self, route: CompactionRoute) -> Result<(), AuthorityError> {
        let prompt_ref = self.authority.write_content(
            self.summary_prompt.as_bytes(),
            "text/plain",
            ProjectionClass::Default,
        )?;
        self.authority.prepare_compaction_request(
            Uuid::new_v4(),
            &recorded_at_now(),
            CompactionRequestPrepared {
                compaction_request_id: self.compaction_request_id,
                compaction_id: self.compaction_id,
                request_ordinal: 0,
                replaces_compaction_request_id: None,
                prompt_template: CompactionPromptTemplate {
                    owner_id: self.prompt_owner.clone(),
                    owner_generation_id: self.prompt_generation.clone(),
                    content_ref: prompt_ref,
                },
                route,
            },
        )?;
        Ok(())
    }

    pub(crate) fn fail_response_attempt(
        &self,
        failure: CompactionResponseAttemptFailure,
        reason_code: &str,
    ) -> Result<(), AuthorityError> {
        let attempt = self
            .response_attempt_ordinal
            .load(std::sync::atomic::Ordering::Acquire);
        let next = attempt.checked_add(1).ok_or_else(|| {
            AuthorityError::Invalid("compaction response-attempt ordinal overflow".into())
        })?;
        self.authority.fail_compaction_response_attempt(
            Uuid::new_v4(),
            &recorded_at_now(),
            CompactionResponseAttemptFailed {
                compaction_request_id: self.compaction_request_id,
                compaction_id: self.compaction_id,
                response_attempt_ordinal: attempt,
                failure,
                reason_code: reason_code.into(),
                retry_disposition: CompactionRetryDisposition::RetrySameRequest,
            },
        )?;
        self.response_attempt_ordinal
            .store(next, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    pub(crate) fn commit_done(
        &self,
        summary: &str,
        usage: Option<CompactionUsage>,
    ) -> Result<(), AuthorityError> {
        if summary.is_empty() {
            return Err(AuthorityError::Invalid(
                "provider Done compaction summary is empty".into(),
            ));
        }
        let summary_ref = self.authority.write_content(
            summary.as_bytes(),
            "text/plain",
            ProjectionClass::Default,
        )?;
        let summary_id = Uuid::new_v4();
        let mut replacement_items = Vec::with_capacity(self.retained_items.len() + 1);
        replacement_items.push(CompactionReplacementItem {
            ordinal: 0,
            source_kind: CompactionReplacementSourceKind::CompactionSummary,
            source_event_id: Uuid::nil(),
            source_identity: summary_id.to_string(),
            content_ref: summary_ref.clone(),
        });
        replacement_items.extend(self.retained_items.iter().enumerate().map(
            |(index, retained)| CompactionReplacementItem {
                ordinal: u32::try_from(index + 1).expect("retained ordinal was validated"),
                source_kind: CompactionReplacementSourceKind::Retained,
                source_event_id: retained.source_event_id,
                source_identity: retained.source_identity.clone(),
                content_ref: retained.content_ref.clone(),
            },
        ));
        self.authority.commit_compaction_summary(
            Uuid::new_v4(),
            &recorded_at_now(),
            CompactionSummaryCommitted {
                compaction_summary_id: summary_id,
                compaction_request_id: self.compaction_request_id,
                compaction_id: self.compaction_id,
                response_attempt_ordinal: self
                    .response_attempt_ordinal
                    .load(std::sync::atomic::Ordering::Acquire),
                completion_evidence: ProviderCompletionEvidence::ProviderDone,
                summary_digest: format!("{:x}", Sha256::digest(summary.as_bytes())),
                summary_ref,
                replacement_manifest_id: String::new(),
                replacement_items,
                usage,
            },
        )?;
        self.authority.close_compaction_request(
            Uuid::new_v4(),
            &recorded_at_now(),
            CompactionRequestClosed {
                compaction_request_id: self.compaction_request_id,
                compaction_id: self.compaction_id,
                response_attempt_ordinal: self
                    .response_attempt_ordinal
                    .load(std::sync::atomic::Ordering::Acquire),
                outcome: CompactionRequestOutcome::SummaryCommitted,
                reason_code: "provider_done".into(),
                recovery_rule_version: None,
            },
        )?;
        let state = self.authority.state();
        let committed = &state.compaction_summaries[&summary_id];
        self.authority.apply_compaction(
            Uuid::new_v4(),
            &recorded_at_now(),
            CompactionApplied {
                compaction_id: self.compaction_id,
                compaction_summary_id: summary_id,
                source_context_revision: self.source_context_revision,
                target_context_revision: self.target_context_revision,
                replacement_manifest_id: committed.replacement_manifest_id.clone(),
                recovery_rule_version: None,
            },
        )?;
        Ok(())
    }

    pub(crate) fn fail(
        &self,
        outcome: CompactionRequestOutcome,
        reason_code: &str,
    ) -> Result<(), AuthorityError> {
        if outcome == CompactionRequestOutcome::SummaryCommitted
            || outcome == CompactionRequestOutcome::SupersededForRouteChange
        {
            return Err(AuthorityError::Invalid(
                "terminal compaction failure outcome is invalid".into(),
            ));
        }
        self.authority.close_compaction_request(
            Uuid::new_v4(),
            &recorded_at_now(),
            CompactionRequestClosed {
                compaction_request_id: self.compaction_request_id,
                compaction_id: self.compaction_id,
                response_attempt_ordinal: self
                    .response_attempt_ordinal
                    .load(std::sync::atomic::Ordering::Acquire),
                outcome,
                reason_code: reason_code.into(),
                recovery_rule_version: None,
            },
        )?;
        self.authority.abandon_compaction(
            Uuid::new_v4(),
            &recorded_at_now(),
            CompactionAbandoned {
                compaction_id: self.compaction_id,
                reason_code: reason_code.into(),
                last_compaction_request_id: Some(self.compaction_request_id),
                last_response_attempt_ordinal: Some(
                    self.response_attempt_ordinal
                        .load(std::sync::atomic::Ordering::Acquire),
                ),
                recovery_rule_version: 1,
            },
        )?;
        Ok(())
    }
}

fn recorded_at_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl crate::loop_driver::LoopCompactionAuthority for SessionCompaction {
    fn provider_payload<'a>(&'a self, _fallback: &'a str) -> &'a str {
        self.provider_payload()
    }

    fn compaction_request_id(&self) -> Option<Uuid> {
        Some(self.compaction_request_id())
    }

    fn is_idle(&self) -> bool {
        matches!(self.owner_scope(), CompactionOwnerScope::SessionIdle)
    }

    fn prepare(
        &self,
        evidence: crate::loop_driver::LoopCompactionRouteEvidence,
    ) -> anyhow::Result<()> {
        let endpoint_provenance = match (
            evidence.endpoint_id.clone(),
            evidence.adapter_id.clone(),
            evidence.inventory_generation,
        ) {
            (Some(endpoint_id), Some(adapter_id), Some(inventory_generation)) => Some(
                crate::session_authority::CompactionEndpointProvenanceRecorded {
                    compaction_request_id: self.compaction_request_id,
                    endpoint_id,
                    adapter_id,
                    inventory_generation,
                },
            ),
            (None, None, None) => None,
            _ => anyhow::bail!("compaction endpoint provenance is incomplete"),
        };
        let route = match self.owner_scope() {
            CompactionOwnerScope::Turn { .. } => CompactionRoute::TurnLease {
                lease_id: evidence
                    .lease_id
                    .ok_or_else(|| anyhow::anyhow!("turn compaction route lease is absent"))?,
            },
            CompactionOwnerScope::SessionIdle => {
                if evidence.lease_id.is_some() {
                    anyhow::bail!("idle compaction cannot claim a turn route lease");
                }
                CompactionRoute::SessionIdle {
                    selected_provider_id: evidence.selected_provider_id,
                    selected_model_id: evidence.selected_model_id,
                    serving_provider_id: evidence.serving_provider_id,
                    serving_model_id: evidence.serving_model_id,
                    schema_dialect: evidence.schema_dialect,
                    credential_source_class: evidence.credential_source_class,
                    fallback_reason: evidence.fallback_reason,
                    contribution_generation_id: evidence.contribution_generation_id,
                    route_policy: evidence.route_policy,
                }
            }
        };
        if matches!(self.owner_scope(), CompactionOwnerScope::SessionIdle)
            && let Some(provenance) = endpoint_provenance
        {
            let recorded = self
                .authority
                .state()
                .compaction_endpoint_provenance
                .get(&self.compaction_request_id)
                .cloned();
            if let Some(recorded) = recorded {
                if recorded != provenance {
                    anyhow::bail!("compaction endpoint provenance changed during retry");
                }
            } else {
                self.authority
                    .record_compaction_endpoint_provenance(
                        &chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        provenance,
                    )
                    .map_err(anyhow::Error::from)?;
            }
        }
        self.prepare(route).map_err(anyhow::Error::from)?;
        Ok(())
    }

    fn commit_done(&self, summary: &str) -> anyhow::Result<()> {
        self.commit_done(summary, None).map_err(anyhow::Error::from)
    }

    fn fail(&self, outcome: CompactionRequestOutcome, reason: &str) -> anyhow::Result<()> {
        self.fail(outcome, reason).map_err(anyhow::Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_authority::{
        ActorIdentity, PromptAdmitted, PromptContent, QueueMode, SessionAuthority, TurnClosed,
        TurnOutcome,
    };

    const NOW: &str = "2026-09-05T12:00:00Z";

    fn authority(directory: &tempfile::TempDir) -> SessionAuthorityHandle {
        SessionAuthorityHandle::new(
            SessionAuthority::open(
                &directory.path().join("session.json"),
                "retention",
                "workspace",
                "composition:test",
                ActorIdentity {
                    principal: "operator".into(),
                    ingress: "test".into(),
                },
                NOW,
            )
            .unwrap(),
        )
    }

    fn admitted_prompt(authority: &SessionAuthorityHandle, text: &str) {
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
                        text: text.into(),
                        attachments: Vec::new(),
                    },
                    metadata: serde_json::json!({}),
                },
            )
            .unwrap();
        let turn_id = Uuid::new_v4();
        authority
            .start_turn(Uuid::new_v4(), NOW, turn_id, prompt_id)
            .unwrap();
        authority
            .start_step(
                Uuid::new_v4(),
                NOW,
                crate::session_authority::StepStarted {
                    step_id: Uuid::new_v4(),
                    turn_id,
                    step_ordinal: 0,
                },
            )
            .unwrap();
        authority
            .terminalize_active_semantic_step(
                NOW,
                crate::session_authority::SemanticTerminalization {
                    turn_id,
                    request_outcome: crate::session_authority::ModelRequestOutcome::Cancelled,
                    reason_code: "fixture_no_provider".into(),
                    rule_version: 1,
                },
            )
            .unwrap();
        authority
            .close_turn(
                Uuid::new_v4(),
                NOW,
                TurnClosed {
                    turn_id,
                    outcome: TurnOutcome::Cancelled,
                    reason_code: "fixture_no_provider".into(),
                    recovery_rule_version: None,
                },
            )
            .unwrap();
    }

    fn plan(
        messages: &[&str],
        previous_summary: Option<&str>,
    ) -> crate::context_compaction_service::ContextCompactionPlanV1 {
        crate::context_compaction_service::ContextCompactionPlanV1 {
            payload: "compatibility payload must not replace authority".into(),
            evict_count: 1,
            source_is_prefix: true,
            reason: None,
            application:
                crate::context_compaction_service::ContextCompactionApplicationV1::KeepRecent(0),
            source_messages: messages
                .iter()
                .map(|text| crate::bridge::LlmMessage::User {
                    content: (*text).into(),
                    images: Vec::new(),
                })
                .collect(),
            previous_summary: previous_summary.map(str::to_string),
        }
    }

    fn finish(compaction: &SessionCompaction, summary: &str) {
        compaction
            .prepare(CompactionRoute::SessionIdle {
                selected_provider_id: "test".into(),
                selected_model_id: "test:model".into(),
                serving_provider_id: "test".into(),
                serving_model_id: "test:model".into(),
                schema_dialect: "test".into(),
                credential_source_class: "test".into(),
                fallback_reason: None,
                contribution_generation_id: "provider:test/v1".into(),
                route_policy: "exact".into(),
            })
            .unwrap();
        compaction.commit_done(summary, None).unwrap();
    }

    #[test]
    fn token_retention_authority_compacts_current_semantic_suffix_and_repeated_summary() {
        let directory = tempfile::tempdir().unwrap();
        let handle = authority(&directory);
        admitted_prompt(&handle, "old");
        admitted_prompt(&handle, "retained");
        let first =
            SessionCompaction::begin_idle(handle.clone(), &plan(&["old", "retained"], None))
                .unwrap()
                .expect("current semantic context must not require a stale prepared request");
        assert!(first.provider_payload().contains("old"));
        assert!(!first.provider_payload().contains("retained"));
        finish(&first, "first summary");
        drop(first);
        admitted_prompt(&handle, "newest");
        let second = SessionCompaction::begin_idle(
            handle.clone(),
            &plan(&["retained", "newest"], Some("first summary")),
        )
        .unwrap()
        .unwrap();
        assert!(second.provider_payload().contains("first summary"));
        assert!(second.provider_payload().contains("retained"));
        assert!(!second.provider_payload().contains("newest"));
        finish(&second, "second summary");
        drop(second);
        drop(handle);
        let reopened = authority(&directory);
        let descriptor = reopened.projection_worker_descriptor();
        let replay = crate::session_replay::SessionReplay::replay_prefix(
            &descriptor.session_snapshot,
            &descriptor.session_id,
            descriptor.stream_id,
            crate::session_replay::ReplayEnd::Event(reopened.state().last_event_id.unwrap()),
        )
        .unwrap();
        let draft = crate::session_current_context::CurrentContextDraftV1::derive(&replay).unwrap();
        assert_eq!(draft.items.len(), 2);
        assert!(
            matches!(&draft.items[0].message, crate::bridge::LlmMessage::User { content, .. } if content.contains("second summary"))
        );
        assert!(
            matches!(&draft.items[1].message, crate::bridge::LlmMessage::User { content, .. } if content == "newest")
        );
    }

    #[test]
    fn token_retention_authority_rejects_mismatched_content_without_mutation() {
        let directory = tempfile::tempdir().unwrap();
        let handle = authority(&directory);
        admitted_prompt(&handle, "old");
        admitted_prompt(&handle, "retained");
        let before = handle.state().last_sequence;
        assert!(
            SessionCompaction::begin_idle(handle.clone(), &plan(&["wrong", "retained"], None))
                .is_err()
        );
        assert_eq!(handle.state().last_sequence, before);
        assert!(handle.state().active_compaction.is_none());
        let mut nonprefix = plan(&["old", "retained"], None);
        nonprefix.source_is_prefix = false;
        let error = SessionCompaction::begin_idle(handle.clone(), &nonprefix).unwrap_err();
        assert!(error.to_string().contains("chronological source prefix"));
        assert_eq!(handle.state().last_sequence, before);
    }

    #[tokio::test]
    async fn prepared_idle_summary_retains_session_authority_without_synthetic_turn() {
        let directory = tempfile::tempdir().unwrap();
        let handle = authority(&directory);
        admitted_prompt(&handle, "old");
        admitted_prompt(&handle, "retained");
        let before = handle.state();
        let compaction =
            SessionCompaction::begin_idle(handle.clone(), &plan(&["old", "retained"], None))
                .unwrap()
                .unwrap();
        let bridge = crate::bridge::MockBridge {
            events: vec![
                crate::bridge::LlmEvent::TextDelta {
                    delta: "idle summary".into(),
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
        crate::provider_route_service::compact_loop_route(
            &bridge,
            crate::provider_route_service::LoopCompactionRequest {
                payload: "compatibility",
                options: &crate::bridge::StreamOptions::default(),
                selected_model: "anthropic:claude-sonnet-4-6",
                scope: &crate::invocation_service::InvocationScope::default(),
                step_id: Uuid::new_v4(),
                authority: Some(&compaction),
            },
        )
        .await
        .unwrap();
        let after = handle.state();
        assert_eq!(after.prompts, before.prompts);
        assert_eq!(after.turn_starts, before.turn_starts);
        assert_eq!(after.steps, before.steps);
        assert_eq!(after.route_leases, before.route_leases);
        assert!(after.active_turn.is_none());
        assert!(after.active_step.is_none());
        assert!(matches!(
            after.compaction_requests[&compaction.compaction_request_id()]
                .preparation()
                .route,
            CompactionRoute::SessionIdle { .. }
        ));
    }

    #[test]
    fn summary_prompt_carries_admitted_content_generation() {
        let (owner, generation, body) = super::summary_prompt().unwrap();
        assert_eq!(owner, "content-pack:omegon-shipped");
        assert!(generation.starts_with("content:omegon-shipped@1.0.0:"));
        assert!(body.contains("conversation summarizer"));
    }

    #[test]
    fn absent_pack_disables_compaction_prompt_locally() {
        let error = super::summary_prompt_from_pack(None).unwrap_err();
        assert_eq!(
            error.to_string(),
            "shipped compaction prompt is unavailable"
        );
    }
}
