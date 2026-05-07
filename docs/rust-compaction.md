+++
id = "f6abba38-a050-4168-a97c-1a268d7684ef"
kind = "document"
title = "Rust compaction — context decay + LLM-driven summarization"
status = "implemented"
tags = ["rust", "compaction", "context", "decay"]
aliases = ["rust-compaction"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "rust-phase-1"
priority = "1"
related = ["perpetual-rolling-context"]
+++

# Rust compaction — context decay + LLM-driven summarization

## Overview

The Rust agent loop needs compaction to run long interactive sessions. Two mechanisms:

1. **Continuous context decay** (loop-level, no LLM): Tool results older than N turns decay to metadata skeletons. Recent results at full fidelity. The ConversationState already has canonical history — add a `build_llm_view()` that applies decay transforms for old messages. Reference-tracking: scan assistant responses for paths/identifiers from recent tool results; referenced results decay slower.

2. **LLM-driven compaction** (the last resort): When context utilization exceeds threshold, send a summarization request through the LLM bridge. The IntentDocument survives compaction verbatim. The summary replaces the oldest N messages.

The existing `lifecycle/mod.rs` has commented-out stubs for this. `conversation.rs` (620 LoC) already manages canonical history and could host the decay logic.

**Key constraint:** The two-view solution — canonical history stays unmodified for session persistence, the LLM-facing view has decay applied. Decay only touches messages outside the provider's cache window (5+ minutes old for Anthropic), so prompt caching isn't invalidated.

## Research

### What already exists in the Rust core

**Continuous context decay — DONE:**
- `ConversationState::build_llm_view()` applies turn-based decay (conversation.rs)
- Tool results → skeleton: `[Tool read completed successfully]`
- Assistant messages → truncated text (500 chars), thinking stripped entirely
- User messages → preserved (small)
- `decay_window` configurable (default 10 turns)
- Tests: decay strips thinking, truncates text, preserves metadata, is turn-based

**IntentDocument — DONE:**
- Auto-populated from tool calls (`update_from_tools`)
- Tracks: current_task, approach, files_read, files_modified, constraints, failed_approaches, open_questions
- SessionStatsAccumulator: turns, tool_calls, tokens_consumed, compactions
- Ambient captures from `omg:` tags in assistant responses

**Session persistence — DONE:**
- `save_session()` / `load_session()` serialize ConversationState + IntentDocument to JSON
- Round-trip tested

**What's MISSING:**
1. **LLM-driven compaction trigger** — no threshold check in the loop, no summarization request
2. **Token counting** — `context_budget_tokens: 4000` is hardcoded in ContextManager. No actual token estimation from the conversation.
3. **Compaction-as-summarization** — when decay alone isn't enough, send the oldest N messages to the LLM bridge with a "summarize this conversation" request, replace them with the summary
4. **IntentDocument injection** — the intent document exists but is never injected into the system prompt (should be injected as a high-priority context block, especially after compaction)
5. **Reference tracking** — the decay_message doesn't check if the assistant referenced the content. Could be added later as an optimization.

### Implementation progress

**Implemented:**
1. Token estimation — `estimate_tokens()` using chars/4 heuristic on `LlmMessage::char_count()`
2. Compaction threshold — `needs_compaction(context_window, threshold)` checks estimated tokens against budget
3. Compaction payload builder — `build_compaction_payload()` collects evictable messages (older than decay window) and formats them for LLM summarization
4. LLM-driven compaction — `compact_via_llm()` in loop.rs sends summarization request through bridge, receives summary
5. Compaction application — `apply_compaction()` evicts old messages, sets summary
6. LLM view with summary — `build_llm_view()` injects compaction summary + IntentDocument as first message
7. IntentDocument injection — `render_intent_for_injection()` and `inject_intent()` in ContextManager
8. Loop integration — compaction check runs before each LLM call at 75% of 200k context window
9. Session persistence — compaction_summary included in save/load
10. 5 new tests for compaction + intent rendering

**Remaining:**
- Reference tracking (scan assistant text for paths/identifiers to slow decay) — deferred, optimization
- Configurable context window (hardcoded 200k) — should come from model metadata
- Compaction model routing (use cheaper model for summarization) — can use bridge's model parameter

### Session 2026-03-27: Emergency compaction and graceful degradation

New compaction features implemented in rc.22–rc.24:

1. **Emergency compaction on context overflow** — When Anthropic returns 429 "Extra usage required for long context", the loop now detects this via `is_context_overflow()`, forces compaction, and retries. If the LLM compaction call itself fails, falls back to `decay_oldest()` (brute-force front-of-buffer removal).

2. **Compaction payload truncation** — `compact_via_llm()` now caps the payload at 100k chars (~25k tokens) to prevent the compaction request itself from exceeding provider limits.

3. **Malformed history recovery** — `is_malformed_history()` detects provider-rejected conversation structure (orphaned tool IDs, missing thinking signatures, role violations) and triggers emergency decay + retry instead of terminal failure.

4. **Orphan stripping** — `strip_orphaned_tool_results()` removes tool_result messages whose tool_use was evicted during compaction.

5. **Role alternation enforcement** — `enforce_role_alternation()` merges adjacent same-role messages and drops structurally invalid sequences after compaction.

6. **Future direction** — The perpetual-rolling-context design (exploring) will make compaction an optional cost optimization rather than a structural necessity. The selector handles budget overflow by construction — compaction only runs to save token costs on pay-per-token providers.

## Decisions

### Decision: Continuous decay is the primary mechanism — LLM compaction is the fallback

**Status:** decided
**Rationale:** Continuous decay (already implemented) handles 80% of the context budget problem with zero LLM calls. LLM-driven compaction should fire only when the decayed conversation still exceeds the context window threshold — which happens in very long sessions. The implementation order is: (1) add token estimation to know when we're near the limit, (2) inject IntentDocument into system prompt so the agent always has session context, (3) add LLM compaction as last resort when token count exceeds threshold. This matches the existing TS behavior where auto-compact fires only when context utilization is high.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/conversation.rs` (modified) — ConversationState: estimate_tokens (chars/4), needs_compaction (threshold check), build_compaction_payload, apply_compaction, build_llm_view (decay + summary injection), IntentDocument (auto-populated from tool calls)
- `core/crates/omegon/src/loop.rs` (modified) — compact_via_llm() sends summarization through bridge. Pre-turn compaction check at 75% of context_window.
- `core/crates/omegon/src/context.rs` (modified) — inject_intent() adds IntentDocument as high-priority context block
- `core/crates/omegon/src/features/auto_compact.rs` (new) — AutoCompact Feature: event-driven compaction trigger with cooldown, 4 tests

### Constraints

- Two-view solution: canonical history unmodified for session persistence, LLM-facing view has decay applied
- Decay only targets messages older than decay_window (default 10 turns)
- IntentDocument survives compaction verbatim — not summarized
- Reference tracking deferred — all results decay at same rate for now
