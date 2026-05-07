+++
id = "8df0185c-b779-4d5f-b999-d02f424d45c5"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# native-dispatch-observability — Tasks

## 1. Rust progress module + orchestrator integration
<!-- specs: progress -->

- [x] 1.1 Create `core/crates/omegon/src/cleave/progress.rs` with ProgressEvent enum (WaveStart, ChildSpawned, ChildStatus, ChildActivity, AutoCommit, MergeStart, MergeResult, Done) and `emit_progress()` fn that serializes to JSON and writes to stdout
- [x] 1.2 Add `pub mod progress;` to `core/crates/omegon/src/cleave/mod.rs`
- [x] 1.3 Wire `emit_progress()` calls into `orchestrator.rs` at all lifecycle points: wave start, child spawn, child complete/fail, auto-commit, merge start, merge result, done
- [x] 1.4 Parse child stderr for tool-call patterns (`→ write`, `→ bash`, `→ read`, `→ edit`) and turn markers (`── Turn N ──`) — emit as ChildActivity events
- [x] 1.5 Add per-child activity throttle (HashMap<String, Instant>, skip if <1s since last)
- [x] 1.6 Ensure no non-JSON output reaches stdout (tracing subscriber must NOT write to stdout)
- [x] 1.7 `cargo build --release` succeeds with no errors

## 2. TS native-dispatch progress parsing
<!-- specs: progress -->

- [x] 2.1 Modify `extensions/cleave/native-dispatch.ts` to read stdout as line-buffered NDJSON and invoke a new `onEvent` callback with parsed events
- [x] 2.2 Define `NativeProgressEvent` type union in native-dispatch.ts matching the Rust enum
- [x] 2.3 Modify `extensions/cleave/index.ts` to pass a progress callback that maps events → `emitCleaveChildProgress(pi, childId, patch)` using a label→index lookup
- [x] 2.4 Map: child_spawned → {status:"running", startedAt}, child_activity → {lastLine}, child_status completed → {status:"done", elapsed}, child_status failed → {status:"failed"}
- [x] 2.5 Add test in `extensions/cleave/native-dispatch.test.ts` for NDJSON parsing + event mapping
- [x] 2.6 `npx tsc --noEmit` passes
