# design-tree-lifecycle — Tasks

## 1. Types and Frontmatter
<!-- specs: types, frontmatter -->

- [ ] 1.1 Add `implementing` and `implemented` to `NodeStatus` union in types.ts
- [ ] 1.2 Add entries in `VALID_STATUSES`, `STATUS_ICONS` (⚙/✓), `STATUS_COLORS` (accent/success) 
- [ ] 1.3 Add `branches: string[]` and `openspec_change?: string` to `DesignNode` interface
- [ ] 1.4 Update frontmatter parsing in tree.ts to read `branches` (default `[]`) and `openspec_change` fields
- [ ] 1.5 Update frontmatter serialization in tree.ts to write `branches` (omit if empty) and `openspec_change` (omit if undefined)
- [ ] 1.6 Add `implementingCount: number` and `implementedCount: number` to `DesignTreeDashboardState` in dashboard/types.ts
- [ ] 1.7 Tests: frontmatter round-trip for branches/openspec_change, STATUS_ICONS/COLORS entries, VALID_STATUSES length

## 2. Implement Action + Branch Creation
<!-- specs: implement-action -->

- [ ] 2.1 Update `implement` case in index.ts: after scaffolding OpenSpec, set node status to `implementing`
- [ ] 2.2 Write `openspec_change` field (= node ID) and `branches: ["feature/<node-id>"]` to node frontmatter
- [ ] 2.3 Create git branch `feature/<node-id>` via `git checkout -b` (or use explicit override from frontmatter `branch` field)
- [ ] 2.4 Support optional `branch` parameter on implement action for prefix override (e.g., `refactor/`)
- [ ] 2.5 Tests: implement sets status, writes fields, rejects non-decided nodes

## 3. Branch Auto-association
<!-- specs: branch-association -->

- [ ] 3.1 Implement `matchBranchToNode(branchName, implementingNodes)` in tree.ts — segment-aware matching, longest node ID wins
- [ ] 3.2 Hook into footer's `onBranchChange` callback in index.ts — on branch change, run matchBranchToNode and append to matched node's branches[]
- [ ] 3.3 Write updated branches array back to the node's frontmatter file
- [ ] 3.4 Tests: segment matching (auth vs authorization), longest match wins, non-implementing nodes excluded, no-match no-op

## 4. Archive Gate
<!-- specs: archive-gate -->

- [ ] 4.1 In openspec/index.ts archive handler, after archiving: scan design tree for nodes with matching `openspec_change`
- [ ] 4.2 If found node has status `implementing`, transition to `implemented`
- [ ] 4.3 Tests: archive transitions implementing→implemented, no-match is no-op, decided nodes unchanged

## 5. Dashboard Rendering
<!-- specs: dashboard -->

- [ ] 5.1 Update `emitDesignTreeState` in design-tree/index.ts to count and emit `implementingCount` and `implementedCount`
- [ ] 5.2 Update compact footer in dashboard/footer.ts: show `◈ D:1 I:2 /5` format
- [ ] 5.3 Update raised footer: show implementing count in status line, render `⚙ <node-id> → <branch>` with accent color for implementing nodes
- [ ] 5.4 Add implementing nodes data to `DesignTreeDashboardState` (list of {id, branch} for raised view)
