# Cleave child timeout and idle detection — Design Spec (extracted)

> Auto-extracted from docs/cleave-child-timeout.md at decide-time.

## Decisions

### Use RPC event-gap idle timeout alongside reduced wall-clock cap (exploring)

Two-tier timeout: (1) a wall-clock cap reduced from 2h to ~15 minutes for default children (configurable), and (2) an idle timeout of ~3 minutes with no RPC events. The idle timer resets on every RPC event (tool_start, tool_end, assistant_message). If the idle timer fires, the child is killed with a clear error message. The wall-clock cap is the hard backstop.

This catches both failure modes: children stuck in inference loops (idle — no tool calls) and children that are technically active but going nowhere (wall clock). The 3-minute idle window is generous enough that normal LLM thinking pauses won't trigger it, but short enough that a stalled child dies in minutes, not hours.

For pipe mode (legacy/review), fall back to wall-clock only since there's no structured event stream.

### Kill idle children immediately rather than sending a graceful signal (exploring)

A "please wrap up" RPC message to a stuck child has two problems: (1) if the child's LLM is in a stalled inference, the message sits unread in stdin; (2) it adds complexity for a case where the child has already failed to produce useful work. Clean kill + preserved worktree is better — the parent can log what happened, and the operator or a retry can pick up from the branch state. The existing pipe-break handling already preserves worktrees for recovery.

### RPC event-gap idle detection, flat 15-min wall clock, kill on idle (decided)

Answers all three open questions: (1) RPC event absence only — git polling is expensive and unreliable. (2) Flat 15-min wall clock — complexity-proportional sizing adds planning overhead for marginal benefit; 15 min covers legitimate large tasks while cutting worst-case from 2h. (3) Kill outright — a graceful signal to a stalled child is unlikely to be received. Idle timeout of 3 minutes with reset on any RPC event. Pipe mode keeps wall-clock only.

## Research Summary

### Failure analysis: chronos-native-ts cleave run

3-child cleave plan: core (chronos.ts) → integration (index.ts rewrite) → tests (chronos.test.ts). Child 0 correctly recognized the work was inseparable and implemented all three in one commit (573 lines, 25 tests). Children 1 and 2 depended on child 0 but had no remaining work. They ran for 1731s each before the RPC pipe broke.

**Why the pipe broke at 29 min**: The child pi process was likely stuck in an LLM inference loop (repeatedly calling Claude to "do" work that was already done), eventua…

### Current timeout architecture

- `dispatchChildren()` receives `childTimeoutMs` — currently hardcoded to `120 * 60 * 1000` (2 hours) at the `cleave_run` tool call site in index.ts:2583.
- The timeout fires in `spawnChildRpc()` / `spawnChildPipe()` via a `setTimeout` that kills the child process.
- No idle detection: if the child is alive but making no progress (no git commits, no tool calls, no output), the parent waits the full timeout.
- RPC mode provides structured events (`tool_start`, `tool_end`, `assistant_message`, etc…

### Available signals for idle/stall detection

RPC mode gives us a rich event stream from the child. Events include:
- `tool_start` / `tool_end` — the child is calling tools (active work)
- `assistant_message` — the child's model is emitting text (thinking/planning)  
- `result` — the child has finished its turn

**Idle detection via RPC event gap**: If no events arrive for N seconds, the child is likely stalled. This is the simplest and most reliable signal — it works regardless of whether the child is stuck in inference, waiting for a cras…
