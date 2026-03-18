# Native dispatch observability — surface Rust child progress to dashboard — Design Spec (extracted)

> Auto-extracted from docs/native-dispatch-observability.md at decide-time.

## Decisions

### Use structured JSON progress events on stdout (Approach B) (decided)

Operator chose B for a clean versioned contract. Rust emits newline-delimited JSON on stdout for lifecycle events. TS parses JSON lines and calls emitCleaveChildProgress. Stdout is currently unused by the Rust binary — repurpose it as the progress channel. Stderr remains for tracing/diagnostic output.

### Surface lifecycle transitions plus tool-call summaries, not raw thinking (decided)

The dashboard should show: spawned, turn N, tool calls (→ write, → bash, → read), completed/failed with duration. Raw LLM thinking output is noisy and expensive to relay. The Rust orchestrator already sees child stderr which contains tracing lines like 'Turn 1', '→ write path' — it can parse these into structured events and emit them as JSON on stdout.

## Research Summary

### Current state — what the Rust binary emits

The Rust orchestrator uses `tracing::info!` with structured fields. Key lifecycle events on stderr:

```
INFO  child spawned  child=test-a  pid=21134
INFO  entering IO loop  child=test-a  wall_timeout_secs=120
INFO  stderr: [child's own agent output]  child=test-a  line_count=5
INFO  stderr EOF — child exited  child=test-b  line_count=90
INFO  child process exited  child=test-b  exit_code=Some(0)  success=true
INFO  child completed  child=test-a  duration="47s"
INFO  auto-committing uncommitted …

### Current state — what the dashboard consumes

`emitCleaveChildProgress(pi, childId, patch)` updates `sharedState.cleave.children[childId]` and fires `DASHBOARD_UPDATE_EVENT`. The dashboard footer reads:

- `child.status`: `"pending" | "running" | "done" | "failed"` → icon (○ ⟳ ✓ ✕)
- `child.startedAt`: timestamp → live elapsed counter
- `child.elapsed`: final ms → static time display
- `child.lastLine`: most recent activity string
- `child.recentLines`: ring buffer (cap 30) → shows last 2 lines for running children

The TS dispatcher (resum…

### Three approaches

**A. Parse tracing lines in TS (client-side parsing)**
The `onProgress` callback in `native-dispatch.ts` already receives every stderr line. Add a parser that recognizes key patterns (`child spawned`, `child completed`, `child failed`, `stderr:`, `dispatching wave`, `merge phase`) and calls `emitCleaveChildProgress` accordingly. No Rust changes needed.

Pros: Zero Rust changes, works with existing binary. Parsing is lightweight — just string matching on known tracing patterns.
Cons: Fragile coup…

### JSON progress event schema

Newline-delimited JSON on stdout. Each line is one event. Schema:

```jsonc
// Lifecycle events from the orchestrator
{"event":"wave_start","wave":0,"children":["test-a","test-b"]}
{"event":"child_spawned","child":"test-a","pid":21134}
{"event":"child_status","child":"test-a","status":"running"}
{"event":"child_activity","child":"test-a","turn":1}
{"event":"child_activity","child":"test-a","tool":"write","target":"tmp/foo.txt"}
{"event":"child_activity","child":"test-a","tool":"bash","target":"n…
