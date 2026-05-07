+++
id = "fbdc559f-77b5-4f6e-bd9f-576ce228f8e4"
kind = "document"
title = "Agent loop resilience — what the hermit crab wants in its shell"
status = "implemented"
tags = ["rust", "agent-loop", "resilience", "self-awareness", "introspective"]
aliases = ["agent-loop-resilience"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "rust-agent-loop"
related = ["perpetual-rolling-context"]
+++

# Agent loop resilience — what the hermit crab wants in its shell

## Overview

A fresh-eyes inspection of the Phase 0 scaffold, asking: "what would I want if I were the agent occupying this loop?" Not just bugs — desires. What's missing that would make me more effective, more resilient, more self-aware?

The current scaffold is structurally sound — the types are clean, the wire format is Omegon-owned, the tool dispatch works. But it's a bare chassis. The engine runs; the safety systems, the instruments, and the comfort features are absent.

This node captures everything from "the loop can hang forever" to "I have no way to know if I'm stuck."

## Research

### The loop can run forever — no turn limits, no runaway detection

The agent loop runs `loop { ... }` with exactly two exits: CancellationToken is cancelled, or the LLM returns no tool calls. If the LLM gets stuck in a cycle (read file → try edit → edit fails → read file → repeat), it runs forever, burning tokens with no escape.

pi's loop has the same structural issue but mitigates it with `getSteeringMessages` — the human can interrupt. The Rust loop has no steering mechanism at all. For a headless cleave child, there's nobody to interrupt.

**What I'd want:**
- A configurable max-turn limit (default: 50 for headless, 200 for interactive)
- A soft wall at N turns: inject a system message — "You've been running for N turns. Summarize progress and either complete or explain what's blocking you."
- A hard wall at 2N turns: force-stop with an error, save conversation state for debugging
- Loop detection: if the same tool is called 3+ times in a row with the same arguments, inject a warning: "You appear to be repeating the same action. Try a different approach."

### LLM errors crash the session — no retry, no fallback

When the bridge returns an error, the loop does `anyhow::bail!("LLM error: {message}")` — the entire session dies. No retry, no backoff, no fallback model.

The user just experienced exactly this problem: repeated `transient_server_error` from Anthropic's API. In the current Omegon, the recovery extension handles retries. In the Rust loop, there's nothing.

LLM errors are overwhelmingly transient. The loop should:
1. **Retry transient errors** (500, 502, 503, 529/overloaded, rate limit) with exponential backoff. Default: 3 attempts with 2s/4s/8s delays.
2. **Distinguish error categories**: transient (retry same model), model-specific (try fallback), bridge crash (restart bridge process), auth expired (re-resolve API key).
3. **On persistent failure**: don't crash. Inject an error message into the conversation: "LLM call failed after 3 retries: {reason}. The session will pause." For interactive sessions, this lets the human decide. For headless, save state and exit with a specific error code.
4. **Bridge process health**: if the Node.js subprocess dies, restart it automatically. The bridge is stateless — restarting is safe.

### I have no self-awareness — context budget, progress, or whether I'm stuck

The loop tracks stats in `IntentDocument.stats` (turns, tool_calls, tokens_consumed) but never feeds this information back to the LLM. I'm running blind:

- I don't know how much context I've consumed. I might read a 500-line file when I'm at 90% of the context window.
- I don't know if I'm making progress. Turn 45 feels the same as turn 5.
- I don't know how much this session has cost. $0.50 and $50 look the same from inside.
- I don't know if I'm approaching compaction. I can't preemptively avoid it.

**What I'd want:** A session HUD injected into the system prompt on every turn:

```
[Session: turn 12 | context: 45k/200k tokens | 3 files read, 2 modified | est. cost: $0.42 | 8m elapsed]
```

This isn't vanity — it's decision-relevant. If I know I'm at 80% context, I'll use targeted reads instead of reading whole files. If I know I've been running for 30 turns on what should be a 5-turn task, I'll reconsider my approach.

The ContextManager already runs before every LLM call — it's the natural place to inject this. The loop tracks the stats — it just needs to render them as a context injection.

### The ContextManager exists but nothing plugs into it

The ContextManager has a clean architecture: `Vec<Box<dyn ContextProvider>>` that are queried per-turn, with signal-driven injection based on recent tools, files, and lifecycle phase. But:

1. **Zero providers are registered.** The `providers` vec is always empty.
2. **`record_tool_call()` is never called.** The `recent_tools` deque stays empty.
3. **`record_file_access()` is never called.** The `recent_files` deque stays empty.
4. **The context budget is hardcoded** to `4000` tokens and never used for actual budgeting.

The infrastructure exists; nothing exercises it. The loop calls `context.update_phase_from_activity(tool_calls)` but not `context.record_tool_call()` — the phase updates but the signal tracking doesn't.

**Fix:** Wire `record_tool_call` and `record_file_access` in the loop after tool dispatch. Implement at least one built-in ContextProvider: the session HUD from the self-awareness research.

### The bridge emits null events — wasted wire traffic

In `llm-bridge.mjs`, `slimEvent()` returns `null` for `done` and `error` events. But the `for await` loop unconditionally sends every slimmed event:

```javascript
for await (const event of eventStream) {
  send({ id, event: slimEvent(event) });
}
```

When `slimEvent` returns `null`, this sends `{"id":1,"event":null}` over the wire. The Rust side deserializes `event: null` as `None` and the routing task processes a no-op BridgeResponse (no event, no result, no error). The real `done` event is sent separately after the `for await` loop via the `toOmegonAssistantMessage` path.

Not a bug — the system works — but it's one wasted ndjson line per stream. Fix: `const slim = slimEvent(event); if (slim) send({ id, event: slim });`

### Tool dispatch is sequential only — independent tool calls waste wall-clock time

The loop dispatches tool calls one at a time in `dispatch_tools`. pi's agent loop supports both sequential and parallel modes (default: parallel). When the LLM returns three independent `read` calls, they could run concurrently via `tokio::join!` or `FuturesUnordered`.

For Phase 0 cleave children this is acceptable — most cleave tasks are dominated by LLM latency, not tool execution. But for Phase 1 interactive sessions, parallel tool dispatch matters: three file reads in 50ms instead of 150ms, or three bash commands in the time of the slowest.

**Design:** Default to parallel dispatch. Execute all tool calls concurrently via `tokio::JoinSet`. Results are collected in the original order (matching pi's behavior). The cancellation token propagates to all concurrent tools — if one fails fatally, the rest are cancelled. Sequential mode available as a flag for tools that have side effects requiring ordering.

### I can't observe the effects of my mutations — validation is always a separate decision

When I edit a file, I get: "Successfully replaced text in foo.rs." I don't know if it compiles. I don't know if tests pass. I have to explicitly call `bash cargo check` — and I often forget, or defer it to "save time," and then broken code ships.

This is the single most common failure mode in my daily work. The human discovers broken code because I forgot to validate. Every time.

**What I'd want in this shell:**

After every `edit` or `write` to a source file, the tool itself runs a lightweight validation and includes the result:

```
Successfully replaced text in src/auth.rs.
Validation: `cargo check` — 0 errors, 0 warnings
```

Or:
```
Successfully replaced text in src/auth.rs.
Validation: `cargo check` — 1 error:
  src/auth.rs:42: expected `AuthToken`, found `String`
```

This isn't the full `change` tool from the design — it's a simpler version: automatic post-mutation validation as part of the tool result. The validation command is discovered per-project (Cargo.toml → cargo check, tsconfig.json → tsc --noEmit) and cached. It runs in the background while the edit tool response is being assembled.

For Phase 0 headless: keep it optional (controlled by a `--validate` flag). For Phase 1 interactive: default on.

The deeper version (the `change` tool with atomic multi-file edits and configurable validation levels) comes later. But even the simple "run the checker after every edit" would catch 80% of my mistakes.

### The model is hardcoded — --model flag doesn't reach the bridge

The CLI accepts `--model` (default: `anthropic:claude-sonnet-4-20250514`) but it's never passed through to the bridge. The `SubprocessBridge::stream()` hardcodes the model string in the params JSON. This means `omegon-agent --model openai:gpt-5.2` would silently use Claude anyway.

Simple fix: pass the model string from the CLI through to the bridge's `stream()` method. The bridge JS already handles model resolution via `resolveModel()`.

### What I'd want most: the ability to detect that I'm stuck

The deepest desire: I want the loop to notice patterns that indicate I'm struggling, and tell me. Not judge me — inform me.

**Patterns the loop can detect from tool call history alone:**

1. **Read-loop:** Same file read 3+ times in N turns without modification. Likely: I keep forgetting what's in it, or I'm hoping for a different result.
   → Inject: "You've read {file} {N} times without modifying it. Consider noting what you need from it."

2. **Edit-fail-read-edit cycle:** Edit fails → read the file → edit fails again → read again. Likely: the text I'm looking for doesn't exist as written.
   → Inject: "Your last {N} edits to {file} failed. The file may have changed or your expected text may be wrong. Consider reading the current state."

3. **Bash retry:** Same bash command run 3+ times. Likely: hoping for a different result (test flake) or stuck on a command that won't work.
   → Inject: "You've run `{cmd}` {N} times. If it's flaky, note the pattern. If it's consistently failing, try a different approach."

4. **Context thrashing:** Alternating between two different areas of the codebase without making changes. Likely: can't figure out how they connect.
   → Inject: "You've been alternating between {area1} and {area2}. Consider mapping the relationship before continuing."

5. **Silence after error:** A tool returns an error and the LLM's next response doesn't acknowledge it. Likely: the error was in the context but I didn't notice it.
   → The loop doesn't need to detect this — it needs to make errors more salient. Bold the error in the tool result, or inject a one-line reminder.

**Implementation:** A `StuckDetector` struct that the loop updates after every tool call. It maintains a sliding window of recent (tool_name, args_hash) pairs and pattern-matches against known stuck signatures. When a pattern matches, it returns an `Option<String>` that the loop injects as a system message before the next LLM call.

This is not AI. It's a finite state machine watching for pathological patterns. The "intelligence" is in choosing which patterns to detect — and that's exactly the kind of thing the agent occupying this loop knows best.

### Session 2026-03-27: Error classification hierarchy and structural recovery

The error handling system now has a three-tier classification in `stream_with_retry()` and the calling loop:

**Tier 1 — Transient (retry with backoff):**
Handled inside `stream_with_retry()`. Matches: rate limit, overloaded, timeout, 500/502/503/529, "too many requests". Retries up to `max_retries` with exponential backoff.

Guard: `is_context_overflow()` and `is_malformed_history()` are explicitly EXCLUDED from transient classification to prevent blind retries on structural errors.

**Tier 2 — Context overflow (compact + retry):**
Caught by the loop after `stream_with_retry()` returns an error. Matches: "long context", "context length", "token limit", "request too large", "extra usage" + "context". Triggers emergency compaction → decay fallback → message rebuild → single retry.

**Tier 3 — Malformed history (decay + retry):**
Also caught by the loop. Matches: "tool_use_id", "tool_result", "thinking.signature", "role must alternate", "field required", "does not match pattern". Triggers aggressive decay (drop first half of history) → message rebuild → single retry.

**Terminal:** Anything not matching tiers 1-3 propagates as an error to the operator.

This covers every failure mode encountered during cross-provider model switching (Codex→Anthropic, GPT→Claude) including tool ID format mismatches, unsigned thinking blocks, orphaned tool results, and context overflow.

## Decisions

### Decision: Implement in priority order: turn limits → retry → context wiring → stuck detection → parallel dispatch

**Status:** decided
**Rationale:** Turn limits and retry are safety-critical for the cleave child use case — a runaway headless agent is worse than a crash. Context wiring is cheap and enables everything else. Stuck detection is the high-value novel capability. Parallel dispatch is nice but not urgent for Phase 0.

## Open Questions

*No open questions.*
