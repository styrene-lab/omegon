//! ConversationState — canonical history, context decay, and IntentDocument.
//!
//! Maintains two views: the canonical (unmodified) history for persistence,
//! and the LLM-facing view with decay applied for context efficiency.

use crate::bridge::{LlmMessage, WireToolCall};
use indexmap::IndexSet;
use omegon_traits::LifecyclePhase;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// A tool call extracted from an assistant message.
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// A tool result entry in the conversation.
#[derive(Debug, Clone)]
pub struct ToolResultEntry {
    pub call_id: String,
    pub tool_name: String,
    pub content: Vec<omegon_traits::ContentBlock>,
    pub is_error: bool,
    /// Key arguments summarized for decay context (e.g. "path: src/auth.rs").
    /// Set by the loop from the tool call arguments when the result is created.
    pub args_summary: Option<String>,
}

/// An assistant message with parsed content.
#[derive(Debug, Clone)]
pub struct AssistantMessage {
    pub text: String,
    pub thinking: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    /// The complete provider response — opaque, preserved for multi-turn continuity
    pub raw: Value,
    /// Actual billing tokens reported by the provider. (0,0,0) = not reported.
    pub provider_tokens: (u64, u64, u64, u64), // (input, output, cache_read, cache_write)
    /// Parsed provider quota/headroom telemetry for this turn, when available.
    pub provider_telemetry: Option<omegon_traits::ProviderTelemetrySnapshot>,
}

impl Default for AssistantMessage {
    fn default() -> Self {
        Self {
            text: String::new(),
            thinking: None,
            tool_calls: Vec::new(),
            raw: Value::Null,
            provider_tokens: (0, 0, 0, 0),
            provider_telemetry: None,
        }
    }
}

impl ConversationState {
    pub fn last_provider_telemetry(
        &self,
        provider: Option<&str>,
    ) -> Option<omegon_traits::ProviderTelemetrySnapshot> {
        self.canonical.iter().rev().find_map(|msg| match msg {
            AgentMessage::Assistant(assistant, _) => {
                assistant.provider_telemetry.clone().and_then(|t| {
                    if provider.is_none_or(|p| t.provider == p) {
                        Some(t)
                    } else {
                        None
                    }
                })
            }
            _ => None,
        })
    }
}

impl AssistantMessage {
    pub fn text_content(&self) -> &str {
        &self.text
    }

    pub fn tool_calls(&self) -> &[ToolCall] {
        &self.tool_calls
    }
}

/// A message in the canonical conversation history.
#[derive(Debug, Clone)]
pub enum AgentMessage {
    User {
        text: String,
        images: Vec<crate::bridge::ImageAttachment>,
        turn: u32,
    },
    Assistant(AssistantMessage, u32), // (msg, turn)
    ToolResult(ToolResultEntry, u32), // (result, turn)
}

impl AgentMessage {
    fn turn(&self) -> u32 {
        match self {
            AgentMessage::User { turn, .. } => *turn,
            AgentMessage::Assistant(_, turn) => *turn,
            AgentMessage::ToolResult(_, turn) => *turn,
        }
    }
}

/// Structured intent tracking — auto-populated, survives compaction verbatim.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IntentDocument {
    pub current_task: Option<String>,
    pub approach: Option<String>,
    pub lifecycle_phase: LifecyclePhase,

    pub files_read: IndexSet<PathBuf>,
    pub files_modified: IndexSet<PathBuf>,
    /// Set to true after the agent has been nudged to commit once.
    /// Persists across loop invocations (TUI re-enters run() per user turn)
    /// to prevent the nudge from firing every turn in the same session.
    pub commit_nudged: bool,

    pub constraints_discovered: Vec<String>,
    pub failed_approaches: Vec<FailedApproach>,
    pub open_questions: Vec<String>,

    pub stats: SessionStatsAccumulator,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FailedApproach {
    pub description: String,
    pub reason: String,
    pub turn: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionStatsAccumulator {
    pub turns: u32,
    pub tool_calls: u32,
    pub tokens_consumed: u64,
    pub compactions: u32,
}

impl IntentDocument {
    /// Update from tool call activity — automatic population.
    pub fn update_from_tools(&mut self, calls: &[ToolCall], results: &[ToolResultEntry]) {
        self.stats.tool_calls += calls.len() as u32;

        for call in calls {
            match call.name.as_str() {
                "read" | "understand" => {
                    if let Some(path) = call.arguments.get("path").and_then(|v| v.as_str()) {
                        self.files_read.insert(PathBuf::from(path));
                    }
                }
                "change" | "write" | "edit" => {
                    if let Some(path) = call.arguments.get("path").and_then(|v| v.as_str()) {
                        self.files_modified.insert(PathBuf::from(path));
                    }
                    // change tool may include multiple file paths in an edits array
                    if let Some(edits) = call.arguments.get("edits").and_then(|v| v.as_array()) {
                        for edit in edits {
                            if let Some(path) = edit.get("file").and_then(|v| v.as_str()) {
                                self.files_modified.insert(PathBuf::from(path));
                            }
                        }
                    }
                }
                // commit clears the mutation set — after a commit the working tree is clean.
                // Also resets commit_nudged so the agent can be nudged again if it makes
                // further changes after committing.
                "commit" => {
                    self.files_modified.clear();
                    self.commit_nudged = false;
                }
                // bash: can't reliably track which files are modified by arbitrary commands.
                // File tracking for bash is inherently best-effort — the agent should use
                // edit/write for trackable mutations. bash is for commands, not file writes.
                _ => {}
            }
        }

        // Track tool errors for failed-approach detection
        for result in results {
            if result.is_error {
                // Don't auto-add failed approaches for individual tool errors —
                // that's too granular. The agent marks failed approaches explicitly
                // via omg:failed tags. But we do count error rate for the HUD.
            }
        }
    }

    /// Auto-populate current_task from the first user message if not set.
    pub fn set_task_from_prompt(&mut self, prompt: &str) {
        if self.current_task.is_some() {
            return;
        }
        // Use first line, truncated to 200 chars
        let first_line = prompt.lines().next().unwrap_or(prompt);
        let task = if first_line.len() > 200 {
            let mut end = 200;
            while end > 0 && !first_line.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}…", &first_line[..end])
        } else {
            first_line.to_string()
        };
        if !task.trim().is_empty() {
            self.current_task = Some(task);
        }
    }

    /// Add a constraint, deduplicating against existing entries.
    pub fn add_constraint(&mut self, text: &str) {
        let normalized = text.trim();
        if !normalized.is_empty() && !self.constraints_discovered.iter().any(|c| c == normalized) {
            self.constraints_discovered.push(normalized.to_string());
        }
    }

    /// Add an open question, deduplicating against existing entries.
    pub fn add_question(&mut self, text: &str) {
        let normalized = text.trim();
        if !normalized.is_empty() && !self.open_questions.iter().any(|q| q == normalized) {
            self.open_questions.push(normalized.to_string());
        }
    }
}

/// Serializable session snapshot for save/resume.
///
/// All fields use `#[serde(default)]` so that sessions saved by older versions
/// (which may lack newer fields) deserialize without error.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
struct SessionSnapshot {
    messages: Vec<LlmMessage>,
    intent: IntentDocument,
    decay_window: usize,
    compaction_summary: Option<String>,
}

impl Default for SessionSnapshot {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            intent: IntentDocument::default(),
            decay_window: 0,
            compaction_summary: None,
        }
    }
}

/// The full conversation state.
pub struct ConversationState {
    /// Canonical, unmodified history. Source of truth for persistence.
    canonical: Vec<AgentMessage>,

    /// Slim runtime mode — enables more aggressive history decay.
    slim_mode: bool,

    /// The IntentDocument — survives compaction verbatim.
    pub intent: IntentDocument,

    /// Decay window: messages older than this many turns get decayed.
    /// Referenced tool results get an extra grace period.
    decay_window: usize,

    /// Turn indices of tool results that the LLM has referenced (mentioned
    /// paths or content from). These get an extended decay window.
    referenced_turns: std::collections::HashSet<u32>,

    /// Compaction summary — if set, injected as the first message after compaction.
    /// Replaces evicted messages so the LLM has continuity.
    compaction_summary: Option<String>,
}

impl ConversationState {
    pub fn new() -> Self {
        Self {
            canonical: Vec::new(),
            slim_mode: false,
            intent: IntentDocument::default(),
            decay_window: 10,
            referenced_turns: std::collections::HashSet::new(),
            compaction_summary: None,
        }
    }

    pub fn set_slim_mode(&mut self, slim: bool) {
        self.slim_mode = slim;
    }

    /// Estimate token count of the LLM-facing view (chars / 4 heuristic).
    /// Good enough for budget decisions — not a precise tokenizer.
    pub fn estimate_tokens(&self) -> usize {
        let view = self.build_llm_view();
        let chars: usize = view.iter().map(|m| m.char_count()).sum();
        chars / 4
    }

    /// Check if compaction is needed given a context budget.
    /// Returns true if estimated tokens exceed the threshold fraction.
    pub fn needs_compaction(&self, context_window: usize, threshold: f32) -> bool {
        let tokens = self.estimate_tokens();
        tokens as f32 > context_window as f32 * threshold
    }

    /// Build the text for an LLM compaction request — the messages that would
    /// be evicted, formatted for summarization.
    pub fn build_compaction_payload(&self) -> Option<(String, usize)> {
        let current_turn = self.intent.stats.turns;
        // Find messages older than the decay window — these are the ones
        // that are already decayed and should be compacted into a summary.
        let evictable: Vec<&AgentMessage> = self
            .canonical
            .iter()
            .filter(|m| current_turn.saturating_sub(m.turn()) > self.decay_window as u32)
            .collect();

        if evictable.is_empty() {
            return None;
        }

        let mut payload = String::new();
        payload.push_str("Summarize this conversation excerpt. Preserve:\n");
        payload.push_str("- What was accomplished (files changed, decisions made)\n");
        payload.push_str("- What failed and why\n");
        payload.push_str("- Current task and approach\n");
        payload.push_str("- Key constraints discovered\n");
        payload.push_str("Be concise but preserve actionable context.\n\n---\n\n");

        for msg in &evictable {
            match msg {
                AgentMessage::User { text, turn, .. } => {
                    payload.push_str(&format!("[Turn {turn}] User: {text}\n\n"));
                }
                AgentMessage::Assistant(a, turn) => {
                    let truncated = if a.text.len() > 200 {
                        crate::util::truncate(&a.text, 200)
                    } else {
                        a.text.clone()
                    };
                    payload.push_str(&format!("[Turn {turn}] Assistant: {truncated}\n"));
                    if !a.tool_calls.is_empty() {
                        let tools: Vec<_> =
                            a.tool_calls.iter().map(|tc| tc.name.as_str()).collect();
                        payload.push_str(&format!("  Tools called: {}\n", tools.join(", ")));
                    }
                    payload.push('\n');
                }
                AgentMessage::ToolResult(r, turn) => {
                    let status = if r.is_error { "ERROR" } else { "ok" };
                    payload.push_str(&format!("[Turn {turn}] Tool {}: {status}\n\n", r.tool_name));
                }
            }
        }

        Some((payload, evictable.len()))
    }

    /// Apply a compaction summary — evict old messages and replace with summary.
    /// Emergency decay: drop the oldest N messages from the canonical history.
    /// Used when compaction fails and the context is too large for the provider.
    pub fn decay_oldest(&mut self, count: usize) {
        let remove = count.min(self.canonical.len());
        self.canonical.drain(..remove);
        tracing::info!(
            removed = remove,
            remaining = self.canonical.len(),
            "Emergency decay applied"
        );
    }

    /// Number of messages in the canonical conversation.
    pub fn message_count(&self) -> usize {
        self.canonical.len()
    }

    pub fn apply_compaction(&mut self, summary: String) {
        let current_turn = self.intent.stats.turns;
        // Remove all messages older than the decay window
        self.canonical
            .retain(|m| current_turn.saturating_sub(m.turn()) <= self.decay_window as u32);
        self.compaction_summary = Some(summary);
        self.intent.stats.compactions += 1;
        tracing::info!(
            compactions = self.intent.stats.compactions,
            remaining_messages = self.canonical.len(),
            "Compaction applied"
        );
    }

    /// Render the IntentDocument as a context injection block.
    pub fn render_intent_for_injection(&self) -> String {
        let intent = &self.intent;
        let mut lines = Vec::new();
        lines.push("[Intent — session state]".to_string());

        if let Some(task) = &intent.current_task {
            lines.push(format!("Task: {task}"));
        }
        if let Some(approach) = &intent.approach {
            lines.push(format!("Approach: {approach}"));
        }
        if !intent.files_modified.is_empty() {
            let files: Vec<_> = intent
                .files_modified
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            lines.push(format!("Files modified: {}", files.join(", ")));
        }
        if !intent.constraints_discovered.is_empty() {
            lines.push(format!(
                "Constraints: {}",
                intent.constraints_discovered.join("; ")
            ));
        }
        if !intent.failed_approaches.is_empty() {
            lines.push("Failed approaches:".to_string());
            for fa in &intent.failed_approaches {
                lines.push(format!(
                    "  - {}: {} (turn {})",
                    fa.description, fa.reason, fa.turn
                ));
            }
        }
        lines.push(format!(
            "Stats: {} turns, {} tool calls, {} compactions",
            intent.stats.turns, intent.stats.tool_calls, intent.stats.compactions
        ));

        lines.join("\n")
    }

    pub fn push_user(&mut self, text: String) {
        self.push_user_with_images(text, Vec::new());
    }

    pub fn push_user_with_images(
        &mut self,
        text: String,
        images: Vec<crate::bridge::ImageAttachment>,
    ) {
        let turn = self.intent.stats.turns;
        // Auto-populate current_task from the first non-system user message
        if !text.starts_with("[System:") {
            self.intent.set_task_from_prompt(&text);
        }
        self.canonical
            .push(AgentMessage::User { text, images, turn });
    }

    pub fn push_assistant(&mut self, msg: AssistantMessage) {
        let turn = self.intent.stats.turns;
        // Reference tracking: scan the assistant's text for paths and identifiers
        // that appear in recent tool results. Referenced results decay slower.
        self.track_references(&msg.text);
        self.canonical.push(AgentMessage::Assistant(msg, turn));
    }

    /// Scan assistant text for references to recent tool results.
    /// If the assistant mentions a file path from a recent read/edit result,
    /// mark that result's turn as "referenced" (extended decay window).
    fn track_references(&mut self, assistant_text: &str) {
        if assistant_text.is_empty() {
            return;
        }

        for msg in self.canonical.iter().rev().take(30) {
            match msg {
                AgentMessage::ToolResult(result, turn) if !result.is_error => {
                    // Check if the assistant mentions paths from tool results
                    let text_content = result
                        .content
                        .iter()
                        .filter_map(|c| c.as_text())
                        .collect::<Vec<_>>()
                        .join("\n");

                    // For read/edit/write results, check if the result content
                    // or known file paths are mentioned in the assistant text
                    let referenced = match result.tool_name.as_str() {
                        "read" | "edit" | "write" => {
                            // Quick heuristic: if the assistant mentions an identifier
                            // from the first few lines of the result, it's referenced.
                            text_content
                                .lines()
                                .take(10)
                                .filter(|l| l.len() > 8 && l.len() < 200)
                                .any(|line| {
                                    // Extract identifiers: sequences of [a-zA-Z0-9_] with length > 4
                                    extract_identifiers(line)
                                        .any(|ident| assistant_text.contains(ident))
                                })
                        }
                        "bash" => {
                            // Bash output is harder to track — check first/last lines
                            text_content
                                .lines()
                                .take(3)
                                .chain(text_content.lines().rev().take(3))
                                .any(|line| {
                                    let trimmed = line.trim();
                                    trimmed.len() > 6
                                        && trimmed.len() < 200
                                        && assistant_text.contains(trimmed)
                                })
                        }
                        _ => false,
                    };

                    if referenced {
                        self.referenced_turns.insert(*turn);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn push_tool_result(&mut self, result: ToolResultEntry) {
        let turn = self.intent.stats.turns;
        self.canonical.push(AgentMessage::ToolResult(result, turn));
    }

    pub fn turn_count(&self) -> u32 {
        self.intent.stats.turns
    }

    pub fn last_user_prompt(&self) -> &str {
        self.canonical
            .iter()
            .rev()
            .find_map(|m| match m {
                AgentMessage::User { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .unwrap_or("")
    }

    pub fn render_attachment_context_injection(&self) -> Option<String> {
        let AgentMessage::User { images, .. } = self
            .canonical
            .iter()
            .rev()
            .find(|m| matches!(m, AgentMessage::User { .. }))?
        else {
            return None;
        };

        let lines: Vec<String> = images
            .iter()
            .enumerate()
            .filter_map(|(idx, image)| {
                image
                    .source_path
                    .as_deref()
                    .map(|path| format!("- [image{idx}] {path}"))
            })
            .collect();

        if lines.is_empty() {
            return None;
        }

        Some(format!(
            "[Attachment files]\n{}\n\nThese are operator-supplied local attachment paths. Use them only when you need to redisplay an attachment through the existing view/display pipeline. Do not quote this manifest back to the operator unless they explicitly ask for the raw path.",
            lines.join("\n")
        ))
    }

    pub fn last_assistant_text(&self) -> Option<&str> {
        self.canonical.iter().rev().find_map(|m| match m {
            AgentMessage::Assistant(a, _) if !a.text.is_empty() => Some(a.text.as_str()),
            _ => None,
        })
    }

    /// Build the LLM-facing view with context decay applied.
    /// Messages older than `decay_window` turns are decayed to skeletons.
    /// If a compaction summary exists, it's injected as the first message.
    pub fn build_llm_view(&self) -> Vec<LlmMessage> {
        let current_turn = self.intent.stats.turns;
        let mut messages: Vec<LlmMessage> = Vec::new();

        // Inject compaction summary as first message if present
        if let Some(summary) = &self.compaction_summary {
            messages.push(LlmMessage::User {
                content: format!(
                    "[Previous conversation summary]\n{summary}\n\n{}\n[End summary — continue from here]",
                    self.render_intent_for_injection()
                ),
                images: vec![],
            });
        }

        for msg in &self.canonical {
            let turn_age = current_turn.saturating_sub(msg.turn());
            // Referenced tool results get 2x the decay window
            let effective_window = if self.referenced_turns.contains(&msg.turn()) {
                self.decay_window as u32 * 2
            } else {
                self.decay_window as u32
            };
            if turn_age > effective_window {
                messages.push(self.decay_message(msg));
            } else {
                messages.push(self.to_llm_message(msg));
            }
        }

        // Strip orphaned tool_result messages whose tool_use was evicted.
        // After compaction/decay, tool_use blocks may be removed while their
        // corresponding tool_result blocks survive. Anthropic rejects these
        // with "unexpected tool_use_id found in tool_result blocks".
        strip_orphaned_tool_results(&mut messages);

        // Enforce role alternation: providers require strict user/assistant
        // alternation. After compaction or decay, adjacent same-role messages
        // can appear. Merge adjacent user messages; drop adjacent assistant
        // messages (keep the last one — it's the most recent).
        enforce_role_alternation(&mut messages);

        messages
    }

    #[allow(dead_code)]
    /// Apply ambient captures from omg: tags.
    pub fn apply_ambient_captures(
        &mut self,
        captures: &[crate::lifecycle::capture::AmbientCapture],
    ) {
        for capture in captures {
            match capture {
                crate::lifecycle::capture::AmbientCapture::Constraint(text) => {
                    self.intent.add_constraint(text);
                }
                crate::lifecycle::capture::AmbientCapture::Question(text) => {
                    self.intent.add_question(text);
                }
                crate::lifecycle::capture::AmbientCapture::Approach(text) => {
                    self.intent.approach = Some(text.clone());
                }
                crate::lifecycle::capture::AmbientCapture::Failed {
                    description,
                    reason,
                } => {
                    // Deduplicate failed approaches by description
                    let normalized = description.trim();
                    if !self
                        .intent
                        .failed_approaches
                        .iter()
                        .any(|fa| fa.description.trim() == normalized)
                    {
                        self.intent.failed_approaches.push(FailedApproach {
                            description: description.clone(),
                            reason: reason.clone(),
                            turn: self.intent.stats.turns,
                        });
                    }
                }
                crate::lifecycle::capture::AmbientCapture::Phase(phase_str) => {
                    let phase = match phase_str.trim().to_lowercase().as_str() {
                        "explore" | "exploring" => {
                            omegon_traits::LifecyclePhase::Exploring { node_id: None }
                        }
                        "specify" | "specifying" => {
                            omegon_traits::LifecyclePhase::Specifying { change_id: None }
                        }
                        "decompose" | "decomposing" => omegon_traits::LifecyclePhase::Decomposing,
                        "implement" | "implementing" => {
                            omegon_traits::LifecyclePhase::Implementing { change_id: None }
                        }
                        "verify" | "verifying" => {
                            omegon_traits::LifecyclePhase::Verifying { change_id: None }
                        }
                        "idle" => omegon_traits::LifecyclePhase::Idle,
                        _ => continue, // Unknown phase string — skip
                    };
                    self.intent.lifecycle_phase = phase;
                }
                crate::lifecycle::capture::AmbientCapture::Decision { .. } => {
                    // Decisions are captured for lifecycle engine integration.
                    // Currently logged — will be routed to design-tree when
                    // the lifecycle store is implemented.
                    tracing::debug!(
                        "Ambient decision captured (not yet routed to lifecycle store)"
                    );
                }
            }
        }
    }

    /// Decay a message to a skeleton — strip bulk content, keep metadata.
    /// The skeleton preserves enough to understand what happened without
    /// the bulk content. Tool-specific metadata (file paths, exit codes,
    /// line counts) is extracted before discarding the full content.
    fn decay_message(&self, msg: &AgentMessage) -> LlmMessage {
        match msg {
            AgentMessage::ToolResult(result, _) => {
                let summary = self.decay_tool_result(result);
                LlmMessage::ToolResult {
                    call_id: result.call_id.clone(),
                    tool_name: result.tool_name.clone(),
                    content: summary,
                    is_error: result.is_error,
                    args_summary: result.args_summary.clone(),
                }
            }
            AgentMessage::Assistant(a, _) => {
                // Decay: strip thinking entirely, truncate long text, preserve tool calls
                let decayed_text = if a.text.len() > 500 {
                    let mut end = 500;
                    while end > 0 && !a.text.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}...[truncated]", &a.text[..end])
                } else {
                    a.text.clone()
                };
                LlmMessage::Assistant {
                    text: if decayed_text.is_empty() {
                        vec![]
                    } else {
                        vec![decayed_text]
                    },
                    thinking: vec![], // Strip thinking blocks entirely on decay
                    tool_calls: a
                        .tool_calls
                        .iter()
                        .map(|tc| WireToolCall {
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                        })
                        .collect(),
                    raw: None, // Don't preserve raw for decayed messages
                }
            }
            AgentMessage::User { text, images, .. } => LlmMessage::User {
                content: text.clone(),
                images: images.clone(),
            },
        }
    }

    // ── Session persistence ──────────────────────────────────────────────

    /// Save conversation state to a JSON file for later resumption.
    /// Persists: the LLM-facing view (not canonical — raw may contain
    /// non-serializable handles), the intent document, and turn count.
    pub fn save_session(&self, path: &Path) -> anyhow::Result<()> {
        let view = self.build_llm_view();
        let session = SessionSnapshot {
            messages: view,
            intent: self.intent.clone(),
            decay_window: self.decay_window,
            compaction_summary: self.compaction_summary.clone(),
        };
        let json = serde_json::to_string_pretty(&session)?;
        std::fs::write(path, json)?;
        tracing::info!(path = %path.display(), turns = self.intent.stats.turns, "session saved");
        Ok(())
    }

    /// Load a previously saved session.
    ///
    /// To avoid blowing the first resumed turn's context budget, we do NOT
    /// hydrate the entire prior session as if it were all recent. Instead we:
    /// - keep only a recent tail of messages as canonical history
    /// - fold older messages into `compaction_summary` so they still inform the model
    pub fn load_session(path: &Path) -> anyhow::Result<Self> {
        const RESUME_TAIL_MESSAGES: usize = 24;

        let json = std::fs::read_to_string(path)?;
        let snapshot: SessionSnapshot = serde_json::from_str(&json)?;
        tracing::info!(
            path = %path.display(),
            turns = snapshot.intent.stats.turns,
            messages = snapshot.messages.len(),
            "session loaded"
        );

        let last_turn = snapshot.intent.stats.turns;
        let total_messages = snapshot.messages.len();
        let split_at = total_messages.saturating_sub(RESUME_TAIL_MESSAGES);
        let (older, recent) = snapshot.messages.split_at(split_at);

        let resume_summary = if older.is_empty() {
            snapshot.compaction_summary.clone()
        } else {
            let mut lines = Vec::new();
            lines.push(format!(
                "Resumed session with {} earlier message(s) compacted.",
                older.len()
            ));
            for msg in older.iter().rev().take(8).rev() {
                match msg {
                    LlmMessage::User { content, .. } => {
                        lines.push(format!("- User: {}", crate::util::truncate(content, 140)));
                    }
                    LlmMessage::Assistant {
                        text, tool_calls, ..
                    } => {
                        let body = crate::util::truncate(&text.join("\n"), 140);
                        if tool_calls.is_empty() {
                            lines.push(format!("- Assistant: {body}"));
                        } else {
                            let tools = tool_calls
                                .iter()
                                .map(|t| t.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ");
                            lines.push(format!("- Assistant ({tools}): {body}"));
                        }
                    }
                    LlmMessage::ToolResult {
                        tool_name, content, ..
                    } => {
                        lines.push(format!(
                            "- Tool {tool_name}: {}",
                            crate::util::truncate(content, 120)
                        ));
                    }
                }
            }
            match snapshot.compaction_summary.as_ref() {
                Some(existing) if !existing.trim().is_empty() => {
                    Some(format!("{existing}\n\n{}", lines.join("\n")))
                }
                _ => Some(lines.join("\n")),
            }
        };

        let canonical: Vec<AgentMessage> = recent
            .iter()
            .cloned()
            .map(|msg| {
                let turn = last_turn;
                match msg {
                    LlmMessage::User { content, images } => AgentMessage::User {
                        text: content,
                        images,
                        turn,
                    },
                    LlmMessage::Assistant {
                        text,
                        thinking,
                        tool_calls,
                        raw,
                    } => AgentMessage::Assistant(
                        AssistantMessage {
                            text: text.join("\n"),
                            thinking: if thinking.is_empty() {
                                None
                            } else {
                                Some(thinking.join("\n"))
                            },
                            tool_calls: tool_calls
                                .into_iter()
                                .map(|tc| ToolCall {
                                    id: tc.id,
                                    name: tc.name,
                                    arguments: tc.arguments,
                                })
                                .collect(),
                            raw: raw.unwrap_or(Value::Null),
                            provider_tokens: (0, 0, 0, 0),
                            provider_telemetry: None,
                        },
                        turn,
                    ),
                    LlmMessage::ToolResult {
                        call_id,
                        tool_name,
                        content,
                        is_error,
                        args_summary,
                    } => AgentMessage::ToolResult(
                        ToolResultEntry {
                            call_id,
                            tool_name,
                            content: vec![omegon_traits::ContentBlock::Text { text: content }],
                            is_error,
                            args_summary,
                        },
                        turn,
                    ),
                }
            })
            .collect();

        Ok(Self {
            canonical,
            slim_mode: false,
            intent: snapshot.intent,
            decay_window: snapshot.decay_window,
            referenced_turns: std::collections::HashSet::new(),
            compaction_summary: resume_summary,
        })
    }

    /// Produce a rich skeleton for a decayed tool result.
    /// Extracts tool-specific metadata so the LLM remembers *what* happened
    /// without the bulk content consuming context budget.
    fn decay_tool_result(&self, result: &ToolResultEntry) -> String {
        let text = result
            .content
            .iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("\n");

        let ctx = result.args_summary.as_deref().unwrap_or("");
        let ctx_suffix = if ctx.is_empty() {
            String::new()
        } else {
            format!(" ({ctx})")
        };

        let error_limit = if self.slim_mode { 180 } else { 300 };
        let generic_limit = if self.slim_mode { 80 } else { 120 };
        let bash_tail_lines = if self.slim_mode { 3 } else { 3 };

        if result.is_error {
            let error_preview = if text.len() > error_limit {
                let mut end = error_limit;
                while end > 0 && !text.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}…", &text[..end])
            } else {
                text
            };
            return format!("[{} ERROR{ctx_suffix}: {error_preview}]", result.tool_name);
        }

        match result.tool_name.as_str() {
            "read" => {
                let lines = text.lines().count();
                let bytes = text.len();
                if self.slim_mode && ctx.is_empty() {
                    format!("[Read: {lines} lines]")
                } else {
                    format!("[Read{ctx_suffix}: {lines} lines, {bytes} bytes]")
                }
            }
            "bash" | "execute" => {
                let lines = text.lines().count();
                let exit_hint = if text.contains("exit code") || text.contains("exited with") {
                    " (non-zero exit)"
                } else {
                    ""
                };
                let tail: Vec<&str> = text.lines().rev().take(bash_tail_lines).collect();
                let tail_str: String = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
                if self.slim_mode && lines > 20 {
                    format!("[bash{ctx_suffix}: {lines} lines{exit_hint}. Tail:\n{tail_str}]")
                } else if lines <= 5 {
                    format!("[bash{ctx_suffix}{exit_hint}: {text}]")
                } else {
                    format!("[bash{ctx_suffix}: {lines} lines{exit_hint}. Tail:\n{tail_str}]")
                }
            }
            "edit" => {
                format!("[edit{ctx_suffix}: {text}]")
            }
            "write" => {
                format!("[write{ctx_suffix}: {text}]")
            }
            "web_search" => {
                let lines = text.lines().count();
                format!("[web_search{ctx_suffix}: {lines} lines of results]")
            }
            _ => {
                let lines = text.lines().count();
                let first_line = text.lines().next().unwrap_or("").trim();
                let preview = if first_line.len() > generic_limit {
                    let mut end = generic_limit;
                    while end > 0 && !first_line.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}…", &first_line[..end])
                } else {
                    first_line.to_string()
                };
                if self.slim_mode && ctx.is_empty() {
                    format!("[{}: {preview}]", result.tool_name)
                } else if lines <= 3 {
                    format!("[{}{ctx_suffix}: {}]", result.tool_name, text.trim())
                } else {
                    format!(
                        "[{}{ctx_suffix}: {lines} lines. {preview}]",
                        result.tool_name
                    )
                }
            }
        }
    }

    /// Convert a canonical message to Omegon's wire format.
    fn to_llm_message(&self, msg: &AgentMessage) -> LlmMessage {
        match msg {
            AgentMessage::User { text, images, .. } => LlmMessage::User {
                content: text.clone(),
                images: images.clone(),
            },
            AgentMessage::Assistant(a, _) => LlmMessage::Assistant {
                text: if a.text.is_empty() {
                    vec![]
                } else {
                    vec![a.text.clone()]
                },
                thinking: a
                    .thinking
                    .as_ref()
                    .map(|t| vec![t.clone()])
                    .unwrap_or_default(),
                tool_calls: a
                    .tool_calls
                    .iter()
                    .map(|tc| WireToolCall {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: tc.arguments.clone(),
                    })
                    .collect(),
                raw: Some(a.raw.clone()),
            },
            AgentMessage::ToolResult(r, _) => {
                // Flatten content blocks to text
                let text = r
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        omegon_traits::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                LlmMessage::ToolResult {
                    call_id: r.call_id.clone(),
                    tool_name: r.tool_name.clone(),
                    content: text,
                    is_error: r.is_error,
                    args_summary: r.args_summary.clone(),
                }
            }
        }
    }
}

/// Extract identifier-like tokens from a line of code.
/// Enforce strict role alternation (user → assistant → user → …).
/// After compaction/decay/orphan stripping, the message list may violate this.
/// - Adjacent User messages: merge content with newline separator
/// - Adjacent Assistant messages: keep only the last one
/// - ToolResult without a preceding Assistant: drop it (orphaned)
/// - Leading Assistant message (no prior User): drop it
fn enforce_role_alternation(messages: &mut Vec<LlmMessage>) {
    if messages.len() < 2 {
        return;
    }

    let mut result: Vec<LlmMessage> = Vec::with_capacity(messages.len());
    for msg in messages.drain(..) {
        let prev_role = result.last().map(|m| match m {
            LlmMessage::User { .. } => "user",
            LlmMessage::Assistant { .. } => "assistant",
            LlmMessage::ToolResult { .. } => "tool_result",
        });

        match (&msg, prev_role) {
            // User after user → merge
            (LlmMessage::User { content, images }, Some("user")) => {
                if let Some(LlmMessage::User {
                    content: prev_content,
                    images: prev_images,
                }) = result.last_mut()
                {
                    prev_content.push('\n');
                    prev_content.push_str(content);
                    prev_images.extend(images.iter().cloned());
                }
            }
            // Assistant after assistant (no tool results between) → replace
            (LlmMessage::Assistant { .. }, Some("assistant")) => {
                result.pop();
                result.push(msg);
            }
            // Tool result is always valid after assistant (part of the same turn)
            (LlmMessage::ToolResult { .. }, Some("assistant" | "tool_result")) => {
                result.push(msg);
            }
            // Tool result without preceding assistant → drop
            (LlmMessage::ToolResult { call_id, .. }, _) => {
                tracing::debug!(call_id, "dropping tool_result with no preceding assistant");
            }
            // Normal alternation (including edge cases like leading assistant)
            _ => result.push(msg),
        }
    }
    *messages = result;
}

/// Remove tool_result messages that reference tool_use IDs not present in any
/// preceding assistant message. This happens after compaction/decay evicts
/// assistant messages but leaves their tool_result responses.
fn strip_orphaned_tool_results(messages: &mut Vec<LlmMessage>) {
    use std::collections::HashSet;
    // Collect all tool_use IDs from assistant messages
    let mut known_ids: HashSet<String> = HashSet::new();
    for msg in messages.iter() {
        if let LlmMessage::Assistant { tool_calls, .. } = msg {
            for tc in tool_calls {
                known_ids.insert(tc.id.clone());
            }
        }
    }
    // Remove tool_result messages whose call_id isn't in known_ids
    messages.retain(|msg| {
        if let LlmMessage::ToolResult { call_id, .. } = msg {
            if !known_ids.contains(call_id) {
                tracing::debug!(call_id, "stripping orphaned tool_result");
                return false;
            }
        }
        true
    });
}

/// Returns sequences of `[a-zA-Z0-9_]` that are at least 8 chars long.
/// Threshold of 8 avoids false positives on common short identifiers
/// (String, Error, value, token, state) that appear in most responses.
fn extract_identifiers(line: &str) -> impl Iterator<Item = &str> {
    const MIN_IDENT_LEN: usize = 8;
    let bytes = line.as_bytes();
    let mut results = Vec::new();
    let mut start = None;
    for (i, &b) in bytes.iter().enumerate() {
        if b.is_ascii_alphanumeric() || b == b'_' {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start {
            if i - s >= MIN_IDENT_LEN {
                results.push(&line[s..i]);
            }
            start = None;
        }
    }
    if let Some(s) = start.filter(|&s| line.len() - s >= MIN_IDENT_LEN) {
        results.push(&line[s..]);
    }
    results.into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Insert an assistant message with a tool_call matching the given call_id.
    /// Required so that subsequent tool_result messages aren't stripped as orphans.
    fn push_matching_assistant(conv: &mut ConversationState, call_id: &str) {
        conv.push_assistant(AssistantMessage {
            text: String::new(),
            thinking: None,
            tool_calls: vec![ToolCall {
                id: call_id.into(),
                name: "test".into(),
                arguments: serde_json::json!({}),
            }],
            raw: serde_json::Value::Null,
            provider_tokens: (0, 0, 0, 0),
            provider_telemetry: None,
        });
    }

    #[test]
    fn assistant_decay_strips_thinking() {
        let mut conv = ConversationState::new();
        conv.decay_window = 0; // Decay everything older than current turn

        // Push message at turn 0, then advance to turn 1 so it's "old"
        conv.push_assistant(AssistantMessage {
            text: "short response".into(),
            thinking: Some("very long internal thinking...".repeat(100)),
            tool_calls: vec![],
            raw: serde_json::Value::Null,
            provider_tokens: (0, 0, 0, 0),
            provider_telemetry: None,
        });
        conv.intent.stats.turns = 1; // Advance turn so the message is old

        let view = conv.build_llm_view();
        assert_eq!(view.len(), 1);
        if let LlmMessage::Assistant { thinking, .. } = &view[0] {
            assert!(thinking.is_empty(), "Thinking should be stripped on decay");
        } else {
            panic!("Expected Assistant message");
        }
    }

    #[test]
    fn assistant_decay_truncates_long_text() {
        let mut conv = ConversationState::new();
        conv.decay_window = 0;

        conv.push_assistant(AssistantMessage {
            text: "x".repeat(1000),
            thinking: None,
            tool_calls: vec![],
            raw: serde_json::Value::Null,
            provider_tokens: (0, 0, 0, 0),
            provider_telemetry: None,
        });
        conv.intent.stats.turns = 1;

        let view = conv.build_llm_view();
        if let LlmMessage::Assistant { text, .. } = &view[0] {
            let combined: String = text.join("");
            assert!(
                combined.len() < 600,
                "Text should be truncated, got {} bytes",
                combined.len()
            );
            assert!(combined.contains("[truncated]"));
        } else {
            panic!("Expected Assistant message");
        }
    }

    #[test]
    fn tool_result_decay_preserves_metadata() {
        let mut conv = ConversationState::new();
        conv.decay_window = 0;

        push_matching_assistant(&mut conv, "t1");
        conv.push_tool_result(ToolResultEntry {
            call_id: "t1".into(),
            tool_name: "read".into(),
            content: vec![omegon_traits::ContentBlock::Text {
                text: "x".repeat(5000),
            }],
            is_error: false,
            args_summary: None,
        });
        conv.intent.stats.turns = 1;

        let view = conv.build_llm_view();
        // view[0] = decayed assistant, view[1] = decayed tool_result
        if let LlmMessage::ToolResult {
            content, tool_name, ..
        } = &view[1]
        {
            assert_eq!(tool_name, "read");
            // Rich decay skeleton includes line/byte counts
            assert!(
                content.contains("Read:") && content.contains("bytes"),
                "got: {content}"
            );
            // Should NOT contain the original bulk content
            assert!(!content.contains("xxxxx"), "should strip bulk content");
        } else {
            panic!("Expected ToolResult message");
        }
    }

    #[test]
    fn decay_is_turn_based_not_message_based() {
        let mut conv = ConversationState::new();
        conv.decay_window = 2; // Keep last 2 turns fresh
        conv.intent.stats.turns = 1;

        // Turn 1: push multiple messages (simulates a turn with 3 tool calls)
        conv.push_user("do something".into());
        conv.push_assistant(AssistantMessage {
            text: "I'll help".into(),
            thinking: Some("detailed thinking here...".repeat(50)),
            tool_calls: vec![],
            raw: serde_json::Value::Null,
            provider_tokens: (0, 0, 0, 0),
            provider_telemetry: None,
        });
        conv.push_tool_result(ToolResultEntry {
            call_id: "t1".into(),
            tool_name: "read".into(),
            content: vec![omegon_traits::ContentBlock::Text {
                text: "big content".repeat(100),
            }],
            is_error: false,
            args_summary: None,
        });

        // Still on turn 1 — everything should be fresh
        let view = conv.build_llm_view();
        if let LlmMessage::Assistant { thinking, .. } = &view[1] {
            assert!(
                !thinking.is_empty(),
                "Turn 1 at turn 1: should NOT be decayed"
            );
        }

        // Advance to turn 4 — turn 1 is now 3 turns old, outside decay_window=2
        conv.intent.stats.turns = 4;
        let view = conv.build_llm_view();
        if let LlmMessage::Assistant { thinking, .. } = &view[1] {
            assert!(
                thinking.is_empty(),
                "Turn 1 at turn 4: should be decayed (age 3 > window 2)"
            );
        }
    }

    #[test]
    fn session_save_load_round_trip() {
        let mut conv = ConversationState::new();
        conv.push_user("Fix the bug".into());
        conv.push_assistant(AssistantMessage {
            text: "I'll fix it".into(),
            thinking: None,
            tool_calls: vec![ToolCall {
                id: "tc1".into(),
                name: "edit".into(),
                arguments: serde_json::json!({"path": "src/foo.rs"}),
            }],
            raw: serde_json::Value::Null,
            provider_tokens: (0, 0, 0, 0),
            provider_telemetry: None,
        });
        conv.push_tool_result(ToolResultEntry {
            call_id: "tc1".into(),
            tool_name: "edit".into(),
            content: vec![omegon_traits::ContentBlock::Text {
                text: "Edited successfully".into(),
            }],
            is_error: false,
            args_summary: None,
        });
        conv.intent.stats.turns = 1;
        conv.intent.current_task = Some("Fix the auth bug".into());
        conv.intent
            .files_modified
            .insert(PathBuf::from("src/foo.rs"));

        // Save
        let tmp = std::env::temp_dir().join("omegon-test-session.json");
        conv.save_session(&tmp).unwrap();

        // Load
        let loaded = ConversationState::load_session(&tmp).unwrap();
        assert_eq!(loaded.intent.stats.turns, 1);
        assert_eq!(
            loaded.intent.current_task.as_deref(),
            Some("Fix the auth bug")
        );
        assert!(
            loaded
                .intent
                .files_modified
                .contains(&PathBuf::from("src/foo.rs"))
        );

        let view = loaded.build_llm_view();
        assert_eq!(view.len(), 3); // user + assistant + tool_result

        // Cleanup
        let _ = std::fs::remove_file(&tmp);
    }

    /// Sessions saved by older omegon versions may lack fields added later
    /// (e.g. commit_nudged). Deserialization must not fail — missing fields
    /// get their Default value.
    #[test]
    fn load_session_tolerates_missing_fields() {
        // Simulate a session file from an older version: no commit_nudged,
        // no compaction_summary, minimal stats.
        let old_format_json = serde_json::json!({
            "messages": [
                {"role": "user", "content": "hello"}
            ],
            "intent": {
                "current_task": "do stuff",
                "approach": null,
                "lifecycle_phase": "Idle",
                "files_read": [],
                "files_modified": [],
                "constraints_discovered": [],
                "failed_approaches": [],
                "open_questions": [],
                "stats": {
                    "turns": 1,
                    "tool_calls": 0,
                    "tokens_consumed": 0,
                    "compactions": 0
                }
            },
            "decay_window": 10
            // note: no "compaction_summary", no "commit_nudged"
        });

        let tmp = std::env::temp_dir().join("omegon-test-old-session.json");
        std::fs::write(&tmp, old_format_json.to_string()).unwrap();

        let loaded = ConversationState::load_session(&tmp).unwrap();
        assert_eq!(loaded.intent.current_task.as_deref(), Some("do stuff"));
        assert!(!loaded.intent.commit_nudged); // defaulted to false
        assert_eq!(loaded.intent.stats.turns, 1);

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn estimate_tokens_chars_div_4() {
        let mut conv = ConversationState::new();
        conv.push_user("hello world".into()); // 11 chars
        let tokens = conv.estimate_tokens();
        // "hello world" = 11 chars → 11/4 = 2 tokens (integer division)
        assert!(tokens >= 2 && tokens <= 4, "got {tokens}");
    }

    #[test]
    fn needs_compaction_threshold() {
        let mut conv = ConversationState::new();
        // Push a message under threshold: 400k chars → ~100k tokens, threshold at 150k
        conv.push_user("x".repeat(400_000));
        assert!(
            !conv.needs_compaction(200_000, 0.75),
            "100k tokens should be under 150k threshold"
        );
        // Push more to exceed: 800k chars → ~200k tokens, threshold at 150k
        conv.push_user("y".repeat(400_000));
        assert!(
            conv.needs_compaction(200_000, 0.75),
            "200k tokens should exceed 150k threshold"
        );
    }

    #[test]
    fn build_compaction_payload_only_evictable() {
        let mut conv = ConversationState::new();
        conv.decay_window = 2;

        // Turn 0 messages (will be evictable at turn 5)
        conv.push_user("old task".into());
        conv.push_assistant(AssistantMessage {
            text: "working on it".into(),
            thinking: None,
            tool_calls: vec![],
            raw: Value::Null,
            provider_tokens: (0, 0, 0, 0),
            provider_telemetry: None,
        });

        // Advance to turn 5 so turn-0 messages are outside decay window
        conv.intent.stats.turns = 5;
        conv.push_user("new task".into());

        let (payload, count) = conv.build_compaction_payload().unwrap();
        assert_eq!(count, 2, "Should evict 2 old messages");
        assert!(payload.contains("old task"));
        assert!(
            !payload.contains("new task"),
            "Recent messages should not be in payload"
        );
    }

    #[test]
    fn apply_compaction_evicts_and_sets_summary() {
        let mut conv = ConversationState::new();
        conv.decay_window = 2;

        // Old messages
        conv.push_user("old".into());
        conv.push_assistant(AssistantMessage {
            text: "old reply".into(),
            thinking: None,
            tool_calls: vec![],
            raw: Value::Null,
            provider_tokens: (0, 0, 0, 0),
            provider_telemetry: None,
        });

        // Advance and add recent
        conv.intent.stats.turns = 5;
        conv.push_user("recent".into());

        conv.apply_compaction("Summary of old conversation.".into());

        assert_eq!(conv.intent.stats.compactions, 1);
        assert!(conv.compaction_summary.is_some());
        // Old messages should be evicted
        assert_eq!(
            conv.canonical.len(),
            1,
            "Only the recent message should remain"
        );

        // The LLM view should have the summary merged with the recent message
        // (both are User role, so role alternation merges them)
        let view = conv.build_llm_view();
        assert_eq!(view.len(), 1); // merged summary + recent into one user message
        if let LlmMessage::User { content, .. } = &view[0] {
            assert!(content.contains("Summary of old conversation"));
            assert!(content.contains("recent"));
        }
    }

    #[test]
    fn render_intent_for_injection() {
        let mut conv = ConversationState::new();
        conv.intent.current_task = Some("Fix auth flow".into());
        conv.intent.approach = Some("Token rotation".into());
        conv.intent
            .files_modified
            .insert(PathBuf::from("src/auth.rs"));
        conv.intent
            .constraints_discovered
            .push("30-minute TTL".into());
        conv.intent.failed_approaches.push(FailedApproach {
            description: "Direct replacement".into(),
            reason: "Cache holds stale refs".into(),
            turn: 5,
        });
        conv.intent.stats.turns = 10;
        conv.intent.stats.tool_calls = 25;

        let block = conv.render_intent_for_injection();
        assert!(block.contains("Fix auth flow"));
        assert!(block.contains("Token rotation"));
        assert!(block.contains("src/auth.rs"));
        assert!(block.contains("30-minute TTL"));
        assert!(block.contains("Direct replacement"));
        assert!(block.contains("Cache holds stale refs"));
    }

    #[test]
    fn intent_tracks_files_from_tool_calls() {
        let mut intent = IntentDocument::default();
        let calls = vec![
            ToolCall {
                id: "1".into(),
                name: "read".into(),
                arguments: serde_json::json!({"path": "src/foo.rs"}),
            },
            ToolCall {
                id: "2".into(),
                name: "edit".into(),
                arguments: serde_json::json!({"path": "src/bar.rs"}),
            },
            ToolCall {
                id: "3".into(),
                name: "write".into(),
                arguments: serde_json::json!({"path": "src/new.rs"}),
            },
            ToolCall {
                id: "4".into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command": "ls"}),
            },
        ];
        intent.update_from_tools(&calls, &[]);
        assert!(intent.files_read.contains(&PathBuf::from("src/foo.rs")));
        assert!(intent.files_modified.contains(&PathBuf::from("src/bar.rs")));
        assert!(intent.files_modified.contains(&PathBuf::from("src/new.rs")));
        assert_eq!(intent.files_read.len(), 1);
        assert_eq!(intent.files_modified.len(), 2);
        assert_eq!(intent.stats.tool_calls, 4);
    }

    #[test]
    fn commit_clears_files_modified() {
        // Regression: commit tool must clear files_modified so the end-of-turn
        // nudge ("You made file changes but did not run git commit") does not
        // fire spuriously after the agent already committed.
        let mut intent = IntentDocument::default();

        // Simulate: agent edits a file, then commits
        intent.update_from_tools(
            &[ToolCall {
                id: "1".into(),
                name: "edit".into(),
                arguments: serde_json::json!({"path": "src/foo.rs"}),
            }],
            &[],
        );
        assert!(!intent.files_modified.is_empty(), "should have a mutation");

        intent.update_from_tools(
            &[ToolCall {
                id: "2".into(),
                name: "commit".into(),
                arguments: serde_json::json!({"message": "fix: foo"}),
            }],
            &[],
        );
        assert!(
            intent.files_modified.is_empty(),
            "commit must clear files_modified to suppress spurious nudge"
        );
    }

    #[test]
    fn auto_task_from_first_user_message() {
        let mut conv = ConversationState::new();
        assert!(conv.intent.current_task.is_none());

        conv.push_user("Fix the authentication bug in src/auth.rs".into());
        assert_eq!(
            conv.intent.current_task.as_deref(),
            Some("Fix the authentication bug in src/auth.rs")
        );

        // Second user message should NOT overwrite the task
        conv.push_user("Also fix the tests".into());
        assert_eq!(
            conv.intent.current_task.as_deref(),
            Some("Fix the authentication bug in src/auth.rs")
        );
    }

    #[test]
    fn system_messages_dont_set_task() {
        let mut conv = ConversationState::new();
        conv.push_user("[System: You've been running for 35 turns.]".into());
        assert!(
            conv.intent.current_task.is_none(),
            "system messages should not set task"
        );

        conv.push_user("Now do the real work".into());
        assert_eq!(
            conv.intent.current_task.as_deref(),
            Some("Now do the real work")
        );
    }

    #[test]
    fn constraint_deduplication() {
        let mut intent = IntentDocument::default();
        intent.add_constraint("OAuth tokens expire in 30 minutes");
        intent.add_constraint("OAuth tokens expire in 30 minutes");
        intent.add_constraint("  OAuth tokens expire in 30 minutes  ");
        assert_eq!(intent.constraints_discovered.len(), 1);

        intent.add_constraint("Different constraint");
        assert_eq!(intent.constraints_discovered.len(), 2);
    }

    #[test]
    fn question_deduplication() {
        let mut intent = IntentDocument::default();
        intent.add_question("How does caching work?");
        intent.add_question("How does caching work?");
        assert_eq!(intent.open_questions.len(), 1);
    }

    #[test]
    fn empty_constraint_ignored() {
        let mut intent = IntentDocument::default();
        intent.add_constraint("");
        intent.add_constraint("   ");
        assert!(intent.constraints_discovered.is_empty());
    }

    #[test]
    fn decay_bash_preserves_tail() {
        let mut conv = ConversationState::new();
        conv.decay_window = 0;

        push_matching_assistant(&mut conv, "t1");
        let output = (1..=20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        conv.push_tool_result(ToolResultEntry {
            call_id: "t1".into(),
            tool_name: "bash".into(),
            content: vec![omegon_traits::ContentBlock::Text { text: output }],
            is_error: false,
            args_summary: None,
        });
        conv.intent.stats.turns = 1;

        let view = conv.build_llm_view();
        if let LlmMessage::ToolResult { content, .. } = &view[1] {
            assert!(
                content.contains("20 lines"),
                "should report line count, got: {content}"
            );
            assert!(content.contains("line 20"), "should preserve tail");
            assert!(!content.contains("line 5"), "should strip middle");
        }
    }

    #[test]
    fn slim_decay_preserves_path_bearing_generic_results() {
        let mut conv = ConversationState::new();
        conv.set_slim_mode(true);
        conv.decay_window = 0;

        push_matching_assistant(&mut conv, "t1");
        conv.push_tool_result(ToolResultEntry {
            call_id: "t1".into(),
            tool_name: "codebase_search".into(),
            content: vec![omegon_traits::ContentBlock::Text {
                text: "found src/auth.rs and src/setup.rs".into(),
            }],
            is_error: false,
            args_summary: Some("query: anthropic auth persistence".into()),
        });
        conv.intent.stats.turns = 1;

        let view = conv.build_llm_view();
        if let LlmMessage::ToolResult { content, .. } = &view[1] {
            assert!(content.contains("query: anthropic auth persistence"));
        }
    }

    #[test]
    fn decay_error_preserves_message() {
        let mut conv = ConversationState::new();
        conv.decay_window = 0;

        push_matching_assistant(&mut conv, "t1");
        conv.push_tool_result(ToolResultEntry {
            call_id: "t1".into(),
            tool_name: "bash".into(),
            content: vec![omegon_traits::ContentBlock::Text {
                text: "command not found: foobar".into(),
            }],
            is_error: true,
            args_summary: None,
        });
        conv.intent.stats.turns = 1;

        let view = conv.build_llm_view();
        if let LlmMessage::ToolResult { content, .. } = &view[1] {
            assert!(content.contains("ERROR"), "should indicate error");
            assert!(
                content.contains("command not found"),
                "should preserve error text"
            );
        }
    }

    #[test]
    fn decay_edit_preserves_path_info() {
        let mut conv = ConversationState::new();
        conv.decay_window = 0;

        push_matching_assistant(&mut conv, "t1");
        conv.push_tool_result(ToolResultEntry {
            call_id: "t1".into(),
            tool_name: "edit".into(),
            content: vec![omegon_traits::ContentBlock::Text {
                text: "Successfully replaced text in src/auth.rs".into(),
            }],
            is_error: false,
            args_summary: None,
        });
        conv.intent.stats.turns = 1;

        let view = conv.build_llm_view();
        if let LlmMessage::ToolResult { content, .. } = &view[1] {
            assert!(
                content.contains("src/auth.rs"),
                "should preserve path, got: {content}"
            );
        }
    }

    #[test]
    fn referenced_results_decay_slower() {
        let mut conv = ConversationState::new();
        conv.decay_window = 2;
        conv.intent.stats.turns = 1;

        // Turn 1: assistant calls a tool, then we get the result
        push_matching_assistant(&mut conv, "t1");
        conv.push_tool_result(ToolResultEntry {
            call_id: "t1".into(),
            tool_name: "read".into(),
            content: vec![omegon_traits::ContentBlock::Text {
                text: "pub fn authenticate_user(token: AuthToken) -> Result<User> {\n    validate_token(token)\n}".into(),
            }],
            is_error: false,
            args_summary: None,
        });

        // Turn 2: assistant references the function name
        conv.intent.stats.turns = 2;
        conv.push_assistant(AssistantMessage {
            text: "I can see the authenticate_user function validates tokens.".into(),
            thinking: None,
            tool_calls: vec![],
            raw: Value::Null,
            provider_tokens: (0, 0, 0, 0),
            provider_telemetry: None,
        });

        // Turn 1's tool result should now be in referenced_turns
        assert!(
            conv.referenced_turns.contains(&1),
            "turn 1 should be marked as referenced"
        );

        // At turn 5 (age 4 for turn-1 result), with decay_window=2:
        // Unreferenced: 4 > 2 → decayed
        // Referenced: 4 > 4 (2*2) → NOT decayed
        conv.intent.stats.turns = 5;
        let view = conv.build_llm_view();
        // The tool result at turn 1 should NOT be decayed (referenced, extended window = 4)
        let tool_msg = view
            .iter()
            .find(|m| matches!(m, LlmMessage::ToolResult { .. }))
            .unwrap();
        if let LlmMessage::ToolResult { content, .. } = tool_msg {
            assert!(
                content.contains("authenticate_user"),
                "referenced result should preserve full content at age 4, got: {content}"
            );
        }

        // At turn 6 (age 5), even referenced results should decay (5 > 4)
        conv.intent.stats.turns = 6;
        let view = conv.build_llm_view();
        let tool_msg = view
            .iter()
            .find(|m| matches!(m, LlmMessage::ToolResult { .. }))
            .unwrap();
        if let LlmMessage::ToolResult { content, .. } = tool_msg {
            assert!(
                !content.contains("authenticate_user"),
                "referenced result should be decayed at age 5"
            );
        }
    }

    #[test]
    fn change_tool_tracks_multi_file_edits() {
        let mut intent = IntentDocument::default();
        let calls = vec![ToolCall {
            id: "1".into(),
            name: "change".into(),
            arguments: serde_json::json!({
                "edits": [
                    {"file": "src/a.rs", "old": "x", "new": "y"},
                    {"file": "src/b.rs", "old": "x", "new": "y"},
                ]
            }),
        }];
        intent.update_from_tools(&calls, &[]);
        assert!(intent.files_modified.contains(&PathBuf::from("src/a.rs")));
        assert!(intent.files_modified.contains(&PathBuf::from("src/b.rs")));
    }

    #[test]
    fn ambient_phase_capture() {
        let mut conv = ConversationState::new();
        let captures = vec![crate::lifecycle::capture::AmbientCapture::Phase(
            "implement".into(),
        )];
        conv.apply_ambient_captures(&captures);
        assert!(matches!(
            conv.intent.lifecycle_phase,
            omegon_traits::LifecyclePhase::Implementing { .. }
        ));
    }

    #[test]
    fn ambient_capture_deduplicates() {
        let mut conv = ConversationState::new();
        let captures = vec![
            crate::lifecycle::capture::AmbientCapture::Constraint("same thing".into()),
            crate::lifecycle::capture::AmbientCapture::Constraint("same thing".into()),
            crate::lifecycle::capture::AmbientCapture::Failed {
                description: "approach A".into(),
                reason: "didn't work".into(),
            },
            crate::lifecycle::capture::AmbientCapture::Failed {
                description: "approach A".into(),
                reason: "still doesn't work".into(),
            },
        ];
        conv.apply_ambient_captures(&captures);
        assert_eq!(conv.intent.constraints_discovered.len(), 1);
        assert_eq!(conv.intent.failed_approaches.len(), 1);
    }

    #[test]
    fn args_summary_survives_session_round_trip() {
        let mut conv = ConversationState::new();
        conv.push_user("read a file".into());
        push_matching_assistant(&mut conv, "t1");
        conv.push_tool_result(ToolResultEntry {
            call_id: "t1".into(),
            tool_name: "read".into(),
            content: vec![omegon_traits::ContentBlock::Text {
                text: "file contents".into(),
            }],
            is_error: false,
            args_summary: Some("src/auth.rs".into()),
        });
        conv.intent.stats.turns = 1;

        let tmp = std::env::temp_dir().join("omegon-test-args-summary-session.json");
        conv.save_session(&tmp).unwrap();

        let loaded = ConversationState::load_session(&tmp).unwrap();
        // After load, verify the args_summary survived
        let view = loaded.build_llm_view();
        // Find the tool result in the view
        let tool_msg = view
            .iter()
            .find(|m| matches!(m, LlmMessage::ToolResult { .. }));
        assert!(tool_msg.is_some(), "should have a tool result in the view");
        if let Some(LlmMessage::ToolResult { args_summary, .. }) = tool_msg {
            assert_eq!(
                args_summary.as_deref(),
                Some("src/auth.rs"),
                "args_summary should survive round-trip"
            );
        }

        // Now advance turns so it decays, verify the skeleton includes the path
        let mut loaded = ConversationState::load_session(&tmp).unwrap();
        loaded.decay_window = 0;
        loaded.intent.stats.turns = 100; // Force decay
        let view = loaded.build_llm_view();
        if let Some(LlmMessage::ToolResult { content, .. }) = view
            .iter()
            .find(|m| matches!(m, LlmMessage::ToolResult { .. }))
        {
            assert!(
                content.contains("src/auth.rs"),
                "decayed skeleton should include path from args_summary, got: {content}"
            );
        }

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn loaded_session_messages_stay_within_decay_window() {
        let mut conv = ConversationState::new();
        conv.intent.stats.turns = 10;
        conv.push_user("old task".into());
        conv.push_assistant(AssistantMessage {
            text: "long response with thinking".into(),
            thinking: Some("deep reasoning here".into()),
            tool_calls: vec![],
            raw: serde_json::Value::Null,
            provider_tokens: (0, 0, 0, 0),
            provider_telemetry: None,
        });

        let tmp = std::env::temp_dir().join("omegon-test-decay-session.json");
        conv.save_session(&tmp).unwrap();

        let loaded = ConversationState::load_session(&tmp).unwrap();
        let view = loaded.build_llm_view();
        if let LlmMessage::Assistant { thinking, .. } = &view[1] {
            assert!(
                !thinking.is_empty(),
                "thinking should be preserved after load, got empty"
            );
        } else {
            panic!("expected assistant message at index 1");
        }

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn load_session_compacts_large_history_into_summary_plus_tail() {
        let mut conv = ConversationState::new();
        conv.intent.stats.turns = 20;
        for i in 0..20 {
            conv.push_user(format!("user-{i}"));
            conv.push_assistant(AssistantMessage {
                text: format!("assistant-{i}"),
                thinking: None,
                tool_calls: vec![],
                raw: serde_json::Value::Null,
                provider_tokens: (0, 0, 0, 0),
                provider_telemetry: None,
            });
        }

        let tmp = std::env::temp_dir().join("omegon-test-load-compacts-session.json");
        conv.save_session(&tmp).unwrap();

        let loaded = ConversationState::load_session(&tmp).unwrap();
        let view = loaded.build_llm_view();
        assert!(
            loaded.compaction_summary.is_some(),
            "large resumed sessions should synthesize a summary"
        );
        assert!(
            view.len() < 40,
            "resumed view should not pull the full prior session back in"
        );
        if let Some(LlmMessage::User { content, .. }) = view.first() {
            assert!(
                content.contains("Resumed session"),
                "summary header should explain the compaction: {content}"
            );
        } else {
            panic!("expected synthesized summary as first message");
        }

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn role_alternation_merges_adjacent_users() {
        let mut msgs = vec![
            LlmMessage::User {
                content: "hello".into(),
                images: vec![crate::bridge::ImageAttachment {
                    data: "aaa".into(),
                    media_type: "image/png".into(),
                    source_path: Some("/tmp/hello.png".into()),
                }],
            },
            LlmMessage::User {
                content: "world".into(),
                images: vec![crate::bridge::ImageAttachment {
                    data: "bbb".into(),
                    media_type: "image/jpeg".into(),
                    source_path: Some("/tmp/world.jpg".into()),
                }],
            },
        ];
        enforce_role_alternation(&mut msgs);
        assert_eq!(msgs.len(), 1);
        if let LlmMessage::User { content, images } = &msgs[0] {
            assert!(content.contains("hello"));
            assert!(content.contains("world"));
            assert_eq!(images.len(), 2);
            assert_eq!(images[0].media_type, "image/png");
            assert_eq!(images[1].media_type, "image/jpeg");
        }
    }

    #[test]
    fn build_llm_view_preserves_user_images() {
        let mut conv = ConversationState::new();
        conv.push_user_with_images(
            "describe this".into(),
            vec![crate::bridge::ImageAttachment {
                data: "abc123".into(),
                media_type: "image/png".into(),
                source_path: Some("/tmp/describe-this.png".into()),
            }],
        );

        let view = conv.build_llm_view();
        assert_eq!(view.len(), 1);
        if let LlmMessage::User { content, images } = &view[0] {
            assert_eq!(content, "describe this");
            assert_eq!(images.len(), 1);
            assert_eq!(images[0].media_type, "image/png");
            assert_eq!(
                images[0].source_path.as_deref(),
                Some("/tmp/describe-this.png")
            );
        } else {
            panic!("expected user message");
        }
    }

    #[test]
    fn save_and_load_session_preserves_user_images() {
        let mut conv = ConversationState::new();
        conv.push_user_with_images(
            "describe this".into(),
            vec![crate::bridge::ImageAttachment {
                data: "abc123".into(),
                media_type: "image/png".into(),
                source_path: Some("/tmp/saved-image.png".into()),
            }],
        );

        let tmp = std::env::temp_dir().join("omegon-test-load-session-images.json");
        conv.save_session(&tmp).unwrap();
        let loaded = ConversationState::load_session(&tmp).unwrap();
        let view = loaded.build_llm_view();
        let _ = std::fs::remove_file(&tmp);

        if let LlmMessage::User { content, images } = &view[0] {
            assert_eq!(content, "describe this");
            assert_eq!(images.len(), 1);
            assert_eq!(images[0].media_type, "image/png");
            assert_eq!(
                images[0].source_path.as_deref(),
                Some("/tmp/saved-image.png")
            );
        } else {
            panic!("expected user message");
        }
    }

    #[test]
    fn attachment_context_injection_uses_source_paths_without_touching_user_text() {
        let mut conv = ConversationState::new();
        conv.push_user_with_images(
            "show me the image again".into(),
            vec![crate::bridge::ImageAttachment {
                data: "abc123".into(),
                media_type: "image/png".into(),
                source_path: Some("/tmp/redisplay.png".into()),
            }],
        );

        let injection = conv
            .render_attachment_context_injection()
            .expect("attachment manifest injection");
        assert!(injection.contains("[Attachment files]"));
        assert!(injection.contains("[image0] /tmp/redisplay.png"));
        assert!(injection.contains("Do not quote this manifest back"));

        let view = conv.build_llm_view();
        match &view[0] {
            LlmMessage::User { content, .. } => assert_eq!(content, "show me the image again"),
            _ => panic!("expected user message"),
        }
    }

    #[test]
    fn role_alternation_drops_orphaned_tool_result_after_user() {
        let mut msgs = vec![
            LlmMessage::User {
                content: "test".into(),
                images: vec![],
            },
            LlmMessage::ToolResult {
                call_id: "t1".into(),
                tool_name: "bash".into(),
                content: "output".into(),
                args_summary: None,
                is_error: false,
            },
        ];
        enforce_role_alternation(&mut msgs);
        assert_eq!(msgs.len(), 1);
        assert!(matches!(&msgs[0], LlmMessage::User { .. }));
    }

    #[test]
    fn orphan_stripping_plus_alternation_produces_valid_sequence() {
        let mut conv = ConversationState::new();
        conv.push_user("do something".into());
        push_matching_assistant(&mut conv, "t1");
        conv.push_tool_result(ToolResultEntry {
            call_id: "t1".into(),
            tool_name: "bash".into(),
            content: vec![omegon_traits::ContentBlock::Text { text: "ok".into() }],
            is_error: false,
            args_summary: None,
        });
        conv.push_user("continue".into());
        conv.intent.stats.turns = 1;

        let view = conv.build_llm_view();
        assert!(view.len() >= 3, "got {} messages", view.len());
        assert!(matches!(&view[0], LlmMessage::User { .. }));
        assert!(matches!(&view[1], LlmMessage::Assistant { .. }));
        assert!(matches!(&view[2], LlmMessage::ToolResult { .. }));
    }

    #[test]
    fn last_provider_telemetry_returns_latest_matching_snapshot() {
        let mut conv = ConversationState::new();
        conv.canonical.push(AgentMessage::Assistant(
            AssistantMessage {
                text: "first".into(),
                provider_telemetry: Some(omegon_traits::ProviderTelemetrySnapshot {
                    provider: "anthropic".into(),
                    source: "response_headers".into(),
                    unified_5h_utilization_pct: Some(72.0),
                    ..Default::default()
                }),
                ..Default::default()
            },
            1,
        ));
        conv.canonical.push(AgentMessage::Assistant(
            AssistantMessage {
                text: "second".into(),
                provider_telemetry: Some(omegon_traits::ProviderTelemetrySnapshot {
                    provider: "anthropic".into(),
                    source: "response_headers".into(),
                    unified_5h_utilization_pct: Some(97.0),
                    ..Default::default()
                }),
                ..Default::default()
            },
            2,
        ));

        let latest = conv
            .last_provider_telemetry(Some("anthropic"))
            .expect("latest telemetry");
        assert_eq!(latest.unified_5h_utilization_pct, Some(97.0));
    }

    #[test]
    fn decay_oldest_removes_from_front() {
        let mut conv = ConversationState::new();
        conv.push_user("a".into());
        conv.push_user("b".into());
        conv.push_user("c".into());
        assert_eq!(conv.message_count(), 3);
        conv.decay_oldest(2);
        assert_eq!(conv.message_count(), 1);
    }
}
