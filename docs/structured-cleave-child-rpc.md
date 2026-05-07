+++
id = "1f062777-2e08-42f7-a54d-ef1cd0909809"
kind = "document"
title = "Structured RPC/event transport for cleave child execution"
status = "decided"
tags = []
aliases = ["structured-cleave-child-rpc"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "cross-provider-session-telemetry-schema"
+++

# Structured RPC/event transport for cleave child execution

## Overview

> Parent: [Cross-provider session telemetry schema for replay and inspection](cross-provider-session-telemetry-schema.md)
> Spawned from: "Should cleave child execution keep using spawned CLI subprocesses with stderr scraping, or move to a structured event/RPC channel with semantic heartbeats and typed progress events?"

*To be explored.*

## Research

### Assessment of the harness agent's explanation

The agent's explanation is **partly right but incomplete**.

What it gets right:
- Using `bash` to invoke `omegon cleave ...` from inside an existing session is the wrong outer execution path when the intent is to exercise the harness's internal capability surface.
- The correct top-level harness path is the `cleave_run` tool, not an external CLI round-trip.

What it misses:
- `cleave_run` does **not** eliminate the underlying brittleness. In `features/cleave.rs`, `execute_run()` calls `cleave::run_cleave(...)` directly, but the orchestrator still spawns full child processes via `current_exe()` in `cleave/orchestrator.rs`.
- Parent/child coordination still relies on **stderr scraping** for liveness/activity (`dispatch_child()` watches child stderr and `parse_child_activity()` infers turns/tool calls from tracing text).
- Child stdout is not consumed as a live structured event stream; it is harvested only after exit.
- Idle timeout is based on lack of stderr output, not on semantic heartbeat or typed progress. So local-model cold starts and long silent reasoning phases still look like hangs.

Therefore:
- The agent correctly identified an **outer-path misuse**.
- But the deeper issue is a **transport design problem inside cleave itself**.
- Switching from `bash omegon cleave ...` to `cleave_run` is necessary for correctness at the harness layer, but it is not sufficient to solve the reliability/observability problem you were actually noticing.

### Assessment of internal cleave_run screenshot

The screenshot shows a second, clearer transport bug: **internal `cleave_run` leaks orchestration NDJSON directly into the active TUI**.

Evidence from implementation:
- `features/cleave.rs::execute_run()` invokes `cleave::run_cleave(...)` directly inside the running harness process.
- `cleave/progress.rs::emit_progress()` is hard-wired to write JSON lines to **process stdout**.
- That stdout transport was designed for the external TS `native-dispatch.ts` wrapper (`progress.rs` says so explicitly).
- When `cleave_run` is executed *inside* the TUI, those stdout writes are no longer an out-of-band machine channel; they are competing with ratatui for terminal ownership.

What the screenshot is showing:
- Raw events like `{"event":"child_spawned",...}` and `{"event":"child_activity",...}` appearing inside the inference/tools panels.
- This is not merely cosmetic noise; it proves the current progress channel is context-sensitive and unsafe. The same orchestrator behaves differently depending on whether it is run from an external wrapper or from the internal harness.

Assessment:
1. The current progress transport is **not composable** — it assumes stdout is always a machine-consumed side channel.
2. `cleave_run` therefore cannot be considered a clean internal primitive yet, because using it from inside the harness corrupts the UI surface.
3. This strengthens the earlier conclusion: progress/event emission must become a pluggable transport (stdout sink, in-process event sink, RPC stream, file sink, etc.) rather than a globally hard-coded `println!` side effect.

Short version: the screenshot is a smoking gun that the cleave orchestration layer was built for one embedding mode (external wrapper) and is being reused in another embedding mode (interactive TUI) without an abstraction boundary.

## Decisions

### Decision: Using cleave_run is the correct harness entrypoint, but child transport still needs structured RPC/events

**Status:** exploring
**Rationale:** There are two layers here. At the top layer, the harness should invoke `cleave_run` instead of shelling out through `bash` to `omegon cleave ...`; that preserves the tool contract and keeps the run inside the harness surface. But at the lower layer, `cleave_run` still delegates to the same subprocess-based child model: full child Omegon processes, stderr scraping for activity, stdout harvested at exit, and idle timeout keyed to log silence. The architectural problem is therefore not merely the wrong command path; it is that the current child transport is weak. Fixing the outer call path improves correctness and UX, but the long-term solution is a typed event/RPC channel with semantic heartbeats.

### Decision: Keep subprocess child isolation for now, but make progress transport embedding-aware

**Status:** decided
**Rationale:** The immediate bug is not child isolation itself; it is that cleave progress emission was globally hard-wired to process stdout. That works for the external CLI/native-dispatch embedding, but it corrupts the interactive TUI when `cleave_run` is invoked inside the harness. The fix is to preserve the current subprocess model for child execution while introducing a pluggable progress sink. External CLI callers continue using a stdout-backed NDJSON sink, while internal harness execution routes progress into shared in-process state. This removes terminal corruption now and establishes the transport boundary needed for future richer RPC/telemetry work.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/cleave/progress.rs` (modified) — Introduce embedding-aware progress sink abstraction with stdout and callback sinks.
- `core/crates/omegon/src/cleave/orchestrator.rs` (modified) — Thread progress sink through orchestration and child dispatch so progress emission is no longer hard-wired to stdout.
- `core/crates/omegon/src/features/cleave.rs` (modified) — Route internal cleave_run progress into shared in-process cleave dashboard state and test event application.
- `core/crates/omegon/src/main.rs` (modified) — Keep external CLI cleave path on stdout-backed NDJSON progress sink.

### Constraints

- Preserve current subprocess child-execution model for this fix.
- Keep external CLI/native-dispatch stdout NDJSON compatibility while preventing internal TUI corruption.
- Treat richer semantic heartbeats / full RPC transport as follow-on work beyond this immediate fix.
