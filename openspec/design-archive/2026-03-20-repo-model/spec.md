+++
id = "c90e3f99-3388-47bb-b705-a8083a7d75e2"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# RepoModel — git state tracking in Rust core — Design Spec (extracted)

> Auto-extracted from docs/repo-model.md at decide-time.

## Decisions

### Use git2 as the primary git library, shell out to git CLI only for gaps (decided)

git2 covers all 7 operation categories we need (discovery, status, index, branch, worktree, submodule, stash). gix is missing checkout, worktree create, and stash entirely. git2 is battle-tested (cargo uses it), adds 121 deps (vs 400+ for gix), and the C dependency (libgit2-sys) is acceptable — omegon already compiles native code via reqwest and rusqlite. Shell out to git CLI only where git2's API is awkward or where we need exact CLI flag parity (e.g., git worktree add with specific branch tracking options). Long-term, gix may catch up and could replace git2, but it's not ready today for our critical paths.

## Research Summary

### Git crate assessment for RepoModel

**Two contenders: `git2` (libgit2 bindings) vs `gix` (pure Rust gitoxide)**

### git2 v0.20 — mature, complete, C dependency

| Need | git2 support |
|------|-------------|
| Repo discovery | ✅ `Repository::discover()` |
| Status (dirty files) | ✅ `Repository::statuses()` — full porcelain-equivalent |
| Current branch | ✅ `repo.head()?.shorthand()` |
| Index add/stage | ✅ `index.add_path()`, `index.add_all()` |
| Create commit | ✅ `repo.commit()` |
| Branch create/delete | ✅ `repo.branch()`, `branch.delete()` |
| Checkout | ✅ `repo.checkout_tree()` |
| Merge | ✅ `repo.merge()` with conflict detection |
| Worktree creat…

### gix v0.80 — pure Rust, incomplete on our critical paths

| Need | gix support |
|------|------------|
| Repo discovery | ✅ `gix::discover()` |
| Status (dirty files) | ✅ `gix status` — recently added submodule awareness |
| Current branch | ✅ `repo.head_ref()` |
| Index add/stage | ⚠️ `index` feature exists but no high-level `add_path` equivalent |
| Create commit | ✅ encode commit objects + write |
| Branch create/delete | ✅ via refs |
| Checkout | ❌ Not implemented (crate-status.md: `[ ] checkout with conversions`) |
| Merge | ⚠️ Blob/tree merge exi…

### Hybrid option: git2 for API, shell out for gaps

Use `git2` for the operations it handles well (status, index, commit, submodules, stash), and shell out to `git` CLI only for operations where library support is marginal or where we need exact CLI parity (worktree create with `git worktree add` flags, merge with `--no-ff`).

### Recommendation: git2

`git2` covers 100% of our operation surface. `gix` covers about 60% and we'd need shell-out fallbacks for the rest. The C dependency cost is real but manageable — omegon already compiles native code (reqwest TLS, rusqlite), and cargo itself uses git2. The pure-Rust story for gix is compelling long-term but it's not there yet for our needs (stash, worktree create, checkout are all missing).
