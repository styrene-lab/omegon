# Tasks

## 1. UX and Binding Semantics
<!-- specs: lifecycle/work-plan-threading -->

- [x] 1.1 Define visible labels for ephemeral, design-bound, OpenSpec-bound, and hybrid plans.
- [x] 1.2 Decide default behavior for completing an OpenSpec-backed item: explicit lifecycle command only; `/plan` completion remains runtime-only unless delegated through `openspec_manage` stable task-id mutation.
- [x] 1.3 Specify clear/detach behavior for bound plans: session clear deletes runtime state; repo-bound clear detaches the projection and never edits durable artifacts.
- [x] 1.4 Specify degradation copy when a backing artifact is missing or stale: mark projection `Stale`, keep last visible summary, and require explicit sync/rebind.

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

- [x] 3.1 Project OpenSpec task groups into compact plan items.
- [x] 3.2 Surface design-tree focus/binding candidates in the plan projection (`core/crates/omegon/src/conversation.rs`, `core/crates/omegon/src/lifecycle/design.rs`, plan list renderer).
- [x] 3.3 Expose source/binding through TUI, IPC, and web snapshots.
- [x] 3.4 Expose read-only `/plan list` for both operator slash UX and remote slash execution.

## 4. Write-through and Reconciliation
<!-- specs: lifecycle/work-plan-threading -->

- [x] 4.1 Keep OpenSpec/design projections read-only until stable task identity is defined and tested.
- [x] 4.2 Define stable OpenSpec task identity strategy before durable checkbox mutation.
- [x] 4.3 Implement explicit write-through through `openspec_manage` only; `/plan` must not gain duplicate OpenSpec mutation actions.
- [x] 4.4 Add `openspec_manage` task-status mutation with strict numeric task-id matching and refusal on ambiguity.
- [x] 4.5 Ensure clear/detach does not delete durable lifecycle artifacts (`apply_plan_action`, slash `/plan clear`, tool `plan` clear).
- [x] 4.6 Add lifecycle reconciliation checks for bound plan/task mismatches (missing tasks.md, changed checkbox ids, deleted design node, divergent progress).

## 5. Plan Registry and Completion Ledger
<!-- specs: lifecycle/work-plan-threading -->
<!-- implementation slice: A registry-core, B registry-projection, C resume-ledger -->

- [x] 5.1 Finalize stable plan id constructors in `core/crates/omegon/src/conversation.rs`: `session:current`, `openspec:<change>[:group:<n>]`, `design:<node-id>`, `hybrid:<change>:<node-id>`, `branch:<name>`.
- [x] 5.2 Implement a read-only `PlanRegistry` builder that recomputes derived entries from visible session state, OpenSpec `tasks.md`, design-tree focus/status, Flynt task-board links, and current git branch/worktree.
- [x] 5.3 Keep registry view state session-local: backgrounded/detached/dismissed/last_visible/resume_hint must not become a tracked repo task database.
- [x] 5.4 Support `PlanStatus::{Active, Backgrounded, Blocked, Completed, Detached, Archived, Stale}` in registry output and snapshot JSON.
- [x] 5.5 Add explicit `PlanEventSource` values for slash/tool actions, OpenSpec task diffs, design_tree_update calls, cleave/delegate/sentry events, git changes, validation results, and manual operator marking.
- [x] 5.6 Record background plan events in session state without replacing `visible_plan`; emit concise notifications only.
- [x] 5.7 Add a session-local completion ledger shape with source, binding, evidence refs, commit refs, validation refs, lifecycle refs, and summary. Do not add tracked JSONL storage in this change.
- [x] 5.8 Add unit/snapshot tests proving completed/backgrounded registry entries do not resurrect as the foreground visible plan after resume.

## 6. Resume UX
<!-- specs: lifecycle/work-plan-threading -->
<!-- implementation slice: C resume-ledger, D acp-surfaces -->

- [x] 6.1 Implement resume ranking: active foreground, backgrounded/blocked lifecycle-bound, incomplete OpenSpec/design with recent activity, then recent completed context.
- [x] 6.2 Add explicit `/plan resume <id>` and `/plan switch <id>` handlers; neither may auto-select stale candidates during startup.
- [x] 6.3 Surface ranked resume candidates in TUI/dashboard startup or Slim plan lane with explicit operator choice copy.
- [x] 6.4 Implement `/plan show <id>`, `/plan background [id]`, `/plan detach [id]`, `/plan promote`, `/plan bind ...`, and `/plan ledger [id]` as registry/session operations.
- [x] 6.5 Add stale-plan copy: `Plan source changed or disappeared; showing last summary. Run /plan sync, /plan rebind, or /plan detach.`
- [x] 6.6 Add tests that startup never silently makes a completed/stale repo-bound plan foreground.
- [x] 6.7 Expose plan and task projection surfaces over ACP: `_plans/list`, `_plans/show`, `_plans/events`, `_plans/switch`, `_plans/detach`, `_tasks/list`, `_tasks/show`, `_tasks/bind`, and `_tasks/events` with capability advertisement.

## 7. Session vs Repo Scope and Non-Coding Tasking
<!-- specs: lifecycle/work-plan-threading -->
<!-- implementation slice: A registry-core, B registry-projection, D acp-surfaces -->

- [x] 7.1 Thread `PlanScope::{Session, Repo}` through plan projections, registry entries, `/plan list`, TUI snapshots, IPC, and web snapshots.
- [x] 7.2 Implement promotion nudges for multi-session, backgrounded, branch-attached, multi-file, public-API, or design-question-heavy work.
- [x] 7.3 Finalize `TaskIntent::{Research, Design, Spec, Implementation, Validation, Documentation, Operations, Review}` and infer intents for OpenSpec groups/design tasks where possible.
- [x] 7.4 Add `TaskCompletionPolicy::{Manual, EvidenceRequired, AllSubtasksDone, LifecycleStateReached, OperatorAccepted}` to item projections/ledger records.
- [x] 7.5 Define `EvidenceRef` variants for findings/citations, decisions/resolved questions, spec scenarios/tasks, diffs, validation runs, docs, branches/worktrees/tags/remotes, deployments, and review blockers.
- [x] 7.6 Bind research/design evidence to design-tree research, decisions, and open-question resolution flows without requiring code diffs.
- [x] 7.7 Bind operations/validation evidence to branches, worktrees, tags, remote state, tests, smoke checks, and assessments.
- [x] 7.8 Add tests for evidence-required research/design/validation policies and manual override behavior.
- [x] 7.9 Add external task refs to task projections/bindings (`system`, `board_id`, `task_id`, external refs) so Flynt board tasks can link to Omegon plan tasks without owning OpenSpec/design completion.
