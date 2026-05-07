+++
id = "1ddb274b-9f71-4ee2-9993-0559c65e49c1"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Merge safety improvements — prevent squash-merge regressions and lost work

## Overview

The 0.15.1 consolidation session exposed catastrophic merge regressions: a squash merge of a long-running feature branch silently took main's versions of shared files, dropping months of TUI improvements, dashboard rewrites, tutorial fixes, provider expansions, and lifecycle hardening. The session spent most of its time discovering and recovering lost work from git history. This document proposes concrete harness-level improvements to prevent this class of failure.\n\nRoot causes:\n1. Long-running feature branches that accumulate unrelated changes\n2. Squash merge resolves conflicts by taking 'ours' without per-file review\n3. No automated regression detection between pre-merge and post-merge state\n4. No 'feature parity gate' that verifies functional equivalence after merge\n5. No visual diff review for TUI changes (only code diff, no screenshot comparison)"

## Decisions

### Decision: Pre-merge test count gate — refuse merge if test count drops

**Status:** decided
**Rationale:** The merge dropped 31 tests silently. A pre-merge check that records the test count from the source branch and refuses to merge if the target has fewer tests would have caught this immediately. Implementation: `just merge-check` recipe that runs tests on both branches and compares counts. Also store test count in a `.test-count` file that's committed with each RC.

### Decision: Post-merge binary smoke test — verify key capabilities survived

**Status:** decided
**Rationale:** After merge, run a capability smoke test: `omegon --version` (binary works), provider count check (auth.rs PROVIDERS length), tool count check (TOOL_COUNT matches), dashboard renders (snapshot test passes), tutorial overlay activates (step count > 0). This catches the class of bug where code compiles and unit tests pass but entire subsystems were silently reverted. Implementation: `just smoke` recipe or a `#[test] fn post_merge_smoke()` that checks structural invariants.

### Decision: Ban long-running feature branches — merge to main weekly or decompose

**Status:** decided
**Rationale:** The orchestratable-provider-model branch had 109 commits and accumulated unrelated TUI, tutorial, lifecycle, and provider work. When it was squash-merged, the conflict resolution was intractable. Policy: feature branches must merge to main within one week or be decomposed into smaller changes. The /cleave system already supports this — use it for the branch work too, not just for implementation tasks.

### Decision: Per-file conflict review — never bulk-resolve with --ours or --theirs

**Status:** decided
**Rationale:** The session used `git checkout --ours` on 6+ files during the squash merge, which silently discarded the branch's versions of dashboard.rs, footer.rs, instruments.rs, tests.rs, theme.rs, and tutorial.rs. Every conflicted file must be reviewed individually. The harness should refuse to auto-resolve conflicts during cleave merges and instead present each conflict for operator review. For agent-driven merges, the agent must read both versions and produce a merged result, never blindly pick a side.

### Decision: Source-of-truth manifest — track which commit owns each subsystem

**Status:** decided
**Rationale:** A `.subsystem-owners` file (or section in CONTRIBUTING.md) that maps file paths to the branch/commit that last authoritatively changed them. During merge, if a file has been modified on both branches, the merge must prefer the version from the branch that owns that subsystem. Example: `tui/dashboard.rs → main (b721e3c)`, `routing.rs → feature/orchestratable-provider-model`. This prevents the scenario where a branch that never touched the dashboard accidentally reverts it.

### Decision: Line-count regression detector in just rc

**Status:** decided
**Rationale:** The dashboard went from 1550 lines to 915 — a 40% drop that was invisible until the operator saw a broken UI. Add a line-count check to `just rc` that compares key files against the previous RC tag. If any file drops by more than 20%, print a warning. Not a hard gate, but a visible signal. Files to track: providers.rs, dashboard.rs, mod.rs, instruments.rs, auth.rs.

## Open Questions

*No open questions.*
