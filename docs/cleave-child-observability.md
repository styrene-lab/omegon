+++
id = "cc9ce76d-916e-4789-83b2-312c1fb0ebf9"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave Child Observability — Live insight into running children

## Overview

Currently a running cleave child is nearly opaque. The only live data surfaced is `lastLine` — one debounced stdout line per child. Full stdout accumulates in memory but is discarded on completion. There's no way to see what a child has done, what files it's touched, or why it's taking long. This node explores the gap and the right way to fill it.

## Research

### What exists today

**Per child in `CleaveChildState`:**
- `status`: pending / running / done / failed
- `elapsed`: ms since dispatch start
- `lastLine`: single debounced (500ms) filtered stdout line — the only live window into what the child is doing
- `error`: first 2000 chars of stderr, only populated after failure
- `backend`: cloud / local

**What `spawnChild` does with stdout:**
- Accumulates the entire stdout in a `string` variable in memory
- Parses it line-by-line and calls `onLine(clean)` for lines passing `isChildStatusLine()`
- `isChildStatusLine` filters out JSON, ANSI, separators, lines < 12 chars, lines > 240 chars
- `lastLine` = last line that passed the filter, debounced 500ms
- **Full stdout is never written to disk and is thrown away after completion**

**What's not surfaced at all:**
- Stdout history — you can't see what the child printed 2 minutes ago
- Worktree file state — which files have been modified so far
- Tool call sequence — which tools has the child called, in what order
- Sub-task progress — if the child has a task list, which items are done
- The task file content itself (what was the child asked to do)

### Four levers available

**Lever 1 — stdout ring buffer in CleaveChildState**
Replace `lastLine: string` with `recentLines: string[]` (last N, e.g. 30). `onLine` appends + trims to cap. No disk I/O. Gives scrollback in the dashboard or an inspect overlay. Cost: minimal (a few hundred bytes per child).

**Lever 2 — per-child log file in the worktree**
`spawnChild` opens `<worktree>/.pi-child-<id>.log` and tees stdout into it. Any command or overlay can then `tail -f` or `readFileSync` it on demand. Survives process death for post-mortem. Cost: disk write amplification (pi -p output is verbose — JSON + tool output), could be MBs per child. Needs cleanup. The log file could also be written with a raw vs filtered flag.

**Lever 3 — live worktree git diff**
At any time during execution, `git status --short` and `git diff --stat` in the worktree reveal what the child has accomplished so far. This is the highest-signal view: you see *files touched* rather than *what the model said*. Can be polled on demand (not streamed). Cost: git subprocess per query, fine for on-demand.

**Lever 4 — /cleave inspect command + overlay**
A `/cleave inspect` command (or keyboard shortcut from the dashboard cleave section) opens a blocking overlay for a selected child. Shows: task file header, ring buffer / log tail, current git diff --stat. Gives the operator a full drill-down without being persistent noise.

## Decisions

### Decision: Ring buffer (30 lines) + on-demand git diff + inspect overlay — no log files

**Status:** decided
**Rationale:** Log files are expensive and noisy — pi -p stdout is JSON tool-call records, could hit 10MB+ per child, and needs cleanup. The ring buffer (last 30 filtered lines) covers "what is it saying" with zero I/O. `git diff --stat` in the worktree covers "what has it accomplished" — the highest-signal view. Together they answer every observability question without disk amplification. Post-mortem coverage: CleaveChildState persists until the run object is GC'd, so the ring buffer is available after completion too.

### Decision: Inspect reachable via /cleave inspect command + global keyboard shortcut — dashboard widget cannot own input

**Status:** decided
**Rationale:** The dashboard is a non-blocking setWidget — it has no keyboard focus, so per-row shortcuts are not wirable. The inspect overlay is a blocking ctx.ui.custom() call. Two trigger paths: (1) /cleave inspect slash command — explicit, always works; (2) a global keyboard shortcut (e.g. Ctrl+Shift+I) registered while a cleave run is active, opens the same overlay. Inside the overlay, arrow keys select a child, Enter drills in, Escape/Q closes. The shortcut is registered on run start and unregistered on completion so it only appears when relevant.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/cleave/types.ts` (modified) — Add recentLines: string[] (cap 30) to CleaveChildState. Keep lastLine as a computed getter (last element of recentLines) for backward compat with dashboard.
- `extensions/cleave/dispatcher.ts` (modified) — onLine callback appends to recentLines ring buffer instead of setting lastLine. emitCleaveChildProgress patch type updated to include recentLines. Add runGitDiffStat(worktreePath): Promise<string> helper using execFile git diff --stat + git status --short.
- `extensions/cleave/index.ts` (modified) — Add /cleave inspect slash command — opens ctx.ui.custom() overlay. Register Ctrl+Shift+I shortcut on run start, unregister on completion. Overlay: child selector list (arrow keys), drill-in view per child (task scope header + recentLines scrollback + git diff --stat), Escape/Q to close.
- `extensions/dashboard/shared-state.ts` (modified) — CleaveChildState patch type: add recentLines?: string[] to the partial patch interface used by emitCleaveChildProgress.

### Constraints

- Ring buffer cap is 30 lines — enforced in onLine via splice or slice
- recentLines contains only lines that passed isChildStatusLine() filter — no raw JSON or ANSI
- lastLine backward compat: kept as recentLines[recentLines.length - 1] ?? '' so dashboard renderResult child table needs no changes
- runGitDiffStat must be non-blocking (async, no await in render paths) and cached for 2s to avoid git subprocess spam on repeated overlay renders
- Keyboard shortcut Ctrl+Shift+I: verify not shadowed in pi-tui (check tui.js hardcoded keys) before committing to it
- Inspect overlay must handle zero children gracefully (show 'No active children')
- Worktree path may be undefined for children that failed preflight — handle gracefully in git diff call
