+++
title = "Live Operation Compact Rows"
tags = ["tui","ux","operations","design"]
+++

# Live Operation Compact Rows

---
title: Live Operation Compact Rows
status: implementing
tags: [tui, ux, operations, design]
---

# Live Operation Compact Rows

## Problem

Recent TUI screenshots show long-lived work collapsing into compact rows that are technically accurate but operationally ambiguous.

Examples:

- Terminal build/link rows lead with `terminal · read · <uuid> · 12000 bytes ...` while the useful state (`build-link`, `running`, `Building omegon(bin) 768/769`) is buried.
- Delegate rows expose raw invocation metadata (`background`, `result_tool`, `status_hint`, `task_id`) but do not provide a stable live row that answers what is running and what changed.
- Cleave has richer progress internally, but compact operation summaries need the same semantic identity/status/progress discipline.

The compact row should answer, in order:

1. What operation is this?
2. Is it still running?
3. What phase/progress is it in?
4. How do I inspect more? (`^O details` as a separate affordance cell)

It should not lead with raw JSON, UUIDs, byte counts, result tool names, or agent-facing status hints.

## Design Rule

Represent long-running tool summaries as structured cells:

```text
kind · semantic-id · status · phase/progress · detail · ^O details
```

The compact renderer remains dumb: it receives cells and keeps the details affordance separate/right-alignable. Tool-specific summarizers decide which cells matter.

## Target Rows

### Terminal

Instead of:

```text
terminal · read · 246bc84a... · 12000 bytes · Terminal 'build-link (...)' — running · Building [...]
```

Prefer:

```text
terminal · build-link · running · 768/769 · omegon(bin) · ^O details
```

or, at narrow widths:

```text
terminal · build-link · running · 768/769 omegon(bin) · ^O details
```

### Delegate

Instead of raw invocation JSON:

```text
delegate · background:true, result_tool:delegate_result, status_hint:/subagent status, task_id:...
```

Prefer:

```text
delegate · delegate_1 · running · scout · cargo test … · ^O details
```

or on completion:

```text
delegate · delegate_1 · completed · 1 finding fixed · ^O details
```

### Cleave

Prefer:

```text
cleave_run · wave 1/2 · alpha running · beta queued · ^O details
```

or:

```text
cleave_run · merging · alpha merged · beta failed · ^O details
```

## Implementation Slices

### Slice 1: Terminal compact summaries

Add terminal-specific summary cell extraction in the existing `slim_tool_summary_cells(...)` path.

Responsibilities:

- Parse terminal arguments for stable session name (`build-link`) and command (`just link`) when available.
- Parse terminal read results for status (`running`, `completed`, etc.).
- Extract progress lines such as `Building [...] 768/769: omegon(bin)` into compact cells.
- Keep session UUIDs and byte counts in details, not compact rows, unless no semantic name exists.

### Slice 2: Delegate compact summaries

Add delegate-specific summary cells.

Responsibilities:

- Surface `task_id` as the stable ID.
- Prefer worker/profile/task phrase over raw JSON fields.
- Surface running/completed/failed status.
- Include last tool/turn when available.
- Preserve `^O details` as its own final cell.

### Slice 3: Cleave compact summaries

Map cleave result/progress shapes into the same cell discipline.

Responsibilities:

- Summarize wave, running children, queued children, merge state, and failures.
- Do not force cleave into delegate's result store.
- Keep cleave orchestration semantics in cleave; emit only compact summary cells to the renderer.

### Slice 4: Workbench active-count cleanup

Investigate `workstreams×99` and distinguish active vs historical workstreams.

Target:

```text
workstreams 2 active / 99 total
```

or only show active count in the bottom status when space is tight.

## Non-goals

- Do not unify delegate and cleave execution loops yet.
- Do not create a large generic operation renderer before terminal/delegate/cleave result shapes are proven.
- Do not hide `^O details`; compact rows must remain expandable.
