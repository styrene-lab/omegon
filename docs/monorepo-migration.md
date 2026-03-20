---
id: monorepo-migration
title: Monorepo migration — absorb core into omegon, eliminate submodule
status: resolved
parent: git-harness-integration
tags: [architecture, git, monorepo, jj]
open_questions: []
priority: 1
---

# Monorepo migration — absorb core into omegon, eliminate submodule

## Overview

The core submodule is the root cause of three entire bug classes: (1) cleave worktree submodule failures, (2) two-level commit dance complexity, (3) ceremony pointer-update commits. Absorbing core into the main repo eliminates all three AND unblocks jj-lib adoption. The core is never used independently — every omegon release pins a specific core SHA. 22 submodule-pointer commits on main are pure noise.

## Research

### What the submodule costs us

**Direct costs (measured this session):**
- 22 ceremony commits on main (`chore: update core submodule`)
- Entire cleave-submodule-worktree bug class (2/4 children failing)
- `salvage_worktree_changes`, `verify_scope_accessible`, `build_submodule_context`, `commit_dirty_submodules` — ~200 lines of code that exists solely because of submodules
- `submodule_init` in every worktree creation — network round-trip, failure mode
- Two-level commit dance (commit inside submodule, then commit pointer in parent)
- git2 can't stage submodule pointers (`index.add_path` fails on gitlinks) — forced CLI fallback
- TS dirty-tree preflight can't classify submodule state — required `parseGitmodules` + special handling

**Indirect costs:**
- Blocks jj-lib adoption (jj ignores submodules entirely)
- Every new developer/CI setup requires `git submodule update --init --recursive`
- `git clone` doesn't get core by default — needs `--recursive`
- Branch operations require coordinating two repos
- Adversarial review found 4 submodule-related issues out of 15 total (27% of findings)

**What the submodule gives us:**
- Independent versioning of core — **never used**. Core version is always locked to omegon.
- Separate CI for core — **not leveraged**. Core is always built as part of omegon.
- Smaller clone for omegon — **marginal**. The Rust crates are not large.

**The submodule provides zero practical value and costs us >200 lines of workaround code, 22 ceremony commits, and blocks the most promising architectural improvement (jj-lib).**

### Migration path — git subtree or direct merge

**Option A: `git subtree add` — preserves core history**

```bash
git subtree add --prefix=core <core-repo-url> main --squash
```

Imports core's content into `core/` as a regular directory. Core's history is squashed into one commit (or preserved with `--no-squash`). Future updates from the core repo can be pulled with `git subtree pull`. The core/ directory becomes a regular part of the monorepo.

**Option B: Direct merge — fresh start**

Remove the submodule, copy the files in, commit. Core's history lives in its own repo (archived). The monorepo starts fresh from the current state.

**Option C: `git subtree merge` with history splice**

More complex but preserves full history as if core was always in the monorepo. Rewrites history — breaks existing SHAs.

**Recommendation: Option A (subtree add, squash).** Simplest, preserves the ability to pull from core repo if ever needed, doesn't rewrite history. The 22 pointer-update commits stay in history but no new ones are created.

**Post-migration cleanup:**
1. Remove `.gitmodules`
2. Remove submodule entry from `.git/config`
3. Update CI workflows (no more `--recursive` clone)
4. Update CONTRIBUTING.md
5. Remove all submodule-specific code from omegon-git and cleave
6. Initialize jj in co-located mode: `jj git init --git-repo .`

## Decisions

### Decision: Absorb omegon-core into omegon as a regular directory, eliminate the submodule

**Status:** decided
**Rationale:** The submodule provides zero practical value (core is never used independently) and costs 200+ lines of workaround code, 22 ceremony commits, an entire bug class (cleave worktree submodule failures), and blocks jj-lib adoption. Absorbing core into the monorepo via git subtree add eliminates all of this. The Rust crates stay at core/ — no path changes needed in Cargo.toml or imports. CI, clone, and branch operations all become simpler. The core repo is archived but remains available if ever needed.

### Decision: After monorepo migration, initialize jj co-located mode and begin jj-lib integration

**Status:** decided
**Rationale:** With the submodule eliminated, jj-lib's only blocker is removed. Co-located mode (jj init --git-repo .) preserves full git compatibility — the operator can still push/pull/PR via git while the harness uses jj-lib internally for commits, workspaces, rebase, and operation tracking. The omegon-git crate's public API stays the same — the implementation swaps from git2 to jj-lib. This is the abstraction boundary we built for exactly this purpose.

## Open Questions

*No open questions.*
