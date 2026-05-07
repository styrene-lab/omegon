+++
id = "7f1eb37f-5a38-4f4d-a989-eae58e6b33b9"
kind = "document"
title = "Enriched Tool Call Rendering"
status = "implemented"
tags = ["dashboard", "tui", "ux", "cleave", "design-tree"]
aliases = ["tool-call-renderer"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
branches = []
open_questions = []
openspec_change = "tool-call-renderer"
+++

# Enriched Tool Call Rendering

## Overview

Pi's ToolDefinition interface exposes renderCall(args, theme) and renderResult(result, {expanded, isPartial}, theme) hooks that return pi-tui Component instances. These replace the default bare tool-name + raw-text rendering with structured, colored, contextual display. Currently design_tree_update shows anemic one-liners with no mention of the actual changed content. cleave_run shows a wave-counter in the box that conflicts visually with the done-counter in the tab. Both can be fixed by adding custom renderers to the existing registerTool() calls — no new extension needed.

## Research

### The mechanism: renderCall + renderResult

Both hooks are optional fields on the object passed to `pi.registerTool()`:

```ts
pi.registerTool({
  name: "design_tree_update",
  // ...
  renderCall(args, theme): Component | undefined {
    // Called immediately when the tool is invoked
    // args = the raw params the LLM passed
    // Returns a pi-tui Component (Text, Box, etc.) replacing the default header
  },
  renderResult(result, { expanded, isPartial }, theme): Component | undefined {
    // isPartial = true: tool is still running (re-called on every onUpdate())
    // isPartial = false: tool finished, result is final
    // expanded = user pressed ctrl+e to expand the box
    // result.details = the structured object the tool returned
    // result.content[0].text = the latest text content (from onUpdate during partial)
  }
})
```

**Key insight for live progress:** Every `onUpdate({ content, details })` call during execution triggers `renderResult` with `isPartial: true`. The `result.content[0].text` contains the latest progress message and `result.details` contains whatever structured data was passed in that update. This is how we get live child-status updates during cleave_run — the dispatcher already calls `onUpdate({ content: [text], details: { phase, children } })` on every wave dispatch.

**Text component:** `new Text(styledString, marginTop, marginBottom)` — the simplest Component. Supports multi-line (newlines in the string). Theme methods: `theme.fg(colorName, text)`, `theme.bold(text)`. Available colors: `success`, `error`, `warning`, `accent`, `dim`, `muted`, `toolTitle`.

### Fix: cleave tab/box counter discrepancy

**The bug:** Tab shows `cleave 2/4` (done/total children). Box shows `Wave 4/4 (child 4/4): dispatching query-and-memory`. Both have `N/4` format but measure different things — the tab counts COMPLETED children, the box counts DISPATCHED waves. At the moment wave 4 is being dispatched, only 2 children may be done (wave 3 might still be running), so the user reads `2/4` and `4/4` simultaneously and concludes there's a bug.

**Fix (two parts):**

1. **Change the dispatch progress text** in `dispatcher.ts` to include the done count explicitly:
   ```
   Wave 2/4 · query-and-memory dispatched · 1/4 done
   ```
   Or even cleaner — drop the wave counter from the text entirely since `renderResult` will show a live child table. The onUpdate text becomes: `dispatching query-and-memory...`

2. **renderResult (isPartial)** reads `result.details.children` (already passed in `onUpdate`) and shows a unified view that matches the tab:
   ```
   ⚡ cleave  dispatching  1/4 done
   ✓ types-and-emission     ⟳ overlay-and-footer
   ○ query-and-memory        ○ spec-scanner
   ```
   This matches exactly what the tab shows (done count) while also showing which children are running vs pending.

### design_tree_update: what to show and when

**renderCall:** Replace the bare `design_tree_update action node_id` header with a semantically meaningful line per action:
```
◈ set_status  dashboard-dual-lifecycle  exploring → decided
◈ add_question  dual-lifecycle-openspec  "What is the right approach?"
◈ add_decision  dashboard-dual-lifecycle  "Design stays in Design Tree tab" 
◈ implement  dashboard-dual-lifecycle
◈ remove_question  dashboard-dual-lifecycle  "Some question text"
◈ add_research  some-node  "Option A — Design spec binding badge"
```
Uses `args.action`, `args.node_id`, `args.question`, `args.decision_title`, `args.heading`, `args.status`. The node title isn't available in args (only the ID), but the text can still be formatted clearly.

**renderResult (collapsed, final):** One-line semantic summary from `result.details`:
```
◈ → decided  dashboard-dual-lifecycle            (set_status)
◈ + question  "What is the right approach?"  (3 remaining)  (add_question)
◈ + decision  "Design stays..."  decided          (add_decision)
◈ ✓ scaffolded  openspec/changes/dashboard-dual-lifecycle/  (implement)
◈ − question  "Some question"  (2 remaining)       (remove_question)
```
The `result.details` already contains `id`, `oldStatus/newStatus`, `question/totalQuestions`, `remainingQuestions`, `decision/status`, etc. — all the data is there.

**renderResult (expanded):** Show the full result text that the tool already produces (the multi-line guidance text). Expanded = the operator pressed ctrl+e wanting details.

**renderResult (isError):** Red icon + error text, clearly distinguished from success.

### cleave_run: full event stream during execution

**renderCall:** Show directive (truncated to 60 chars) + child count from parsed plan_json:
```
⚡ cleave  3 children  "Implement dashboard dual-lifecycle…"
```

**renderResult (isPartial — live during dispatch):** 
The dispatcher already calls `onUpdate({ content: [text], details: { phase, children } })` at each wave. `result.details.children` contains the live child array with `status: "pending" | "running" | "done" | "failed"`. Render a compact child table:
```
⚡ cleave  dispatching  2/4 done
  ✓ types-and-emission (122s)
  ⟳ overlay-and-footer  running...
  ○ spec-scanner-and-web-api
  ○ query-and-memory
```
This matches the tab's `2/4` counter exactly and shows WHO is done/running/pending.

**renderResult (isPartial — merging phase):**
```
⚡ cleave  merging  4/4 dispatched
```

**renderResult (isPartial — review phase):**
```
⚡ cleave  reviewing  overlay-and-footer  round 1
```

**renderResult (final — success):**
```
⚡ cleave  ✓ done  3/3 merged  995s
```
Expanded: show the full markdown report.

**renderResult (final — conflicts/failures):**
```
⚡ cleave  ⚠ conflicts  2/3 merged  1 conflict in dashboard/types.ts
```

**cleave_assess:**
```
◊ assess  complexity 3.0  → cleave        (renderCall)
◊ assess  complexity 3.0  exceeds 2.0  3 systems  → decompose  (renderResult)
```

**Phase info in details:** The `onUpdate` details already include `{ phase: "dispatch" | "harvest" | "merge", children: CleaveChildState[] }` so `renderResult` has everything it needs.

### Other tools worth enriching

**openspec_manage:** Action-specific headers like `◎ propose  my-feature`, `◎ add_spec  auth/tokens`, `◎ archive  my-feature`. Result shows stage transition: `◎ → specified  my-feature  3 scenarios`.

**memory_store/memory_recall/memory_recall:** `○ store  [Architecture]  "design-tree and cleave are bridged…"` → `✓ stored  fact-id: abc123`. For recall: `○ recall  "dashboard dual lifecycle"  → 8 results`.

**cleave_assess:** `◊ assess  → cleave  complexity: 3.0`. Result: `◊ 3 systems · 1 modifier = 3.0  exceeds 2.0  → decompose`.

**design_tree (query):** `◈ query  node  dashboard-dual-lifecycle` → collapsed: `◈ dashboard-dual-lifecycle  decided  3 decisions  0 questions`. Expanded: show the full overview.

**Implementation approach:** All renderers live in a NEW `extensions/tool-renderer/` extension that imports `Text` from `@styrene-lab/pi-tui` and re-registers the tools using `renderCall`/`renderResult` only — no execute override needed. This keeps the rendering concern isolated from the tool logic. The tool names match exactly so pi replaces the default renderer silently.

Wait — actually this won't work: re-registering a tool from a different extension would replace the tool entirely, not just its renderer. The `execute` would be lost. The correct approach is to add `renderCall`/`renderResult` directly to the existing `pi.registerTool()` calls in each extension. No new extension.

Alternative: A dedicated `tool-renderer.ts` extension that uses `pi.on("tool_call")` to intercept and re-render... but that event type isn't in the API.

**Conclusion:** Add `renderCall` + `renderResult` inline to the existing registerTool() calls in:
- `extensions/design-tree/index.ts` (design_tree_update)
- `extensions/cleave/index.ts` (cleave_run, cleave_assess)
- `extensions/openspec/index.ts` (openspec_manage)
- Optionally: `extensions/project-memory/index.ts` (memory_store, memory_recall)

## Decisions

### Decision: Renderers added inline to existing registerTool() calls — no new extension

**Status:** decided
**Rationale:** Re-registering a tool from a separate extension would need to re-implement the execute() logic. Adding renderCall/renderResult directly to the existing registerTool() calls in each extension is surgical and keeps the rendering concern co-located with the tool's behavior. The hooks are just optional fields on the same object.

### Decision: cleave_run isPartial renderResult shows unified done/running/pending child table

**Status:** decided
**Rationale:** The tab/box discrepancy is caused by wave-counter vs done-counter using the same N/4 format. Fix by making the box show the SAME metric as the tab (done count) alongside a per-child status table. Drop "Wave X/Y (child A/B)" from the onUpdate text — it becomes redundant once the child table is rendered. The dispatcher onUpdate text simplifies to "dispatching child-label…"

### Decision: Tier 1: design_tree_update + cleave_run/assess + openspec_manage; Tier 2: memory tools

**Status:** decided
**Rationale:** design_tree_update and cleave_run are the highest-frequency tools in the operator's event stream and have the most visible defects. openspec_manage is secondary but appears often enough to warrant enrichment. Memory tools are lower priority — their results are already fairly readable and they're called less frequently in the primary workflow.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/design-tree/index.ts` (modified) — Add renderCall + renderResult to both design_tree and design_tree_update registerTool() calls. renderCall: action-semantic header with icon, action name, node_id, key param (status/question/heading/decision_title). renderResult collapsed: one-liner from details (oldStatus→newStatus, question + remainingCount, decision title, etc.). renderResult expanded: full existing text. renderResult isError: red icon + error line.
- `extensions/cleave/index.ts` (modified) — Add renderCall + renderResult to cleave_run registerTool(). renderCall: directive truncated to 60 chars + child count from parsed plan_json. renderResult isPartial: phase-aware — dispatch shows per-child status table (✓/⟳/○/✕ + elapsed for done) + done/total count matching tab; merge shows merging N/N; review shows review round info. renderResult final: ✓ done N/N merged Xs, or ⚠ conflicts summary. Add renderCall + renderResult to cleave_assess. Simplify dispatcher.ts onUpdate text to remove wave/child counters (now redundant with renderResult table).
- `extensions/openspec/index.ts` (modified) — Add renderCall + renderResult to openspec_manage registerTool(). renderCall: action-specific header (◎ propose name, ◎ add_spec domain, ◎ archive name). renderResult: stage transition or confirmation (◎ → specified name N scenarios, ◎ archived name).
- `extensions/cleave/dispatcher.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- renderCall receives args as typed Static<TParams> — use args.action, args.node_id, etc. directly; do NOT call any external functions or read from sharedState in renderCall
- renderResult details type must be cast (result.details as Record<string,unknown>) since AgentToolResult<TDetails> detail type is generic
- isPartial renderResult must be fast and non-blocking — no filesystem I/O, no awaiting
- child table in cleave_run isPartial: max 8 children visible; truncate with '… N more' if overflow
- Text component constructor: new Text(styledString, marginTop, marginBottom) — marginTop/Bottom are numbers
- Import Text from @styrene-lab/pi-tui in each file that uses it
- The dispatcher.ts onUpdate text simplification (removing wave/child counter) must be done alongside the renderResult addition — doing one without the other leaves the partial text unchanged but now redundant
