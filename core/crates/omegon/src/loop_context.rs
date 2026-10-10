//! Compatibility implementation for the release-coupled loop context boundary.

use crate::bridge::LlmMessage;
use crate::context::{ContextManager, PromptTelemetry};
use crate::conversation::{ConversationState, ToolCall};
use crate::r#loop::LoopConfig;
use crate::util::estimate_chars_to_tokens;

#[derive(Debug, Clone)]
pub(crate) struct LoopContextWindows {
    pub(crate) provider_window: usize,
    pub(crate) assembly_window: usize,
    pub(crate) reply_reserve: usize,
}

impl LoopContextWindows {
    pub(crate) fn validate_fixed_context(
        &self,
        system_prompt: &str,
        tools: &[omegon_traits::ToolDefinition],
    ) -> anyhow::Result<()> {
        // Mandatory instructions must remain complete. Conversation compaction
        // cannot repair a request whose fixed context alone exceeds capacity.
        // Reuse the composition estimator and reserve actual schema cost here,
        // rather than charging both actual schemas and the planning reserve.
        let system_tokens = estimate_chars_to_tokens(system_prompt.len());
        let schema_tokens = estimate_tool_schema_tokens(tools);
        let required = system_tokens
            .saturating_add(schema_tokens)
            .saturating_add(self.reply_reserve);
        anyhow::ensure!(
            required <= self.provider_window,
            "Fixed context exceeds model capacity: estimated system instructions {system_tokens} + tool schemas {schema_tokens} + reply reserve {} = {required} tokens, model window {}. Reduce instruction or tool content, or select a larger-context model; required instructions were not truncated.",
            self.reply_reserve,
            self.provider_window,
        );
        Ok(())
    }
}

pub(crate) struct LoopContextAssembly {
    pub(crate) system_prompt: String,
    pub(crate) messages: Vec<LlmMessage>,
    pub(crate) composition: omegon_traits::ContextComposition,
}

pub(crate) struct LoopContextUpdate {
    pub(crate) tokens: u64,
    pub(crate) context_window: u64,
    pub(crate) context_class: String,
    pub(crate) thinking_level: String,
}

pub(crate) type LoopCompactionPlan = crate::context_compaction_service::ContextCompactionPlanV1;

pub(crate) struct LoopContextCompatibilityAdapter<'a> {
    manager: &'a mut ContextManager,
    compaction: crate::context_compaction_service::ContextCompactionBinding,
}

impl<'a> LoopContextCompatibilityAdapter<'a> {
    pub(crate) fn apply_policy(
        &mut self,
        policy: &crate::inference_policy::ResolvedPolicy,
        schema_tokens: usize,
    ) {
        self.manager
            .set_selector_policy(crate::settings::SelectorPolicy {
                model_window: policy.input_budget,
                requested_class: crate::settings::ContextClass::Massive,
                reply_reserve: 0,
                tool_schema_reserve: schema_tokens,
            });
    }
    pub(crate) fn new(
        manager: &'a mut ContextManager,
        compaction: crate::context_compaction_service::ContextCompactionBinding,
    ) -> Self {
        Self {
            manager,
            compaction,
        }
    }

    pub(crate) fn resolve_windows(&mut self, config: &LoopConfig) -> LoopContextWindows {
        if let Some(settings) = config
            .settings
            .as_ref()
            .and_then(|settings| settings.lock().ok().map(|guard| guard.clone()))
        {
            let policy = settings.selector_policy();
            let windows = LoopContextWindows {
                provider_window: settings.context_window,
                assembly_window: policy.assembly_window(),
                reply_reserve: policy.reply_reserve,
            };
            self.manager.set_selector_policy(policy);
            windows
        } else {
            self.manager.set_context_window(200_000);
            LoopContextWindows {
                provider_window: 200_000,
                assembly_window: 200_000,
                reply_reserve: 8_192,
            }
        }
    }

    pub(crate) async fn prepare_turn(
        &mut self,
        conversation: &mut ConversationState,
        runtime: &mut crate::bus::EventBus,
        turn: u32,
        tools: &[omegon_traits::ToolDefinition],
        context_window: usize,
    ) -> LoopContextAssembly {
        if conversation.intent.stats.tool_calls > 0
            || conversation.intent.current_task.is_some()
            || conversation.intent.stats.compactions > 0
            || conversation.intent.has_active_work_plan_context()
        {
            self.manager
                .inject_intent(conversation.render_intent_for_injection());
        }

        let user_prompt = conversation.last_user_prompt();
        runtime.emit(&omegon_traits::BusEvent::ContextBuild {
            user_prompt: user_prompt.to_string(),
            turn,
        });
        let (recent_tools, recent_files, budget) = self.manager.signals_data();
        let signals = omegon_traits::ContextSignals {
            user_prompt,
            recent_tools: &recent_tools,
            recent_files: &recent_files,
            lifecycle_phase: self.manager.phase(),
            turn_number: turn,
            context_budget_tokens: budget,
        };
        let injections = runtime.collect_context(&signals);
        if !injections.is_empty() {
            tracing::debug!(count = injections.len(), "bus context injections");
            self.manager.inject_external(injections);
        }

        if let Some(content) = conversation.render_attachment_context_injection() {
            self.manager
                .inject_external(vec![omegon_traits::ContextInjection {
                    source: "attachment-files".into(),
                    content,
                    priority: 190,
                    ttl_turns: 1,
                }]);
        }

        self.manager.prepare_embeddings(user_prompt).await;
        self.compose(conversation, tools, context_window)
    }

    pub(crate) fn compose(
        &mut self,
        conversation: &ConversationState,
        tools: &[omegon_traits::ToolDefinition],
        context_window: usize,
    ) -> LoopContextAssembly {
        compose_with_manager(self.manager, conversation, tools, context_window)
    }

    pub(crate) fn messages(&self, conversation: &ConversationState) -> Vec<LlmMessage> {
        conversation.build_llm_view()
    }

    pub(crate) fn record_activity(&mut self, calls: &[ToolCall]) {
        for call in calls {
            self.manager.record_tool_call(&call.name);
            if let Some(path) = call
                .arguments
                .get("path")
                .and_then(serde_json::Value::as_str)
            {
                self.manager
                    .record_file_access(std::path::PathBuf::from(path));
            }
        }
        self.manager.update_phase_from_activity(calls);
    }

    pub(crate) async fn pressure_compaction_plan(
        &self,
        snapshot: crate::context_compaction_service::ContextCompactionSnapshotV1,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<
        Option<LoopCompactionPlan>,
        omegon_traits::ManagedServiceCallError<
            crate::context_compaction_service::ContextCompactionServiceErrorV1,
        >,
    > {
        self.compaction
            .plan(
                snapshot.with_retained_token_budget(self.manager.retained_context_budget()),
                crate::context_compaction_service::ContextCompactionModeV1::Pressure,
                cancellation,
            )
            .await
    }

    pub(crate) async fn overflow_compaction_plan(
        &self,
        snapshot: crate::context_compaction_service::ContextCompactionSnapshotV1,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<
        Option<LoopCompactionPlan>,
        omegon_traits::ManagedServiceCallError<
            crate::context_compaction_service::ContextCompactionServiceErrorV1,
        >,
    > {
        self.compaction
            .plan(
                snapshot.with_retained_token_budget(self.manager.retained_context_budget()),
                crate::context_compaction_service::ContextCompactionModeV1::Overflow,
                cancellation,
            )
            .await
    }

    pub(crate) fn apply_compaction(
        &self,
        conversation: &mut ConversationState,
        plan: LoopCompactionPlan,
        summary: String,
    ) {
        plan.apply(conversation, summary);
    }

    pub(crate) fn decay_failed_compaction(
        &self,
        conversation: &mut ConversationState,
        plan: &LoopCompactionPlan,
    ) {
        conversation.decay_oldest(plan.evict_count);
    }

    pub(crate) fn repair_overflow_without_plan(
        &self,
        conversation: &mut ConversationState,
    ) -> usize {
        let evict_count = conversation.message_count() / 2;
        conversation.decay_oldest(evict_count);
        evict_count
    }

    pub(crate) fn repair_malformed_history(&self, conversation: &mut ConversationState) {
        let half = conversation.message_count() / 2;
        conversation.decay_oldest(half.max(1));
    }

    pub(crate) fn tighten_decay(&self, conversation: &mut ConversationState) {
        conversation.tighten_decay();
    }

    pub(crate) fn context_update(
        &self,
        config: &LoopConfig,
        conversation: &ConversationState,
        context_window: usize,
    ) -> LoopContextUpdate {
        LoopContextUpdate {
            tokens: conversation.estimate_tokens() as u64,
            context_window: context_window as u64,
            context_class: config
                .settings
                .as_ref()
                .and_then(|settings| {
                    settings
                        .lock()
                        .ok()
                        .map(|guard| guard.context_class.label().to_string())
                })
                .unwrap_or_else(|| {
                    crate::settings::ContextClass::from_tokens(context_window)
                        .label()
                        .to_string()
                }),
            thinking_level: config
                .settings
                .as_ref()
                .and_then(|settings| {
                    settings
                        .lock()
                        .ok()
                        .map(|guard| guard.thinking.as_str().to_string())
                })
                .unwrap_or_else(|| "off".to_string()),
        }
    }
}

pub(crate) fn compose_with_manager(
    manager: &mut ContextManager,
    conversation: &ConversationState,
    tools: &[omegon_traits::ToolDefinition],
    context_window: usize,
) -> LoopContextAssembly {
    manager.set_request_tools(tools);
    let system_prompt = manager.build_system_prompt(conversation.last_user_prompt(), conversation);
    let messages = conversation.build_llm_view();
    let telemetry = manager.last_prompt_telemetry();
    let composition =
        compute_context_composition(&system_prompt, &messages, tools, context_window, &telemetry);
    LoopContextAssembly {
        system_prompt,
        messages,
        composition,
    }
}

pub(crate) fn default_context_composition(
    context_window: usize,
) -> omegon_traits::ContextComposition {
    omegon_traits::ContextComposition {
        free_tokens: context_window,
        ..omegon_traits::ContextComposition::default()
    }
}

pub(crate) fn estimate_tool_schema_tokens(tools: &[omegon_traits::ToolDefinition]) -> usize {
    tools
        .iter()
        .map(|tool| {
            let schema_json = serde_json::to_string(&tool.parameters).unwrap_or_default();
            estimate_chars_to_tokens(tool.name.len() + tool.description.len() + schema_json.len())
        })
        .sum()
}

fn compute_context_composition(
    system_prompt: &str,
    messages: &[LlmMessage],
    tools: &[omegon_traits::ToolDefinition],
    context_window: usize,
    telemetry: &PromptTelemetry,
) -> omegon_traits::ContextComposition {
    let system_tokens = estimate_chars_to_tokens(system_prompt.len());
    let tool_schema_tokens = estimate_tool_schema_tokens(tools);
    let mut conversation_tokens = 0usize;
    let mut memory_tokens = 0usize;
    let mut tool_history_tokens = 0usize;
    let mut thinking_tokens = 0usize;

    for message in messages {
        match message {
            LlmMessage::User { content, .. } => {
                conversation_tokens += estimate_chars_to_tokens(content.len());
            }
            LlmMessage::Assistant {
                text,
                thinking,
                tool_calls,
                ..
            } => {
                conversation_tokens +=
                    estimate_chars_to_tokens(text.iter().map(String::len).sum::<usize>());
                thinking_tokens +=
                    estimate_chars_to_tokens(thinking.iter().map(String::len).sum::<usize>());
                tool_history_tokens += estimate_chars_to_tokens(
                    tool_calls
                        .iter()
                        .map(|call| call.name.len() + call.arguments.to_string().len())
                        .sum::<usize>(),
                );
            }
            LlmMessage::ToolResult {
                content,
                tool_name,
                images,
                ..
            } => {
                let image_chars = images
                    .iter()
                    .map(|image| image.data.len() + image.media_type.len())
                    .sum::<usize>();
                tool_history_tokens +=
                    estimate_chars_to_tokens(content.len() + tool_name.len() + image_chars);
                if is_declared_memory_tool(tool_name) {
                    memory_tokens += estimate_chars_to_tokens(content.len());
                }
            }
        }
    }

    // Memory is a subcategory of tool history, not an additional input copy.
    tool_history_tokens = tool_history_tokens.saturating_sub(memory_tokens);
    let used = system_tokens
        .saturating_add(conversation_tokens)
        .saturating_add(memory_tokens)
        .saturating_add(tool_schema_tokens)
        .saturating_add(tool_history_tokens)
        .saturating_add(thinking_tokens);
    omegon_traits::ContextComposition {
        conversation_tokens,
        system_tokens,
        memory_tokens,
        tool_schema_tokens,
        tool_history_tokens,
        thinking_tokens,
        free_tokens: context_window.saturating_sub(used),
        base_prompt_tokens: estimate_chars_to_tokens(telemetry.base_prompt_chars),
        session_hud_tokens: estimate_chars_to_tokens(telemetry.session_hud_chars),
        intent_tokens: estimate_chars_to_tokens(telemetry.intent_chars),
        external_injection_tokens: estimate_chars_to_tokens(telemetry.external_injection_chars),
        tool_guidance_tokens: estimate_chars_to_tokens(telemetry.tool_guidance_chars),
        file_guidance_tokens: estimate_chars_to_tokens(telemetry.file_guidance_chars),
    }
}

fn is_declared_memory_tool(name: &str) -> bool {
    use crate::tool_registry::memory;
    matches!(
        name,
        memory::MEMORY_STORE
            | memory::MEMORY_RECALL
            | memory::MEMORY_QUERY
            | memory::MEMORY_ARCHIVE
            | memory::MEMORY_SUPERSEDE
            | memory::MEMORY_CONNECT
            | memory::MEMORY_FOCUS
            | memory::MEMORY_RELEASE
            | memory::MEMORY_EPISODES
            | memory::MEMORY_COMPACT
            | memory::MEMORY_SEARCH_ARCHIVE
            | memory::MEMORY_INGEST_LIFECYCLE
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolved_policy_configured_threshold_controls_real_loop_compaction() {
        struct Counting(std::sync::atomic::AtomicUsize);
        #[async_trait::async_trait]
        impl crate::bridge::LlmBridge for Counting {
            fn serving_model_hint(&self) -> Option<&str> {
                Some("openai-codex:gpt-6-astra")
            }
            async fn stream(
                &self,
                _: &str,
                _: &[LlmMessage],
                _: &[omegon_traits::ToolDefinition],
                _: &crate::bridge::StreamOptions,
            ) -> anyhow::Result<tokio::sync::mpsc::Receiver<crate::bridge::LlmEvent>> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let (tx, rx) = tokio::sync::mpsc::channel(2);
                tx.try_send(crate::bridge::LlmEvent::TextDelta {
                    delta: "Finished.".into(),
                })?;
                tx.try_send(crate::bridge::LlmEvent::Done {
                    message: serde_json::json!({}),
                    input_tokens: 1,
                    output_tokens: 1,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                    provider_telemetry: None,
                })?;
                Ok(rx)
            }
        }
        for (threshold, requested_compaction) in [(0.8, true), (0.99, false)] {
            let bridge = Counting(std::sync::atomic::AtomicUsize::new(0));
            let mut bus = crate::bus::EventBus::new();
            bus.finalize();
            let mut manager = ContextManager::new("Complete this task.".into(), vec![]);
            let mut conversation = ConversationState::new();
            conversation.push_user("old ".repeat(17_000));
            conversation.intent.stats.turns = 1;
            conversation.push_user("Finish the task.".into());
            let settings = crate::settings::shared("openai-codex:gpt-6-astra");
            {
                let mut settings = settings.lock().unwrap();
                // Explicit fixture: W=38192, existing Medium host G_h=18192,
                // P=0 leaves a 20000-token assembly budget.
                settings.context_window = 38_192;
                settings.compaction_threshold = threshold;
            }
            let config = LoopConfig {
                model: "openai-codex:gpt-6-astra".into(),
                settings: Some(settings),
                max_turns: 1,
                max_retries: 1,
                allow_commit_nudge: false,
                compatibility: crate::loop_driver::LoopCompatibilityBindings {
                    context_compaction:
                        crate::context_compaction_service::ContextCompactionBinding::direct_for_test(
                        ),
                    ..Default::default()
                },
                ..Default::default()
            };
            let (events, mut observed) = tokio::sync::broadcast::channel(64);
            let turn = crate::loop_driver::LoopDriverTurn::new(
                &bridge,
                &mut bus,
                &mut manager,
                &mut conversation,
                &events,
                tokio_util::sync::CancellationToken::new(),
                &config,
                std::sync::Arc::new(crate::provider_route_service::ProviderRouteService),
            );
            let execution = crate::loop_driver::ReleaseCoupledLoopDriver.run(turn).await;
            let diagnostics: Vec<_> = std::iter::from_fn(|| observed.try_recv().ok())
                .filter(|event| matches!(event, omegon_traits::AgentEvent::ContextCompaction(_)))
                .collect();
            assert!(
                execution.result.is_ok(),
                "threshold {threshold}: {:?}",
                execution.result
            );
            assert_eq!(
                bridge.0.load(std::sync::atomic::Ordering::SeqCst),
                1,
                "threshold {threshold}: {diagnostics:?}"
            );
            assert_eq!(diagnostics.iter().any(|event| matches!(event,
                omegon_traits::AgentEvent::ContextCompaction(event) if event.status == omegon_traits::ContextCompactionStatus::Started)),
                requested_compaction, "threshold {threshold}: {diagnostics:?}");
            // This sessionless fixture has no authority for a turn summary.
            // The pressure request must still pass through that existing owner.
            if requested_compaction {
                assert!(diagnostics.iter().any(|event| matches!(event,
                    omegon_traits::AgentEvent::ContextCompaction(event) if event.reason.as_deref() == Some("turn compaction requires complete session authority"))));
            }
        }
    }

    #[tokio::test]
    async fn fixed_context_budget_stops_loop_before_provider_dispatch() {
        struct CountingBridge(std::sync::atomic::AtomicUsize);
        #[async_trait::async_trait]
        impl crate::bridge::LlmBridge for CountingBridge {
            fn serving_model_hint(&self) -> Option<&str> {
                Some("anthropic:claude-sonnet-4-6")
            }

            async fn stream(
                &self,
                _system: &str,
                _messages: &[LlmMessage],
                _tools: &[omegon_traits::ToolDefinition],
                _options: &crate::bridge::StreamOptions,
            ) -> anyhow::Result<tokio::sync::mpsc::Receiver<crate::bridge::LlmEvent>> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                anyhow::bail!("oversized context reached provider")
            }
        }

        let bridge = CountingBridge(std::sync::atomic::AtomicUsize::new(0));
        let mut bus = crate::bus::EventBus::new();
        bus.finalize();
        let mut manager = ContextManager::new("required policy ".repeat(100_000), vec![]);
        let mut conversation = ConversationState::new();
        conversation.push_user("Hello".into());
        let (events, _) = tokio::sync::broadcast::channel(32);
        let config = LoopConfig {
            model: "anthropic:claude-sonnet-4-6".into(),
            max_retries: 1,
            ..Default::default()
        };
        let turn = crate::loop_driver::LoopDriverTurn::new(
            &bridge,
            &mut bus,
            &mut manager,
            &mut conversation,
            &events,
            tokio_util::sync::CancellationToken::new(),
            &config,
            std::sync::Arc::new(crate::provider_route_service::ProviderRouteService),
        );
        let execution = crate::loop_driver::ReleaseCoupledLoopDriver.run(turn).await;
        let error = execution
            .result
            .expect_err("oversized fixed context must fail");
        assert!(
            error
                .to_string()
                .contains("Fixed context exceeds model capacity"),
            "{error:#}"
        );
        assert_eq!(bridge.0.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(
            execution.terminal.outcome,
            crate::runtime_turn::RuntimeTurnOutcome::Failed
        );
        assert_eq!(execution.terminal.reason_code, "loop_failed");
    }

    #[test]
    fn fixed_context_budget_rejects_complete_oversized_instructions() {
        let instructions = format!("{}\nRequired final instruction.", "policy ".repeat(10_000));
        let mut manager = ContextManager::new(instructions.clone(), vec![]);
        manager.set_context_window(16_000);
        let assembly = compose_with_manager(&mut manager, &ConversationState::new(), &[], 16_000);
        assert!(assembly.system_prompt.contains(&instructions));
        let windows = LoopContextWindows {
            provider_window: 16_000,
            assembly_window: 16_000,
            reply_reserve: 8_192,
        };
        let error = windows
            .validate_fixed_context(&assembly.system_prompt, &[])
            .expect_err("required instructions exceeding model capacity must fail before dispatch");
        assert!(
            error
                .to_string()
                .contains("Fixed context exceeds model capacity")
        );
    }

    #[test]
    fn fixed_context_budget_counts_actual_schemas_and_reply_reserve() {
        let tools = vec![omegon_traits::ToolDefinition {
            name: "large_schema".into(),
            label: "Large schema".into(),
            description: "tool description ".repeat(1_000),
            parameters: serde_json::json!({"type": "object"}),
            capabilities: vec![],
        }];
        let system_prompt = "required instruction".repeat(100);
        let required = estimate_chars_to_tokens(system_prompt.len())
            + estimate_tool_schema_tokens(&tools)
            + 8_192;
        let mut windows = LoopContextWindows {
            provider_window: required,
            // Working-set breadth does not authorize truncating fixed instructions.
            assembly_window: 1_000,
            reply_reserve: 8_192,
        };
        windows
            .validate_fixed_context(&system_prompt, &tools)
            .unwrap();
        windows.provider_window -= 1;
        assert!(
            windows
                .validate_fixed_context(&system_prompt, &tools)
                .is_err()
        );
        // Removing the schema makes the same complete instructions fit again.
        windows.validate_fixed_context(&system_prompt, &[]).unwrap();
    }

    #[test]
    fn memory_attribution_uses_declared_tools_not_name_prefixes() {
        let messages = vec![
            LlmMessage::ToolResult {
                call_id: "declared".into(),
                tool_name: crate::tool_registry::memory::MEMORY_RECALL.into(),
                content: "durable context".into(),
                images: vec![],
                is_error: false,
                args_summary: None,
            },
            LlmMessage::ToolResult {
                call_id: "lookalike".into(),
                tool_name: "memory_untrusted_plugin".into(),
                content: "not runtime memory".into(),
                images: vec![],
                is_error: false,
                args_summary: None,
            },
        ];

        let composition = compute_context_composition(
            "system",
            &messages,
            &[],
            1_000,
            &PromptTelemetry::default(),
        );

        assert_eq!(
            composition.memory_tokens,
            estimate_chars_to_tokens("durable context".len())
        );
    }

    #[test]
    fn malformed_history_repair_is_bounded_and_keeps_a_legal_view() {
        let mut manager = ContextManager::new(String::new(), vec![]);
        let adapter = LoopContextCompatibilityAdapter::new(&mut manager, Default::default());
        let mut conversation = ConversationState::new();
        conversation.push_user("oldest".into());
        conversation.push_user("middle".into());
        conversation.push_user("newest".into());
        let before = conversation.message_count();

        adapter.repair_malformed_history(&mut conversation);

        assert!(conversation.message_count() < before);
        assert!(!conversation.build_llm_view().is_empty());
    }

    #[test]
    fn overflow_repair_without_compaction_plan_decays_exactly_half_the_history() {
        let mut manager = ContextManager::new(String::new(), vec![]);
        let adapter = LoopContextCompatibilityAdapter::new(&mut manager, Default::default());
        let mut conversation = ConversationState::new();
        for message in ["oldest", "older", "recent", "newest"] {
            conversation.push_user(message.into());
        }
        let before = conversation.message_count();

        let evicted = adapter.repair_overflow_without_plan(&mut conversation);

        assert_eq!(evicted, before / 2);
        assert_eq!(conversation.message_count(), before - evicted);
        assert_eq!(conversation.last_user_prompt(), "newest");
    }

    #[tokio::test]
    async fn token_retention_loop_adapter_budgets_pressure_and_overflow() {
        let mut manager = ContextManager::new(String::new(), vec![]);
        manager.set_selector_policy(crate::settings::SelectorPolicy {
            model_window: 32_000,
            requested_class: crate::settings::ContextClass::from_tokens(200_000),
            reply_reserve: 8_192,
            tool_schema_reserve: 4_096,
        });
        let adapter = LoopContextCompatibilityAdapter::new(
            &mut manager,
            crate::context_compaction_service::ContextCompactionBinding::direct_for_test(),
        );
        for overflow in [false, true] {
            let mut conversation = ConversationState::new();
            conversation.push_user("large recent history ".repeat(8_000));
            conversation.intent.stats.turns = 1;
            conversation.push_user("current task".into());
            let snapshot = conversation.context_compaction_snapshot();
            let cancellation = tokio_util::sync::CancellationToken::new();
            let plan = if overflow {
                adapter
                    .overflow_compaction_plan(snapshot, cancellation)
                    .await
            } else {
                adapter
                    .pressure_compaction_plan(snapshot, cancellation)
                    .await
            }
            .unwrap()
            .expect("recent history must yield a budgeted plan");
            assert_eq!(plan.evict_count, 1);
            adapter.apply_compaction(&mut conversation, plan, "previous work".into());
            assert_eq!(conversation.message_count(), 1);
            assert_eq!(conversation.last_user_prompt(), "current task");
        }
    }

    #[test]
    fn selector_resolution_uses_requested_assembly_window() {
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-6");
        settings
            .lock()
            .unwrap()
            .set_requested_context_class(crate::settings::ContextClass::Compact);
        let config = LoopConfig {
            settings: Some(settings),
            ..LoopConfig::default()
        };
        let mut manager = ContextManager::new(String::new(), vec![]);
        let mut adapter = LoopContextCompatibilityAdapter::new(&mut manager, Default::default());

        let windows = adapter.resolve_windows(&config);

        assert!(windows.provider_window > windows.assembly_window);
        assert_eq!(
            windows.assembly_window,
            crate::settings::ContextClass::Compact.nominal_tokens()
        );
    }
}
