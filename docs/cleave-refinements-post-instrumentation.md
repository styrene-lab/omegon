---
id: cleave-refinements-post-instrumentation
title: "Cleave refinements: panel restore, segment rendering, schema hardening"
status: resolved
tags: [cleave, tui, dx, harness]
open_questions:
  - "Why does the instruments panel not revert to the tools view after cleave completes? Is the CleaveProgress.active flag being cleared, and is the render-path guard checking it correctly?"
  - "What should the cleave_run segment renderer show? Compact header (N children, truncated directive) + per-child label/status icon on result — but where in the TS harness is the current renderer and what does it currently emit?"
  - "Where in the harness is the cleave tool schema defined (system prompt, tool spec, AGENTS.md, skill file)? What is the authoritative injection point so the schema is always present at call time?"
  - "Does the cleave merge logic do any deduplication of test blocks, or does it blindly append? What is the right fix — pre-merge dedup pass, or anchor detection that skips adding to a file if the target symbol already exists?"
dependencies: []
related: []
---

# Cleave refinements: panel restore, segment rendering, schema hardening

## Overview

Three related issues surfaced from live cleave instrumentation work:

1. **Tools panel does not restore after cleave ends** — the instruments panel swaps to the child grid when a run starts but never reverts to the tools view when the run completes. The `total_children == 0` guard on the render path is likely not firing correctly on the tick after the run ends.

2. **cleave_run segment output is unreadable** — the tool-call renderer dumps the raw plan_json string directly into the conversation, producing a wall of JSON. It should render a compact summary: child count, labels, directive truncated to ~80 chars, per-child status icons on completion.

3. **Cleave schema is not baked into bedrock** — the harness reaches for cleave_run, cleave_assess, and cleave_delegate with ad-hoc guessed schemas (wrong field names, missing required fields). The correct CleavePlan schema (children[].label, .description, .scope[], optional .depends_on[], .rationale) must be injected into the harness system prompt or tool spec as authoritative bedrock — not discoverable at runtime.

Secondary: merge strategy produces duplicate test blocks when multiple cleave children share the same pre-existing test mod as base. Needs a dedup pass or smarter merge anchor detection.

## Research

### Root cause investigation

**Issue 1 — Panel doesn't restore**
`instruments.rs` render guard is `cp.active || cp.total_children > 0`. `total_children` is set on run start and never cleared to 0 — `apply_progress_event` only handles `Done` by setting `active = false`. So once a run ends, `active=false` but `total_children` is still e.g. 3, and the cleave panel stays rendered. Fix: guard should be `cp.active` alone, or clear `total_children` in `Done` handler.

**Issue 2 — Segment output vomit**
In `tui/mod.rs` `ToolStart` handler, `detail_args` falls through to `serde_json::to_string_pretty(&args)` for any unrecognized tool. For `cleave_run`, `args` contains `directive` (long string) + `plan_json` (full JSON blob as a string) — both dumped raw. Fix: add `cleave_run` | `cleave_assess` | `cleave_delegate` arms to the `detail_args` match AND add entries to `summarize_tool_args` in `loop.rs`. The summary should show child count + labels. Detail should show: directive (truncated ~100 chars) + child list with labels.

**Issue 3 — Schema not in bedrock**
`summarize_tool_args` has no cleave arms → falls to `_ => None`. In `segments.rs` `summarize_args` closure, same: no cleave match → falls to `_ => None`. The CleavePlan schema (label, description, scope[], optional depends_on[], rationale) lives only in `cleave/plan.rs` as a Rust struct. It is not present in the SKILL.md, AGENTS.md, or any system-prompt injection. `skills/cleave/SKILL.md` describes intent and examples but doesn't state the required JSON field names as authoritative schema. Fix: add a `## plan_json Schema` section to `skills/cleave/SKILL.md` with explicit field table.

**Issue 4 — Merge duplicates**
Cleave merge strategy (`git merge --no-ff`) is additive. When N children all append to the same `mod tests {}` block, all N additions land, producing N copies of the same symbol. No dedup pass exists. Fix options: (a) post-merge dedup pass that strips duplicate `fn` definitions, or (b) children check if symbol already exists before inserting.

## Decisions

### Panel restore: guard on active only, clear total_children in Done

**Status:** accepted

**Rationale:** Simplest correct fix. `total_children` surviving the run is a data artifact; the guard intent is "is a run active right now". Clearing `total_children` in the Done handler keeps both the guard and the data consistent. Alternatively guard on `cp.active` alone without clearing — but keeping stale totals in memory is confusing for future readers.

### Segment rendering: add cleave_run/cleave_assess arms to detail_args and summarize_tool_args

**Status:** accepted

**Rationale:** Targeted per-tool override is the established pattern (bash, read, edit, change all have explicit arms). Summary = "N children: label1, label2…" (truncated to 60 chars). Detail = directive truncated to 100 chars + newline + child list "  • label: description[:60]".

### Schema hardening: add authoritative plan_json schema block to skills/cleave/SKILL.md

**Status:** accepted

**Rationale:** SKILL.md is loaded into the system prompt at session start. Adding an explicit field table with types and required/optional markers means the schema is present before the first cleave call. No code change needed — doc change only. The Rust struct is the source of truth; the SKILL.md is the harness-visible projection of it.

### Merge dedup: children should check for symbol existence before inserting

**Status:** deferred

**Rationale:** A pre-insert existence check requires semantic parsing of Rust (or at minimum a grep before appending). This is a medium-complexity change to the cleave merge harness. For now, the mitigation is operator awareness: single-file test additions per child, never more than one child touching the same test mod. Deferred to a dedicated merge-quality pass.

## Open Questions

- Why does the instruments panel not revert to the tools view after cleave completes? Is the CleaveProgress.active flag being cleared, and is the render-path guard checking it correctly?
- What should the cleave_run segment renderer show? Compact header (N children, truncated directive) + per-child label/status icon on result — but where in the TS harness is the current renderer and what does it currently emit?
- Where in the harness is the cleave tool schema defined (system prompt, tool spec, AGENTS.md, skill file)? What is the authoritative injection point so the schema is always present at call time?
- Does the cleave merge logic do any deduplication of test blocks, or does it blindly append? What is the right fix — pre-merge dedup pass, or anchor detection that skips adding to a file if the target symbol already exists?
