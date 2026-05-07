+++
id = "5da37531-88ca-44b0-bce1-778d0dcdaa2c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Structured RPC/event transport for cleave child execution — Design Spec (extracted)

> Auto-extracted from docs/structured-cleave-child-rpc.md at decide-time.

## Decisions

### Using cleave_run is the correct harness entrypoint, but child transport still needs structured RPC/events (exploring)

There are two layers here. At the top layer, the harness should invoke `cleave_run` instead of shelling out through `bash` to `omegon cleave ...`; that preserves the tool contract and keeps the run inside the harness surface. But at the lower layer, `cleave_run` still delegates to the same subprocess-based child model: full child Omegon processes, stderr scraping for activity, stdout harvested at exit, and idle timeout keyed to log silence. The architectural problem is therefore not merely the wrong command path; it is that the current child transport is weak. Fixing the outer call path improves correctness and UX, but the long-term solution is a typed event/RPC channel with semantic heartbeats.

### Keep subprocess child isolation for now, but make progress transport embedding-aware (decided)

The immediate bug is not child isolation itself; it is that cleave progress emission was globally hard-wired to process stdout. That works for the external CLI/native-dispatch embedding, but it corrupts the interactive TUI when `cleave_run` is invoked inside the harness. The fix is to preserve the current subprocess model for child execution while introducing a pluggable progress sink. External CLI callers continue using a stdout-backed NDJSON sink, while internal harness execution routes progress into shared in-process state. This removes terminal corruption now and establishes the transport boundary needed for future richer RPC/telemetry work.

## Research Summary

### Assessment of the harness agent's explanation

The agent's explanation is **partly right but incomplete**.

What it gets right:
- Using `bash` to invoke `omegon cleave ...` from inside an existing session is the wrong outer execution path when the intent is to exercise the harness's internal capability surface.
- The correct top-level harness path is the `cleave_run` tool, not an external CLI round-trip.

What it misses:
- `cleave_run` does **not** eliminate the underlying brittleness. In `features/cleave.rs`, `execute_run()` calls `cleave::…

### Assessment of internal cleave_run screenshot

The screenshot shows a second, clearer transport bug: **internal `cleave_run` leaks orchestration NDJSON directly into the active TUI**.

Evidence from implementation:
- `features/cleave.rs::execute_run()` invokes `cleave::run_cleave(...)` directly inside the running harness process.
- `cleave/progress.rs::emit_progress()` is hard-wired to write JSON lines to **process stdout**.
- That stdout transport was designed for the external TS `native-dispatch.ts` wrapper (`progress.rs` says so explicitl…
