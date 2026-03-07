---
id: unified-dashboard
title: Unified Live Dashboard for Cleave + Design Tree + OpenSpec
status: decided
tags: [tui, ux, dashboard, pi-kit]
open_questions: []
---

# Unified Live Dashboard for Cleave + Design Tree + OpenSpec

## Overview

Explore how to create a persistent, live, navigable view that unifies Design Tree state, OpenSpec change status, and Cleave execution progress into a single always-visible dashboard, with optional interactive detail panel.

## Research

### pi TUI Extension Points — What's Available

After a thorough audit of pi's TUI architecture (`@mariozechner/pi-tui` + `@mariozechner/pi-coding-agent`), here are the building blocks:

### Persistent (Non-Blocking) UI

These render alongside the normal conversation flow without stealing focus:

1. **`ctx.ui.setWidget(key, content, options)`** — Renders above or below the editor. Accepts `string[]` or `(tui, theme) => Component`. Multiple widgets stack. Current design-tree widget uses `belowEditor` placement. **This is the only non-blocking persistent view mechanism.**

2. **`ctx.ui.setStatus(key, content)`** — Single-line footer indicators. Used by status-bar for the context gauge. Multiple extensions can set independent status keys.

3. **`ctx.ui.setFooter(factory)`** — Replace the entire footer. Gets `FooterDataProvider` with git branch and extension statuses. Can react to branch changes via `footerData.onBranchChange()`.

### Interactive (Blocking) UI

These take keyboard focus and block until dismissed:

4. **`ctx.ui.custom(factory, { overlay: true, overlayOptions })`** — Full interactive component rendered as a floating overlay. Supports 9 anchor positions, percentage sizing, responsive `visible()` callback, and `onHandle` for external visibility toggle. **Key limitation: `await ctx.ui.custom()` blocks the command handler** — keyboard input goes to the overlay, not the editor.

5. **Built-in dialogs** — `select()`, `confirm()`, `input()`, `editor()` — all blocking modal.

### Communication Channels

6. **`sharedState` (globalThis singleton)** — Cross-extension state sharing. Currently used for `memoryTokenEstimate` (project-memory → status-bar).

7. **`pi.events` (EventEmitter)** — Inter-extension event bus. Can emit/listen for custom events.

8. **Tool execution events** — `tool_execution_start/update/end` fire during tool runs, enabling real-time status updates.

### Key Constraint

**There is no "persistent overlay" concept** — overlays always take focus. A sidebar that stays visible while the user types requires a different approach: either a richer widget, or running the overlay in a fire-and-forget pattern with external handle control.

### Current State — Fragmented Views

Today's implementations are isolated:

**Design Tree Widget** (`extensions/design-tree/index.ts`):
- `setWidget("design-tree", lines, { placement: "belowEditor" })` 
- Shows: summary line (decided/exploring/questions) + focused node + first open question
- Updates on: tool execution, focus/unfocus, status changes
- 3 lines max

**OpenSpec Status** (`extensions/cleave/index.ts`):
- Injected into session start as a text message via `before_agent_start`
- Shows: change list with task counts and stages
- **Not persistent** — only appears once at session start, scrolls away

**Cleave Execution** (`extensions/cleave/index.ts`):
- No persistent UI at all during execution
- Progress only visible via tool_result rendering (renderResult)
- After completion, results are in the conversation as rendered tool output
- No live dispatch tracking

**Status Bar** (`extensions/status-bar.ts`):
- `setStatus("status-bar", ...)` — context gauge in footer
- Shows: turn count + context % with memory segment
- Already occupies footer status space

The screenshot shows the design tree widget belowEditor with its current 2-3 line format. The user wants all three systems unified and richer.

### Architecture Options



### Option A: Enhanced Unified Widget

Merge all three views into a single `setWidget` that renders below the editor. No interactivity (read-only), but always visible and live-updated.

```
◈ Design 3/4 decided · 1 exploring · 6?   ◎ OpenSpec 2 changes   ⚡ Cleave idle
▸ ● skill-aware-dispatch — 5 open questions
  ✓ scenario-first-task-gen 16/16   ◦ skill-aware-dispatch 0/31 [proposal,design,specs]
```

**Pros:** Always visible, zero-cost, no focus issues
**Cons:** No interactivity, limited space, can't navigate/click

### Option B: Widget + Toggle Overlay

Widget (Option A) for summary, plus a keyboard shortcut (e.g., Ctrl+Shift+D) that opens a full interactive overlay panel (right-anchored sidepanel) for detailed navigation.

The overlay would provide:
- Navigable design tree with fold/expand
- OpenSpec change detail viewer
- Cleave run history with drill-down
- Close with Esc to return to editing

**Pros:** Best of both worlds, rich interaction on demand
**Cons:** Overlay blocks input — can't type while browsing. Requires good shortcut ergonomics.

### Option C: Widget + External Terminal Panel (Zellij/tmux)

Use pi's TUI for the compact widget, but spawn a separate TUI panel in a Zellij/tmux pane that shows the dashboard. Communicate via file watchers or IPC.

**Pros:** Truly persistent sidebar, no focus conflict
**Cons:** Requires Zellij/tmux, complex IPC, separate rendering code

### Option D: Custom Footer Dashboard

Replace the footer with a multi-line dashboard that compresses all three views. Footer is always visible and doesn't compete with editor space.

**Pros:** Always visible, doesn't grow conversation area
**Cons:** Footer is typically 1-2 lines — multi-line footer would shrink available space. May conflict with status-bar.

### Recommendation: Option B (Widget + Toggle Overlay)

This gives the best UX within pi's current architecture:
1. **Always-on widget** below editor shows the critical summary (~3-5 lines)
2. **Ctrl+Shift+D** opens a rich interactive overlay for navigation
3. **During cleave dispatch**, widget updates live via `tool_execution_update` events
4. **sharedState** handles cross-extension data flow

### Technical Details for Q1 — Data Flow Architecture



### Layout Stack (top to bottom)

```
headerContainer          ← setHeader()
chatContainer            ← conversation messages
widgetContainerAbove     ← setWidget(key, ..., {placement: "aboveEditor"})
editorContainer          ← user input editor
widgetContainerBelow     ← setWidget(key, ..., {placement: "belowEditor"})
footer                   ← setFooter() or built-in FooterComponent
```

### Capacity Limits

- **Widgets**: `MAX_WIDGET_LINES = 10` per widget (string[] mode). Component factory mode is uncapped but should be bounded. Multiple widgets per zone stack vertically.
- **Footer**: Built-in renders **2-3 lines** (pwd+stats+extension_statuses). Custom footer has no line limit — it's a `Component.render(width)` that returns `string[]`.
- **setStatus(key, text)**: Contributes to the footer's 3rd line. Multiple statuses are sorted alphabetically and joined with spaces.

### Inter-Extension Communication Mechanisms

1. **`sharedState` (globalThis singleton)** — Synchronous, zero overhead. Already in use (status-bar reads `memoryTokenEstimate` written by project-memory). Module-level import. Any extension can read/write.
2. **`pi.events` (EventBus)** — `emit(channel, data)` / `on(channel, handler)` returning unsubscribe fn. Simple pub/sub. Not currently used by any pi-kit extension.
3. **Lifecycle events** — `tool_execution_end`, `agent_end`, `turn_end` etc. fire for ALL extensions. Each extension can observe events from any tool.
4. **Widget key ownership** — Multiple extensions can own different widget keys in the same zone. They render in insertion order (Map iteration order).

### Extension Load Order (package.json)

cleave → openspec → ... → status-bar → ... → design-tree → version-check

This means design-tree loads LAST among the data producers. A dashboard extension placed after it would see all providers initialized.

### Q1 Deep Analysis — Three Approaches + Hybrid



## Decisions

### Decision: D1: Decentralized emitters + custom footer panel

**Status:** decided
**Rationale:** Each extension (design-tree, openspec, cleave) emits its own in-memory state to sharedState and fires pi.events.emit("dashboard:update"). A new dashboard extension reads sharedState and renders a custom footer via setFooter() that supports raise/lower. This avoids duplicating logic, gives access to in-memory state (focusedNode, dispatch progress), and keeps each extension authoritative over its own data. The custom footer absorbs status-bar.ts. ~20 lines added per emitter, ~300-400 lines for the dashboard extension.

### Decision: D2: Footer raise/lower + overlay, no external pane

**Status:** decided
**Rationale:** External Zellij/tmux pane (Option C) adds complex IPC, requires a multiplexer, and fragments the codebase across two rendering targets. The three-layer approach within pi covers all use cases: Layer 0 (compact footer, always visible), Layer 1 (raised footer, toggle), Layer 2 (interactive overlay, on-demand). Zellij integration can be revisited later as an optional enhancement but is not worth the complexity for v1.

### Decision: D3: Ctrl+Shift+D for raise/lower toggle

**Status:** decided
**Rationale:** Audited all pi built-in keybindings — Ctrl+Shift+D is unbound. The only Ctrl+D binding is "exit" (when editor empty), and Ctrl+Shift+P is "cycle model backward". D for Dashboard is mnemonic. The shortcut toggles the footer between compact (Layer 0) and raised (Layer 1). From raised mode, a second press or Enter on a focused item opens the interactive overlay (Layer 2).

### Decision: D4: Live cleave progress via sharedState mutation from dispatch callbacks

**Status:** decided
**Rationale:** dispatchChildren() in dispatcher.ts already has spawn callbacks for stdout/stderr and exit. Adding sharedState.cleave.children[n].status updates there is ~10 lines. The dashboard subscribes to pi.events("dashboard:update") which cleave emits on each child state transition (start, done, fail). No file-based IPC needed — sharedState is synchronous in-process. The tool_execution_update events are unnecessary since the cleave tool itself can emit events directly via pi.events during its execute() function (it has access to the pi closure).

### Decision: D5: Footer collapsible via raise/lower; state persisted via appendEntry

**Status:** decided
**Rationale:** The raise/lower mechanism IS the collapse. Lowered = compact 3-line footer. Raised = expanded 7-10 line footer. State (raised/lowered) persisted via pi.appendEntry("dashboard-state", { raised }) and restored on session_start. This replaces the old /design widget toggle — design-tree stops owning its own widget entirely. A /dashboard command provides explicit toggle + settings as an alternative to Ctrl+Shift+D.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/shared-state.ts` (modified) — Extend SharedState interface with designTree?, openspec?, cleave? dashboard state types
- `extensions/dashboard/index.ts` (new) — New dashboard extension: custom footer (compact+raised), Ctrl+Shift+D shortcut, /dashboard command, pi.events subscription, session persistence
- `extensions/dashboard/footer.ts` (new) — Custom footer Component: compact renderer (Layer 0), raised renderer (Layer 1), reimplements built-in footer data (pwd, tokens, cost, model, context%, git branch)
- `extensions/dashboard/overlay.ts` (new) — Interactive overlay panel (Layer 2): design tree navigator, openspec detail viewer, cleave run drill-down
- `extensions/dashboard/types.ts` (new) — Dashboard-specific types: DashboardMode, FooterState, section render interfaces
- `extensions/design-tree/index.ts` (modified) — Remove setWidget calls, add sharedState.designTree emit + pi.events.emit('dashboard:update') in updateWidget path
- `extensions/openspec/index.ts` (modified) — Add sharedState.openspec emit after listChanges/computeStage, emit dashboard:update event
- `extensions/cleave/index.ts` (modified) — Add sharedState.cleave emit on state transitions (assess/plan/dispatch/merge/done/fail), emit dashboard:update
- `extensions/cleave/dispatcher.ts` (modified) — Add sharedState.cleave.children[n] updates in spawn callbacks (start/done/fail per child)
- `extensions/status-bar.ts` (deleted) — Remove — context gauge and turn counter absorbed into dashboard footer
- `package.json` (modified) — Add extensions/dashboard to pi.extensions list (after design-tree), remove extensions/status-bar.ts

### Constraints

- Custom footer must reimplement all built-in footer data: pwd, git branch, session name, token stats (input/output/cache), cost, context%, model name, thinking level, extension statuses
- MAX_WIDGET_LINES=10 does not apply to custom footer (Component-based), but raised mode should stay under 10 lines to preserve conversation space on 40-line terminals
- Dashboard extension must load AFTER design-tree, openspec, and cleave in package.json to ensure sharedState is populated on session_start
- pi.events handlers receive unknown data — dashboard must type-narrow or use sharedState directly rather than relying on event payload types
- Overlay (Layer 2) steals keyboard focus — must handle Esc cleanly and not interfere with agent streaming

## Approach 1: Single Dashboard Extension (Centralized)

A new `extensions/dashboard/index.ts` that:
- Reads design-tree files directly (via `scanDesignDocs`)
- Reads openspec state directly (via `listChanges`)
- Reads cleave state from sharedState (cleave must emit)
- Owns the unified widget and overlay
- Replaces the current design-tree widget

### Pros
- **Single source of truth for rendering** — one extension owns the widget, no ordering conflicts
- **Consistent styling** — one file controls all colors, icons, layout
- **No coupling between extensions** — cleave/openspec/design-tree don't need to know about the dashboard
- **Simpler testing** — one component to test, mock data sources

### Cons
- **Duplicates logic** — must re-import and call `scanDesignDocs`, `listChanges`, `getActiveChangesStatus`. This creates coupling to the *implementation* of those extensions, not just their data.
- **Stale data risk** — dashboard reads filesystem state on its own schedule, may lag behind the actual extension's in-memory state. Design-tree holds `focusedNode` in memory; dashboard can't know this without IPC.
- **Maintenance burden** — changes to design-tree, openspec, or cleave data structures require updating both the extension AND the dashboard
- **Can't access in-memory state** — `focusedNode` (design-tree), active `CleaveState` (cleave), cached `ChangeInfo[]` (openspec) live in extension closures. Dashboard can't reach them.

---

## Approach 2: Each Extension Emits to Shared State (Decentralized)

Each existing extension writes its own state to `sharedState` and emits `pi.events.emit("dashboard:update")`. A lightweight dashboard extension subscribes and renders.

```typescript
// In design-tree/index.ts:
sharedState.designTree = { nodes: [...], focusedNode, decidedCount, openQuestions };
pi.events.emit("dashboard:update");

// In openspec/index.ts:
sharedState.openspec = { changes: [...], stages: [...] };
pi.events.emit("dashboard:update");

// In cleave/index.ts:
sharedState.cleave = { status, runId, children: [...] };
pi.events.emit("dashboard:update");
```

### Pros
- **Authoritative data** — each extension emits its own in-memory state (focusedNode, dispatch progress, etc.)
- **Real-time** — updates fire immediately when state changes, not on a polling schedule
- **No duplicated logic** — extensions own their data format, dashboard just renders
- **Natural extension points** — adding a new panel to the dashboard just means a new sharedState key + event

### Cons
- **Coupling via sharedState shape** — dashboard must know the schema of each extension's emitted state. Changes to `DesignTree` types break the dashboard.
- **Three extensions modified** — requires touching cleave, openspec, and design-tree to add emit logic
- **Event ordering** — if two extensions emit "dashboard:update" in the same tick, dashboard re-renders twice
- **sharedState grows** — global mutable state is a maintenance risk; need discipline to not let it sprawl
- **Design-tree widget conflict** — design-tree currently owns `setWidget("design-tree", ...)`. Dashboard must either absorb that (design-tree stops rendering its widget) or use a separate key (two widgets render).

---

## Approach 3: Collocated Hybrid

The dashboard is a **thin rendering layer** that lives in its own extension, but each data-producing extension exports a **`getDashboardState()`** function (or exposes it via sharedState). The dashboard calls these functions on events rather than re-reading the filesystem.

```typescript
// design-tree/index.ts exports:
export function getDesignTreeDashboardState(): DesignTreeDashboardState { ... }

// But pi's jiti loader doesn't support cross-extension imports. So we use sharedState:
sharedState.designTree = { get: () => getDesignTreeDashboardState() };

// Or simpler: each extension writes a structured snapshot to sharedState on state changes.
```

### Pros
- Same benefits as Approach 2, but with explicit data contracts
- Each extension defines what it exposes (information hiding)
- Dashboard never reads filesystem directly

### Cons
- Same cons as Approach 2 re: sharedState coupling
- The "getter function on sharedState" pattern is unusual; simpler to just write the data directly

---

## Approach 4: Footer "Panel Raise/Lower"

Use `setFooter()` to create a multi-line footer that can expand/collapse between a 1-line summary and a 5-10 line detail view.

### How It Would Work
```
LOWERED (default):
  ◈ D:3/4 ◎ OS:2 ⚡ idle  │  T3 ▓▓████░░░░ 28%  │  ~/workspace/pi-kit (main)

RAISED (toggle via shortcut):
  ◈ Design Tree  3/4 decided · 1 exploring · 6?
    ▸ ● skill-aware-dispatch — 5 open questions
  ◎ OpenSpec  2 changes
    ✓ scenario-first 16/16  ◦ skill-aware 0/31 [specs]
  ⚡ Cleave  idle
  ─────────────────────────────────────────────────
  T3 ▓▓████░░░░ 28%  │  ~/workspace/pi-kit (main)
```

### Pros
- **Footer is always at the bottom** — never scrolls, never competes with conversation space
- **Natural position** for status data (already has pwd, tokens, context)
- **Raise/lower is a clean metaphor** — one shortcut toggles between compact and expanded
- **No widget stacking issues** — doesn't use setWidget at all
- **Absorbs status-bar** — context gauge, turn counter, and dashboard all live together

### Cons
- **`setFooter` replaces the built-in** — we lose the default footer (pwd, tokens, cost, model, context%, git branch, extension statuses). Must **reimplement all of it** in the custom footer.
- **Vertical space** — raised mode takes 7-10 lines from the conversation area. On a 40-line terminal, that's 25%.
- **No interactivity** — footer renders string[], no handleInput. Can't navigate, expand/collapse sections.
- **Only one footer** — if another extension wants to customize the footer, conflicts.
- **Still needs data access** — same sharedState/event question applies for getting data into the footer.

### Feasibility Assessment
**Fully feasible** technically. `setFooter(factory)` returns a `Component` that can render any number of lines. The component can hold state (raised/lowered). A keyboard shortcut (`pi.registerShortcut`) can toggle the state and call `tui.requestRender()`.

The cost is **reimplementing the built-in footer** — that's about 60 lines of logic for pwd, git branch, token stats, cost, context%, model name, thinking level, extension statuses. Non-trivial but doable.

---

## Usage Reference

### Keyboard Shortcut

**`Ctrl+Shift+B`** — Dashboard toggle (mnemonic: **B**ar)

| Current State | Action |
|---|---|
| Compact (Layer 0) | → Raised (Layer 1) |
| Raised (Layer 1) | → Interactive Overlay (Layer 2) |
| Overlay (Layer 2) | Esc → back to Raised |

### Slash Command

| Command | Effect |
|---|---|
| `/dashboard` | Toggle compact ↔ raised |
| `/dashboard compact` | Force compact mode |
| `/dashboard raised` | Force raised mode |
| `/dashboard open` | Open interactive overlay |

### Overlay Navigation (Layer 2)

| Key | Action |
|---|---|
| `Tab` | Cycle tabs (Design Tree → OpenSpec → Cleave) |
| `1` / `2` / `3` | Jump to tab directly |
| `↑` / `↓` | Navigate items |
| `Enter` / `→` | Expand item |
| `←` | Collapse item |
| `Esc` | Close overlay |

### Display Modes

- **Compact (Layer 0):** 3-line footer — status icons, context gauge, token stats, model/thinking indicator
- **Raised (Layer 1):** Up to 10-line footer — design tree status, openspec changes, cleave dispatch, separator, original footer data
- **Overlay (Layer 2):** Right-anchored 40% width panel — three navigable tabs with expand/collapse, live refresh via dashboard:update events

---

## Recommendation

**Approach 2 (emitter) + 4 (footer panel) are compatible and complementary:**

1. Each extension emits state to `sharedState` + `pi.events` (Approach 2)
2. A new `dashboard` extension reads sharedState, renders a **custom footer** that supports raise/lower (Approach 4)
3. The raised footer shows the full dashboard; the lowered footer shows the compact summary
4. For truly interactive navigation (design-tree drill-down, openspec detail), a shortcut opens an **overlay** (Approach B from earlier)
5. Design-tree stops rendering its own widget (dashboard subsumes it)

This gives three layers:
- **Layer 0 (always):** Compact footer line — status icons + key metrics
- **Layer 1 (raised):** Expanded footer — detailed status of all three systems
- **Layer 2 (overlay):** Interactive panel — full navigation (on-demand, focus-stealing)
