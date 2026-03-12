# Dashboard: Surface Dual-Lifecycle State — Tasks

## 1. Type definitions + data emission
<!-- specs: dashboard/spec -->

- [x] 1.1 `extensions/dashboard/types.ts` — add `DesignSpecBindingState` interface: `{ active: boolean; archived: boolean; missing: boolean }`
- [x] 1.2 `extensions/dashboard/types.ts` — add `AcSummary` interface: `{ scenarios: number; falsifiability: number; constraints: number }`
- [x] 1.3 `extensions/dashboard/types.ts` — add `DesignAssessmentResult` interface: `{ pass: boolean; capturedAt: string }`
- [x] 1.4 `extensions/dashboard/types.ts` — add `DesignPipelineCounts` interface: `{ needsSpec: number; designing: number; decided: number; implementing: number; done: number }`
- [x] 1.5 `extensions/dashboard/types.ts` — extend `DesignTreeDashboardState.nodes[]` entries with optional `designSpec?: DesignSpecBindingState`, `acSummary?: AcSummary | null`, `assessmentResult?: DesignAssessmentResult | null`
- [x] 1.6 `extensions/dashboard/types.ts` — add `designPipeline?: DesignPipelineCounts` field to `DesignTreeDashboardState`
- [x] 1.7 `extensions/design-tree/dashboard-state.ts` — in `emitDesignTreeState()`: scan `openspec/design-archive/` once (pre-build `archivedIds: Set<string>`), then per eligible node call `resolveDesignSpecBinding` using the pre-built set (inline, matching the blocked-query pattern), call `countAcceptanceCriteria`, and read `assessment.json` if it exists in the active design dir
- [x] 1.8 `extensions/design-tree/dashboard-state.ts` — compute `designPipeline` counts from the per-node binding state and emit alongside existing state fields
- [x] 1.9 `extensions/design-tree/dashboard-state.ts` — nodes with status `seed` get `designSpec: { active: false, archived: false, missing: false }` (seed is exempt — no spec expected)

## 2. Overlay panel — Design Tree tab + Implementation tab
<!-- specs: dashboard/spec -->

- [x] 2.1 `extensions/dashboard/overlay-data.ts` — add `designSpecBadge(binding: DesignSpecBindingState | undefined, assessmentResult: DesignAssessmentResult | null | undefined, th: ThemeFn): string` helper: returns `✓` (success) if archived+passed, `✦` (warning) if missing and status is exploring/decided, `◐` (accent) if active, `●` (success) if archived+no-assessment, `""` for seed/undefined
- [x] 2.2 `extensions/dashboard/overlay-data.ts` — `buildDesignItems()`: render `designSpecBadge` after status icon on each node row; render `&` suffix (dim) when node has `openspecChange` (implementation link); NO filesystem I/O — read all data from `sharedState.designTree`
- [x] 2.3 `extensions/dashboard/overlay-data.ts` — `buildDesignItems()`: add collapsible pipeline funnel item at top of Design Tree tab (key `dt-pipeline`) showing `needsSpec · designing · decided · implementing · done` counts from `sharedState.designTree.designPipeline`; expandable to show which nodes are in `needsSpec` state
- [x] 2.4 `extensions/dashboard/overlay-data.ts` — `buildOpenSpecItems()`: render `&` suffix (dim) on each implementation change row when that change name matches a design node's `openspec_change` field; determine match by checking `sharedState.designTree.nodes` for any node with `openspecChange === change.name`

## 3. Footer raised mode
<!-- specs: dashboard/spec -->

- [x] 3.1 `extensions/dashboard/footer.ts` — `buildDesignTreeLines()`: after the existing status-counts summary line, add a pipeline row: `  → N designing · N decided · N implementing · N done` (dim arrow prefix, colored counts); only render if `designPipeline` is present in sharedState
- [x] 3.2 `extensions/dashboard/footer.ts` — if `designPipeline.needsSpec > 0`, append `  ✦ N need spec` (warning color) as a separate sub-line below the pipeline row — this is the actionable alert in raised mode
- [x] 3.3 `extensions/dashboard/footer.ts` — `buildDesignTreeLines()`: for focused node row, append `designSpecBadge` (reuse same badge logic, extracted to a shared util or inline); for non-focused node list, append badge per row
- [x] 3.4 `extensions/dashboard/footer.ts` — no changes to compact mode rendering

## 4. OpenSpec spec.ts — listDesignChanges
<!-- specs: dashboard/spec -->

- [x] 4.1 `extensions/openspec/spec.ts` — add `DesignChangeInfo` interface: `{ nodeId: string; path: string; hasProposal: boolean; hasSpec: boolean; hasTasks: boolean; hasAssessment: boolean; assessmentPass: boolean | null; tasksDone: number; tasksTotal: number; isArchived: boolean; archivedPath?: string }`
- [x] 4.2 `extensions/openspec/spec.ts` — implement `listDesignChanges(repoRoot: string): DesignChangeInfo[]`: safely return `[]` if `openspec/design/` does not exist; scan both `openspec/design/` (active) and `openspec/design-archive/` (archived); for each entry parse `tasks.md` checkbox progress using the existing `parseTasksFile` logic; read `assessment.json` for pass/capturedAt if present; include `hasProposal/hasSpec/hasTasks/hasAssessment` flags from file existence checks

## 5. Web API types + state + server
<!-- specs: dashboard/spec -->

- [x] 5.1 `extensions/web-ui/types.ts` — bump `SCHEMA_VERSION` from `1` to `2`
- [x] 5.2 `extensions/web-ui/types.ts` — extend `DesignNodeSummary` with: `designSpec: { active: boolean; archived: boolean; missing: boolean } | null`, `acSummary: { scenarios: number; falsifiability: number; constraints: number } | null`, `assessmentResult: { pass: boolean; capturedAt: string } | null`
- [x] 5.3 `extensions/web-ui/types.ts` — add `DesignChangeSummary` interface (mirrors `DesignChangeInfo` from spec.ts, plain JSON-serialisable)
- [x] 5.4 `extensions/web-ui/types.ts` — add `DesignPipelineSnapshot` interface: `{ funnelCounts: { needsSpec: number; designing: number; decided: number; implementing: number; done: number }; activeChanges: DesignChangeSummary[]; archivedChanges: DesignChangeSummary[] }`
- [x] 5.5 `extensions/web-ui/types.ts` — add `designPipeline: DesignPipelineSnapshot` to `ControlPlaneState`
- [x] 5.6 `extensions/web-ui/state.ts` — in `buildDesignTree()`: for each scanned node, call `resolveDesignSpecBinding`, `countAcceptanceCriteria`, and read `assessment.json` to populate the new `DesignNodeSummary` fields
- [x] 5.7 `extensions/web-ui/state.ts` — implement `buildDesignPipeline(repoRoot: string): DesignPipelineSnapshot`: call `listDesignChanges(repoRoot)` and compute funnel counts from the `designTree` state (scan once, reuse archivedIds set)
- [x] 5.8 `extensions/web-ui/state.ts` — add `designPipeline` to the `buildControlPlaneState()` return value and to the `buildSlice()` dispatch map
- [x] 5.9 `extensions/web-ui/server.ts` — add `"/api/design-pipeline": "designPipeline"` to the slice route map
