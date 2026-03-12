# Dual-Lifecycle OpenSpec — Design Layer + Implementation Layer — Tasks

## 1. Acceptance Criteria document model
<!-- specs: design-tree/acceptance-criteria -->

- [x] 1.1 Add `AcceptanceCriteriaScenario` interface to `extensions/design-tree/types.ts`: `{ given: string; when: string; then: string }`
- [x] 1.2 Add `AcceptanceCriteriaConstraint` interface to `extensions/design-tree/types.ts`: `{ text: string; checked: boolean }`
- [x] 1.3 Add `acceptanceCriteria` field to `DocumentSections` in `types.ts`: `{ scenarios: AcceptanceCriteriaScenario[]; falsifiability: string[]; constraints: AcceptanceCriteriaConstraint[] }`
- [x] 1.4 Add `SECTION_HEADINGS.acceptanceCriteria = "## Acceptance Criteria"` and subsection constants to `types.ts`
- [x] 1.5 Implement `## Acceptance Criteria` section parser in `tree.ts` — parse `### Scenarios` (bold Given/When/Then blocks), `### Falsifiability` (bullet list prefixed "This decision is wrong if:"), `### Constraints` (GFM `- [x]`/`- [x]` checkboxes)
- [x] 1.6 Include `acceptanceCriteria` in `generateFrontmatter` / `writeNodeDocument` — section is written back as-is (not serialized to frontmatter)
- [x] 1.7 Surface `acceptanceCriteria` in `design_tree(action="node")` response under `sections`
- [x] 1.8 Add `acceptanceCriteria` to `design_tree(action="list")` response as a summary: `{ scenarioCount, falsifiabilityCount, constraintsTotal, constraintsMet }`
- [x] 1.9 Add unit tests in `tree.test.ts` covering: empty section, fully-populated section, partial subsections, checked/unchecked constraints

## 2. Design OpenSpec scaffolding on set_status(exploring)
<!-- specs: design-tree/design-openspec-scaffold -->

- [x] 2.1 Add `scaffoldDesignOpenSpecChange(cwd, node)` to `tree.ts` — creates `openspec/design/<node-id>/` with `proposal.md` (one-liner intent + link to docs/<id>.md), `spec.md` (template with empty Scenarios/Falsifiability/Constraints subsections), `tasks.md` (Open Questions mirrored as unchecked tasks)
- [x] 2.2 Create `openspec/design/` and `openspec/design-archive/` directories with `.gitkeep` files
- [x] 2.3 Invoke `scaffoldDesignOpenSpecChange` from the `set_status` handler in `index.ts` when transitioning to `exploring` — skip if `openspec/design/<node-id>/` already exists
- [x] 2.4 Mirror Open Questions to `tasks.md` on every `add_question` and `remove_question` call — write `openspec/design/<node-id>/tasks.md` atomically if the design change directory exists
- [x] 2.5 Add unit tests for scaffold creation, idempotency (re-transition to exploring), and tasks.md mirroring

## 3. Design spec hard gates on set_status(decided) and implement
<!-- specs: design-tree/design-spec-gates -->

- [x] 3.1 Add `resolveDesignSpecBinding(cwd, node)` to `archive-gate.ts` (or new `design-gate.ts`) — checks `openspec/design-archive/` for `<date>-<node-id>` entry; returns `{ archived: boolean, active: boolean, missing: boolean }`
- [x] 3.2 Gate `set_status(decided)` in `index.ts`: if design spec exists in `openspec/design/<node-id>/` (active, not archived) block with message "Run /assess design then archive the design change before marking decided". If missing entirely, block with "Scaffold design spec first via set_status(exploring)". Hard block — no warning-only path.
- [x] 3.3 Gate `implement` action in `index.ts`: require `resolveDesignSpecBinding` returns `archived: true`. Block with specific message for active-not-archived vs missing cases.
- [x] 3.4 Add unit tests for both gates covering: archived (pass), active (block), missing (block), pre-existing node without design dir (migration path — block with migration message)

## 4. ready and blocked query updates
<!-- specs: design-tree/query-updates -->

- [x] 4.1 Update `action: "ready"` in `index.ts`: add design spec check to filter — node must have `resolveDesignSpecBinding(cwd, n).archived === true` to appear in ready list. Nodes failing this check appear in `blocked` with synthetic dep `{ id: "design-spec-missing", title: "Design spec not archived", status: "missing" }`
- [x] 4.2 Update `action: "blocked"` in `index.ts`: include nodes with missing/active design spec alongside dep-blocked nodes. Annotate with `blocking_reason: "design-spec-not-archived" | "dependencies" | "explicit"`
- [x] 4.3 Add unit tests for ready and blocked with and without design spec present

## 5. Open Questions → memory facts emission
<!-- specs: project-memory/question-facts -->

- [x] 5.1 In `index.ts` `add_question` handler: after writing the node, emit a memory fact via `pi.tools.memory_store` equivalent — section `"Specs"`, content `"OPEN [<node-id>]: <question text>"`. Tag with node id for retrieval.
- [x] 5.2 In `index.ts` `remove_question` handler: attempt to find and archive the corresponding memory fact by querying for matching content prefix. If found, archive it.
- [x] 5.3 Add unit/integration tests for emission and archival

## 6. /assess design subcommand
<!-- specs: cleave/assess-design -->

- [x] 6.1 Add `"design"` to `AssessmentKind` union in `extensions/cleave/assessment.ts`
- [x] 6.2 Implement `runDesignAssessment(cwd, nodeId, pi)` in `assessment.ts`:
  - Resolve node from design tree; error if not found or no acceptanceCriteria section
  - Structural pre-check: `open_questions.length === 0`, `decisions.length > 0`, `acceptanceCriteria` present — fail fast with specific finding if not
  - Read full node document body as assessment context
  - Build LLM prompt: for each Scenario, ask "Is the Then clause satisfiable from the document?"; for each Falsifiability item, ask "Is this condition addressed, ruled out, or accepted as known risk?"; for each unchecked Constraint, ask "Is this satisfied by the document?"
  - Parse LLM response into `DesignAssessmentFinding[]`: `{ type: "scenario"|"falsifiability"|"constraint", index: number, pass: boolean, finding: string }`
  - Return `DesignAssessmentResult`: `{ nodeId, pass: boolean, findings: DesignAssessmentFinding[], structuralPass: boolean }`
- [x] 6.3 Store result as `assessment.json` in `openspec/design/<node-id>/`
- [x] 6.4 Add `"design"` case to assess bridge dispatcher in `bridge.ts` — resolve node_id from args or focused node, call `runDesignAssessment`, format output
- [x] 6.5 Update `/assess` command description and `promptGuidelines` in `index.ts` (cleave extension) to include `design` subcommand: "Run /assess design before set_status(decided)"
- [x] 6.6 Add tests for structural pre-check short-circuit, finding parsing, and result storage

## 7. Dashboard label rename and /opsx:propose labeling
<!-- specs: dashboard/label-rename -->

- [x] 7.1 In `extensions/dashboard/index.ts` (footer/overlay): rename "OpenSpec" section label to "Implementation" wherever it appears as a display label. Command names (`/opsx:*`) unchanged.
- [x] 7.2 In `extensions/openspec/index.ts`: update `/opsx:propose` description and promptGuidelines to clearly label it "untracked/throwaway only — for tracked work use design_tree_update(implement) from a decided node"
- [x] 7.3 Update `missing_design_binding` reconciliation issue from a hard block to an informational warning — binding is now guaranteed by construction for new nodes; this only fires for pre-migration legacy nodes
- [x] 7.4 Update `docs/design-tree.md` and `docs/openspec.md` to reflect new lifecycle, label rename, and dual-directory structure
