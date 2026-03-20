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
    User { text: String, turn: u32 },
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
pub struct IntentDocument {
    pub current_task: Option<String>,
    pub approach: Option<String>,
    pub lifecycle_phase: LifecyclePhase,

    pub files_read: IndexSet<PathBuf>,
    pub files_modified: IndexSet<PathBuf>,

    pub constraints_discovered: Vec<String>,
    pub failed_approaches: Vec<FailedApproach>,
    pub open_questions: Vec<String>,

    pub stats: SessionStatsAccumulator,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FailedApproach {
    pub description: String,
    pub reason: String,
    pub turn: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
        if !normalized.is_empty()
            && !self.constraints_discovered.iter().any(|c| c == normalized)
        {
            self.constraints_discovered.push(normalized.to_string());
        }
    }

    /// Add an open question, deduplicating against existing entries.
    pub fn add_question(&mut self, text: &str) {
        let normalized = text.trim();
        if !normalized.is_empty()
            && !self.open_questions.iter().any(|q| q == normalized)
        {
            self.open_questions.push(normalized.to_string());
        }
    }
}

/// Serializable session snapshot for save/resume.
#[derive(Debug, Serialize, Deserialize)]
struct SessionSnapshot {
    messages: Vec<LlmMessage>,
    intent: IntentDocument,
    decay_window: usize,
    #[serde(default)]
    compaction_summary: Option<String>,
}

/// The full conversation state.
pub struct ConversationState {
    /// Canonical, unmodified history. Source of truth for persistence.
    canonical: Vec<AgentMessage>,

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

    /// Pending image attachments from clipboard paste — consumed on next LLM call.
    pub pending_images: Vec<crate::bridge::ImageAttachment>,
}

impl ConversationState {
    pub fn new() -> Self {
        Self {
            canonical: Vec::new(),
            intent: IntentDocument::default(),
            decay_window: 10,
            referenced_turns: std::collections::HashSet::new(),
            compaction_summary: None,
            pending_images: Vec::new(),
        }
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
        let evictable: Vec<&AgentMessage> = self.canonical.iter()
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
                AgentMessage::User { text, turn } => {
                    payload.push_str(&format!("[Turn {turn}] User: {text}\n\n"));
                }
                AgentMessage::Assistant(a, turn) => {
                    let truncated = if a.text.len() > 200 {
                        format!("{}...", &a.text[..200])
                    } else {
                        a.text.clone()
                    };
                    payload.push_str(&format!("[Turn {turn}] Assistant: {truncated}\n"));
                    if !a.tool_calls.is_empty() {
                        let tools: Vec<_> = a.tool_calls.iter().map(|tc| tc.name.as_str()).collect();
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
    pub fn apply_compaction(&mut self, summary: String) {
        let current_turn = self.intent.stats.turns;
        // Remove all messages older than the decay window
        self.canonical.retain(|m| {
            current_turn.saturating_sub(m.turn()) <= self.decay_window as u32
        });
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
            let files: Vec<_> = intent.files_modified.iter()
                .map(|p| p.display().to_string()).collect();
            lines.push(format!("Files modified: {}", files.join(", ")));
        }
        if !intent.constraints_discovered.is_empty() {
            lines.push(format!("Constraints: {}", intent.constraints_discovered.join("; ")));
        }
        if !intent.failed_approaches.is_empty() {
            lines.push("Failed approaches:".to_string());
            for fa in &intent.failed_approaches {
                lines.push(format!("  - {}: {} (turn {})", fa.description, fa.reason, fa.turn));
            }
        }
        lines.push(format!(
            "Stats: {} turns, {} tool calls, {} compactions",
            intent.stats.turns, intent.stats.tool_calls, intent.stats.compactions
        ));

        lines.join("\n")
    }

    pub fn push_user(&mut self, text: String) {
        let turn = self.intent.stats.turns;
        // Auto-populate current_task from the first non-system user message
        if !text.starts_with("[System:") {
            self.intent.set_task_from_prompt(&text);
        }
        self.canonical.push(AgentMessage::User { text, turn });
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

        // Attach pending images to the last user message
        if !self.pending_images.is_empty()
            && let Some(LlmMessage::User { images, .. }) = messages.iter_mut().rev().find(|m| matches!(m, LlmMessage::User { .. }))
        {
            *images = self.pending_images.clone();
        }

        messages
    }

    /// Clear pending images after they've been sent to the LLM.
    pub fn clear_pending_images(&mut self) {
        self.pending_images.clear();
    }

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
                        "decompose" | "decomposing" => {
                            omegon_traits::LifecyclePhase::Decomposing
                        }
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
            // User messages are small — don't decay
            AgentMessage::User { text, .. } => LlmMessage::User {
                content: text.clone(),
                images: vec![],
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

    /// Load a previously saved session. The loaded messages become the
    /// canonical history (since they were already decay-processed at save time).
    pub fn load_session(path: &Path) -> anyhow::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let snapshot: SessionSnapshot = serde_json::from_str(&json)?;
        tracing::info!(
            path = %path.display(),
            turns = snapshot.intent.stats.turns,
            messages = snapshot.messages.len(),
            "session loaded"
        );

        // Reconstruct canonical from the saved LLM view.
        // Assign all messages to the last turn so they stay within the decay window
        // on the next build_llm_view() call. (Original per-message turns are lost
        // through the LLM view serialization, but that's acceptable — the saved
        // view was already decay-processed at save time.)
        let last_turn = snapshot.intent.stats.turns;
        let canonical: Vec<AgentMessage> = snapshot
            .messages
            .into_iter()
            .map(|msg| {
                let turn = last_turn;
                match msg {
                    LlmMessage::User { content, .. } => AgentMessage::User { text: content, turn },
                    LlmMessage::Assistant { text, thinking, tool_calls, raw } => {
                        AgentMessage::Assistant(
                            AssistantMessage {
                                text: text.join("\n"),
                                thinking: if thinking.is_empty() { None } else { Some(thinking.join("\n")) },
                                tool_calls: tool_calls.into_iter().map(|tc| ToolCall {
                                    id: tc.id,
                                    name: tc.name,
                                    arguments: tc.arguments,
                                }).collect(),
                                raw: raw.unwrap_or(Value::Null),
                            },
                            turn,
                        )
                    }
                    LlmMessage::ToolResult { call_id, tool_name, content, is_error, args_summary } => {
                        AgentMessage::ToolResult(
                            ToolResultEntry {
                                call_id,
                                tool_name,
                                content: vec![omegon_traits::ContentBlock::Text { text: content }],
                                is_error,
                                args_summary,
                            },
                            turn,
                        )
                    }
                }
            })
            .collect();

        Ok(Self {
            canonical,
            intent: snapshot.intent,
            decay_window: snapshot.decay_window,
            referenced_turns: std::collections::HashSet::new(),
            compaction_summary: snapshot.compaction_summary,
            pending_images: Vec::new(),
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

        if result.is_error {
            // Preserve error message — errors are high-signal
            let error_preview = if text.len() > 300 {
                let mut end = 300;
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
                format!("[Read{ctx_suffix}: {lines} lines, {bytes} bytes]")
            }
            "bash" | "execute" => {
                let lines = text.lines().count();
                let exit_hint = if text.contains("exit code") || text.contains("exited with") {
                    " (non-zero exit)"
                } else {
                    ""
                };
                let tail: Vec<&str> = text.lines().rev().take(3).collect();
                let tail_str: String = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
                if lines <= 5 {
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
                let preview = if first_line.len() > 120 {
                    let mut end = 120;
                    while end > 0 && !first_line.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}…", &first_line[..end])
                } else {
                    first_line.to_string()
                };
                if lines <= 3 {
                    format!("[{}{ctx_suffix}: {}]", result.tool_name, text.trim())
                } else {
                    format!("[{}{ctx_suffix}: {lines} lines. {preview}]", result.tool_name)
                }
            }
        }
    }

    /// Convert a canonical message to Omegon's wire format.
    fn to_llm_message(&self, msg: &AgentMessage) -> LlmMessage {
        match msg {
            AgentMessage::User { text, .. } => LlmMessage::User {
                content: text.clone(),
                images: vec![],
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
        });
        conv.intent.stats.turns = 1;

        let view = conv.build_llm_view();
        if let LlmMessage::Assistant { text, .. } = &view[0] {
            let combined: String = text.join("");
            assert!(combined.len() < 600, "Text should be truncated, got {} bytes", combined.len());
            assert!(combined.contains("[truncated]"));
        } else {
            panic!("Expected Assistant message");
        }
    }

    #[test]
    fn tool_result_decay_preserves_metadata() {
        let mut conv = ConversationState::new();
        conv.decay_window = 0;

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
        if let LlmMessage::ToolResult { content, tool_name, .. } = &view[0] {
            assert_eq!(tool_name, "read");
            // Rich decay skeleton includes line/byte counts
            assert!(content.contains("Read:") && content.contains("bytes"), "got: {content}");
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
        });
        conv.push_tool_result(ToolResultEntry {
            call_id: "t1".into(),
            tool_name: "read".into(),
            content: vec![omegon_traits::ContentBlock::Text { text: "big content".repeat(100) }],
            is_error: false,
            args_summary: None,
        });

        // Still on turn 1 — everything should be fresh
        let view = conv.build_llm_view();
        if let LlmMessage::Assistant { thinking, .. } = &view[1] {
            assert!(!thinking.is_empty(), "Turn 1 at turn 1: should NOT be decayed");
        }

        // Advance to turn 4 — turn 1 is now 3 turns old, outside decay_window=2
        conv.intent.stats.turns = 4;
        let view = conv.build_llm_view();
        if let LlmMessage::Assistant { thinking, .. } = &view[1] {
            assert!(thinking.is_empty(), "Turn 1 at turn 4: should be decayed (age 3 > window 2)");
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
        });
        conv.push_tool_result(ToolResultEntry {
            call_id: "tc1".into(),
            tool_name: "edit".into(),
            content: vec![omegon_traits::ContentBlock::Text { text: "Edited successfully".into() }],
            is_error: false,
            args_summary: None,
        });
        conv.intent.stats.turns = 1;
        conv.intent.current_task = Some("Fix the auth bug".into());
        conv.intent.files_modified.insert(PathBuf::from("src/foo.rs"));

        // Save
        let tmp = std::env::temp_dir().join("omegon-test-session.json");
        conv.save_session(&tmp).unwrap();

        // Load
        let loaded = ConversationState::load_session(&tmp).unwrap();
        assert_eq!(loaded.intent.stats.turns, 1);
        assert_eq!(loaded.intent.current_task.as_deref(), Some("Fix the auth bug"));
        assert!(loaded.intent.files_modified.contains(&PathBuf::from("src/foo.rs")));

        let view = loaded.build_llm_view();
        assert_eq!(view.len(), 3); // user + assistant + tool_result

        // Cleanup
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
        assert!(!conv.needs_compaction(200_000, 0.75), "100k tokens should be under 150k threshold");
        // Push more to exceed: 800k chars → ~200k tokens, threshold at 150k
        conv.push_user("y".repeat(400_000));
        assert!(conv.needs_compaction(200_000, 0.75), "200k tokens should exceed 150k threshold");
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
        });

        // Advance to turn 5 so turn-0 messages are outside decay window
        conv.intent.stats.turns = 5;
        conv.push_user("new task".into());

        let (payload, count) = conv.build_compaction_payload().unwrap();
        assert_eq!(count, 2, "Should evict 2 old messages");
        assert!(payload.contains("old task"));
        assert!(!payload.contains("new task"), "Recent messages should not be in payload");
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
        });

        // Advance and add recent
        conv.intent.stats.turns = 5;
        conv.push_user("recent".into());

        conv.apply_compaction("Summary of old conversation.".into());

        assert_eq!(conv.intent.stats.compactions, 1);
        assert!(conv.compaction_summary.is_some());
        // Old messages should be evicted
        assert_eq!(conv.canonical.len(), 1, "Only the recent message should remain");

        // The LLM view should have the summary + the recent message
        let view = conv.build_llm_view();
        assert_eq!(view.len(), 2); // summary pseudo-message + recent
        if let LlmMessage::User { content, .. } = &view[0] {
            assert!(content.contains("Summary of old conversation"));
            assert!(content.contains("[Intent"));
        }
    }

    #[test]
    fn render_intent_for_injection() {
        let mut conv = ConversationState::new();
        conv.intent.current_task = Some("Fix auth flow".into());
        conv.intent.approach = Some("Token rotation".into());
        conv.intent.files_modified.insert(PathBuf::from("src/auth.rs"));
        conv.intent.constraints_discovered.push("30-minute TTL".into());
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
            ToolCall { id: "1".into(), name: "read".into(), arguments: serde_json::json!({"path": "src/foo.rs"}) },
            ToolCall { id: "2".into(), name: "edit".into(), arguments: serde_json::json!({"path": "src/bar.rs"}) },
            ToolCall { id: "3".into(), name: "write".into(), arguments: serde_json::json!({"path": "src/new.rs"}) },
            ToolCall { id: "4".into(), name: "bash".into(), arguments: serde_json::json!({"command": "ls"}) },
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
        assert!(conv.intent.current_task.is_none(), "system messages should not set task");

        conv.push_user("Now do the real work".into());
        assert_eq!(conv.intent.current_task.as_deref(), Some("Now do the real work"));
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

        let output = (1..=20).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        conv.push_tool_result(ToolResultEntry {
            call_id: "t1".into(),
            tool_name: "bash".into(),
            content: vec![omegon_traits::ContentBlock::Text { text: output }],
            is_error: false,
            args_summary: None,
        });
        conv.intent.stats.turns = 1;

        let view = conv.build_llm_view();
        if let LlmMessage::ToolResult { content, .. } = &view[0] {
            assert!(content.contains("20 lines"), "should report line count, got: {content}");
            assert!(content.contains("line 20"), "should preserve tail");
            assert!(!content.contains("line 5"), "should strip middle");
        }
    }

    #[test]
    fn decay_error_preserves_message() {
        let mut conv = ConversationState::new();
        conv.decay_window = 0;

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
        if let LlmMessage::ToolResult { content, .. } = &view[0] {
            assert!(content.contains("ERROR"), "should indicate error");
            assert!(content.contains("command not found"), "should preserve error text");
        }
    }

    #[test]
    fn decay_edit_preserves_path_info() {
        let mut conv = ConversationState::new();
        conv.decay_window = 0;

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
        if let LlmMessage::ToolResult { content, .. } = &view[0] {
            assert!(content.contains("src/auth.rs"), "should preserve path, got: {content}");
        }
    }

    #[test]
    fn referenced_results_decay_slower() {
        let mut conv = ConversationState::new();
        conv.decay_window = 2;
        conv.intent.stats.turns = 1;

        // Turn 1: read a file with identifiable content
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
        });

        // Turn 1's tool result should now be in referenced_turns
        assert!(conv.referenced_turns.contains(&1), "turn 1 should be marked as referenced");

        // At turn 5 (age 4 for turn-1 result), with decay_window=2:
        // Unreferenced: 4 > 2 → decayed
        // Referenced: 4 > 4 (2*2) → NOT decayed
        conv.intent.stats.turns = 5;
        let view = conv.build_llm_view();
        // The tool result at turn 1 should NOT be decayed (referenced, extended window = 4)
        if let LlmMessage::ToolResult { content, .. } = &view[0] {
            assert!(
                content.contains("authenticate_user"),
                "referenced result should preserve full content at age 4, got: {content}"
            );
        }

        // At turn 6 (age 5), even referenced results should decay (5 > 4)
        conv.intent.stats.turns = 6;
        let view = conv.build_llm_view();
        if let LlmMessage::ToolResult { content, .. } = &view[0] {
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
        let captures = vec![
            crate::lifecycle::capture::AmbientCapture::Phase("implement".into()),
        ];
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
        conv.push_tool_result(ToolResultEntry {
            call_id: "t1".into(),
            tool_name: "read".into(),
            content: vec![omegon_traits::ContentBlock::Text { text: "file contents".into() }],
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
        let tool_msg = view.iter().find(|m| matches!(m, LlmMessage::ToolResult { .. }));
        assert!(tool_msg.is_some(), "should have a tool result in the view");
        if let Some(LlmMessage::ToolResult { args_summary, .. }) = tool_msg {
            assert_eq!(args_summary.as_deref(), Some("src/auth.rs"), "args_summary should survive round-trip");
        }

        // Now advance turns so it decays, verify the skeleton includes the path
        let mut loaded = ConversationState::load_session(&tmp).unwrap();
        loaded.decay_window = 0;
        loaded.intent.stats.turns = 100; // Force decay
        let view = loaded.build_llm_view();
        if let Some(LlmMessage::ToolResult { content, .. }) = view.iter().find(|m| matches!(m, LlmMessage::ToolResult { .. })) {
            assert!(content.contains("src/auth.rs"), "decayed skeleton should include path from args_summary, got: {content}");
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
        });

        let tmp = std::env::temp_dir().join("omegon-test-decay-session.json");
        conv.save_session(&tmp).unwrap();

        let loaded = ConversationState::load_session(&tmp).unwrap();
        // After load, all messages are at last_turn=10.
        // With decay_window=10, messages at turn 10 with current_turn=10 → age 0 → NOT decayed.
        let view = loaded.build_llm_view();
        if let LlmMessage::Assistant { thinking, .. } = &view[1] {
            // Thinking should be PRESERVED (not decayed) because the message
            // is within the decay window after load
            assert!(!thinking.is_empty(), "thinking should be preserved after load, got empty");
        } else {
            panic!("expected assistant message at index 1");
        }

        let _ = std::fs::remove_file(&tmp);
    }
}
