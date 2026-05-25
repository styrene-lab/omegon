# Tasks

## 1. UX and Binding Semantics
<!-- specs: lifecycle/work-plan-threading -->

- [ ] 1.1 Define visible labels for ephemeral, design-bound, OpenSpec-bound, and hybrid plans.
- [ ] 1.2 Decide default behavior for completing an OpenSpec-backed item: prompt, explicit command, or configured write-through.
- [ ] 1.3 Specify clear/detach behavior for bound plans.
- [ ] 1.4 Specify degradation copy when a backing artifact is missing or stale.

## 2. Runtime Model
<!-- specs: lifecycle/work-plan-threading -->

- [x] 2.1 Define VisiblePlanState and PlanAction compatibility wrapper before adding registry/write-through behavior.
- [x] 2.2 Route slash-command and plan-tool mutations through one apply_plan_action API.
- [x] 2.3 Preserve backward-compatible work_plan_snapshot_json fields: mode, guidance, completed, total, items.
- [x] 2.4 Migrate legacy session snapshots with work_plan and plan_mode into ephemeral session VisiblePlanState.
- [x] 2.5 Add snapshot tests for active, completed, cleared, detached, and legacy-resume states.
- [x] 2.6 Define PlanScope, PlanSource, PlanStatus, TaskIntent, PlanBinding, PlanRegistryEntry, and PlanItemProjection types only after the compatibility wrapper is in place.

## 3. Lifecycle Projection
<!-- specs: lifecycle/work-plan-threading -->

- [ ] 3.1 Project OpenSpec task groups into compact plan items.
- [ ] 3.2 Surface design-tree focus/binding candidates in the plan projection.
- [ ] 3.3 Expose source/binding through TUI, IPC, and web snapshots.

## 4. Write-through and Reconciliation
<!-- specs: lifecycle/work-plan-threading -->

- [ ] 4.1 Keep OpenSpec/design projections read-only until stable task identity is defined and tested.
- [ ] 4.2 Define stable OpenSpec task identity strategy before durable checkbox mutation.
- [ ] 4.3 Implement explicit write-through for OpenSpec task completion only after identity support, or document why it is deferred.
- [ ] 4.4 Ensure clear/detach does not delete durable lifecycle artifacts.
- [ ] 4.5 Add lifecycle reconciliation checks for bound plan/task mismatches.

## 5. Plan Registry and Completion Ledger
<!-- specs: lifecycle/work-plan-threading -->

- [ ] 5.1 Define stable plan identity and registry entry data structures.
- [ ] 5.2 Split registry derived state from view/session state so the registry does not become a competing database.
- [ ] 5.3 Track active, backgrounded, blocked, completed, detached, archived, and stale plan statuses.
- [ ] 5.4 Define explicit event sources: slash/tool actions, OpenSpec task diffs, design_tree_update calls, cleave/delegate/sentry events, git changes, validation results, and manual operator marking.
- [ ] 5.5 Record background plan events without replacing the visible plan.
- [ ] 5.6 Keep first ledger/event implementation local or session-scoped; do not add a tracked JSONL ledger until evidence boundaries are resolved.
- [ ] 5.7 Add a completion ledger shape with source, binding, evidence, commit, validation, and lifecycle references.

## 6. Resume UX
<!-- specs: lifecycle/work-plan-threading -->

- [ ] 6.1 Rank resume candidates from active foreground, backgrounded/blocked lifecycle-bound, incomplete OpenSpec/design, and recent completed plans.
- [ ] 6.2 Add explicit resume/switch behavior so no stale plan becomes active silently.
- [ ] 6.3 Surface resume candidates in the TUI/dashboard startup flow.
- [ ] 6.4 Define /plan list, /plan show, /plan switch, /plan resume, /plan background, /plan detach, /plan promote, /plan bind, and /plan ledger UX.

## 7. Session vs Repo Scope and Non-Coding Tasking
<!-- specs: lifecycle/work-plan-threading -->

- [ ] 7.1 Add explicit session/repo scope metadata to plan projections and registry entries.
- [ ] 7.2 Define promotion triggers from session plans to repo-bound lifecycle work.
- [ ] 7.3 Define non-coding task intents: research, design, spec, validation, documentation, operations, and review.
- [ ] 7.4 Define TaskCompletionPolicy options: manual, evidence-required, all-subtasks-done, lifecycle-state-reached, and operator-accepted.
- [ ] 7.5 Define completion evidence by task intent, including findings/citations for research and decisions/resolved questions for design.
- [ ] 7.6 Bind research/design task evidence to design-tree research, decisions, and open-question resolution flows.
- [ ] 7.7 Define operations and validation evidence references for branches, worktrees, tags, remote state, tests, smoke checks, and assessments.
