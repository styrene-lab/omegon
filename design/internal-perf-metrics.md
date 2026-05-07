+++
id = "928f1686-3dda-46a8-9190-dfdcb229e938"
kind = "design_node"
title = "Internal performance metrics — wall-clock timing across the agent loop"
status = "exploring"
tags = ["performance", "metrics", "timing", "observability", "agent-loop", "internal"]
aliases = ["internal-perf-metrics"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "feature"
open_questions = ["Should timing fields be added to existing TurnEnd events or emitted as separate PerfEvent?", "Is tracing::Span timing sufficient, or do we need explicit Instant-based fields on events?", "Should /bench show per-turn breakdown or just session aggregates?"]
parent = "null"
priority = "3"
+++

# Internal performance metrics — wall-clock timing across the agent loop

## Problem

Omegon measures **counts** well (tokens, turns, tool calls, facts, compactions) but lacks **wall-clock timing** at every layer that matters for diagnosing slowness:

- No per-turn duration (loop start → turn end)
- No LLM call latency or TTFT (time to first token)
- No tool-call-to-result duration on events (tools track `elapsed_ms` internally via `ToolProgress`, but `AgentEvent::ToolEnd` has no timestamp)
- No context assembly / prompt-building time
- No compaction summarization call duration
- No memory operation latency (fact store, retrieval, embedding)

The `/bench` command shows session-level averages (total session time / turn count) but can't show per-turn breakdown, slow-tool identification, or provider latency trends. When a session feels slow, there's no data to diagnose *where* the time went.

## What already exists

### Measured now
- Session elapsed time (`session_start: Instant` in TUI)
- Per-tool `elapsed_ms` in `ToolProgress` heartbeats (bash, local_inference)
- Cleave child `started_at_unix_ms`, `last_activity_unix_ms`, `duration_secs`
- Provider quota/rate-limit telemetry (Anthropic utilization %, OpenAI remaining tokens)
- Token counts per turn (`actual_input_tokens`, `actual_output_tokens`, `cache_read_tokens`)
- `/bench` output: RSS, process age, avg turn time (session ÷ turns), tokens/turn

### Not measured
| Gap | Where it matters |
|-----|-----------------|
| Per-turn wall-clock duration | "Was that turn slow or did I imagine it?" |
| LLM streaming latency / TTFT | "Is the provider slow or is my prompt huge?" |
| Tool execution duration on events | "Which tool call took 45 seconds?" |
| Context assembly time | "Is prompt building a bottleneck at high context %?" |
| Compaction LLM call duration | "How long did that tier-2 compaction take?" |
| Memory retrieval latency | "Is fact injection slowing down context assembly?" |

## Design

### Principle: instrument at the seams, not inside the guts

Add `Instant::now()` captures at the boundaries between phases — not deep inside provider clients or memory backends. This gives us the timing data without coupling to internal implementations.

The five instrumentation points:

### 1. Per-turn timing in the agent loop

**File:** `core/crates/omegon/src/loop.rs`

Add `let turn_start = Instant::now();` at the top of each loop iteration (after `turn += 1`, ~line 253). Capture `turn_duration_ms` before emitting `TurnEnd`:

```rust
let turn_duration_ms = turn_start.elapsed().as_millis() as u64;
```

Add `turn_duration_ms: u64` to both `AgentEvent::TurnEnd` and `BusEvent::TurnEnd` in `omegon-traits/src/lib.rs`. Default to 0 for backwards compatibility with existing consumers.

This is the single highest-value metric — it tells you how long the LLM + tool dispatch + context assembly took per turn, with no ambiguity.

**Files touched:**
- `core/crates/omegon/src/loop.rs` — `Instant::now()` at loop top, elapsed at each `TurnEnd` emit site (~6 sites)
- `core/crates/omegon-traits/src/lib.rs` — `turn_duration_ms: u64` on `AgentEvent::TurnEnd` and `BusEvent::TurnEnd`

### 2. LLM call timing (TTFT + total streaming)

**File:** `core/crates/omegon/src/loop.rs` (around the `stream_with_retry` call, ~line 515)

The LLM call is the dominant cost of most turns. Two measurements:

- `llm_ttft_ms` — time from request dispatch to first streaming token
- `llm_total_ms` — time from request dispatch to final response

Wrap the `stream_with_retry` call:

```rust
let llm_start = Instant::now();
let assistant_msg = stream_with_retry(...).await;
let llm_total_ms = llm_start.elapsed().as_millis() as u64;
```

TTFT requires a hook inside the streaming path. The `AgentEvent::MessageDelta` already fires on each token — track when the first one arrives:

```rust
let llm_first_token: Option<Instant> = None;
// In the streaming callback:
if llm_first_token.is_none() {
    llm_first_token = Some(Instant::now());
}
let llm_ttft_ms = llm_first_token.map(|t| (t - llm_start).as_millis() as u64).unwrap_or(0);
```

Add `llm_ttft_ms` and `llm_total_ms` to `TurnEnd` events.

**Files touched:**
- `core/crates/omegon/src/loop.rs` — timing around `stream_with_retry`
- `core/crates/omegon-traits/src/lib.rs` — fields on `TurnEnd`

### 3. Tool execution duration on events

**File:** `core/crates/omegon/src/loop.rs` (tool dispatch section, ~line 829)

Tools already track `elapsed_ms` via `ToolProgress`, but the `AgentEvent::ToolEnd` event doesn't carry duration. The TUI can't show "this bash call took 38s" without parsing progress heartbeats.

Add `duration_ms: u64` to `AgentEvent::ToolEnd`:

```rust
// At ToolStart:
let tool_start = Instant::now();

// At ToolEnd:
let duration_ms = tool_start.elapsed().as_millis() as u64;
```

The TUI already renders `ToolEnd` — it can display duration inline in the tool card.

**Files touched:**
- `core/crates/omegon/src/loop.rs` — `Instant::now()` at tool dispatch, elapsed at result
- `core/crates/omegon-traits/src/lib.rs` — `duration_ms: u64` on `ToolEnd` event variants

### 4. Enhanced `/bench` output

**File:** `core/crates/omegon/src/tui/mod.rs` (~line 4342)

With per-turn timing available, `/bench` can show richer data. Track a rolling window (last 10 turns) of `turn_duration_ms` and `llm_total_ms`:

```
Omegon Performance — v0.17.0

Startup
────────────────────────────────
Process age:        342s
RSS memory:         198.3 MB

Session (14 turns)
────────────────────────────────
Model:              anthropic:claude-sonnet-4-6
Avg turn:           8.2s
Slowest turn:       34.1s (turn 7)
Fastest turn:       1.8s (turn 12)
LLM avg TTFT:       1.1s
LLM avg total:      6.4s
Tool time:          24% of total
Input tokens:       48,201
Output tokens:      12,847
Tokens/turn:        4,360
Context:            42% of 272,000
Compactions:        0

Last 5 turns
────────────────────────────────
 #10   3.2s  (LLM 2.8s · tools 0.4s · 2,100 tok)
 #11   8.7s  (LLM 3.1s · tools 5.6s · 4,800 tok)
 #12   1.8s  (LLM 1.6s · tools 0.2s · 1,200 tok)
 #13  12.4s  (LLM 4.2s · tools 8.2s · 6,100 tok)
 #14   4.1s  (LLM 3.4s · tools 0.7s · 2,900 tok)
```

This requires a `Vec<TurnPerfRecord>` accumulator in the TUI state:

```rust
struct TurnPerfRecord {
    turn: u32,
    duration_ms: u64,
    llm_ttft_ms: u64,
    llm_total_ms: u64,
    tool_time_ms: u64,  // sum of tool durations this turn
    input_tokens: u64,
    output_tokens: u64,
}
```

Populated from `AgentEvent::TurnEnd` in the TUI's event handler.

**Files touched:**
- `core/crates/omegon/src/tui/mod.rs` — `TurnPerfRecord` struct, accumulator, `/bench` renderer

### 5. Slow-turn indicator in status line

**File:** `core/crates/omegon/src/tui/statusline.rs`

When a turn exceeds 15s (or 2x the session average), flash a brief indicator in the status line:

```
 42% · T14 · sonnet · 48K/12K · main · ⏱ 34s
```

The `⏱ 34s` appears only during the active turn (while streaming) and for 3 seconds after `TurnEnd`. This gives the operator real-time feedback without requiring them to run `/bench`.

**Files touched:**
- `core/crates/omegon/src/tui/statusline.rs` — conditional duration display
- `core/crates/omegon/src/tui/mod.rs` — feed active-turn elapsed into status line

## Implementation order

```
  [1] Per-turn timing     — highest value, simplest change (Instant + field)
       ↓
  [2] LLM call timing     — second highest value, needs streaming hook
       ↓
  [3] Tool duration        — additive field on existing event
       ↓
  [4] Enhanced /bench      — consumes [1]-[3], display-only change
       ↓
  [5] Status line indicator — consumes [1], small UI polish
```

[1] and [3] can be implemented in parallel. [2] depends on understanding the streaming path. [4] and [5] depend on [1].

## Non-goals

- **External metrics export** (Prometheus, OpenTelemetry) — useful but a separate workstream. This design adds the *data*; export surfaces can consume it later.
- **Memory operation timing** — the memory crate is a separate concern. Per-turn timing will reveal if memory injection is a bottleneck; fine-grained memory profiling can follow if it is.
- **Provider-level retry timing** — `stream_with_retry` handles transient failures. The `llm_total_ms` measurement includes retries, which is what the operator cares about. Per-retry breakdown is a debugging concern, not an operator concern.

## Success criteria

- `/bench` shows per-turn breakdown with LLM vs tool time split
- A 30-second turn is identifiable as "LLM slow" vs "bash tool slow" without parsing logs
- Status line shows active-turn elapsed time during long turns
- No measurable performance overhead from the instrumentation (Instant::now is ~25ns)
