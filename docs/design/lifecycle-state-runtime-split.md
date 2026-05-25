+++
id = "lifecycle-state-runtime-split"
tags = ["lifecycle", "git-hygiene", "runtime-state", "design-tree"]
aliases = ["lifecycle-runtime-state-split", "tracked-lifecycle-state"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Lifecycle state/runtime split

## Overview

Omegon currently writes mutable lifecycle runtime state into `ai/lifecycle/state.json`, and that file is tracked by git. Running the tool inside its own repository therefore produces recurring dirty-tree churn during release hardening and ordinary development.

Observed current dirty state on `release/0.23`:

- timestamp-only updates on active changes such as `knowledge-quadrant-lifecycle`, `omega-daemon-runtime-v1`, `splash-systems-integration`, and `styrene-identity-secrets`
- unrelated 0.24 host-action changes materialized into the 0.23 working tree:
  - `host-actions-core-runtime`
  - `terminal-create-host-action`
- transition-log entries for those unrelated changes

The target architecture is Option C: split lifecycle information into a tracked baseline artifact and an ignored mutable runtime state store.

```text
tracked baseline / portable project memory
  ai/lifecycle/baseline.json or docs/design/*

ignored mutable runtime state
  .omegon/lifecycle/state.json
  .omegon/lifecycle/state.json.lock
```

The goal is to preserve useful lifecycle/design knowledge while preventing runtime bookkeeping from dirtying source-control state.

## Problem statement

`ai/lifecycle/state.json` is trying to serve two incompatible roles:

1. **Project artifact** — reviewable lifecycle metadata that can be committed, branched, and shared.
2. **Runtime state database** — timestamps, transient change registration, lock files, and per-session lifecycle transitions.

Because it is tracked, runtime writes show up in every `git status`, pollute release patch commits, and can leak unrelated branch/session state into stable release lines.

## Scope

### In scope

- Define the separation between tracked lifecycle baseline and ignored runtime state.
- Move default runtime writes from `ai/lifecycle/state.json` to an ignored `.omegon/lifecycle/state.json` path.
- Keep a migration path for existing repositories with tracked `ai/lifecycle/state.json`.
- Update dirty-tree classification so runtime lifecycle state is treated as volatile/ignored, while intentional baseline changes remain visible.
- Add tests covering path resolution, migration, and dirty-tree hygiene behavior.

### Out of scope

- Redesigning the design-tree document format.
- Changing OpenSpec proposal/spec/task semantics.
- Removing historical design docs.
- Changing 0.24 host-action lifecycle content itself; the issue is where runtime state is stored, not whether those changes exist.

## Research

### Current evidence

`git ls-files ai/lifecycle/state.json` confirms the current state file is tracked.

`git diff -- ai/lifecycle/state.json` shows runtime churn rather than source changes:

- `updated_at` fields change whenever lifecycle state is touched.
- unrelated changes can be registered from another branch/session.
- a `state.json.lock` file exists next to the tracked file, reinforcing that the file is runtime database state rather than a purely authored document.

### Existing dirty-tree handling

`scripts/dirty_report.py` already classifies `ai/lifecycle/state.json` as `lifecycle-state` and warns that it should be committed separately only when intentional. This is a mitigation, not a fix: the file still appears dirty and still requires manual revert/discipline.

### Design tension

Some lifecycle state is valuable as project memory. But timestamp updates, lock files, and active-session transition logs are not release artifacts. The split should make project memory explicit and runtime state private-by-default.

## Open questions

1. [assumption] `.omegon/` is the correct ignored runtime namespace for lifecycle state in this repository and other Omegon-managed projects.
2. [assumption] Existing `ai/lifecycle/state.json` content can be migrated or copied into the new runtime state location without needing to preserve all timestamp churn in git history.
3. Which lifecycle records are baseline-worthy and should remain tracked: active change names only, design-tree bindings, archived transitions, milestones, or none of the runtime JSON?
4. Should `ai/lifecycle/baseline.json` exist as a machine-readable tracked artifact, or should tracked lifecycle knowledge live only in `docs/design/` and OpenSpec files?
5. How should branch-specific runtime state be isolated so 0.24 lifecycle changes do not materialize in a 0.23 release worktree?
6. What should happen if both old tracked `ai/lifecycle/state.json` and new `.omegon/lifecycle/state.json` exist and conflict?
7. Should migration automatically remove/deprecate `ai/lifecycle/state.json`, or only warn and prefer the new runtime path?

## Candidate design

### Storage layout

```text
ai/lifecycle/baseline.json          # optional tracked export / baseline, stable and reviewable
.omegon/lifecycle/state.json        # ignored mutable runtime state
.omegon/lifecycle/state.json.lock   # ignored lock file
```

Path policy:

1. Runtime lifecycle reads/writes use `.omegon/lifecycle/state.json` by default.
2. If `.omegon/lifecycle/state.json` does not exist but legacy `ai/lifecycle/state.json` exists, runtime performs a one-time migration/copy.
3. Runtime never updates legacy `ai/lifecycle/state.json` unless an explicit export/baseline command is invoked.
4. Dirty-tree tooling classifies `.omegon/lifecycle/*` as ignored/volatile and keeps tracked baseline files visible.

### Baseline/export policy

Tracked baseline export should be explicit:

```text
omegon lifecycle export-baseline
```

or an existing lifecycle/design command can gain an explicit export action. Export should write stable, deterministic content only:

- no wall-clock `updated_at` churn unless semantically required
- no lock/session metadata
- no branch-local transient change registrations
- deterministic sort order

### Migration policy

On first run after the split:

1. Create `.omegon/lifecycle/` if missing.
2. If `.omegon/lifecycle/state.json` is absent and legacy `ai/lifecycle/state.json` exists, copy legacy state into the runtime path.
3. Emit a one-time notice that runtime lifecycle state moved.
4. Leave tracked legacy file untouched; a later cleanup commit can remove or replace it with a baseline artifact.

Conflict policy:

- If both files exist, prefer `.omegon/lifecycle/state.json` for runtime.
- Provide a lifecycle doctor warning if legacy tracked state is newer or diverges.
- Do not silently merge divergent state.

## Decisions to make

1. **Runtime state path** — use `.omegon/lifecycle/state.json` for mutable runtime state.
2. **Tracked baseline format** — decide whether to introduce `ai/lifecycle/baseline.json` or rely on docs/OpenSpec/design-tree files.
3. **Migration behavior** — copy legacy state into runtime on first run, then stop writing the legacy path.
4. **Export behavior** — only explicit commands write tracked lifecycle baseline artifacts.
5. **Branch isolation** — runtime state should be per-worktree by default; cross-session/project sync belongs in memory/vault export, not a tracked mutable JSON file.

## Implementation sketch

1. Locate lifecycle state path construction code.
2. Introduce a `LifecycleStatePaths` helper:
   - `runtime_state_path(root) -> .omegon/lifecycle/state.json`
   - `legacy_state_path(root) -> ai/lifecycle/state.json`
   - `baseline_path(root) -> ai/lifecycle/baseline.json` if adopted
3. Update lifecycle state load/save to use runtime path.
4. Add migration-on-load from legacy path if runtime path is missing.
5. Update `.gitignore` if `.omegon/lifecycle/` is not already ignored.
6. Update dirty report classification:
   - runtime path: volatile/ignored
   - legacy tracked path: legacy warning
   - baseline path: normal tracked lifecycle artifact
7. Add tests:
   - new repos write runtime state under `.omegon/lifecycle/`
   - legacy state migrates without mutating `ai/lifecycle/state.json`
   - both-files conflict prefers runtime and reports warning
   - deterministic baseline export omits timestamp-only churn
8. Update docs and changelog.

## Acceptance criteria

- Running Omegon in its own repo no longer dirties `ai/lifecycle/state.json` during normal lifecycle/tool activity.
- 0.24 lifecycle changes cannot appear as tracked dirty state in the 0.23 release worktree merely because the runtime touched lifecycle state.
- Operators can still export/review intentional lifecycle baseline changes.
- Existing repositories with `ai/lifecycle/state.json` keep working through migration.
- Dirty-tree reports stop treating routine lifecycle runtime writes as release-work noise.

## Risks

- Migration could hide meaningful lifecycle state if operators assumed the tracked JSON was canonical.
- Tests or scripts may reference `ai/lifecycle/state.json` directly.
- Branch/worktree-local runtime state may surprise users who expected lifecycle active-change state to move through git.
- Removing timestamp churn from tracked state may require a new explicit export command to preserve useful lifecycle summaries.
