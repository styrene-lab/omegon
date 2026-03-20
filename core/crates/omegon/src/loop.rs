//! Agent loop state machine.
//!
//! The core prompt → LLM → tool dispatch → repeat cycle.
//! Includes: turn limits, retry with backoff, stuck detection,
//! context wiring, and parallel tool dispatch.

use crate::bridge::{LlmBridge, LlmEvent, StreamOptions};
use crate::context::ContextManager;
use crate::conversation::{AssistantMessage, ConversationState, ToolCall, ToolResultEntry};
use omegon_traits::{AgentEvent, ContentBlock};
use serde_json::Value;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::Instant;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

/// Configuration for the agent loop.
pub struct LoopConfig {
    /// Maximum turns before forced stop. 0 = no limit.
    pub max_turns: u32,
    /// Turn at which to inject a "you're running long" advisory.
    /// Defaults to max_turns * 2/3.
    pub soft_limit_turns: u32,
    /// Max retries on transient LLM errors.
    pub max_retries: u32,
    /// Initial retry delay in milliseconds.
    pub retry_delay_ms: u64,
    /// Model string to pass to the bridge (e.g. "anthropic:claude-sonnet-4-6")
    pub model: String,
    /// Working directory — used for path resolution in auto-batch rollback.
    pub cwd: std::path::PathBuf,
    /// Extended context window (1M for Anthropic).
    pub extended_context: bool,
    /// Thinking level — shared settings handle for live reads.
    pub settings: Option<crate::settings::SharedSettings>,
    /// Secrets manager for output redaction and tool guards.
    pub secrets: Option<std::sync::Arc<omegon_secrets::SecretsManager>>,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_turns: 50,
            soft_limit_turns: 35,
            max_retries: 3,
            retry_delay_ms: 2000,
            model: "anthropic:claude-sonnet-4-6".into(),
            cwd: std::env::current_dir().unwrap_or_default(),
            extended_context: false,
            settings: None,
            secrets: None,
        }
    }
}

/// Run the agent loop to completion.
///
/// The `bus` owns all features and dispatches tool calls.
pub async fn run(
    bridge: &dyn LlmBridge,
    bus: &mut crate::bus::EventBus,
    context: &mut ContextManager,
    conversation: &mut ConversationState,
    events: &broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
    config: &LoopConfig,
) -> anyhow::Result<()> {
    let tool_defs = bus.tool_definitions();

    let base_stream_options = StreamOptions {
        model: Some(config.model.clone()),
        reasoning: None,
        extended_context: config.extended_context,
    };

    let mut stuck_detector = StuckDetector::new();
    let session_start = Instant::now();
    let mut turn: u32 = 0;
    let mut commit_nudged = false;

    loop {
        if cancel.is_cancelled() {
            break;
        }

        turn += 1;
        conversation.intent.stats.turns = turn;

        // ─── Turn limit enforcement ─────────────────────────────────
        if config.max_turns > 0 && turn > config.max_turns {
            tracing::warn!("Hard turn limit reached ({} turns). Stopping.", config.max_turns);
            let _ = events.send(AgentEvent::TurnStart { turn });
            bus.emit(&omegon_traits::BusEvent::TurnEnd { turn });
            let _ = events.send(AgentEvent::TurnEnd { turn });
            break;
        }

        if config.soft_limit_turns > 0 && turn == config.soft_limit_turns {
            tracing::info!("Soft turn limit — injecting advisory");
            conversation.push_user(format!(
                "[System: You've been running for {} turns. If you're stuck, \
                 summarize your progress and what's blocking you. If you're \
                 making progress, continue — hard limit is {} turns.]",
                turn, config.max_turns
            ));
        }

        let _ = events.send(AgentEvent::TurnStart { turn });
        bus.emit(&omegon_traits::BusEvent::TurnStart { turn });

        // ─── Stuck detection ────────────────────────────────────────
        if let Some(warning) = stuck_detector.check() {
            tracing::info!("Stuck detector: {warning}");
            conversation.push_user(format!("[System: {warning}]"));
        }

        // ─── Compaction check ────────────────────────────────────────
        // If context is getting large, try LLM-driven compaction.
        // The context_window default is 200k tokens (Anthropic models).
        // Trigger at 75% utilization.
        let context_window = config.settings.as_ref()
            .and_then(|s| s.lock().ok().map(|g| g.context_window))
            .unwrap_or(200_000);
        if conversation.needs_compaction(context_window, 0.75)
            && let Some((payload, evict_count)) = conversation.build_compaction_payload() {
                tracing::info!(
                    estimated_tokens = conversation.estimate_tokens(),
                    evict_count,
                    "Context utilization high — requesting LLM compaction"
                );
                // Use the bridge to summarize the evictable messages
                match compact_via_llm(bridge, &payload, &base_stream_options).await {
                    Ok(summary) => {
                        conversation.apply_compaction(summary);
                    }
                    Err(e) => {
                        tracing::warn!("LLM compaction failed: {e} — continuing with decay only");
                    }
                }
            }

        // ─── Inject IntentDocument if meaningful ─────────────────────
        if conversation.intent.stats.tool_calls > 0
            || conversation.intent.current_task.is_some()
            || conversation.intent.stats.compactions > 0
        {
            let intent_block = conversation.render_intent_for_injection();
            context.inject_intent(intent_block);
        }

        // ─── Collect context from bus features ──────────────────────
        {
            let user_prompt = conversation.last_user_prompt();
            let (tools_vec, files_vec, budget) = context.signals_data();
            let signals = omegon_traits::ContextSignals {
                user_prompt,
                recent_tools: &tools_vec,
                recent_files: &files_vec,
                lifecycle_phase: context.phase(),
                turn_number: turn,
                context_budget_tokens: budget,
            };
            let bus_injections = bus.collect_context(&signals);
            if !bus_injections.is_empty() {
                tracing::debug!(count = bus_injections.len(), "bus context injections");
                context.inject_external(bus_injections);
            }
        }

        // ─── Build LLM-facing context ───────────────────────────────
        let system_prompt =
            context.build_system_prompt(conversation.last_user_prompt(), conversation);
        let llm_messages = conversation.build_llm_view();
        conversation.clear_pending_images();

        tracing::debug!(
            turn,
            system_prompt_len = system_prompt.len(),
            messages = llm_messages.len(),
            tools = tool_defs.len(),
            estimated_tokens = conversation.estimate_tokens(),
            "LLM context assembled"
        );

        // ─── Stream LLM response with retry ─────────────────────────
        // Re-read thinking level each turn (can change mid-session via /thinking)
        let stream_options = {
            let mut opts = base_stream_options.clone();
            opts.reasoning = config.settings.as_ref().and_then(|s| {
                let guard = s.lock().ok()?;
                match guard.thinking {
                    crate::settings::ThinkingLevel::Off => None,
                    crate::settings::ThinkingLevel::Low => Some("low".to_string()),
                    crate::settings::ThinkingLevel::Medium => Some("medium".to_string()),
                    crate::settings::ThinkingLevel::High => Some("high".to_string()),
                }
            });
            // Also re-read model (can change via /sonnet, /opus, etc.)
            opts.model = config.settings.as_ref().and_then(|s| {
                s.lock().ok().map(|g| g.model.clone())
            }).or_else(|| Some(config.model.clone()));
            opts
        };

        let assistant_msg = tokio::select! {
            result = stream_with_retry(
                bridge,
                &system_prompt,
                &llm_messages,
                &tool_defs,
                &stream_options,
                events,
                config,
            ) => result?,
            _ = cancel.cancelled() => {
                tracing::info!("Agent loop cancelled during LLM streaming");
                bus.emit(&omegon_traits::BusEvent::TurnEnd { turn });
                let _ = events.send(AgentEvent::TurnEnd { turn });
                break;
            }
        };

        // ─── Parse ambient capture blocks (omg: tags) ───────────────
        let captured =
            crate::lifecycle::capture::parse_ambient_blocks(assistant_msg.text_content());
        if !captured.is_empty() {
            conversation.apply_ambient_captures(&captured);
        }

        // Push assistant message to conversation
        conversation.push_assistant(assistant_msg.clone());

        // Extract tool calls
        let tool_calls = assistant_msg.tool_calls();
        if tool_calls.is_empty() {
            // Check if the agent skipped committing.
            // If the conversation has edit/write calls but hasn't been nudged yet,
            // give it one more turn to commit.
            if !commit_nudged && has_mutations(conversation) && turn < config.max_turns {
                commit_nudged = true;
                tracing::info!("Agent stopped without committing — nudging");
                conversation.push_user(
                    "[System: You made file changes but did not run `git add` and `git commit`. \
                     Please commit your work now with a descriptive message, then summarize what you did.]"
                        .to_string(),
                );
                bus.emit(&omegon_traits::BusEvent::TurnEnd { turn });
                let _ = events.send(AgentEvent::TurnEnd { turn });
                continue; // give it one more turn to commit
            }
            bus.emit(&omegon_traits::BusEvent::TurnEnd { turn });
            let _ = events.send(AgentEvent::TurnEnd { turn });
            break;
        }

        // ─── Emit ToolStart bus events before dispatch ──────────────
        for call in tool_calls {
            bus.emit(&omegon_traits::BusEvent::ToolStart {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.arguments.clone(),
            });
        }

        // ─── Dispatch tool calls ────────────────────────────────────
        let results =
            dispatch_tools(bus, tool_calls, events, cancel.clone(), &config.cwd,
                config.secrets.as_deref()).await;

        // Push tool results to conversation and update intent
        for result in &results {
            conversation.push_tool_result(result.clone());
        }
        conversation.intent.update_from_tools(tool_calls, &results);

        // ─── Emit tool events to bus features ───────────────────────
        for (call, result) in tool_calls.iter().zip(results.iter()) {
            bus.emit(&omegon_traits::BusEvent::ToolEnd {
                id: call.id.clone(),
                name: call.name.clone(),
                result: omegon_traits::ToolResult {
                    content: result.content.clone(),
                    details: serde_json::Value::Null,
                },
                is_error: result.is_error,
            });
        }

        // ─── Wire context signals ───────────────────────────────────
        for call in tool_calls {
            context.record_tool_call(&call.name);
            // Track file access from tool arguments
            if let Some(path) = call.arguments.get("path").and_then(|v| v.as_str()) {
                context.record_file_access(std::path::PathBuf::from(path));
            }
        }
        context.update_phase_from_activity(tool_calls);

        // ─── Feed stuck detector ────────────────────────────────────
        for call in tool_calls {
            let is_error = results
                .iter()
                .find(|r| r.call_id == call.id)
                .is_some_and(|r| r.is_error);
            stuck_detector.record(call, is_error);
        }

        bus.emit(&omegon_traits::BusEvent::TurnEnd { turn });

        // ─── Handle bus requests from features ──────────────────────
        for request in bus.drain_requests() {
            match request {
                omegon_traits::BusRequest::Notify { message, level } => {
                    tracing::info!(level = ?level, "Bus: {message}");
                }
                omegon_traits::BusRequest::InjectSystemMessage { content } => {
                    conversation.push_user(format!("[System: {content}]"));
                }
                omegon_traits::BusRequest::RequestCompaction => {
                    tracing::info!("Bus: compaction requested by feature");
                    // Compaction will be checked at the top of the next turn
                }
            }
        }

        let _ = events.send(AgentEvent::TurnEnd { turn });
    }

    let elapsed = session_start.elapsed();
    tracing::info!(
        turns = turn,
        tool_calls = conversation.intent.stats.tool_calls,
        elapsed_secs = elapsed.as_secs(),
        "Agent loop complete"
    );

    bus.emit(&omegon_traits::BusEvent::AgentEnd);
    let _ = events.send(AgentEvent::AgentEnd);

    // Process any pending bus requests (e.g. auto-compact notifications)
    for request in bus.drain_requests() {
        match request {
            omegon_traits::BusRequest::Notify { message, level } => {
                tracing::info!(level = ?level, "Bus notification: {message}");
            }
            omegon_traits::BusRequest::InjectSystemMessage { content } => {
                conversation.push_user(format!("[System: {content}]"));
            }
            omegon_traits::BusRequest::RequestCompaction => {
                tracing::info!("Bus requested compaction (post-loop — ignored)");
            }
        }
    }

    Ok(())
}

/// Request an LLM-driven compaction summary for old conversation messages.
async fn compact_via_llm(
    bridge: &dyn LlmBridge,
    payload: &str,
    options: &StreamOptions,
) -> anyhow::Result<String> {
    let system = "You are a conversation summarizer. Produce a concise summary \
                  preserving: what was done, what failed, constraints discovered, \
                  and current approach. Output only the summary, no preamble.";

    let messages = vec![crate::bridge::LlmMessage::User {
        content: payload.to_string(),
        images: vec![],
    }];

    let mut rx = bridge
        .stream(system, &messages, &[], options)
        .await?;

    let mut summary = String::new();
    let summary_idle = std::time::Duration::from_secs(120);
    while let Some(event) = match tokio::time::timeout(summary_idle, rx.recv()).await {
        Ok(e) => e,
        Err(_) => {
            tracing::warn!("summary stream idle timeout");
            None
        }
    } {
        match event {
            LlmEvent::TextDelta { delta } => summary.push_str(&delta),
            LlmEvent::Done { .. } => break,
            LlmEvent::Error { message } => {
                return Err(anyhow::anyhow!("Compaction LLM error: {message}"));
            }
            _ => {}
        }
    }

    if summary.is_empty() {
        return Err(anyhow::anyhow!("Compaction produced empty summary"));
    }

    tracing::info!(summary_len = summary.len(), "Compaction summary received");
    Ok(summary)
}

/// Stream an LLM response with retry on transient errors.
async fn stream_with_retry(
    bridge: &dyn LlmBridge,
    system_prompt: &str,
    messages: &[crate::bridge::LlmMessage],
    tools: &[omegon_traits::ToolDefinition],
    options: &StreamOptions,
    events: &broadcast::Sender<AgentEvent>,
    config: &LoopConfig,
) -> anyhow::Result<AssistantMessage> {
    let mut attempt = 0;
    let mut delay = config.retry_delay_ms;

    loop {
        attempt += 1;
        let mut rx = bridge
            .stream(system_prompt, messages, tools, options)
            .await?;

        match consume_llm_stream(&mut rx, events).await {
            Ok(msg) => return Ok(msg),
            Err(e) => {
                let err_msg = e.to_string();
                let is_transient = is_transient_error(&err_msg);

                if !is_transient || attempt > config.max_retries {
                    if attempt > 1 {
                        tracing::error!(
                            "LLM error after {attempt} attempts: {err_msg}"
                        );
                    }
                    return Err(e);
                }

                tracing::warn!(
                    attempt,
                    max = config.max_retries,
                    delay_ms = delay,
                    "Transient LLM error, retrying: {err_msg}"
                );
                // Notify the TUI so the user knows why it's paused
                let short_err = if err_msg.len() > 80 { &err_msg[..80] } else { &err_msg };
                let _ = events.send(AgentEvent::SystemNotification {
                    message: format!("⟳ Retrying ({attempt}/{})… {short_err}", config.max_retries),
                });
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                delay = (delay * 2).min(30_000); // exponential backoff, cap at 30s
            }
        }
    }
}

/// Heuristic: is this error message transient (worth retrying)?
///
/// Matches known transient error patterns. HTTP status codes use word-boundary
/// matching to avoid false positives (e.g. "model gpt-500" shouldn't match).
fn is_transient_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();

    // Semantic patterns — safe as substring matches
    if lower.contains("overloaded")
        || lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("timeout")
        || lower.contains("server_error")
        || lower.contains("capacity")
        || lower.contains("temporarily")
        || lower.contains("try again")
        || lower.contains("service unavailable")
        || lower.contains("bad gateway")
        || lower.contains("internal server error")
    {
        return true;
    }

    // HTTP status codes — require word boundary (space, punctuation, or start/end)
    // to avoid matching model names like "gpt-500" or version strings
    for code in ["500", "502", "503", "529"] {
        if contains_word(&lower, code) {
            return true;
        }
    }

    false
}

/// Check if `text` contains `word` as a standalone token.
/// Word boundaries: spaces, punctuation, and start/end of string.
/// Hyphens and underscores are treated as word-joining (so "gpt-500" doesn't match "500").
fn contains_word(text: &str, word: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = text[start..].find(word) {
        let abs_pos = start + pos;
        let before_ok = abs_pos == 0 || !is_word_char(text.as_bytes()[abs_pos - 1]);
        let after_pos = abs_pos + word.len();
        let after_ok = after_pos >= text.len() || !is_word_char(text.as_bytes()[after_pos]);
        if before_ok && after_ok {
            return true;
        }
        start = abs_pos + 1;
    }
    false
}

/// Is this byte part of a "word" for boundary detection?
/// Alphanumeric plus hyphen and underscore (common in model names, versions).
fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

/// Consume LlmEvents from the bridge, build an AssistantMessage.
async fn consume_llm_stream(
    rx: &mut tokio::sync::mpsc::Receiver<LlmEvent>,
    events: &broadcast::Sender<AgentEvent>,
) -> anyhow::Result<AssistantMessage> {
    let mut text_parts: Vec<String> = Vec::new();
    let mut thinking_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut final_raw: Value = Value::Null;

    let _ = events.send(AgentEvent::MessageStart {
        role: "assistant".into(),
    });

    let stream_idle_timeout = std::time::Duration::from_secs(120);
    while let Some(event) = match tokio::time::timeout(stream_idle_timeout, rx.recv()).await {
        Ok(event) => event,
        Err(_) => {
            let _ = events.send(AgentEvent::MessageEnd);
            anyhow::bail!(
                "LLM stream idle for {}s — connection may be stalled",
                stream_idle_timeout.as_secs()
            );
        }
    } {
        match event {
            LlmEvent::Start => {} // Initial partial message — ignored
            LlmEvent::TextStart => {}
            LlmEvent::TextDelta { delta } => {
                let _ = events.send(AgentEvent::MessageChunk { text: delta.clone() });
                if let Some(last) = text_parts.last_mut() {
                    last.push_str(&delta);
                } else {
                    text_parts.push(delta);
                }
            }
            LlmEvent::TextEnd => {
                text_parts.push(String::new());
            }
            LlmEvent::ThinkingStart => {}
            LlmEvent::ThinkingDelta { delta } => {
                let _ = events.send(AgentEvent::ThinkingChunk { text: delta.clone() });
                if let Some(last) = thinking_parts.last_mut() {
                    last.push_str(&delta);
                } else {
                    thinking_parts.push(delta);
                }
            }
            LlmEvent::ThinkingEnd => {
                thinking_parts.push(String::new());
            }
            LlmEvent::ToolCallStart => {}
            LlmEvent::ToolCallDelta { .. } => {
                // Deltas accumulated by the bridge — complete tool call in ToolCallEnd
            }
            LlmEvent::ToolCallEnd { tool_call } => {
                tool_calls.push(ToolCall {
                    id: tool_call.id,
                    name: tool_call.name,
                    arguments: tool_call.arguments,
                });
            }
            LlmEvent::Done { message } => {
                final_raw = message.get("raw").cloned().unwrap_or(message);
                break;
            }
            LlmEvent::Error { message } => {
                let _ = events.send(AgentEvent::MessageEnd);
                anyhow::bail!("LLM error: {message}");
            }
        }
    }

    let _ = events.send(AgentEvent::MessageEnd);

    // Detect incomplete streams — if we never got a Done event, the bridge
    // probably died. An empty message with no text and no tool calls is
    // almost certainly a dropped connection, not a valid LLM response.
    if final_raw == Value::Null && text_parts.is_empty() && tool_calls.is_empty() {
        anyhow::bail!(
            "LLM stream ended without a completion event — the bridge may have crashed"
        );
    }

    // Clean up empty trailing parts
    while text_parts.last().is_some_and(|s| s.is_empty()) {
        text_parts.pop();
    }
    while thinking_parts.last().is_some_and(|s| s.is_empty()) {
        thinking_parts.pop();
    }

    let text = text_parts.join("");
    let thinking = if thinking_parts.is_empty() {
        None
    } else {
        Some(thinking_parts.join(""))
    };

    Ok(AssistantMessage {
        text,
        thinking,
        tool_calls,
        raw: final_raw,
    })
}

/// Dispatch tool calls via the EventBus.
///
/// **Auto-batching**: when the LLM returns multiple edit/write calls in one turn,
/// the loop snapshots target files before execution. If any mutation fails, all
/// previously applied mutations are rolled back. This makes the existing `edit`
/// tool secretly atomic across multi-file changes — the agent doesn't need to
/// learn the `change` tool to get atomic behavior.
async fn dispatch_tools(
    bus: &crate::bus::EventBus,
    tool_calls: &[ToolCall],
    events: &broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
    cwd: &std::path::Path,
    secrets: Option<&omegon_secrets::SecretsManager>,
) -> Vec<ToolResultEntry> {
    let mut results = Vec::with_capacity(tool_calls.len());

    // ── Auto-batch: snapshot files targeted by mutation tools ────────
    let mutation_count = tool_calls
        .iter()
        .filter(|c| is_mutation_tool(&c.name))
        .count();
    let batch_mode = mutation_count >= 2;

    let mut snapshots: HashMap<std::path::PathBuf, String> = HashMap::new();
    let mut created_files: Vec<std::path::PathBuf> = Vec::new(); // new files to delete on rollback
    let mut mutated_files: Vec<std::path::PathBuf> = Vec::new();

    if batch_mode {
        for call in tool_calls {
            if is_mutation_tool(&call.name)
                && let Some(path_str) = extract_mutation_path(&call.arguments) {
                    // Resolve against cwd — same as tools/mod.rs resolve_path
                    let full = cwd.join(&path_str);
                    if full.exists() {
                        if !snapshots.contains_key(&full)
                            && let Ok(content) = tokio::fs::read_to_string(&full).await {
                                snapshots.insert(full, content);
                            }
                    } else {
                        // File doesn't exist yet — mark for deletion on rollback
                        created_files.push(full);
                    }
                }
        }
        if !snapshots.is_empty() {
            tracing::info!(
                files = snapshots.len(),
                edits = mutation_count,
                "Auto-batch: snapshotted {} file(s) for {} mutations",
                snapshots.len(),
                mutation_count
            );
        }
    }

    let mut batch_failed = false;

    for call in tool_calls {
        // ── Tool guard check ────────────────────────────────────────
        if let Some(sm) = secrets {
            if let Some(decision) = sm.check_guard(&call.name, &call.arguments) {
                if decision.is_block() {
                    let msg = match &decision {
                        omegon_secrets::GuardDecision::Block { reason, path } =>
                            format!("Blocked: {reason} ({path})"),
                        _ => unreachable!(),
                    };
                    tracing::warn!(tool = call.name, %msg, "tool guard blocked");
                    let _ = events.send(AgentEvent::ToolEnd {
                        id: call.id.clone(),
                        result: omegon_traits::ToolResult {
                            content: vec![ContentBlock::Text { text: msg.clone() }],
                            details: Value::Null,
                        },
                        is_error: true,
                    });
                    results.push(ToolResultEntry {
                        call_id: call.id.clone(),
                        tool_name: call.name.clone(),
                        content: vec![ContentBlock::Text { text: msg }],
                        is_error: true,
                        args_summary: summarize_tool_args(&call.name, &call.arguments),
                    });
                    continue;
                }
            }
        }

        let _ = events.send(AgentEvent::ToolStart {
            id: call.id.clone(),
            name: call.name.clone(),
            args: call.arguments.clone(),
        });

        let (result, is_error) = match bus
            .execute_tool(&call.name, &call.id, call.arguments.clone(), cancel.clone())
            .await
        {
            Ok(result) => (result, false),
            Err(e) => (
                omegon_traits::ToolResult {
                    content: vec![ContentBlock::Text {
                        text: e.to_string(),
                    }],
                    details: Value::Null,
                },
                true,
            ),
        };

        // Track which files were successfully mutated (for rollback)
        if !is_error && is_mutation_tool(&call.name)
            && let Some(path_str) = extract_mutation_path(&call.arguments) {
                mutated_files.push(cwd.join(&path_str));
            }

        // ── Auto-batch rollback on mutation failure ─────────────────
        if is_error && batch_mode && is_mutation_tool(&call.name) && !mutated_files.is_empty() {
            batch_failed = true;
            tracing::warn!(
                failed_tool = call.name,
                mutated = mutated_files.len(),
                "Auto-batch: mutation failed — rolling back {} file(s)",
                mutated_files.len()
            );

            let mut rollback_report = Vec::new();
            for file in &mutated_files {
                if let Some(original) = snapshots.get(file) {
                    match tokio::fs::write(file, original).await {
                        Ok(_) => rollback_report.push(format!("  ✓ restored {}", file.display())),
                        Err(e) => rollback_report.push(format!("  ✗ rollback failed {}: {e}", file.display())),
                    }
                } else if created_files.contains(file) {
                    // File was newly created — delete it
                    match tokio::fs::remove_file(file).await {
                        Ok(_) => rollback_report.push(format!("  ✓ removed {}", file.display())),
                        Err(e) => rollback_report.push(format!("  ✗ remove failed {}: {e}", file.display())),
                    }
                }
            }

            // Append rollback info to the error result
            let mut error_text = result.content.iter()
                .filter_map(|c| c.as_text())
                .collect::<Vec<_>>()
                .join("\n");
            error_text.push_str("\n\n[Auto-rollback: previous edits in this turn were reverted]\n");
            error_text.push_str(&rollback_report.join("\n"));

            let _ = events.send(AgentEvent::ToolEnd {
                id: call.id.clone(),
                result: omegon_traits::ToolResult {
                    content: vec![ContentBlock::Text { text: error_text.clone() }],
                    details: Value::Null,
                },
                is_error: true,
            });

            results.push(ToolResultEntry {
                call_id: call.id.clone(),
                tool_name: call.name.clone(),
                content: vec![ContentBlock::Text { text: error_text }],
                is_error: true,
                args_summary: summarize_tool_args(&call.name, &call.arguments),
            });

            // Skip remaining mutations — they'd operate on rolled-back state
            // Continue dispatching non-mutation tools (reads, bash, etc.)
            continue;
        }

        // Skip remaining mutations if we've already rolled back
        if batch_failed && is_mutation_tool(&call.name) {
            let skip_text = format!(
                "Skipped {} — previous edit in this turn failed and triggered rollback.",
                call.name
            );
            let _ = events.send(AgentEvent::ToolEnd {
                id: call.id.clone(),
                result: omegon_traits::ToolResult {
                    content: vec![ContentBlock::Text { text: skip_text.clone() }],
                    details: Value::Null,
                },
                is_error: true,
            });
            results.push(ToolResultEntry {
                call_id: call.id.clone(),
                tool_name: call.name.clone(),
                content: vec![ContentBlock::Text { text: skip_text }],
                is_error: true,
                args_summary: summarize_tool_args(&call.name, &call.arguments),
            });
            continue;
        }

        // ── Redact secrets from output ────────────────────────────
        let mut final_content = result.content;
        if let Some(sm) = secrets {
            sm.redact_content(&mut final_content);
        }

        let _ = events.send(AgentEvent::ToolEnd {
            id: call.id.clone(),
            result: omegon_traits::ToolResult {
                content: final_content.clone(),
                details: result.details,
            },
            is_error,
        });

        results.push(ToolResultEntry {
            call_id: call.id.clone(),
            tool_name: call.name.clone(),
            content: final_content,
            is_error,
            args_summary: summarize_tool_args(&call.name, &call.arguments),
        });
    }

    results
}

/// Is this tool a file mutation (edit, write)?
/// Used for auto-batch snapshotting and rollback.
fn is_mutation_tool(name: &str) -> bool {
    matches!(name, "edit" | "write" | "change")
}

/// Extract the target file path from mutation tool arguments.
fn extract_mutation_path(args: &Value) -> Option<String> {
    args.get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Check if the conversation contains any file mutations (edit or write calls).
fn has_mutations(conversation: &ConversationState) -> bool {
    !conversation.intent.files_modified.is_empty()
}



// ─── Stuck detection ────────────────────────────────────────────────────────

/// Detects pathological tool-call patterns that indicate the agent is stuck.
struct StuckDetector {
    /// Recent tool calls as (name, args_hash, was_error)
    recent: Vec<(String, u64, bool)>,
    /// Window size for pattern detection
    window: usize,
}

impl StuckDetector {
    fn new() -> Self {
        Self {
            recent: Vec::new(),
            window: 10,
        }
    }

    /// Record a tool call for pattern analysis.
    fn record(&mut self, call: &ToolCall, is_error: bool) {
        let args_hash = hash_value(&call.arguments);
        self.recent
            .push((call.name.clone(), args_hash, is_error));
        if self.recent.len() > self.window * 2 {
            self.recent.drain(..self.window);
        }
    }

    /// Check for stuck patterns. Returns a warning message if detected.
    fn check(&self) -> Option<String> {
        let len = self.recent.len();
        if len < 3 {
            return None;
        }

        let window = &self.recent[len.saturating_sub(self.window)..];

        // Pattern 1: Same tool + same args called 3+ times
        if let Some(repeated) = self.find_repeated_call(window, 3) {
            return Some(format!(
                "You've called `{}` with the same arguments {} times. \
                 If it's not producing the result you need, try a different approach.",
                repeated.0, repeated.1
            ));
        }

        // Pattern 2: Edit failures — repeated error on the same tool
        let recent_errors: Vec<_> = window
            .iter()
            .filter(|(_, _, err)| *err)
            .collect();
        if recent_errors.len() >= 3 {
            let names: Vec<_> = recent_errors.iter().map(|(n, _, _)| n.as_str()).collect();
            if names.windows(3).any(|w| w[0] == w[1] && w[1] == w[2]) {
                return Some(format!(
                    "Your last several `{}` calls returned errors. \
                     Consider reading the current file state before retrying.",
                    recent_errors.last().unwrap().0
                ));
            }
        }

        // Pattern 3: read-without-modify loop — same file read 3+ times
        // without any write/edit to that file
        let reads: Vec<_> = window
            .iter()
            .filter(|(name, _, _)| name == "read")
            .collect();
        if reads.len() >= 3 {
            // Check if the same args hash appears 3+ times
            let mut hash_counts: HashMap<u64, u32> = HashMap::new();
            for (_, h, _) in &reads {
                *hash_counts.entry(*h).or_default() += 1;
            }
            if hash_counts.values().any(|&c| c >= 3) {
                return Some(
                    "You've read the same file multiple times without modifying it. \
                     Consider noting what you need from it, or try a different approach."
                        .into(),
                );
            }
        }

        None
    }

    /// Find a (tool_name, count) where the same tool+args appears N+ times in the window.
    fn find_repeated_call(&self, window: &[(String, u64, bool)], threshold: usize) -> Option<(String, usize)> {
        let mut counts: HashMap<(String, u64), usize> = HashMap::new();
        for (name, hash, _) in window {
            let key = (name.clone(), *hash);
            *counts.entry(key).or_default() += 1;
        }
        counts
            .into_iter()
            .find(|(_, count)| *count >= threshold)
            .map(|((name, _), count)| (name, count))
    }
}

/// Summarize tool call arguments into a compact string for decay context.
/// Returns None if no useful summary can be extracted.
pub fn summarize_tool_args(tool_name: &str, args: &Value) -> Option<String> {
    match tool_name {
        "read" | "edit" | "write" | "view" => {
            args.get("path").and_then(|v| v.as_str()).map(|p| {
                // Strip common cwd prefixes to show relative paths
                let cwd = std::env::current_dir()
                    .map(|d| d.to_string_lossy().to_string())
                    .unwrap_or_default();
                if !cwd.is_empty() && p.starts_with(&cwd) {
                    p[cwd.len()..].strip_prefix('/').unwrap_or(&p[cwd.len()..]).to_string()
                } else {
                    p.to_string()
                }
            })
        }
        "bash" => {
            let cmd = args.get("command").and_then(|v| v.as_str())?;
            // Strip common cwd wrappers: "cd /long/path && actual command"
            let clean = if let Some(rest) = cmd.strip_prefix("cd ") {
                // Find the && and take what's after it
                rest.split_once(" && ")
                    .map(|(_, after)| after)
                    .unwrap_or(rest)
            } else {
                cmd
            };
            // Truncate to keep it compact
            let short = if clean.len() > 60 {
                let mut end = 60;
                while end > 0 && !clean.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}…", &clean[..end])
            } else {
                clean.to_string()
            };
            Some(short)
        }
        "change" => {
            let edits = args.get("edits").and_then(|v| v.as_array())?;
            let files: Vec<&str> = edits.iter()
                .filter_map(|e| e.get("file").and_then(|v| v.as_str()))
                .collect();
            Some(files.join(", "))
        }
        "web_search" => {
            args.get("query").and_then(|v| v.as_str()).map(|q| {
                if q.len() > 60 {
                    format!("{}…", &q[..60])
                } else {
                    q.to_string()
                }
            })
        }
        "memory_recall" | "memory_store" | "memory_query" => {
            args.get("query")
                .or_else(|| args.get("content"))
                .and_then(|v| v.as_str())
                .map(|s| {
                    if s.len() > 60 {
                        format!("{}…", &s[..60])
                    } else {
                        s.to_string()
                    }
                })
        }
        _ => None,
    }
}

/// Hash a serde_json::Value for comparison (not cryptographic — just dedup).
fn hash_value(v: &Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    let s = v.to_string();
    s.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use omegon_traits::ToolProvider;

    #[test]
    fn transient_error_detection() {
        // Should match: known transient patterns
        assert!(is_transient_error("503 Service Unavailable"));
        assert!(is_transient_error("Request rate limit exceeded"));
        assert!(is_transient_error("Server is overloaded"));
        assert!(is_transient_error("transient_server_error"));
        assert!(is_transient_error("temporarily unavailable, try again later"));
        assert!(is_transient_error("HTTP 500 Internal Server Error"));
        assert!(is_transient_error("error 529: capacity exceeded"));
        assert!(is_transient_error("502 Bad Gateway"));
        assert!(is_transient_error("service unavailable"));

        // Should NOT match: permanent errors
        assert!(!is_transient_error("Invalid API key"));
        assert!(!is_transient_error("Model not found"));
        assert!(!is_transient_error("400 Bad Request: Input should be a valid dictionary"));
        assert!(!is_transient_error("401 Unauthorized"));

        // Should NOT match: status codes embedded in non-error contexts
        assert!(!is_transient_error("model gpt-500 not found"));
        assert!(!is_transient_error("using port 5029"));
        assert!(!is_transient_error("version 5.0.3 released"));
    }

    #[test]
    fn contains_word_boundary() {
        // Standalone status codes — should match
        assert!(contains_word("error 500 occurred", "500"));
        assert!(contains_word("500 error", "500"));
        assert!(contains_word("error: 500", "500"));
        assert!(contains_word("HTTP/1.1 503", "503"));

        // Hyphen-joined (model names, identifiers) — should NOT match
        assert!(!contains_word("gpt-500", "500"));
        assert!(!contains_word("model-500x", "500"));
        assert!(!contains_word("error_code_500x", "500"));

        // Embedded in larger numbers — should NOT match
        assert!(!contains_word("port5003", "500"));
        assert!(!contains_word("50000 items", "500"));
    }

    #[test]
    fn stuck_detector_repeated_calls() {
        let mut detector = StuckDetector::new();
        let call = ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: serde_json::json!({"path": "foo.rs"}),
        };

        detector.record(&call, false);
        detector.record(&call, false);
        assert!(detector.check().is_none());

        detector.record(&call, false);
        let warning = detector.check();
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("same arguments"));
    }

    #[test]
    fn stuck_detector_repeated_errors() {
        let mut detector = StuckDetector::new();
        let call = ToolCall {
            id: "1".into(),
            name: "edit".into(),
            arguments: serde_json::json!({"path": "foo.rs", "oldText": "a", "newText": "b"}),
        };

        detector.record(&call, true);
        detector.record(&call, true);
        detector.record(&call, true);

        // This triggers the repeated-call pattern (same args 3x)
        let warning = detector.check();
        assert!(warning.is_some());
    }

    // ── Auto-batch tests ────────────────────────────────────────────

    #[test]
    fn mutation_tool_detection() {
        assert!(is_mutation_tool("edit"));
        assert!(is_mutation_tool("write"));
        assert!(is_mutation_tool("change"));
        assert!(!is_mutation_tool("read"));
        assert!(!is_mutation_tool("bash"));
        assert!(!is_mutation_tool("web_search"));
    }

    #[test]
    fn extract_path_from_args() {
        let args = serde_json::json!({"path": "src/main.rs", "oldText": "a", "newText": "b"});
        assert_eq!(extract_mutation_path(&args).as_deref(), Some("src/main.rs"));

        let no_path = serde_json::json!({"command": "ls"});
        assert!(extract_mutation_path(&no_path).is_none());
    }

    #[test]
    fn summarize_args_by_tool() {
        assert_eq!(
            summarize_tool_args("read", &serde_json::json!({"path": "src/foo.rs"})).as_deref(),
            Some("src/foo.rs")
        );
        assert_eq!(
            summarize_tool_args("bash", &serde_json::json!({"command": "cargo test"})).as_deref(),
            Some("cargo test")
        );
        assert_eq!(
            summarize_tool_args("change", &serde_json::json!({
                "edits": [{"file": "a.rs"}, {"file": "b.rs"}]
            })).as_deref(),
            Some("a.rs, b.rs")
        );
        // Memory tools
        assert_eq!(
            summarize_tool_args("memory_recall", &serde_json::json!({"query": "auth architecture"})).as_deref(),
            Some("auth architecture")
        );
        assert_eq!(
            summarize_tool_args("memory_store", &serde_json::json!({"content": "Omegon uses ratatui"})).as_deref(),
            Some("Omegon uses ratatui")
        );

        // Long command gets truncated
        let long_cmd = "x".repeat(100);
        let summary = summarize_tool_args("bash", &serde_json::json!({"command": long_cmd})).unwrap();
        assert!(summary.len() <= 84, "got len {}", summary.len()); // 80 + "…" (3 bytes UTF-8)
        assert!(summary.ends_with('…'));
    }

    #[tokio::test]
    async fn auto_batch_rollback_on_second_edit_failure() {
        use std::io::Write as IoWrite;
        use omegon_traits::ToolResult;

        // Create a mock tool provider that does real file I/O
        struct FileEditProvider { dir: std::path::PathBuf }

        #[async_trait::async_trait]
        impl ToolProvider for FileEditProvider {
            fn tools(&self) -> Vec<omegon_traits::ToolDefinition> {
                vec![omegon_traits::ToolDefinition {
                    name: "edit".into(),
                    label: "edit".into(),
                    description: "test".into(),
                    parameters: serde_json::json!({}),
                }]
            }

            async fn execute(
                &self,
                _tool_name: &str,
                _call_id: &str,
                args: Value,
                _cancel: CancellationToken,
            ) -> anyhow::Result<ToolResult> {
                let path_str = args["path"].as_str().unwrap();
                let path = std::path::Path::new(path_str);
                let old_text = args["oldText"].as_str().unwrap();
                let new_text = args["newText"].as_str().unwrap();

                let content = tokio::fs::read_to_string(path).await?;
                if !content.contains(old_text) {
                    anyhow::bail!("Could not find exact text in {}", path.display());
                }
                let new_content = content.replacen(old_text, new_text, 1);
                tokio::fs::write(path, &new_content).await?;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!("Edited {}", path.display()),
                    }],
                    details: Value::Null,
                })
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.txt");
        let file_b = dir.path().join("b.txt");
        std::fs::File::create(&file_a).unwrap().write_all(b"hello world").unwrap();
        std::fs::File::create(&file_b).unwrap().write_all(b"foo bar baz").unwrap();

        let provider = FileEditProvider { dir: dir.path().to_path_buf() };
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::adapter::ToolAdapter::new(
            "test-edit", Box::new(provider),
        )));
        bus.finalize();

        let (events_tx, _rx) = broadcast::channel(64);
        let cancel = CancellationToken::new();

        // Two edits: first succeeds, second will fail (text not found)
        let calls = vec![
            ToolCall {
                id: "1".into(),
                name: "edit".into(),
                arguments: serde_json::json!({
                    "path": file_a.display().to_string(),
                    "oldText": "hello",
                    "newText": "goodbye"
                }),
            },
            ToolCall {
                id: "2".into(),
                name: "edit".into(),
                arguments: serde_json::json!({
                    "path": file_b.display().to_string(),
                    "oldText": "NONEXISTENT",
                    "newText": "replaced"
                }),
            },
        ];

        let results = dispatch_tools(&bus, &calls, &events_tx, cancel, dir.path(), None).await;

        // The second edit should have failed
        assert!(results[1].is_error, "second edit should fail");

        // The first file should be ROLLED BACK to original content
        let a_content = std::fs::read_to_string(&file_a).unwrap();
        assert_eq!(a_content, "hello world", "file_a should be rolled back, got: {a_content}");

        // The error message should mention the rollback
        let error_text = results[1].content[0].as_text().unwrap();
        assert!(error_text.contains("Auto-rollback"), "should mention rollback, got: {error_text}");
    }

    #[tokio::test]
    async fn single_edit_has_no_batch_overhead() {
        use omegon_traits::ToolResult;
        let dir = tempfile::tempdir().unwrap();

        struct PassProvider;

        #[async_trait::async_trait]
        impl ToolProvider for PassProvider {
            fn tools(&self) -> Vec<omegon_traits::ToolDefinition> {
                vec![omegon_traits::ToolDefinition {
                    name: "edit".into(),
                    label: "edit".into(),
                    description: "test".into(),
                    parameters: serde_json::json!({}),
                }]
            }

            async fn execute(
                &self,
                _tool_name: &str,
                _call_id: &str,
                _args: Value,
                _cancel: CancellationToken,
            ) -> anyhow::Result<ToolResult> {
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: "Edited ok".into() }],
                    details: Value::Null,
                })
            }
        }

        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::adapter::ToolAdapter::new(
            "test-pass", Box::new(PassProvider),
        )));
        bus.finalize();

        let (events_tx, _rx) = broadcast::channel(64);
        let cancel = CancellationToken::new();

        let calls = vec![ToolCall {
            id: "1".into(),
            name: "edit".into(),
            arguments: serde_json::json!({"path": "/tmp/fake.rs", "oldText": "a", "newText": "b"}),
        }];

        let results = dispatch_tools(&bus, &calls, &events_tx, cancel, dir.path(), None).await;
        assert!(!results[0].is_error);
        let text = results[0].content[0].as_text().unwrap();
        assert!(!text.contains("rollback"), "single edit should have no batch overhead");
    }
}
