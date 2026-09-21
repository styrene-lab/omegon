//! Release-coupled lifecycle and session-finalization compatibility policy.

use crate::loop_driver::{
    LoopFinalizationRequest, LoopInternalInvocationRequest, LoopInvocationContract,
    LoopLifecycleRequest, LoopTurnAdvisory,
};

pub(crate) async fn process_turn_requests(
    invocations: &mut dyn LoopInvocationContract,
    mut request: LoopLifecycleRequest<'_>,
) -> Vec<LoopTurnAdvisory> {
    let requests = invocations.runtime().drain_requests();
    let mut advisories = Vec::new();
    for bus_request in requests {
        match bus_request {
            omegon_traits::BusRequest::Notify { message, level } => {
                advisories.push(LoopTurnAdvisory::Notify { message, level });
            }
            omegon_traits::BusRequest::InjectSystemMessage { content } => {
                advisories.push(LoopTurnAdvisory::InjectSystemMessage { content });
            }
            omegon_traits::BusRequest::EmitAgentEvent { event } => {
                advisories.push(LoopTurnAdvisory::EmitAgentEvent { event });
            }
            omegon_traits::BusRequest::RequestAggressiveDecay => {
                tracing::info!("Bus: tier 1 aggressive decay requested");
                invocations
                    .runtime()
                    .before_context_eviction(request.cancellation.clone())
                    .await;
                request.context.tighten_decay(request.conversation);
                invocations
                    .runtime()
                    .emit(&omegon_traits::BusEvent::Compacted);
            }
            omegon_traits::BusRequest::RequestCompaction => {
                compact_from_request(invocations, &mut request).await;
            }
            omegon_traits::BusRequest::RefreshHarnessStatus => {
                tracing::debug!("Bus: harness status refresh requested");
                if let Some(binding) = invocations.memory_binding() {
                    crate::status::refresh_managed_memory_status(
                        binding,
                        invocations.runtime_ref().project_root(),
                    )
                    .await;
                }
                let status =
                    crate::status::HarnessStatus::assemble(invocations.runtime().project_root());
                if let Ok(status_json) = serde_json::to_value(&status) {
                    let _ = request
                        .events
                        .send(omegon_traits::AgentEvent::HarnessStatusChanged { status_json });
                }
            }
            omegon_traits::BusRequest::AutoStoreFact {
                section,
                content,
                source,
            } => {
                let args = serde_json::json!({
                    "content": content,
                    "section": section,
                    "source": source,
                });
                let call_id = format!("turn-auto-ingest:{}", uuid::Uuid::new_v4());
                if let Err(error) = invocations
                    .dispatch_internal(LoopInternalInvocationRequest {
                        name: crate::tool_registry::memory::MEMORY_STORE,
                        call_id: &call_id,
                        args,
                        cancel: request.cancellation.clone(),
                        principal: "kernel:turn-auto-ingest",
                        authority_scope: Some(request.invocation_scope),
                    })
                    .await
                {
                    tracing::debug!(source, "auto-store fact skipped: {error}");
                }
            }
        }
    }
    advisories
}

async fn compact_from_request(
    invocations: &mut dyn LoopInvocationContract,
    request: &mut LoopLifecycleRequest<'_>,
) {
    tracing::info!("Bus: tier 2 compaction requested by feature");
    let before_tokens = request.conversation.estimate_tokens() as u64;
    let planning = request
        .context
        .pressure_compaction_plan(
            request.conversation.context_compaction_snapshot(),
            request.cancellation.clone(),
        )
        .await;
    let Some(selection) = planning.unwrap_or_else(|error| {
        tracing::warn!(%error, "feature-requested compaction planning unavailable");
        None
    }) else {
        emit_compaction(
            request.events,
            compaction_event(
                omegon_traits::ContextCompactionStatus::NoPayload,
                before_tokens,
                Some(before_tokens),
                Some(0),
                None,
                Some("auto-compaction requested but nothing was eligible to compact".into()),
            ),
        );
        tracing::debug!("auto-compaction requested but nothing was eligible to compact");
        invocations
            .runtime()
            .emit(&omegon_traits::BusEvent::Compacted);
        return;
    };

    let evict_count = selection.evict_count;
    emit_compaction(
        request.events,
        compaction_event(
            omegon_traits::ContextCompactionStatus::Started,
            before_tokens,
            None,
            Some(evict_count),
            None,
            selection.reason.clone(),
        ),
    );
    invocations
        .runtime()
        .before_context_eviction(request.cancellation.clone())
        .await;
    let compaction_authority = match request.context.begin_compaction(
        &selection,
        request.invocation_scope,
        request.route_step_id,
        crate::loop_driver::LoopCompactionTrigger::ContextPressure,
    ) {
        Ok(authority) => authority,
        Err(error) => {
            emit_compaction(
                request.events,
                compaction_event(
                    omegon_traits::ContextCompactionStatus::Failed,
                    before_tokens,
                    None,
                    Some(evict_count),
                    None,
                    Some(error.to_string()),
                ),
            );
            return;
        }
    };
    match request
        .route
        .compact(crate::loop_driver::LoopCompactionRequest {
            payload: &selection.payload,
            selected_model: &request.active_route.selected_model,
            scope: request.invocation_scope,
            step_id: request.route_step_id,
            authority: compaction_authority.as_ref(),
        })
        .await
    {
        Ok(summary) => {
            let summary_chars = summary.chars().count();
            request
                .context
                .apply_compaction(request.conversation, selection, summary);
            emit_compaction(
                request.events,
                compaction_event(
                    omegon_traits::ContextCompactionStatus::Succeeded,
                    before_tokens,
                    Some(request.conversation.estimate_tokens() as u64),
                    Some(evict_count),
                    Some(summary_chars),
                    None,
                ),
            );
        }
        Err(error) => {
            let message = error.to_string();
            emit_compaction(
                request.events,
                compaction_event(
                    omegon_traits::ContextCompactionStatus::Failed,
                    before_tokens,
                    None,
                    Some(evict_count),
                    None,
                    Some(message.clone()),
                ),
            );
            tracing::warn!(error = %message, "auto-compaction failed");
        }
    }
    invocations
        .runtime()
        .emit(&omegon_traits::BusEvent::Compacted);
}

fn compaction_event(
    status: omegon_traits::ContextCompactionStatus,
    before_tokens: u64,
    after_tokens: Option<u64>,
    evicted_messages: Option<usize>,
    summary_chars: Option<usize>,
    reason: Option<String>,
) -> omegon_traits::ContextCompactionEvent {
    omegon_traits::ContextCompactionEvent {
        trigger: omegon_traits::ContextCompactionTrigger::AutoTier2,
        status,
        before_tokens,
        after_tokens,
        evicted_messages,
        summary_chars,
        reason,
    }
}

fn emit_compaction(
    events: &tokio::sync::broadcast::Sender<omegon_traits::AgentEvent>,
    event: omegon_traits::ContextCompactionEvent,
) {
    let _ = events.send(omegon_traits::AgentEvent::ContextCompaction(event));
}

pub(crate) async fn finalize_session(
    invocations: &mut dyn LoopInvocationContract,
    request: LoopFinalizationRequest<'_>,
) {
    invocations
        .runtime()
        .emit(&omegon_traits::BusEvent::AgentEnd);
    let _ = request.events.send(omegon_traits::AgentEvent::AgentEnd);

    invocations
        .runtime()
        .emit(&omegon_traits::BusEvent::SessionEnd {
            turns: request.turns,
            tool_calls: request.tool_calls,
            duration_secs: request.elapsed.as_secs_f64(),
            initial_prompt: request.initial_prompt,
            outcome_summary: request.outcome_summary,
        });

    if !invocations.drain_late_requests() {
        return;
    }
    drain_late_requests(invocations, request.events, request.cancellation).await;
}

async fn drain_late_requests(
    invocations: &mut dyn LoopInvocationContract,
    events: &tokio::sync::broadcast::Sender<omegon_traits::AgentEvent>,
    cancellation: tokio_util::sync::CancellationToken,
) {
    let requests = invocations.runtime().drain_requests();
    for request in requests {
        match request {
            omegon_traits::BusRequest::Notify { message, level } => {
                tracing::info!(level = ?level, "Bus notification: {message}");
            }
            omegon_traits::BusRequest::InjectSystemMessage { content } => {
                tracing::debug!("post-loop InjectSystemMessage ignored (loop complete): {content}");
            }
            omegon_traits::BusRequest::RequestCompaction
            | omegon_traits::BusRequest::RequestAggressiveDecay => {
                tracing::info!("Bus requested compaction (post-loop - ignored)");
            }
            omegon_traits::BusRequest::RefreshHarnessStatus => {}
            omegon_traits::BusRequest::AutoStoreFact {
                section,
                content,
                source,
            } => {
                let args = serde_json::json!({
                    "content": content,
                    "section": section,
                    "source": source,
                });
                let call_id = format!("post-loop-auto-ingest:{}", uuid::Uuid::new_v4());
                match tokio::time::timeout(
                    post_loop_store_timeout(),
                    invocations.dispatch_internal(LoopInternalInvocationRequest {
                        name: crate::tool_registry::memory::MEMORY_STORE,
                        call_id: &call_id,
                        args,
                        cancel: cancellation.clone(),
                        principal: "kernel:post-loop-auto-ingest",
                        authority_scope: None,
                    }),
                )
                .await
                {
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => {
                        tracing::debug!(source, "post-loop auto-store fact skipped: {error}")
                    }
                    Err(_) => tracing::warn!(
                        source,
                        "post-loop auto-store fact timed out; continuing turn completion"
                    ),
                }
            }
            omegon_traits::BusRequest::EmitAgentEvent { event } => {
                let _ = events.send(*event);
            }
        }
    }
}

fn post_loop_store_timeout() -> std::time::Duration {
    #[cfg(test)]
    return std::time::Duration::from_millis(25);
    #[cfg(not(test))]
    std::time::Duration::from_secs(5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loop_driver::{LoopFinalizationRequest, LoopInvocationPort};
    use omegon_traits::{BusEvent, BusRequest, Feature, ToolDefinition, ToolProvider, ToolResult};

    #[derive(Clone)]
    struct FinalizationFeature {
        order: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
        late_requests: bool,
    }

    #[async_trait::async_trait]
    impl Feature for FinalizationFeature {
        fn name(&self) -> &str {
            "finalization-probe"
        }

        fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
            match event {
                BusEvent::AgentEnd => {
                    self.order.lock().unwrap().push("agent-end");
                    Vec::new()
                }
                BusEvent::SessionEnd { .. } => {
                    self.order.lock().unwrap().push("session-end");
                    if self.late_requests {
                        vec![
                            BusRequest::RequestCompaction,
                            BusRequest::RequestAggressiveDecay,
                            BusRequest::RefreshHarnessStatus,
                            BusRequest::AutoStoreFact {
                                section: "decisions".into(),
                                content: "bounded finalization".into(),
                                source: "test".into(),
                            },
                            BusRequest::EmitAgentEvent {
                                event: Box::new(omegon_traits::AgentEvent::SystemNotification {
                                    message: "late event".into(),
                                }),
                            },
                        ]
                    } else {
                        Vec::new()
                    }
                }
                _ => Vec::new(),
            }
        }
    }

    struct MemoryStoreProbe {
        order: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
        delay: Option<std::time::Duration>,
    }

    #[async_trait::async_trait]
    impl ToolProvider for MemoryStoreProbe {
        fn tools(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_STORE.into(),
                label: "memory store".into(),
                description: "test memory store".into(),
                parameters: serde_json::json!({"type": "object"}),
                capabilities: Vec::new(),
            }]
        }

        async fn execute(
            &self,
            _tool_name: &str,
            _call_id: &str,
            _args: serde_json::Value,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> anyhow::Result<ToolResult> {
            if let Some(delay) = self.delay {
                tokio::time::sleep(delay).await;
            }
            self.order.lock().unwrap().push("memory-store");
            Ok(ToolResult {
                content: Vec::new(),
                details: serde_json::json!({"status": "ok"}),
            })
        }
    }

    fn finalization_request(
        events: &tokio::sync::broadcast::Sender<omegon_traits::AgentEvent>,
        _drain_late_requests: bool,
    ) -> LoopFinalizationRequest<'_> {
        LoopFinalizationRequest {
            events,
            cancellation: tokio_util::sync::CancellationToken::new(),
            turns: 3,
            tool_calls: 4,
            elapsed: std::time::Duration::from_secs(2),
            initial_prompt: Some("prompt".into()),
            outcome_summary: Some("outcome".into()),
        }
    }

    fn finalization_bus(
        order: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
        delay: Option<std::time::Duration>,
    ) -> crate::bus::EventBus {
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(FinalizationFeature {
            order: order.clone(),
            late_requests: true,
        }));
        bus.register(Box::new(crate::features::adapter::ToolAdapter::new(
            "memory-store-probe",
            Box::new(MemoryStoreProbe { order, delay }),
        )));
        bus.register_internal_tool(
            crate::tool_registry::memory::MEMORY_STORE,
            "memory-store-probe",
        );
        bus.finalize();
        bus
    }

    #[tokio::test]
    async fn finalization_orders_agent_end_session_end_then_late_memory_dispatch() {
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut bus = finalization_bus(order.clone(), None);
        let (events, mut receiver) = tokio::sync::broadcast::channel(8);
        {
            let mut invocations = LoopInvocationPort::new(&mut bus);
            invocations.set_drain_late_requests(true);
            finalize_session(&mut invocations, finalization_request(&events, true)).await;
        }

        assert_eq!(
            order.lock().unwrap().as_slice(),
            ["agent-end", "session-end", "memory-store"]
        );
        assert!(matches!(
            receiver.try_recv().unwrap(),
            omegon_traits::AgentEvent::AgentEnd
        ));
        assert!(matches!(
            receiver.try_recv().unwrap(),
            omegon_traits::AgentEvent::SystemNotification { message } if message == "late event"
        ));
        assert!(bus.drain_requests().is_empty());
    }

    #[tokio::test]
    async fn interactive_no_drain_returns_without_running_late_memory_work() {
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut bus = finalization_bus(order.clone(), Some(std::time::Duration::from_secs(1)));
        let (events, _receiver) = tokio::sync::broadcast::channel(8);
        let started = std::time::Instant::now();
        {
            let mut invocations = LoopInvocationPort::new(&mut bus);
            invocations.set_drain_late_requests(false);
            finalize_session(&mut invocations, finalization_request(&events, false)).await;
        }

        assert!(started.elapsed() < std::time::Duration::from_millis(100));
        assert_eq!(
            order.lock().unwrap().as_slice(),
            ["agent-end", "session-end"]
        );
        assert_eq!(bus.drain_requests().len(), 5);
    }

    #[tokio::test]
    async fn late_memory_dispatch_timeout_bounds_session_release() {
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut bus = finalization_bus(order.clone(), Some(std::time::Duration::from_secs(1)));
        let (events, _receiver) = tokio::sync::broadcast::channel(8);
        let started = std::time::Instant::now();
        {
            let mut invocations = LoopInvocationPort::new(&mut bus);
            invocations.set_drain_late_requests(true);
            finalize_session(&mut invocations, finalization_request(&events, true)).await;
        }

        assert!(started.elapsed() < std::time::Duration::from_millis(250));
        assert_eq!(
            order.lock().unwrap().as_slice(),
            ["agent-end", "session-end"]
        );
        assert!(bus.drain_requests().is_empty());
    }

    #[test]
    fn production_loop_cannot_regain_lifecycle_finalization_policy() {
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
            "parse_ambient_blocks",
            "AmbientCapture",
            "tool_registry::memory",
            "BusEvent::SessionEnd",
            "BusRequest::",
            ".drain_requests()",
            "post-loop-auto-ingest",
        ] {
            assert!(
                !production.contains(forbidden),
                "production loop.rs regained lifecycle policy marker {forbidden:?}"
            );
        }
    }

    #[test]
    fn production_loop_keeps_compaction_authority_behind_neutral_contracts() {
        let source = include_str!("loop.rs");
        let production = source
            .split_once("#[cfg(test)]\nmod legacy_route_policy_tests")
            .map_or(source, |(production, _)| production);
        for forbidden in [
            "session_authority::Compaction",
            "session_compaction::",
            "SessionCompaction",
        ] {
            assert!(
                !production.contains(forbidden),
                "production loop.rs owns concrete compaction authority marker {forbidden:?}"
            );
        }
    }
}
