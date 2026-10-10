//! ContextManager — dynamic per-turn system prompt injection.
//!
//! Starts with a minimal base prompt (~500 tokens) and injects
//! context based on deterministic signals: recent tools, file types,
//! lifecycle phase, memory facts, explicit declarations.
//!
//! Includes built-in providers:
//! - SessionHud: ambient awareness of session state (turn, budget, files, duration)

use omegon_traits::{ContextInjection, ContextProvider, ContextSignals, LifecyclePhase};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

use crate::conversation::ConversationState;
use crate::shadow_context::{ContextKind, EntryBody, ShadowContext, ShadowEntry};

/// Manages dynamic system prompt assembly.
pub struct ContextManager {
    base_prompt: String,
    providers: Vec<Box<dyn ContextProvider>>,
    active_injections: Vec<ActiveInjection>,
    recent_tools: VecDeque<String>,
    recent_files: VecDeque<PathBuf>,
    phase: LifecyclePhase,
    session_start: Instant,
    /// Context window size in tokens for budget calculations.
    context_window: usize,
    shadow: ShadowContext,
    last_prompt_telemetry: PromptTelemetry,
    /// Optional embedding service for semantic context relevance.
    embed_service: Option<std::sync::Arc<dyn omegon_memory::EmbeddingService>>,
    /// Cached query embedding from the last prepare_embeddings() call.
    query_embedding: Option<Vec<f32>>,
    subagent_policy: crate::autonomy::SubagentPolicy,
    subagent_settings: Option<crate::settings::SharedSettings>,
}

#[derive(Debug, Clone, Default)]
pub struct PromptTelemetry {
    pub base_prompt_chars: usize,
    pub session_hud_chars: usize,
    pub intent_chars: usize,
    pub external_injection_chars: usize,
    pub tool_guidance_chars: usize,
    pub file_guidance_chars: usize,
}

struct ActiveInjection {
    injection: ContextInjection,
    remaining_turns: u32,
}

impl ContextManager {
    pub fn new(base_prompt: String, providers: Vec<Box<dyn ContextProvider>>) -> Self {
        Self {
            base_prompt,
            providers,
            active_injections: Vec::new(),
            recent_tools: VecDeque::with_capacity(10),
            recent_files: VecDeque::with_capacity(20),
            phase: LifecyclePhase::default(),
            session_start: Instant::now(),
            context_window: 200_000, // Default for Anthropic models
            shadow: ShadowContext::new(crate::settings::SelectorPolicy {
                model_window: 200_000,
                requested_class: crate::settings::ContextClass::Standard,
                reply_reserve: 8_192,
                tool_schema_reserve: 4_096,
            }),
            last_prompt_telemetry: PromptTelemetry::default(),
            embed_service: None,
            query_embedding: None,
            subagent_policy: crate::autonomy::active_subagent_policy(),
            subagent_settings: None,
        }
    }

    /// Set the embedding service for semantic context relevance scoring.
    pub fn set_embed_service(
        &mut self,
        service: std::sync::Arc<dyn omegon_memory::EmbeddingService>,
    ) {
        self.embed_service = Some(service);
    }

    /// Pre-compute embeddings for the query and any entries that need them.
    /// Call this before `build_system_prompt()` to enable semantic scoring.
    /// Async because embedding requires a network call to Ollama.
    pub async fn prepare_embeddings(&mut self, user_prompt: &str) {
        let Some(ref service) = self.embed_service else {
            return;
        };

        // Clear stale query embedding before computing new one —
        // if embed fails, we don't want the old one lingering.
        self.query_embedding = None;

        // Compute query embedding
        match service.embed(user_prompt).await {
            Ok(vec) => self.query_embedding = Some(vec),
            Err(e) => {
                tracing::debug!("Embedding query failed (falling back to substring): {e}");
                self.query_embedding = None;
                return;
            }
        }

        // Compute embeddings for entries that don't have them yet
        let needed = self.shadow.entries_needing_embeddings();
        if needed.is_empty() {
            return;
        }
        // Batch: embed up to 20 entries per turn to avoid blocking
        let batch: Vec<_> = needed.into_iter().take(20).collect();
        let mut computed = Vec::new();
        for (id, text) in &batch {
            match service.embed(text).await {
                Ok(vec) => computed.push((id.clone(), vec)),
                Err(e) => {
                    tracing::debug!("Embedding entry {id} failed: {e}");
                }
            }
        }
        if !computed.is_empty() {
            tracing::debug!(
                count = computed.len(),
                "Computed embeddings for shadow entries"
            );
            self.shadow.set_embeddings(&computed);
        }
    }

    /// Set the context window size (in tokens) for budget calculations.
    pub fn set_context_window(&mut self, tokens: usize) {
        self.context_window = tokens;
        let mut policy = self.shadow.selector_policy();
        policy.model_window = tokens;
        self.shadow.set_selector_policy(policy);
    }

    pub(crate) fn set_subagent_policy(&mut self, policy: crate::autonomy::SubagentPolicy) {
        self.subagent_policy = policy;
    }

    pub(crate) fn bind_subagent_settings(
        &mut self,
        settings: Option<crate::settings::SharedSettings>,
    ) {
        self.subagent_settings = settings;
    }

    /// Replace request-local capability facts from the exact exposed schema set.
    /// Mandatory input participates in selection, accounting and capture. This
    /// snapshot is replaced on every composition, not aged as advisory context.
    pub(crate) fn set_request_tools(&mut self, tools: &[omegon_traits::ToolDefinition]) {
        let policy = self
            .subagent_settings
            .as_ref()
            .and_then(|settings| {
                settings.lock().ok().map(|settings| {
                    crate::autonomy::subagent_policy_for_automation(settings.automation_level)
                })
            })
            .unwrap_or_else(|| self.subagent_policy.clone());
        self.inject_external(vec![ContextInjection {
            source: "request-tool-surface".into(),
            content: crate::prompt::request_tool_context(tools, &policy),
            priority: 200,
            ttl_turns: u32::MAX,
        }]);
    }

    /// Update the full selector policy for turn assembly.
    pub fn set_selector_policy(&mut self, policy: crate::settings::SelectorPolicy) {
        self.context_window = policy.model_window;
        self.shadow.set_selector_policy(policy);
    }

    /// Target for verbatim history after compaction. This is planning headroom,
    /// not an exact tokenizer or a hard bound on the generated summary.
    pub(crate) fn retained_context_budget(&self) -> usize {
        let policy = self.shadow.selector_policy();
        let known_chars = self
            .active_injections
            .iter()
            .fold(self.base_prompt.len(), |total, active| {
                total.saturating_add(active.injection.content.len())
            });
        let telemetry = &self.last_prompt_telemetry;
        let observed_chars = [
            telemetry.base_prompt_chars,
            telemetry.session_hud_chars,
            telemetry.intent_chars,
            telemetry.external_injection_chars,
            telemetry.tool_guidance_chars,
            telemetry.file_guidance_chars,
        ]
        .into_iter()
        .fold(0usize, usize::saturating_add);
        // Planning precedes this turn's prompt assembly. Reserve at least the
        // normal system share, plus any larger known or last-observed prompt.
        let system_reserve = (policy.assembly_window() / 5).max(
            crate::util::estimate_chars_to_tokens(known_chars.max(observed_chars)),
        );
        policy
            .assembly_budget()
            .saturating_sub(system_reserve)
            .saturating_sub(policy.reply_reserve)
    }

    /// Context budget in tokens available for injections this turn.
    /// Reserve ~80% of the context window for conversation, 20% for system prompt.
    /// System prompt budget = context_window * 0.2 minus the base prompt size.
    pub fn context_budget(&self) -> usize {
        (self.context_window / 5).saturating_sub(self.base_prompt.len() / 4)
    }

    /// Build the system prompt for this turn.
    /// Called once per LLM request, runs in <1ms.
    pub fn build_system_prompt(
        &mut self,
        user_prompt: &str,
        conversation: &ConversationState,
    ) -> String {
        let recent_tools_vec: Vec<String> = self.recent_tools.iter().cloned().collect();
        let recent_files_vec: Vec<PathBuf> = self.recent_files.iter().cloned().collect();

        let system_budget = self.context_budget();

        let signals = ContextSignals {
            user_prompt,
            recent_tools: &recent_tools_vec,
            recent_files: &recent_files_vec,
            lifecycle_phase: &self.phase,
            turn_number: conversation.turn_count(),
            context_budget_tokens: system_budget,
        };

        // Static providers and runtime features share replacement-by-source
        // semantics, including finite TTLs and explicit empty replacements.
        let injections = self
            .providers
            .iter()
            .filter_map(|provider| provider.provide_context(&signals))
            .collect();
        self.inject_external(injections);

        // Inject session HUD (high priority, always present, refreshed each turn)
        let hud = self.build_session_hud(conversation);
        // Remove previous HUD injection (it's re-built each turn)
        self.active_injections
            .retain(|a| a.injection.source != "session-hud");
        self.active_injections.push(ActiveInjection {
            remaining_turns: 1,
            injection: ContextInjection {
                source: "session-hud".into(),
                content: hud,
                priority: 200, // High — but after base prompt
                ttl_turns: 1,
            },
        });

        let prompt = self.assemble(user_prompt, conversation);
        // TTL counts completed prompt assemblies. Decaying before assembly made
        // one-turn snapshots (intent, active plan, attachments) expire without
        // ever becoming visible to the provider.
        self.decay_expired();
        prompt
    }

    /// Build the session HUD line.
    fn build_session_hud(&self, conversation: &ConversationState) -> String {
        let intent = &conversation.intent;
        let elapsed = self.session_start.elapsed();
        let elapsed_str = if elapsed.as_secs() >= 3600 {
            format!(
                "{}h{}m",
                elapsed.as_secs() / 3600,
                (elapsed.as_secs() % 3600) / 60
            )
        } else if elapsed.as_secs() >= 60 {
            format!("{}m{}s", elapsed.as_secs() / 60, elapsed.as_secs() % 60)
        } else {
            format!("{}s", elapsed.as_secs())
        };

        let files_read = intent.files_read.len();
        let files_modified = intent.files_modified.len();

        format!(
            "[Session: turn {} | {} tool calls | {} files read, {} modified | {}]",
            intent.stats.turns, intent.stats.tool_calls, files_read, files_modified, elapsed_str,
        )
    }

    /// Record a tool call for signal tracking.
    pub fn record_tool_call(&mut self, tool_name: &str) {
        self.recent_tools.push_back(tool_name.to_string());
        if self.recent_tools.len() > 10 {
            self.recent_tools.pop_front();
        }
    }

    /// Record a file access for signal tracking.
    pub fn record_file_access(&mut self, path: PathBuf) {
        // Deduplicate consecutive accesses to the same file
        if self.recent_files.back() != Some(&path) {
            self.recent_files.push_back(path);
            if self.recent_files.len() > 20 {
                self.recent_files.pop_front();
            }
        }
    }

    /// Update lifecycle phase based on tool activity.
    pub fn update_phase_from_activity(&mut self, tool_calls: &[crate::conversation::ToolCall]) {
        for call in tool_calls {
            match call.name.as_str() {
                "write" | "edit" if !matches!(self.phase, LifecyclePhase::Implementing { .. }) => {
                    self.phase = LifecyclePhase::Implementing { change_id: None };
                }
                "understand" | "read" => {
                    if matches!(self.phase, LifecyclePhase::Idle) {
                        self.phase = LifecyclePhase::Exploring { node_id: None };
                    }
                }
                _ => {}
            }
        }
    }

    fn decay_expired(&mut self) {
        self.active_injections.retain_mut(|a| {
            a.remaining_turns = a.remaining_turns.saturating_sub(1);
            if a.remaining_turns == 0 {
                self.shadow
                    .remove_by_source_prefix(&format!("inj:{}", a.injection.source));
                false
            } else {
                true
            }
        });
    }

    /// Inject the IntentDocument as a high-priority context block.
    /// Called externally when intent has meaningful content.
    /// Build context signals data for external consumers (e.g. EventBus).
    /// Returns the components needed to construct a `ContextSignals` struct.
    pub fn signals_data(&self) -> (Vec<String>, Vec<PathBuf>, usize) {
        let recent_tools_vec: Vec<String> = self.recent_tools.iter().cloned().collect();
        let recent_files_vec: Vec<PathBuf> = self.recent_files.iter().cloned().collect();
        (recent_tools_vec, recent_files_vec, self.context_budget())
    }

    /// The current lifecycle phase.
    pub fn phase(&self) -> &LifecyclePhase {
        &self.phase
    }

    /// Inject context from external sources (e.g. EventBus features).
    /// Called by the loop after collecting context from bus.collect_context().
    pub fn inject_external(&mut self, injections: Vec<ContextInjection>) {
        for injection in injections {
            // Deduplicate by source — replace existing injection from same source
            self.active_injections
                .retain(|a| a.injection.source != injection.source);
            self.active_injections.push(ActiveInjection {
                remaining_turns: injection.ttl_turns,
                injection,
            });
        }
    }

    pub fn inject_intent(&mut self, intent_block: String) {
        // Remove previous intent injection
        self.active_injections
            .retain(|a| a.injection.source != "intent-document");
        if !intent_block.is_empty() {
            self.active_injections.push(ActiveInjection {
                remaining_turns: 1, // Refreshed each turn
                injection: ContextInjection {
                    source: "intent-document".into(),
                    content: intent_block,
                    priority: 190, // High — after base, before other context
                    ttl_turns: 1,
                },
            });
        }
    }

    fn assemble(&mut self, user_prompt: &str, conversation: &ConversationState) -> String {
        self.shadow.remove_by_source_prefix("base-prompt");
        let mut base = ShadowEntry::new(
            "base-prompt",
            ContextKind::BaseSystemPrompt,
            EntryBody::Inline(self.base_prompt.clone()),
        );
        base.mandatory = true;
        self.shadow.upsert(base);

        let mut telemetry = PromptTelemetry {
            base_prompt_chars: self.base_prompt.len(),
            ..PromptTelemetry::default()
        };

        for active in &self.active_injections {
            let kind = match active.injection.source.as_str() {
                "session-hud" => {
                    telemetry.session_hud_chars += active.injection.content.len();
                    ContextKind::SessionHud
                }
                "intent-document" => {
                    telemetry.intent_chars += active.injection.content.len();
                    ContextKind::IntentDocument
                }
                source if source.starts_with("tool-group:") => {
                    telemetry.tool_guidance_chars += active.injection.content.len();
                    ContextKind::TaskArtifact
                }
                source if source.starts_with("file-type:") => {
                    telemetry.file_guidance_chars += active.injection.content.len();
                    ContextKind::TaskArtifact
                }
                _ => {
                    telemetry.external_injection_chars += active.injection.content.len();
                    ContextKind::TaskArtifact
                }
            };
            let mut entry = ShadowEntry::new(
                format!("inj:{}", active.injection.source),
                kind,
                EntryBody::Inline(active.injection.content.clone()),
            );
            entry.priority = active.injection.priority as i32;
            entry.ttl_turns = Some(active.remaining_turns);
            entry.mandatory = active.injection.priority >= 190;
            self.shadow.upsert(entry);
        }

        let selected = {
            let budget = self.shadow.selector_policy().assembly_budget();
            self.shadow.select_for_turn_with_budget(
                conversation.turn_count(),
                user_prompt,
                budget,
                self.query_embedding.as_deref(),
            )
        };
        self.last_prompt_telemetry = telemetry;
        self.shadow.render_selection(&selected)
    }

    pub fn last_prompt_telemetry(&self) -> PromptTelemetry {
        self.last_prompt_telemetry.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_retention_budget_reserves_system_and_summary_under_requested_class() {
        let mut manager = ContextManager::new("policy".into(), vec![]);
        let policy = crate::settings::SelectorPolicy {
            model_window: 1_000_000,
            requested_class: crate::settings::ContextClass::from_tokens(200_000),
            reply_reserve: 8_192,
            tool_schema_reserve: 4_096,
        };
        let effective = policy.assembly_window();
        let expected = policy.assembly_budget() - effective / 5 - policy.reply_reserve;
        manager.set_selector_policy(policy);
        assert_eq!(manager.retained_context_budget(), expected);
        assert!(manager.retained_context_budget() < effective);
    }

    #[test]
    fn token_retention_budget_charges_large_known_injections_and_saturates() {
        let mut manager = ContextManager::new("p".repeat(40_000), vec![]);
        manager.set_selector_policy(crate::settings::SelectorPolicy {
            model_window: 32_000,
            requested_class: crate::settings::ContextClass::from_tokens(200_000),
            reply_reserve: 8_192,
            tool_schema_reserve: 4_096,
        });
        manager.inject_external(vec![omegon_traits::ContextInjection {
            source: "project-policy".into(),
            content: "x".repeat(80_000),
            priority: 200,
            ttl_turns: 50,
        }]);
        assert_eq!(manager.retained_context_budget(), 0);
    }

    /// A provider that injects a distinct persistent payload every turn,
    /// mirroring `PersonaFeature` under progressive disclosure.
    struct VaryingPersistentProvider {
        source: &'static str,
        calls: std::sync::atomic::AtomicUsize,
    }

    struct FiniteMemoryProvider(std::sync::atomic::AtomicUsize);
    impl omegon_traits::ContextProvider for FiniteMemoryProvider {
        fn provide_context(&self, _signals: &ContextSignals<'_>) -> Option<ContextInjection> {
            let turn = self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if turn >= 3 {
                return None;
            }
            Some(ContextInjection {
                source: "memory".into(),
                content: match turn {
                    0 => "obsolete-memory-marker",
                    1 => "replacement-memory-marker",
                    _ => "",
                }
                .into(),
                priority: 200,
                ttl_turns: if turn == 2 { 1 } else { 3 },
            })
        }
    }

    #[test]
    fn finite_memory_selection_replacements_cannot_accumulate_or_reappear() {
        let mut manager = ContextManager::new(
            String::new(),
            vec![Box::new(FiniteMemoryProvider(
                std::sync::atomic::AtomicUsize::new(0),
            ))],
        );
        let conversation = ConversationState::new();
        for _ in 0..3 {
            manager.build_system_prompt("memory", &conversation);
            assert!(
                manager
                    .active_injections
                    .iter()
                    .filter(|active| active.injection.source == "memory")
                    .count()
                    <= 1
            );
        }
        let prompt = manager.build_system_prompt("memory", &conversation);
        assert!(!prompt.contains("obsolete-memory-marker"));
        assert!(!prompt.contains("replacement-memory-marker"));
    }

    impl omegon_traits::ContextProvider for VaryingPersistentProvider {
        fn provide_context(
            &self,
            _signals: &ContextSignals<'_>,
        ) -> Option<omegon_traits::ContextInjection> {
            let n = self
                .calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1;
            Some(omegon_traits::ContextInjection {
                content: format!("payload variant {n}"),
                source: self.source.to_string(),
                priority: 85,
                ttl_turns: u32::MAX,
            })
        }
    }

    #[test]
    fn persistent_provider_injections_replace_rather_than_accumulate() {
        let mut mgr = ContextManager::new(
            String::new(),
            vec![Box::new(VaryingPersistentProvider {
                source: "persona",
                calls: std::sync::atomic::AtomicUsize::new(0),
            })],
        );
        let convo = ConversationState::new();

        for _ in 0..5 {
            let _ = mgr.build_system_prompt("do the thing", &convo);
        }

        let persona: Vec<_> = mgr
            .active_injections
            .iter()
            .filter(|a| a.injection.source == "persona")
            .collect();
        assert_eq!(
            persona.len(),
            1,
            "5 turns produced {} persona injections",
            persona.len()
        );
        assert_eq!(
            persona[0].injection.content, "payload variant 5",
            "the surviving injection must be the newest, not the first"
        );

        let prompt = mgr.build_system_prompt("do the thing", &convo);
        assert!(!prompt.contains("payload variant 1"));
        assert!(prompt.contains("payload variant 6"));
    }

    #[test]
    fn session_hud_format() {
        let cm = ContextManager::new("base".into(), vec![]);
        let conv = ConversationState::new();
        let hud = cm.build_session_hud(&conv);
        assert!(hud.starts_with("[Session:"));
        assert!(hud.contains("turn 0"));
        assert!(hud.contains("0 tool calls"));
        assert!(hud.ends_with(']'));
    }

    #[test]
    fn context_manager_includes_hud() {
        let mut cm = ContextManager::new("You are an assistant.".into(), vec![]);
        let conv = ConversationState::new();
        let prompt = cm.build_system_prompt("hello", &conv);
        assert!(prompt.contains("You are an assistant."));
        assert!(prompt.contains("[Session:"));
    }

    #[test]
    fn external_attachment_injection_is_hidden_in_system_prompt() {
        let mut cm = ContextManager::new("You are an assistant.".into(), vec![]);
        cm.inject_external(vec![omegon_traits::ContextInjection {
            source: "attachment-files".into(),
            content: "[Attachment files]\n- [image0] /tmp/demo.png".into(),
            priority: 190,
            ttl_turns: 2,
        }]);
        let conv = ConversationState::new();
        let prompt = cm.build_system_prompt("show me the image again", &conv);
        assert!(prompt.contains("[Attachment files]"));
        assert!(prompt.contains("/tmp/demo.png"));
    }

    #[test]
    fn active_plan_intent_is_visible_in_the_current_provider_prompt() {
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .set_work_plan(vec!["Inspect".into(), "Patch".into()]);
        conversation.intent.execute_work_plan();

        let mut cm = ContextManager::new("You are an assistant.".into(), vec![]);
        cm.inject_intent(conversation.render_intent_for_injection());
        let prompt = cm.build_system_prompt("continue", &conversation);

        assert!(prompt.contains("[Intent — session state]"));
        assert!(prompt.contains("Plan (0/2):"));
        assert!(prompt.contains("Plan mode: executing"));
        assert!(prompt.contains("Plan execution contract:"));
        assert!(prompt.contains("◐ Inspect"));
        assert!(prompt.contains("○ Patch"));
    }

    #[test]
    fn one_turn_external_injection_participates_in_current_prompt() {
        let mut cm = ContextManager::new("You are an assistant.".into(), vec![]);
        cm.inject_external(vec![omegon_traits::ContextInjection {
            source: "current-turn-state".into(),
            content: "[Current authoritative state]".into(),
            priority: 190,
            ttl_turns: 1,
        }]);
        let conv = ConversationState::new();

        let current = cm.build_system_prompt("continue", &conv);
        assert!(current.contains("[Current authoritative state]"));

        let next = cm.build_system_prompt("continue", &conv);
        assert!(!next.contains("[Current authoritative state]"));
    }

    #[test]
    fn recent_files_dedup_consecutive() {
        let mut cm = ContextManager::new("base".into(), vec![]);
        cm.record_file_access(PathBuf::from("foo.rs"));
        cm.record_file_access(PathBuf::from("foo.rs"));
        cm.record_file_access(PathBuf::from("bar.rs"));
        cm.record_file_access(PathBuf::from("foo.rs"));
        assert_eq!(cm.recent_files.len(), 3); // foo, bar, foo (not 4)
    }

    #[test]
    fn tool_use_does_not_adopt_workflows_or_inject_procedure_bundles() {
        let mut cm = ContextManager::new("base".into(), vec![]);
        let conv = ConversationState::new();
        let prompt = cm.build_system_prompt("test", &conv);
        assert!(
            !prompt.contains("Memory guidelines"),
            "should not inject before tool use"
        );

        for tool in [
            "memory_store",
            "design_tree",
            "openspec_manage",
            "cleave_run",
            "ask_local_model",
        ] {
            cm.record_tool_call(tool);
        }
        let prompt = cm.build_system_prompt("test", &conv);
        assert!(!prompt.contains("guidelines:"));
        assert!(prompt.contains("[Session:"));
        assert!(
            cm.active_injections
                .iter()
                .all(|active| !active.injection.source.starts_with("tool-group:"))
        );
    }

    #[test]
    fn repeated_tool_use_keeps_signals_without_recreating_removed_context() {
        let mut cm = ContextManager::new("base".into(), vec![]);
        let conv = ConversationState::new();

        cm.record_tool_call("memory_store");
        let _ = cm.build_system_prompt("test", &conv);
        cm.record_tool_call("memory_recall");
        let _ = cm.build_system_prompt("test", &conv);
        cm.record_tool_call("memory_query");
        let prompt = cm.build_system_prompt("test", &conv);

        assert_eq!(cm.recent_tools.len(), 3);
        assert_eq!(
            prompt.matches("Memory guidelines").count(),
            0,
            "tool contracts own guidance, not activity-triggered bundles"
        );
        assert_eq!(cm.last_prompt_telemetry().tool_guidance_chars, 0);
    }

    #[test]
    fn explicitly_admitted_workflow_guidance_retains_ttl_and_telemetry() {
        let mut cm = ContextManager::new("base".into(), vec![]);
        let mut conv = ConversationState::new();

        cm.inject_external(vec![ContextInjection {
            source: "tool-group:adopted-workflow".into(),
            content: "Adopted workflow marker".into(),
            priority: 190,
            ttl_turns: 2,
        }]);
        let prompt = cm.build_system_prompt("test", &conv);
        assert!(prompt.contains("Adopted workflow marker"));
        assert_eq!(
            cm.last_prompt_telemetry().tool_guidance_chars,
            "Adopted workflow marker".len()
        );

        for i in 1..=3 {
            conv.intent.stats.turns = i;
            let _ = cm.build_system_prompt("test", &conv);
        }

        let prompt = cm.build_system_prompt("test", &conv);
        assert!(
            !prompt.contains("Adopted workflow marker"),
            "should expire after TTL"
        );
    }

    #[test]
    fn file_extensions_do_not_override_project_language_and_test_policy() {
        let mut cm = ContextManager::new(
            "Project: use the repository's custom test runner.".into(),
            vec![],
        );
        let conv = ConversationState::new();

        for file in [
            "main.rs",
            "main.ts",
            "view.tsx",
            "main.py",
            "main.go",
            "Cargo.toml",
        ] {
            cm.record_file_access(PathBuf::from(file));
        }
        let prompt = cm.build_system_prompt("test", &conv);
        assert!(
            prompt.contains("repository's custom test runner"),
            "project policy must remain intact"
        );
        for instruction in [
            "cargo check",
            "node:test",
            "npx tsc",
            "pytest",
            "go vet",
            "Language context:",
        ] {
            assert!(!prompt.contains(instruction));
        }
        assert_eq!(cm.recent_files.len(), 6);
        assert_eq!(cm.last_prompt_telemetry().file_guidance_chars, 0);
    }

    #[test]
    fn no_injection_for_unknown_tools() {
        let mut cm = ContextManager::new("base".into(), vec![]);
        let conv = ConversationState::new();

        cm.record_tool_call("bash");
        cm.record_tool_call("read");
        cm.record_tool_call("edit");
        let prompt = cm.build_system_prompt("test", &conv);

        // Core tools don't trigger group injection (they have static guidelines)
        assert!(
            !prompt.contains("guidelines:"),
            "core tools should not trigger group injection"
        );
    }

    #[test]
    fn hidden_change_tool_does_not_advance_implementing_phase() {
        let mut cm = ContextManager::new("base".into(), vec![]);
        let calls = vec![crate::conversation::ToolCall {
            id: "1".into(),
            name: "change".into(),
            arguments: serde_json::json!({}),
        }];

        cm.update_phase_from_activity(&calls);
        assert!(matches!(cm.phase(), LifecyclePhase::Idle));
    }
}
