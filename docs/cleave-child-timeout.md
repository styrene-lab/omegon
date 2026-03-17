---
id: cleave-child-timeout
title: Cleave child timeout and idle detection
status: decided
tags: [cleave, reliability, timeout]
open_questions: []
---

# Cleave child timeout and idle detection

## Overview

Cleave children currently get a flat 2-hour timeout with no idle detection. When a child has no work (e.g. a sibling already completed it), or gets stuck in a loop, it burns through the full timeout before failing. The chronos-native-ts cleave run had children 1 and 2 hang for 29 minutes before RPC pipe break, consuming API tokens and wall clock time on zero-value work.

## Research

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

## Decisions

### Decision: Use RPC event-gap idle timeout alongside reduced wall-clock cap

**Status:** exploring
**Rationale:** Two-tier timeout: (1) a wall-clock cap reduced from 2h to ~15 minutes for default children (configurable), and (2) an idle timeout of ~3 minutes with no RPC events. The idle timer resets on every RPC event (tool_start, tool_end, assistant_message). If the idle timer fires, the child is killed with a clear error message. The wall-clock cap is the hard backstop.

This catches both failure modes: children stuck in inference loops (idle — no tool calls) and children that are technically active but going nowhere (wall clock). The 3-minute idle window is generous enough that normal LLM thinking pauses won't trigger it, but short enough that a stalled child dies in minutes, not hours.

For pipe mode (legacy/review), fall back to wall-clock only since there's no structured event stream.

### Decision: Kill idle children immediately rather than sending a graceful signal

**Status:** exploring
**Rationale:** A "please wrap up" RPC message to a stuck child has two problems: (1) if the child's LLM is in a stalled inference, the message sits unread in stdin; (2) it adds complexity for a case where the child has already failed to produce useful work. Clean kill + preserved worktree is better — the parent can log what happened, and the operator or a retry can pick up from the branch state. The existing pipe-break handling already preserves worktrees for recovery.

### Decision: RPC event-gap idle detection, flat 15-min wall clock, kill on idle

**Status:** decided
**Rationale:** Answers all three open questions: (1) RPC event absence only — git polling is expensive and unreliable. (2) Flat 15-min wall clock — complexity-proportional sizing adds planning overhead for marginal benefit; 15 min covers legitimate large tasks while cutting worst-case from 2h. (3) Kill outright — a graceful signal to a stalled child is unlikely to be received. Idle timeout of 3 minutes with reset on any RPC event. Pipe mode keeps wall-clock only.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/cleave/dispatcher.ts` (modified) — Add idle timer in spawnChildRpc() — reset on each RPC event, kill child when fired. Reduce default wall-clock timeout constant.
- `extensions/cleave/index.ts` (modified) — Change hardcoded 120*60*1000 to a configurable default (e.g. 15 min wall clock). Expose idle_timeout_ms as optional cleave_run param.

### Constraints

- Idle timeout only applies to RPC mode — pipe mode children keep wall-clock-only timeout
- Idle timer must reset on ANY RPC event, not just tool events (assistant_message counts as activity)
- Default idle timeout should be generous enough that normal thinking pauses (60-90s for complex reasoning) don't trigger false kills
- Wall-clock default should still allow legitimately large tasks (15 min) but not 2 hours
