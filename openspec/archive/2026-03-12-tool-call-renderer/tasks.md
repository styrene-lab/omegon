+++
id = "b74424ff-93e8-4f55-be10-aa40f67cf9e9"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Enriched Tool Call Rendering — Tasks

## 1. design_tree + design_tree_update renderers

- [x] 1.1 `extensions/design-tree/index.ts` — import `Text` from `@cwilson613/pi-tui`
- [x] 1.2 Add `renderCall(args, theme)` to `design_tree` registerTool(): `◈ query <action> [node_id]`
- [x] 1.3 Add `renderResult` to `design_tree`: collapsed — first line truncated to 80 chars; expanded — full text; isPartial — `◈ loading…`
- [x] 1.4 Add `renderCall(args, theme)` to `design_tree_update` registerTool(): action-semantic headers (set_status shows `→ status`, add_question/remove_question shows truncated question text, add_decision shows title, add_research shows heading, create shows title, implement/focus/unfocus handled)
- [x] 1.5 Add `renderResult(result, { expanded, isPartial }, theme)` to `design_tree_update`: isPartial → `◈ updating…`; isError → red `✕ msg`; collapsed dispatches on details fields (newStatus, totalQuestions/question, remainingQuestions/question, decision/status, heading, changePath, node, focusedNode); expanded → full text

## 2. cleave_run + cleave_assess renderers + dispatcher simplification

- [x] 2.1 `extensions/cleave/index.ts` — import `Text` from `@cwilson613/pi-tui`
- [x] 2.2 Add `renderCall(args, theme)` to `cleave_run`: `⚡ cleave  N children  "directive…"`
- [x] 2.3 Add `renderResult(result, { expanded, isPartial }, theme)` to `cleave_run`: isPartial phase-aware child table (✓/⟳/○/✕, done/total matching tab); final → `✓ done` / `⚠ conflicts` / `✕ failed`
- [x] 2.4 Add `renderCall(args, theme)` to `cleave_assess`: `◊ assess  "directive…"`
- [x] 2.5 Add `renderResult` to `cleave_assess`: `◊ complexity N.N  → execute/cleave` with colors; expanded → full text
- [x] 2.6 `extensions/cleave/dispatcher.ts` — simplified onUpdate text to `dispatching label1, label2` (removed Wave X/Y counter); cleaned up unused childStart/childEnd/childRange variables

## 3. openspec_manage renderer

- [x] 3.1 `extensions/openspec/index.ts` — `Text` already imported from `@cwilson613/pi-tui`
- [x] 3.2 Add `renderCall(args, theme)`: `◎ propose name`, `◎ add_spec change/domain`, `◎ archive name`, `◎ status`, `◎ get name`, etc.
- [x] 3.3 Add `renderResult(result, { expanded }, theme)`: action-specific one-liners (`◎ ✓ proposed`, `◎ ✓ spec added change/domain`, `◎ ✓ archived`, `◎ N changes`, `◎ name (stage)`, etc.); isError → `◎ ✕ msg`; expanded → full text
