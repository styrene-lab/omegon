---
id: native-dispatch-observability
title: Native dispatch observability — surface Rust child progress to dashboard
status: implementing
parent: cleave-child-timeout
tags: [cleave, dashboard, observability]
open_questions: []
branches: ["feature/native-dispatch-observability"]
openspec_change: native-dispatch-observability
---

# Native dispatch observability — surface Rust child progress to dashboard

## Overview

The Rust cleave orchestrator emits structured tracing::info! lines to stderr (child spawned, child completed, wave dispatching, merge phase, etc.) but the TS native-dispatch.ts wrapper treats them as opaque text — it forwards them to onProgress but never parses them into structured state updates. The dashboard footer already renders per-child status (icon, elapsed, recent activity lines) from sharedState.cleave.children[], but native dispatch never populates these fields during execution. Children stay as grey circles until the entire run completes and state.json is read back.

## Research

### Current state — what the Rust binary emits

The Rust orchestrator uses `tracing::info!` with structured fields. Key lifecycle events on stderr:

```
INFO  child spawned  child=test-a  pid=21134
INFO  entering IO loop  child=test-a  wall_timeout_secs=120
INFO  stderr: [child's own agent output]  child=test-a  line_count=5
INFO  stderr EOF — child exited  child=test-b  line_count=90
INFO  child process exited  child=test-b  exit_code=Some(0)  success=true
INFO  child completed  child=test-a  duration="47s"
INFO  auto-committing uncommitted changes  child=test-a  files=2
INFO  merge phase starting
INFO  merged successfully  child=test-a
```

The `child=` field is always present and matches the child label. The `line_count=` field on stderr lines shows activity. The `child completed` / `child failed` messages signal status transitions.

The tracing format uses the default `tracing_subscriber::fmt` format with ANSI colors. Fields are `key=value` pairs after the message text.

### Current state — what the dashboard consumes

`emitCleaveChildProgress(pi, childId, patch)` updates `sharedState.cleave.children[childId]` and fires `DASHBOARD_UPDATE_EVENT`. The dashboard footer reads:

- `child.status`: `"pending" | "running" | "done" | "failed"` → icon (○ ⟳ ✓ ✕)
- `child.startedAt`: timestamp → live elapsed counter
- `child.elapsed`: final ms → static time display
- `child.lastLine`: most recent activity string
- `child.recentLines`: ring buffer (cap 30) → shows last 2 lines for running children

The TS dispatcher (resume path) already populates all of these via RPC events. The native path populates none of them during execution — only after the Rust binary exits and state.json is read back, at which point children jump directly from pending→completed.

### Three approaches

**A. Parse tracing lines in TS (client-side parsing)**
The `onProgress` callback in `native-dispatch.ts` already receives every stderr line. Add a parser that recognizes key patterns (`child spawned`, `child completed`, `child failed`, `stderr:`, `dispatching wave`, `merge phase`) and calls `emitCleaveChildProgress` accordingly. No Rust changes needed.

Pros: Zero Rust changes, works with existing binary. Parsing is lightweight — just string matching on known tracing patterns.
Cons: Fragile coupling to tracing format. If tracing output changes (field order, message wording), parsing silently breaks. ANSI escape codes in tracing output must be stripped before matching.

**B. Structured JSON progress events from Rust (stdout or dedicated fd)**
Add a `--progress-format json` flag to the Rust binary. Emit structured JSON lines on stdout (or fd 3) for lifecycle events: `{"event":"child_spawned","child":"test-a","pid":1234}`, `{"event":"child_completed","child":"test-a","duration_secs":47}`. The TS wrapper parses JSON instead of regex-matching tracing lines.

Pros: Clean contract, easy to parse, version-safe. 
Cons: Requires Rust changes. Stdout is currently unused — could repurpose it.

**C. Poll state.json during execution**
The Rust binary persists `state.json` after each wave. The TS side could poll it every N seconds during dispatch and update dashboard state accordingly.

Pros: No parsing, no Rust changes, uses existing persistence.
Cons: Polling is wasteful, latency is coarse (only updates between waves, not per-child), misses intra-wave activity.

### JSON progress event schema

Newline-delimited JSON on stdout. Each line is one event. Schema:

```jsonc
// Lifecycle events from the orchestrator
{"event":"wave_start","wave":0,"children":["test-a","test-b"]}
{"event":"child_spawned","child":"test-a","pid":21134}
{"event":"child_status","child":"test-a","status":"running"}
{"event":"child_activity","child":"test-a","turn":1}
{"event":"child_activity","child":"test-a","tool":"write","target":"tmp/foo.txt"}
{"event":"child_activity","child":"test-a","tool":"bash","target":"npx tsc --noEmit"}
{"event":"child_status","child":"test-a","status":"completed","duration_secs":47.2}
{"event":"child_status","child":"test-b","status":"failed","error":"idle timeout","duration_secs":180.0}
{"event":"auto_commit","child":"test-a","files":2}
{"event":"merge_start"}
{"event":"merge_result","child":"test-a","success":true}
{"event":"merge_result","child":"test-b","success":false,"detail":"no new commits"}
{"event":"done","completed":2,"failed":0,"duration_secs":63.5}
```

Event types:
- `wave_start` — a wave of children begins
- `child_spawned` — process started
- `child_status` — running/completed/failed transition
- `child_activity` — tool call or turn boundary (parsed from child stderr)
- `auto_commit` — orchestrator auto-committed work
- `merge_start` / `merge_result` — merge phase
- `done` — orchestration complete

The TS side maps these to `emitCleaveChildProgress` calls:
- `child_spawned` → `{status: "running", startedAt: Date.now()}`
- `child_activity` → `{lastLine: "→ write tmp/foo.txt"}`
- `child_status completed` → `{status: "done", elapsed: duration_secs * 1000}`
- `child_status failed` → `{status: "failed"}`

## Decisions

### Decision: Use structured JSON progress events on stdout (Approach B)

**Status:** decided
**Rationale:** Operator chose B for a clean versioned contract. Rust emits newline-delimited JSON on stdout for lifecycle events. TS parses JSON lines and calls emitCleaveChildProgress. Stdout is currently unused by the Rust binary — repurpose it as the progress channel. Stderr remains for tracing/diagnostic output.

### Decision: Surface lifecycle transitions plus tool-call summaries, not raw thinking

**Status:** decided
**Rationale:** The dashboard should show: spawned, turn N, tool calls (→ write, → bash, → read), completed/failed with duration. Raw LLM thinking output is noisy and expensive to relay. The Rust orchestrator already sees child stderr which contains tracing lines like 'Turn 1', '→ write path' — it can parse these into structured events and emit them as JSON on stdout.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/cleave/progress.rs` (new) — New module: ProgressEvent enum + emit_progress() that serializes to JSON and writes to stdout
- `core/crates/omegon/src/cleave/orchestrator.rs` (modified) — Call emit_progress() at lifecycle points: wave start, child spawn, child complete/fail, auto-commit, merge start/result, done. Parse child stderr lines for tool calls (→ prefix) and turn markers.
- `core/crates/omegon/src/cleave/mod.rs` (modified) — Add mod progress
- `extensions/cleave/native-dispatch.ts` (modified) — Parse stdout as NDJSON, map events to emitCleaveChildProgress calls. Stop accumulating stdout as a string.
- `extensions/cleave/index.ts` (modified) — Pass pi to dispatchViaNative so it can call emitCleaveChildProgress

### Constraints

- stdout is the JSON progress channel — must not contain any other output (tracing stays on stderr)
- Events must be self-contained (no cross-event state needed to parse)
- child_activity events should be throttled — at most 1 per second per child to avoid flooding the dashboard
- The label field in events must exactly match the ChildState.label used by the TS side
