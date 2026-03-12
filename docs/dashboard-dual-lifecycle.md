---
id: dashboard-dual-lifecycle
title: "Dashboard: Surface Dual-Lifecycle State"
status: implemented
tags: [dashboard, openspec, design-tree, web-ui]
open_questions: []
branches: ["feature/dashboard-dual-lifecycle"]
openspec_change: dashboard-dual-lifecycle
---

# Dashboard: Surface Dual-Lifecycle State

## Overview

Now that every design node has a paired design-phase OpenSpec change (openspec/design/<id>/) and the implementation layer has openspec/changes/<id>/, the dashboard and web endpoint need to reflect both lifecycles visually. Currently neither the footer, the overlay panel, nor the web API expose design-spec binding state, acceptance criteria health, or the dual-pipeline at-a-glance. This node explores what to surface and where.

## Research

### Current dashboard state — what's visible today

**Footer compact (1 line):** branch · model · design summary · implementation summary · cleave status · memory gauge. No design-spec binding info.

**Footer raised (multi-line):** `◈ Design Tree  N decided · N exploring · N blocked · N?` then focused node or implementing nodes. `◎ Implementation  N changes  N/N`. Design nodes show status icons but zero indication of whether they have a paired design spec — binding state is invisible.

**Overlay panel (4 tabs):**
- Design Tree tab: shows all nodes with status icons and question counts. No spec binding badge.
- Implementation tab: shows openspec/changes/ only. openspec/design/ is entirely invisible.
- Cleave tab: child dispatch status.
- System tab: env vars, routing, effort, memory.

**Web API (`GET /api/state`):**
- `designTree.nodes[].openspecChange` — the implementation-layer binding (null for most nodes)
- `openspec.changes[]` — only scans openspec/changes/ (listChanges calls getOpenSpecDir which returns openspec/changes/)
- No design-spec binding state anywhere in the schema
- No acceptance criteria summary
- No assessment.json outcome exposed

### Data that now exists but isn't surfaced

For each design node in `exploring` or higher status, the following data exists on disk:

1. **Design spec binding** — `resolveDesignSpecBinding(cwd, nodeId)` → `{ active, archived, missing }`.
   - `active`: openspec/design/<id>/ has files (in-progress design change)
   - `archived`: openspec/design-archive/YYYY-MM-DD-<id>/ exists (design complete)
   - `missing`: neither (should be scaffolded on set_status(exploring))

2. **Acceptance criteria summary** — `countAcceptanceCriteria(node)` → `{ scenarios, falsifiability, constraints } | null`. Tells you whether a node has measurable criteria defined.

3. **Assessment outcome** — `openspec/design/<id>/assessment.json` → `{ pass, findings[], capturedAt }`. Tells you if `/assess design` has been run and whether it passed.

4. **Design tasks progress** — `openspec/design/<id>/tasks.md` has checkboxes. Same structure as implementation tasks.md.

5. **Design changes** — the entire `openspec/design/` subtree is a parallel OpenSpec universe that `listChanges` never sees (it only reads `openspec/changes/`).

The infrastructure to compute all of this exists today. Nothing wires it into the dashboard state or the web schema.

### Option A — Design spec binding badge on each node (minimal, additive)

Add a design-spec binding indicator to each node row in the Design Tree tab and the footer raised mode.

**Badge system:**
```
◈  (dim)     — seed, no spec expected
◐  (accent)  — exploring/active design change in openspec/design/
✦  (warning) — exploring/decided but spec MISSING (gate will block)
●  (success) — design spec archived, node ready to decide/implement
✗  (dim)     — assessment ran but failed
✓  (success) — assessment ran and passed
```

**Changes required:**
1. `DesignTreeDashboardState.nodes[]` — add `designSpec: { active: boolean; archived: boolean; missing: boolean }` and `acSummary?: { scenarios: number; constraints: number } | null`
2. `dashboard-state.ts emitDesignTreeState()` — populate per-node binding by calling `resolveDesignSpecBinding` for exploring/decided/implemented nodes
3. `overlay-data.ts buildDesignItems()` — render badge inline after status icon
4. `footer.ts buildDesignTreeLines()` — add `N missing spec` counter to the summary line

**Footprint:** ~100 lines, no new tabs, purely additive. The archive dir scan O(n) issue we already fixed applies here too — we can use the same pre-scan pattern.

**Tradeoff:** Badge requires per-node filesystem scan at emit time (called on every tree mutation). For 50+ nodes this is ~50 readdirSync calls per keystroke. Need caching.

### Option B — Design pipeline summary line (aggregate, high-signal)

Add a single "pipeline health" summary line to both the footer and overlay, showing the dual-lifecycle as a funnel:

```
◈ Design Pipeline   8 exploring → 12 designing → 22 decided → 14 implementing → 51 done
                    ↑ 3 missing spec (need /assess design)
```

This is the "at a glance" view — no per-node detail, just counts across the full lifecycle including the design-phase states.

**New lifecycle states implied:**
- `exploring / no spec` = seed or exploring without openspec/design/<id>/  → "needs spec"
- `designing` = exploring with active design spec in openspec/design/<id>/
- `decided` = decided with archived design spec
- `implementing` = implementing (existing)
- `done` = implemented (existing)

**Changes required:**
1. `DesignTreeDashboardState` — add `designPipeline: { needsSpec: number; designing: number; decided: number; implementing: number; done: number }`
2. `dashboard-state.ts emitDesignTreeState()` — compute pipeline counts during scan (archive dir scan once)
3. `footer.ts buildDesignTreeLines()` — render pipeline row in raised mode
4. `overlay-data.ts buildDesignItems()` — render as collapsible summary item in Design Tree tab

**Tradeoff:** Aggregate counts avoid per-node badge complexity. Pipeline funnel shows the full design→implement arc. But doesn't tell you WHICH nodes are stuck — user still has to navigate to find them. Combine with Option A's per-node badge for full picture.

### Option C — Design Changes as a sub-section in the Implementation tab

The Implementation tab currently shows only `openspec/changes/`. Add a "Design Changes" sub-section at the top showing active `openspec/design/` changes.

**Layout in overlay (Implementation tab):**
```
◎ Implementation

  ── Design Phase ───────────────────────────
  ◐ dual-lifecycle-openspec    ✓ assessed  3/3 AC criteria
  ◐ dashboard-dual-lifecycle   ✦ no assessment yet

  ── Implementation Phase ───────────────────
  ◦ skill-aware-dispatch       18/31
  ✓ unified-dashboard          (archived)
```

**Changes required:**
1. New `listDesignChanges(repoRoot)` function in openspec/spec.ts that scans `openspec/design/` — returns same shape as `listChanges()` plus `nodeId`, `assessmentResult`
2. `OpenSpecDashboardState` — add `designChanges: DesignChangeEntry[]`
3. `overlay-data.ts buildOpenSpecItems()` — two sections within Implementation tab
4. `footer.ts buildOpenSpecLines()` — add design-phase count to the implementation summary

**Tradeoff:** Keeps design and implementation cleanly separated in the panel UI. The "Implementation" label stays correct — design-phase changes are conceptually distinct. Requires a new disk scanner for the design dir.

### Option D — Web API schema extension for dual-lifecycle

The web endpoint (`GET /api/state`) needs to evolve to expose the design-phase data that web consumers (monitoring dashboards, CI checks, external tools) might want.

**Proposed schema additions:**

`DesignNodeSummary`:
```ts
designSpec?: {
  active: boolean;
  archived: boolean;
  missing: boolean;
};
acSummary?: { scenarios: number; falsifiability: number; constraints: number } | null;
assessmentResult?: { pass: boolean; capturedAt: string } | null;
```

`OpenSpecSnapshot` → split or extend:
```ts
// Option D1: extend with designChanges field (additive, non-breaking)
designChanges: DesignChangeSummary[];  // new

interface DesignChangeSummary {
  nodeId: string;
  hasProposal: boolean;
  hasSpec: boolean;
  hasTasks: boolean;
  hasAssessment: boolean;
  assessmentPass: boolean | null;  // null = not run
  tasksTotal: number;
  tasksDone: number;
  active: boolean;  // true = openspec/design/, false = archived
}
```

Option D1 is additive (no SCHEMA_VERSION bump required if done carefully). D2 (restructuring) would require bumping SCHEMA_VERSION.

**Also useful:** a `GET /api/design-pipeline` endpoint that returns the funnel counts directly — optimized for status-board polling without loading the full tree.

**Tradeoff:** The web API is the most complete story for external consumers. Changes need to be additive to avoid breaking existing API clients. `state.ts buildDesignTree()` already calls `scanDesignDocs()` — extending it to also read design specs is a natural fit, but adds more filesystem I/O per request.

### Caching strategy for binding state

The binding state (resolveDesignSpecBinding) requires filesystem I/O per node. Two call sites with different freshness requirements:

**emitDesignTreeState() (footer/raised):** Called on every tree mutation. With 50+ nodes, 50+ readdirSync calls per mutation is expensive. Strategy:
- Build an `archivedIds` Set by scanning openspec/design-archive/ ONCE per emit call
- Check openspec/design/<id>/ existence inline (one existsSync per node, cheap)
- Result: 1 readdirSync for archive + N existsSync for active = O(n) but low per-call cost

**buildDesignTree() in state.ts (web API):** Called on every GET /api/state request. Same O(n) pattern, acceptable since this is on-demand not on keystroke.

**overlay-data.ts (panel rendering):** Called on every panel redraw (~30fps). Must use the pre-computed binding data from sharedState, NOT do filesystem I/O directly. The binding state must live in DesignTreeDashboardState.

**File-watching optimization (future):** The file-watch extension already watches openspec/ changes. We could extend it to watch openspec/design/ and openspec/design-archive/ and invalidate the binding cache only on relevant mutations. Not necessary for the initial implementation.

## Decisions

### Decision: Design stays in Design Tree tab; implementation linkage via shared symbol suffix

**Status:** decided
**Rationale:** Keep design nodes in the Design Tree tab and implementation changes in the Implementation tab — don't mix them. Instead, add a shared visual symbol (e.g. `&`) as a suffix to both the design node row AND the paired implementation change row(s). When a node has been bridged to implementation via 'implement', both the design node in tab 1 and its linked openspec change in tab 2 show `&`. This creates a visual tie without structural mixing. The badge system (◐/●/✦/✓/✗) lives on the design node row in tab 1 and reflects the design-spec binding state. The `&` symbol is purely a cross-tab linkage indicator.

### Decision: Footer compact mode stays clean — no dual-lifecycle signal

**Status:** decided
**Rationale:** Compact is for orientation at a glance. Adding dual-lifecycle detail would clutter the one line it has. The raised footer already has room for a pipeline summary row. Panel has tabs. Compact does nothing new.

### Decision: Web API: new designPipeline top-level slice + SCHEMA_VERSION bump to 2

**Status:** decided
**Rationale:** Maximum exposure for easy web UI consumption. Add a `designPipeline` key to ControlPlaneState containing: pipeline funnel counts, every node's designSpec binding + acSummary + assessmentResult, every active and archived design change summary. Extend DesignNodeSummary with designSpec/acSummary/assessmentResult. Add a GET /api/design-pipeline slice route. SCHEMA_VERSION → 2 because ControlPlaneState shape changes. The existing openspec snapshot keeps scanning openspec/changes/ only — designChanges go into designPipeline, not openspec, since they are pre-implementation artifacts. Web consumers wanting the full picture poll /api/state; status-board consumers poll /api/design-pipeline directly.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/dashboard/types.ts` (modified) — Add DesignSpecBindingState, DesignPipelineCounts, DesignChangeSummary interfaces. Extend DesignTreeDashboardState.nodes[] with designSpec + acSummary + assessmentResult fields. Add designPipeline to DesignTreeDashboardState.
- `extensions/design-tree/dashboard-state.ts` (modified) — emitDesignTreeState: scan design-archive once, build per-node binding state + AC summary + assessment result. Compute pipeline counts. Include in emitted state.
- `extensions/dashboard/overlay-data.ts` (modified) — buildDesignItems: render design-spec badge after status icon; render & linkage suffix when node has openspec_change; pipeline funnel as collapsible summary item at top. buildOpenSpecItems: render & suffix on changes that are linked to a design node.
- `extensions/dashboard/footer.ts` (modified) — buildDesignTreeLines (raised mode): add pipeline funnel row and N-missing-spec warning. nodeStatusIcon: new designSpecBadge helper. No compact changes.
- `extensions/web-ui/types.ts` (modified) — SCHEMA_VERSION → 2. Extend DesignNodeSummary with designSpec + acSummary + assessmentResult. Add DesignChangeSummary interface. Add DesignPipelineSnapshot interface. Add designPipeline to ControlPlaneState.
- `extensions/web-ui/state.ts` (modified) — buildDesignTree: populate designSpec/acSummary/assessmentResult per node. New buildDesignPipeline(): scan openspec/design/ + openspec/design-archive/, build DesignPipelineSnapshot with funnel counts and all design change summaries. Add 'designPipeline' to buildSlice dispatch. Call countAcceptanceCriteria and read assessment.json per node.
- `extensions/web-ui/server.ts` (modified) — Register GET /api/design-pipeline slice route.
- `extensions/openspec/spec.ts` (modified) — New listDesignChanges(repoRoot): scans openspec/design/ + openspec/design-archive/, returns DesignChangeInfo[] with nodeId, artifacts, tasksDone/Total, assessmentResult, isArchived.

### Constraints

- Design-spec binding scan must read design-archive/ exactly once per emit call — use pre-built Set<string> pattern established in the blocked-query fix
- overlay-data.ts must NOT do filesystem I/O — all binding state must come from sharedState (pre-computed in emitDesignTreeState)
- SCHEMA_VERSION bump to 2 is required — ControlPlaneState shape changes with designPipeline key
- The & linkage symbol must appear on BOTH the design node row (tab 1) AND the implementation change row (tab 2) — the symbol must be consistent
- No changes to compact footer behavior
- assessmentResult is read from openspec/design/<id>/assessment.json — parsed fields: pass (boolean), capturedAt (string). Other fields ignored for dashboard purposes
- listDesignChanges must be idempotent and safe when openspec/design/ does not exist
