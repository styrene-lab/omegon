+++
id = "9881f84b-5af9-4363-9d72-6e331c20f500"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave child timeout and idle detection

## Overview

Cleave children previously had a flat 2-hour timeout with no idle detection. When a child had no work (e.g. a sibling already completed it), or got stuck in a loop, it burned through the full timeout before failing. This was fixed with a two-tier timeout: a 15-minute wall-clock cap and a 3-minute idle timeout that resets on child activity.

## Research

### Failure analysis: chronos-native-ts cleave run

3-child cleave plan: core (chronos.ts) → integration (index.ts rewrite) → tests (chronos.test.ts). Child 0 correctly recognized the work was inseparable and implemented all three in one commit (573 lines, 25 tests). Children 1 and 2 depended on child 0 but had no remaining work. They ran for 1731s each before the RPC pipe broke.

**Why the pipe broke at 29 min**: The child pi process was likely stuck in an LLM inference loop (repeatedly calling Claude to "do" work that was already done), eventually exhausting context or hitting a provider-side timeout. The subprocess stdout closed, triggering the pipe_break detection. But the 2-hour timeout hadn't fired yet, so the parent was content to wait.

**Cost**: ~58 minutes of wasted wall time, plus API token spend for two children doing nothing useful.

## Decisions

### Decision: Activity-gap idle timeout alongside reduced wall-clock cap

**Status:** decided
**Rationale:** Two-tier timeout: (1) a wall-clock cap reduced from 2h to 15 minutes for default children (configurable), and (2) an idle timeout of 3 minutes with no child activity. In the Rust native orchestrator (primary path), activity = stderr output lines. In the TS resume path, activity = RPC events. If the idle timer fires, the child is killed immediately. The wall-clock cap is the hard backstop.

### Decision: Kill idle children immediately rather than sending a graceful signal

**Status:** decided
**Rationale:** A graceful "wrap up" message to a stuck child is unlikely to be received (stalled inference, closed stdin). Clean kill + preserved worktree is better — the parent logs what happened, and the operator or a retry can pick up from the branch state. Both Rust (kill_on_drop + explicit kill) and TS (killCleaveProc) implement immediate kill.

### Decision: Dual-backend idle detection — Rust stderr monitoring as primary, TS RPC as resume fallback

**Status:** decided
**Rationale:** The primary cleave_run path uses the Rust orchestrator (dispatchViaNative), which monitors stderr line output as the activity signal. The TS dispatchChildren path with RPC event-gap detection remains reachable only via the resume code path. Both backends enforce the same timeout constants (3-min idle, 15-min wall clock) and the same kill semantics. idle_timeout_ms is threaded through to both.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/cleave/dispatcher.ts` (modified) — DEFAULT_CHILD_TIMEOUT_MS=15min, IDLE_TIMEOUT_MS=3min constants. Idle timer in spawnChildRpc() for TS resume path.
- `extensions/cleave/index.ts` (modified) — idle_timeout_ms param in cleave_run schema. Threads timeoutSecs/idleTimeoutSecs to native dispatch.
- `extensions/cleave/native-dispatch.ts` (new) — Spawns Rust omegon-agent cleave with --timeout and --idle-timeout args.
- `core/crates/omegon/src/cleave/orchestrator.rs` (new) — Rust idle timeout via tokio::time::timeout on stderr lines. Wall-clock timeout via tokio::select.

### Constraints

- Primary dispatch is Rust (dispatchViaNative) — idle detection monitors stderr lines
- TS resume path uses RPC event-gap detection (spawnChildRpc) — same timeout constants
- Idle timer must reset on ANY activity (stderr line or RPC event), not just specific event types
- Default idle timeout (3 min) must be generous enough for normal thinking pauses (60-90s)
- Wall-clock default (15 min) must allow legitimate large tasks but not 2 hours
