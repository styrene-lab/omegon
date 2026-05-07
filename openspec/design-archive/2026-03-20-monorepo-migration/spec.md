+++
id = "a84fee27-6adf-4717-a7dc-340d6356c000"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Monorepo migration — absorb core into omegon, eliminate submodule — Design Spec (extracted)

> Auto-extracted from docs/monorepo-migration.md at decide-time.

## Decisions

### Absorb omegon-core into omegon as a regular directory, eliminate the submodule (decided)

The submodule provides zero practical value (core is never used independently) and costs 200+ lines of workaround code, 22 ceremony commits, an entire bug class (cleave worktree submodule failures), and blocks jj-lib adoption. Absorbing core into the monorepo via git subtree add eliminates all of this. The Rust crates stay at core/ — no path changes needed in Cargo.toml or imports. CI, clone, and branch operations all become simpler. The core repo is archived but remains available if ever needed.

### After monorepo migration, initialize jj co-located mode and begin jj-lib integration (decided)

With the submodule eliminated, jj-lib's only blocker is removed. Co-located mode (jj init --git-repo .) preserves full git compatibility — the operator can still push/pull/PR via git while the harness uses jj-lib internally for commits, workspaces, rebase, and operation tracking. The omegon-git crate's public API stays the same — the implementation swaps from git2 to jj-lib. This is the abstraction boundary we built for exactly this purpose.

## Research Summary

### What the submodule costs us

**Direct costs (measured this session):**
- 22 ceremony commits on main (`chore: update core submodule`)
- Entire cleave-submodule-worktree bug class (2/4 children failing)
- `salvage_worktree_changes`, `verify_scope_accessible`, `build_submodule_context`, `commit_dirty_submodules` — ~200 lines of code that exists solely because of submodules
- `submodule_init` in every worktree creation — network round-trip, failure mode
- Two-level commit dance (commit inside submodule, then commit pointer in …

### Migration path — git subtree or direct merge

**Option A: `git subtree add` — preserves core history**

```bash
git subtree add --prefix=core <core-repo-url> main --squash
```

Imports core's content into `core/` as a regular directory. Core's history is squashed into one commit (or preserved with `--no-squash`). Future updates from the core repo can be pulled with `git subtree pull`. The core/ directory becomes a regular part of the monorepo.

**Option B: Direct merge — fresh start**

Remove the submodule, copy the files in, commit. Core's…
