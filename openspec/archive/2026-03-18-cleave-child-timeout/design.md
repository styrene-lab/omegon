+++
id = "6aa4e7b5-e528-445f-8ed8-c2cf0f93d893"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave child timeout and idle detection — Design

## Architecture Decisions

### Decision: Activity-gap idle timeout alongside reduced wall-clock cap

**Status:** decided
**Rationale:** Two-tier timeout: (1) a wall-clock cap reduced from 2h to 15 minutes for default children (configurable), and (2) an idle timeout of 3 minutes with no child activity. In the Rust native orchestrator (primary path), activity = stderr output lines. In the TS resume path, activity = RPC events. If the idle timer fires, the child is killed immediately. The wall-clock cap is the hard backstop.

This catches both failure modes: children stuck in inference loops (idle — no tool calls) and children that are technically active but going nowhere (wall clock). The 3-minute idle window is generous enough that normal LLM thinking pauses won't trigger it, but short enough that a stalled child dies in minutes, not hours.

### Decision: Kill idle children immediately rather than sending a graceful signal

**Status:** decided
**Rationale:** A graceful "wrap up" message to a stuck child is unlikely to be received (stalled inference, closed stdin). Clean kill + preserved worktree is better — the parent logs what happened, and the operator or a retry can pick up from the branch state. Both Rust (kill_on_drop + explicit kill) and TS (killCleaveProc) implement immediate kill.

### Decision: Dual-backend idle detection — Rust stderr monitoring as primary, TS RPC as resume fallback

**Status:** decided
**Rationale:** The primary cleave_run path uses the Rust orchestrator (dispatchViaNative), which monitors stderr line output as the activity signal. The TS dispatchChildren path with RPC event-gap detection remains reachable only via the resume code path. Both backends enforce the same timeout constants (3-min idle, 15-min wall clock) and the same kill semantics. idle_timeout_ms is threaded through to both.

## Research Context

### Failure analysis: chronos-native-ts cleave run

3-child cleave plan: core (chronos.ts) → integration (index.ts rewrite) → tests (chronos.test.ts). Child 0 correctly recognized the work was inseparable and implemented all three in one commit (573 lines, 25 tests). Children 1 and 2 depended on child 0 but had no remaining work. They ran for 1731s each before the RPC pipe broke.

**Why the pipe broke at 29 min**: The child pi process was likely stuck in an LLM inference loop (repeatedly calling Claude to "do" work that was already done), eventually exhausting context or hitting a provider-side timeout. The subprocess stdout closed, triggering the pipe_break detection. But the 2-hour timeout hadn't fired yet, so the parent was content to wait.

**Cost**: ~58 minutes of wasted wall time, plus API token spend for two children doing nothing useful.

### Current timeout architecture

- `dispatchChildren()` receives `childTimeoutMs` — currently hardcoded to `120 * 60 * 1000` (2 hours) at the `cleave_run` tool call site in index.ts:2583.
- The timeout fires in `spawnChildRpc()` / `spawnChildPipe()` via a `setTimeout` that kills the child process.
- No idle detection: if the child is alive but making no progress (no git commits, no tool calls, no output), the parent waits the full timeout.
- RPC mode provides structured events (`tool_start`, `tool_end`, `assistant_message`, etc.) but these are only forwarded to the dashboard — never used to detect stalls.
- Pipe break detection exists but is reactive (catches the crash), not preventive.

### Available signals for idle/stall detection

RPC mode gives us a rich event stream from the child. Events include:
- `tool_start` / `tool_end` — the child is calling tools (active work)
- `assistant_message` — the child's model is emitting text (thinking/planning)  
- `result` — the child has finished its turn

**Idle detection via RPC event gap**: If no events arrive for N seconds, the child is likely stalled. This is the simplest and most reliable signal — it works regardless of whether the child is stuck in inference, waiting for a crashed tool, or just spinning.

**Git commit polling**: Expensive (spawn git every N seconds) and unreliable — a child might be doing useful work (running tests, reading files) without committing. Only useful as a secondary signal.

**Key insight**: The RPC event stream already exists and is consumed by the dashboard. Adding an idle timer alongside the existing event handler is minimal code. When an event arrives, reset the timer. When the timer fires, kill the child.

## File Changes

- `extensions/cleave/dispatcher.ts` (modified) — DEFAULT_CHILD_TIMEOUT_MS=15min, IDLE_TIMEOUT_MS=3min constants. Idle timer in spawnChildRpc() for TS resume path.
- `extensions/cleave/index.ts` (modified) — idle_timeout_ms param in cleave_run schema. Threads timeoutSecs/idleTimeoutSecs to native dispatch.
- `extensions/cleave/native-dispatch.ts` (new) — Spawns Rust omegon-agent cleave with --timeout and --idle-timeout args.
- `core/crates/omegon/src/cleave/orchestrator.rs` (new) — Rust idle timeout via tokio::time::timeout on stderr lines. Wall-clock timeout via tokio::select.

## Constraints

- Primary dispatch is Rust (dispatchViaNative) — idle detection monitors stderr lines
- TS resume path uses RPC event-gap detection (spawnChildRpc) — same timeout constants
- Idle timer must reset on ANY activity (stderr line or RPC event), not just specific event types
- Default idle timeout (3 min) must be generous enough for normal thinking pauses (60-90s)
- Wall-clock default (15 min) must allow legitimate large tasks but not 2 hours
