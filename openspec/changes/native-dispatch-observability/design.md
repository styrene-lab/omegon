# native-dispatch-observability — Design

## Spec-Derived Architecture

### progress

- **Rust orchestrator emits NDJSON progress events on stdout** (added) — 3 scenarios
- **Child activity events from agent tool calls** (added) — 2 scenarios
- **TS wrapper maps progress events to dashboard state** (added) — 2 scenarios

## File Changes

### Rust side (core/)

1. **`core/crates/omegon/src/cleave/progress.rs`** (new)
   - `ProgressEvent` enum with serde Serialize (tag = "event")
   - `emit_progress(event: &ProgressEvent)` — serialize to JSON, println! to stdout
   - Variants: WaveStart, ChildSpawned, ChildStatus, ChildActivity, AutoCommit, MergeStart, MergeResult, Done

2. **`core/crates/omegon/src/cleave/orchestrator.rs`** (modified)
   - Import and call `emit_progress()` at each lifecycle point
   - Parse child stderr for tool-call patterns (`→ write`, `→ bash`, `→ read`, `→ edit`) and turn markers (`Turn N`)
   - Per-child activity throttle: HashMap<label, Instant> — skip if < 1s since last activity event

3. **`core/crates/omegon/src/cleave/mod.rs`** (modified)
   - `pub mod progress;`

### TS side (extensions/)

4. **`extensions/cleave/native-dispatch.ts`** (modified)
   - Read stdout as line-buffered stream
   - Parse each line as JSON → ProgressEvent
   - Map events to `emitCleaveChildProgress()` calls via a callback
   - Remove stdout string accumulation (was unused)

5. **`extensions/cleave/index.ts`** (modified)
   - Pass `pi` reference into the progress callback so it can call `emitCleaveChildProgress`
   - Map child labels to childId indexes for the callback

## Decisions

- stdout = JSON progress channel. No other output allowed on stdout.
- stderr = tracing diagnostics (unchanged).
- Activity events throttled to 1/sec/child.
- Tool-call parsing in Rust: look for `→ ` prefix in child stderr lines.
- Events are self-contained — no cross-event state needed.

## Constraints

- The `child` field in events must exactly match `ChildState.label` from the plan.
- `child_activity` events must include either `tool` + `target` or `turn` (not both).
