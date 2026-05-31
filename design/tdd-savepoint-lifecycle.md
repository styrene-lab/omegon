+++
id = "8d214819-082b-4742-8b4b-bcca1c528a9c"
kind = "design_node"

[data]
title = "TDD Savepoint Lifecycle"
status = "decided"
issue_type = "architecture"
priority = 1
dependencies = []
open_questions = []
+++

## Overview

Omegon will adopt Savepoint's deterministic red→green transition primitive as a lifecycle-native TDD kernel, not merely as semantic OpenSpec metadata. The system has two layers:

1. **Deterministic transition kernel** — mechanically observes the same normalized command move from failing to passing over a scoped worktree.
2. **Lifecycle attribution shell** — attaches the observed event to OpenSpec changes, scenarios, tasks, design nodes, branches, and assessment evidence.

The deterministic event is the source of truth. Agents may attribute meaning to the event, but they must not fabricate the event itself.

## Research

Savepoint is a language-agnostic CLI pattern implemented in Rust that watches files by extension, runs a command, tracks Passing/Failing state via `.checkpoint.error`, and commits when the command transitions from failing to passing. Its core state machine is:

```text
PASSING --fail--> FAILING
FAILING --pass; commit--> PASSING
```

The useful property is not the generic commit behavior. The useful property is mechanical proof that:

```text
command C failed before
command C passed later
therefore the edit crossed a red→green boundary
```

This resists greenwashing where implementation is written first, tests are added later, and current passing state is incorrectly treated as TDD evidence.

## Decisions

### Decision: Preserve Savepoint determinism as a kernel

**Status:** accepted

Omegon's TDD savepoints must be emitted by a runner/watcher that observes process exit codes for a stable command identity. The raw red→green event is independent of OpenSpec semantics, agent judgment, task completion, or scenario mapping.

### Decision: Layer OpenSpec attribution on top of raw events

**Status:** accepted

OpenSpec change names, scenario IDs, task IDs, design node IDs, and assessment statuses are attribution metadata attached to a raw savepoint event. They are not the source of truth for whether red→green occurred.

### Decision: Require same-command identity for TDD credit

**Status:** accepted

A red→green transition counts as TDD evidence only when the failing and passing runs share the same normalized command identity. The runner records the command vector and command hash.

### Decision: Record code-state identity at the transition

**Status:** accepted

A savepoint event must identify the relevant code state. The first implementation records git branch, HEAD before/after, worktree diff hash, and dirty/staged status. Auto-commit is optional and explicit. If enabled, it stages scoped files rather than using `git commit -am`, so new tests are not lost.

### Decision: Default to lifecycle event, opt in to git commit

**Status:** accepted

Default behavior writes a lifecycle savepoint event. `--commit` may additionally create a structured conventional commit. This avoids commit spam while preserving deterministic TDD evidence.

### Decision: Archive/assessment can distinguish current pass from TDD pass

**Status:** accepted

OpenSpec assessment should eventually distinguish:

- `pass`: scenario passes now
- `tdd-pass`: scenario passes and has red→green savepoint evidence
- `pass-no-red`: scenario passes but no failing baseline was captured
- `stale-pass`: previously passed but command was not rerun for current code state
- `fail`: scenario fails

### Decision: Store raw event logs under `.omegon/lifecycle/savepoints/` and project durable summaries into OpenSpec artifacts

**Status:** accepted

Raw runner events are append-only local lifecycle evidence because they include noisy command output/diff identity and may churn during development. OpenSpec artifacts receive durable summaries and attribution references when a savepoint is attached to a change/scenario. This follows the existing principle that the lifecycle registry must not become a competing durable task database.

### Decision: Ship the first command as `omegon tdd watch`

**Status:** accepted

`tdd` is the deterministic kernel surface. `opsx` integration is an attribution layer that can call or consume kernel events later. This prevents the raw watcher from depending on OpenSpec availability and keeps the trust boundary clear.

## Non-goals

- Do not adopt Savepoint wholesale as the authoritative lifecycle implementation.
- Do not let agents manually author raw red→green events.
- Do not make every OpenSpec scenario require watch mode in the first implementation.
- Do not auto-commit by default.
- Do not use a single global marker file like `.checkpoint.error` for all changes/scenarios.

## Command Surface

Minimal kernel form:

```sh
omegon tdd watch \
  --filetype <ext> \
  --watch <src-or-test-path> \
  -- <test-command> <filter>
```

Optional attribution flags:

```sh
omegon tdd watch \
  --change <openspec-change> \
  --scenario <domain/scenario-id> \
  --task <task-id> \
  --filetype <ext> \
  --watch <src-or-test-path> \
  -- <test-command> <filter>
```

Commit mode:

```sh
omegon tdd watch --commit --commit-scope <src-or-test-path> -- <test-command> <filter>
```

## Raw Savepoint Event Shape

```json
{
  "kind": "tdd_savepoint",
  "event_id": "redgreen-...",
  "transition": "failing_to_passing",
  "command": ["<test-runner>", "<filter>"],
  "command_hash": "sha256:...",
  "previous_exit": 101,
  "current_exit": 0,
  "watched_paths": ["<src-or-test-path>"],
  "branch": "exploration/rust-savepoint-tdd",
  "head_before": "...",
  "head_after": "...",
  "worktree_diff_hash_before": "...",
  "worktree_diff_hash_after": "...",
  "dirty_before": true,
  "dirty_after": true,
  "commit": null,
  "created_at": "..."
}
```

## Attribution Event Shape

```json
{
  "kind": "tdd_savepoint_attribution",
  "savepoint_event": "redgreen-...",
  "openspec_change": "jwt-auth",
  "scenario": "auth/expired-token-rejected",
  "task": "2.1",
  "design_node": "jwt-auth"
}
```

## Lifecycle Integration

```text
Planned
  ↓ register_test_file
Testing
  ↓ deterministic failing baseline captured
Implementing
  ↓ deterministic red→green savepoint observed
Verifying
  ↓ /assess spec consumes event evidence
Archived
```

The savepoint event strengthens lifecycle gates, but initial implementation reports missing TDD evidence before it blocks archive by default.

## Implementation Plan

### Phase 1 — Kernel data model and command hashing

- Add a Rust module for TDD savepoint events, likely under `core/crates/omegon/src/features/tdd/` or as a small `omegon-opsx`-owned module if lifecycle storage reuse is cleaner.
- Define `TddCommand`, `CommandHash`, `RunOutcome`, `SavepointEvent`, and `SavepointState`.
- Normalize command identity from argv after `--`; preserve exact argv in the event.
- Hash command identity with SHA-256 or the repo's existing hash helper if one exists.
- Unit-test command normalization and same-command matching.

### Phase 2 — Deterministic runner without watch mode

- Implement a non-watch internal function that runs the command and returns exit status, stdout/stderr summary, and duration.
- Use process spawning with argument arrays, not shell interpolation.
- Add timeout support or inherit the project command timeout convention if one exists.
- Unit-test pass/fail classification using trivial commands.

### Phase 3 — Git/worktree identity capture

- Capture branch, HEAD, dirty status, staged status, and scoped diff hash before and after a run.
- Prefer argument-array git invocations.
- Include untracked files in diff identity for configured scopes so new tests are not invisible.
- Unit-test against temporary git repos.

### Phase 4 — Event storage

- Append raw events to `.omegon/lifecycle/savepoints/<command_hash>.jsonl` or `.omegon/lifecycle/savepoints/<change-or-session>.jsonl`.
- Use atomic append semantics or write-temp-then-rename if append atomicity is not sufficient on target platforms.
- Add a reader that can query events by command hash, change, scenario, task, and transition.
- Unit-test event roundtrip and corrupt-line tolerance.

### Phase 5 — Watch loop

- Add `omegon tdd watch` CLI wiring.
- Watch configured paths recursively, with `--filetype` filters and debounce/drain behavior inspired by Savepoint.
- On startup, run once to establish baseline state.
- On each relevant change, rerun the command.
- Emit a raw savepoint event only on `Failing → Passing` for the same command hash.
- Record failing baseline events separately or as state so lifecycle can prove red existed before green.

### Phase 6 — Optional commit mode

- Add explicit `--commit` and `--commit-scope` flags.
- Stage only configured scopes.
- Include new files in scope.
- Commit with a structured conventional message, e.g. `test(tdd): savepoint <scenario-or-command>`.
- Include trailers such as `TDD-Transition`, `OpenSpec-Change`, `OpenSpec-Scenario`, `Command-Hash`, and `Savepoint-Event` when attribution exists.
- Unit-test commit behavior in a temporary git repo.

### Phase 7 — OpenSpec attribution

- If `--change`, `--scenario`, `--task`, or `--design-node` are provided, write attribution records linked to the raw event.
- Project durable summaries into `openspec/changes/<change>/` without making raw event logs the OpenSpec source of truth.
- Add lookup APIs for `/assess spec` to ask whether a scenario has red→green evidence.

### Phase 8 — Lifecycle reporting and gates

- Extend lifecycle/status reporting to show TDD evidence state: no baseline, red captured, red→green captured, stale, or missing.
- Initially warn on missing TDD evidence rather than blocking archive.
- Add a later strict mode for changes that opt into TDD-required verification.

### Phase 9 — Assessment integration

- Extend assessment JSON with TDD evidence metadata.
- Distinguish `pass`, `tdd-pass`, `pass-no-red`, `stale-pass`, and `fail` in reports.
- Ensure archive gate can report TDD evidence gaps explicitly.

### Phase 10 — Documentation and operator workflow

- Document the command in lifecycle/OpenSpec docs.
- Add an example workflow for writing a failing test, starting `omegon tdd watch`, observing red baseline, implementing, and capturing red→green.
- Document why raw red→green events are runner-emitted and not agent-authored.

## Acceptance Criteria

- A stable command that fails and later passes produces exactly one red→green savepoint event for that transition.
- A command that starts passing produces no red→green event until a failure has first been observed.
- Changing semantic attribution without changing command identity cannot fabricate raw TDD evidence.
- Raw events include command hash and code-state identity.
- New test files in commit scope are included when `--commit` is used.
- OpenSpec can report whether a scenario has linked TDD evidence.
- Missing TDD evidence is visible in assessment/lifecycle reports.
