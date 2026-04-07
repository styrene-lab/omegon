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
                requested_class: crate::settings::ContextClass::Maniple,
                reply_reserve: 8_192,
                tool_schema_reserve: 4_096,
            }),
        }
    }

    /// Set the context window size (in tokens) for budget calculations.
    pub fn set_context_window(&mut self, tokens: usize) {
        self.context_window = tokens;
        let mut policy = self.shadow.selector_policy();
        policy.model_window = tokens;
        self.shadow.set_selector_policy(policy);
    }

    /// Update the full selector policy for turn assembly.
    pub fn set_selector_policy(&mut self, policy: crate::settings::SelectorPolicy) {
        self.context_window = policy.model_window;
        self.shadow.set_selector_policy(policy);
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
        self.decay_expired();

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

        // Collect injections from all providers
        for provider in &self.providers {
            if let Some(injection) = provider.provide_context(&signals) {
                self.active_injections.push(ActiveInjection {
                    remaining_turns: injection.ttl_turns,
                    injection,
                });
            }
        }

        // Inject tool-group and file-type guidance based on recent activity
        self.inject_tool_group_context();
        self.inject_file_type_context();

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

        self.assemble(user_prompt)
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
                "change" | "write" | "edit" => {
                    if !matches!(self.phase, LifecyclePhase::Implementing { .. }) {
                        self.phase = LifecyclePhase::Implementing { change_id: None };
                    }
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
            a.remaining_turns > 0
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

    /// Inject tool-group guidelines based on which tools were recently called.
    /// Only injects once per group — removed after TTL expires.
    fn inject_tool_group_context(&mut self) {
        let already_injected: std::collections::HashSet<String> = self
            .active_injections
            .iter()
            .filter(|a| a.injection.source.starts_with("tool-group:"))
            .map(|a| a.injection.source.clone())
            .collect();

        for tool in self.recent_tools.iter() {
            let (group, guidance) = match tool.as_str() {
                // Memory tools — inject memory best practices
                "memory_store" | "memory_recall" | "memory_query" | "memory_supersede"
                | "memory_archive" | "memory_focus" | "memory_episodes" | "memory_connect" => (
                    "memory",
                    "Memory guidelines:\n\
                     - Use memory_recall(query) for targeted retrieval — cheaper than memory_query\n\
                     - Store conclusions, not investigation steps. Current state, not transitions.\n\
                     - Before storing, check if an existing fact covers it — use memory_supersede\n\
                     - Prefer pointer facts ('X does Y. See path/to/file') over inlining details",
                ),

                // Design tree — inject lifecycle guidance
                "design_tree" | "design_tree_update" => (
                    "design",
                    "Design tree guidelines:\n\
                     - Use 'node' to read full content. Use 'frontier' to find open questions.\n\
                     - Use 'branch' to spawn child nodes from open questions.\n\
                     - Transition: seed → exploring → resolved → decided → implementing.\n\
                     - Use 'focus' to inject a node's context into the conversation.",
                ),

                // Cleave — inject decomposition guidance
                "cleave_assess" | "cleave_run" => (
                    "cleave",
                    "Cleave guidelines:\n\
                     - cleave_assess determines complexity. Score ≥ 2.0 suggests decomposition.\n\
                     - When using OpenSpec, pass openspec_change_path to cleave_run.\n\
                     - After cleave_run, reconcile tasks.md and design-tree status.",
                ),

                // OpenSpec — inject lifecycle guidance
                "openspec_manage" => (
                    "openspec",
                    "OpenSpec guidelines:\n\
                     - The lifecycle: propose → spec → fast_forward → /cleave → /assess spec → archive\n\
                     - Specs define what must be true BEFORE code is written.\n\
                     - For tracked changes, use design_tree_update(implement) from a decided node.",
                ),

                // Local inference — inject model guidance
                "ask_local_model" | "list_local_models" | "manage_ollama" => (
                    "local-inference",
                    "Local inference guidelines:\n\
                     - Include ALL necessary context in prompts — local models can't see our conversation.\n\
                     - Use manage_ollama(start) if Ollama isn't running.\n\
                     - Use for boilerplate, summaries, transforms — not accuracy-critical work.",
                ),

                _ => continue,
            };

            let source_key = format!("tool-group:{group}");
            if already_injected.contains(&source_key) {
                continue;
            }

            self.active_injections.push(ActiveInjection {
                remaining_turns: 10,
                injection: ContextInjection {
                    source: source_key,
                    content: format!("[{guidance}]"),
                    priority: 80, // Between file-type (50) and HUD (200)
                    ttl_turns: 10,
                },
            });
        }
    }

    /// Inject language-specific guidance based on recently-touched file types.
    /// Only injects once per file type per session (avoids repetition).
    fn inject_file_type_context(&mut self) {
        // Check if we already have a file-type injection active
        let already_injected: std::collections::HashSet<String> = self
            .active_injections
            .iter()
            .filter(|a| a.injection.source.starts_with("file-type:"))
            .map(|a| a.injection.source.clone())
            .collect();

        for file in self.recent_files.iter().rev().take(5) {
            let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
            let source_key = format!("file-type:{ext}");

            if already_injected.contains(&source_key) {
                continue;
            }

            let guidance = match ext {
                "rs" => Some(
                    "Rust: use `cargo check` for type checking, `cargo clippy` for lints. Prefer `impl` blocks over free functions. Use `?` for error propagation. Tests go in `#[cfg(test)] mod tests` at the bottom of the file.",
                ),
                "ts" | "tsx" => Some(
                    "TypeScript: use `npx tsc --noEmit` for type checking. Prefer strict types over `any`. Use `node:test` for testing. ESM imports.",
                ),
                "py" => Some(
                    "Python: use `ruff check` for linting, `mypy` for type checking, `pytest` for tests. Prefer type hints. Use `pathlib` over `os.path`.",
                ),
                "go" => Some(
                    "Go: use `go vet` for checking, `go test ./...` for tests. Exported names start with uppercase. Error handling via returned `error` values.",
                ),
                "toml" if file.file_name().is_some_and(|n| n == "Cargo.toml") => Some(
                    "Cargo.toml: Rust workspace/package manifest. After dependency changes, run `cargo check`.",
                ),
                _ => None,
            };

            if let Some(text) = guidance {
                self.active_injections.push(ActiveInjection {
                    remaining_turns: 8, // Persist for 8 turns
                    injection: ContextInjection {
                        source: source_key,
                        content: format!("[Language context: {text}]"),
                        priority: 50, // Lower than HUD/intent
                        ttl_turns: 8,
                    },
                });
            }
        }
    }

    fn assemble(&mut self, user_prompt: &str) -> String {
        self.shadow.remove_by_source_prefix("base-prompt");
        let mut base = ShadowEntry::new(
            "base-prompt",
            ContextKind::BaseSystemPrompt,
            EntryBody::Inline(self.base_prompt.clone()),
        );
        base.mandatory = true;
        self.shadow.upsert(base);

        for active in &self.active_injections {
            let kind = match active.injection.source.as_str() {
                "session-hud" => ContextKind::SessionHud,
                "intent-document" => ContextKind::IntentDocument,
                source if source.starts_with("tool-group:") || source.starts_with("file-type:") => {
                    ContextKind::TaskArtifact
                }
                _ => ContextKind::TaskArtifact,
            };
            let mut entry = ShadowEntry::new(
                format!("inj:{}", active.injection.source),
                kind,
                EntryBody::Inline(active.injection.content.clone()),
            );
            entry.ttl_turns = Some(active.remaining_turns);
            entry.mandatory = active.injection.priority >= 190;
            self.shadow.upsert(entry);
        }

        let selected = self
            .shadow
            .select_for_turn(self.active_injections.len() as u32 + 1, user_prompt);
        self.shadow.render_selection(&selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn recent_files_dedup_consecutive() {
        let mut cm = ContextManager::new("base".into(), vec![]);
        cm.record_file_access(PathBuf::from("foo.rs"));
        cm.record_file_access(PathBuf::from("foo.rs"));
        cm.record_file_access(PathBuf::from("bar.rs"));
        cm.record_file_access(PathBuf::from("foo.rs"));
        assert_eq!(cm.recent_files.len(), 3); // foo, bar, foo (not 4)
    }

    #[test]
    fn tool_group_injection_on_memory_use() {
        let mut cm = ContextManager::new("base".into(), vec![]);
        // Before calling memory tool — no memory guidelines
        let conv = ConversationState::new();
        let prompt = cm.build_system_prompt("test", &conv);
        assert!(
            !prompt.contains("Memory guidelines"),
            "should not inject before tool use"
        );

        // Record a memory tool call
        cm.record_tool_call("memory_store");
        let prompt = cm.build_system_prompt("test", &conv);
        assert!(
            prompt.contains("Memory guidelines"),
            "should inject after memory tool use"
        );
        assert!(
            prompt.contains("memory_recall"),
            "should include recall guidance"
        );
    }

    #[test]
    fn tool_group_injection_deduplicates() {
        let mut cm = ContextManager::new("base".into(), vec![]);
        let conv = ConversationState::new();

        cm.record_tool_call("memory_store");
        let _ = cm.build_system_prompt("test", &conv);
        cm.record_tool_call("memory_recall");
        let _ = cm.build_system_prompt("test", &conv);
        cm.record_tool_call("memory_query");
        let prompt = cm.build_system_prompt("test", &conv);

        // Should only appear once despite 3 memory tool calls
        assert_eq!(
            prompt.matches("Memory guidelines").count(),
            1,
            "memory guidelines should appear exactly once"
        );
    }

    #[test]
    fn tool_group_injection_expires() {
        let mut cm = ContextManager::new("base".into(), vec![]);
        let mut conv = ConversationState::new();

        cm.record_tool_call("memory_store");
        let prompt = cm.build_system_prompt("test", &conv);
        assert!(prompt.contains("Memory guidelines"));

        // Clear recent tools — simulates the agent not calling memory tools anymore
        cm.recent_tools.clear();

        // Advance 11 turns (TTL is 10) — each build_system_prompt decrements remaining_turns
        for i in 1..=11 {
            conv.intent.stats.turns = i;
            let _ = cm.build_system_prompt("test", &conv);
        }

        let prompt = cm.build_system_prompt("test", &conv);
        assert!(
            !prompt.contains("Memory guidelines"),
            "should expire after TTL"
        );
    }

    #[test]
    fn file_type_injection_on_rust_file() {
        let mut cm = ContextManager::new("base".into(), vec![]);
        let conv = ConversationState::new();

        cm.record_file_access(PathBuf::from("src/main.rs"));
        let prompt = cm.build_system_prompt("test", &conv);
        assert!(
            prompt.contains("cargo check"),
            "should inject Rust guidance for .rs files"
        );
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
}
