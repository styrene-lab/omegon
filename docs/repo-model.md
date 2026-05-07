+++
id = "2c2a89c3-c7f5-4205-8624-ab75aeb204d5"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# RepoModel — git state tracking in Rust core

## Overview

Shared struct initialized at agent startup. Tracks current branch, dirty files (working set), submodule map, and pending lifecycle changes. Updated by edit/write/change tools on every file mutation. Queried by cleave preflight, commit tool, and session-close handler. Replaces all ad-hoc git status calls with a coherent model.

## Research

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
| Worktree create/remove | ✅ `repo.worktree()` |
| Submodule list/init | ✅ `repo.submodules()`, `sub.update()` |
| Stash push/pop | ✅ `repo.stash_save()`, `stash_pop()` |

**Deps:** 121 crates, includes libgit2-sys (C compilation), openssl-sys, libz-sys, libssh2-sys.
**Tradeoff:** Complete API, battle-tested (used by cargo itself). But C dependency means cross-compilation friction and OpenSSL linkage.

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
| Merge | ⚠️ Blob/tree merge exists, but full `git merge` workflow not wrapped |
| Worktree create/remove | ❌ Can open existing worktrees, cannot create new ones |
| Submodule list/init | ⚠️ `gix-submodule` crate exists, but init/update missing |
| Stash push/pop | ❌ Explicitly listed as `[ ] stashing` |

**Deps:** 400-530 crates (pure Rust, no C). No OpenSSL, no libz-sys.
**Tradeoff:** Pure Rust is great for portability, but three of our critical operations (checkout, worktree create, stash) are unimplemented. We'd still need `Command::new("git")` fallbacks for those.

### Hybrid option: git2 for API, shell out for gaps

Use `git2` for the operations it handles well (status, index, commit, submodules, stash), and shell out to `git` CLI only for operations where library support is marginal or where we need exact CLI parity (worktree create with `git worktree add` flags, merge with `--no-ff`).

### Recommendation: git2

`git2` covers 100% of our operation surface. `gix` covers about 60% and we'd need shell-out fallbacks for the rest. The C dependency cost is real but manageable — omegon already compiles native code (reqwest TLS, rusqlite), and cargo itself uses git2. The pure-Rust story for gix is compelling long-term but it's not there yet for our needs (stash, worktree create, checkout are all missing).

### jj-lib assessment — would jujutsu work for our needs?

**jj-lib (jujutsu library crate) — pure Rust, git-compatible VCS**

### What it offers that's compelling

- **No index.lock contention** — jj's model doesn't use git's index lock. Multiple agents can operate concurrently without lock failures. This is the exact problem cleave children hit.
- **First-class rebase** — rebase is a core operation, not a CLI hack with sed/python. `MutableRepo::rebase_descendants()` does what our `cleanup_and_merge` attempts.
- **Working copy as a commit** — jj treats the working directory as a mutable commit. This maps perfectly to our RepoModel's "working set" concept — it's not a side data structure, it's the VCS itself.
- **Workspaces** — jj's workspace concept is similar to git worktrees but better integrated. Multiple workspaces share the same repo without submodule-init friction.
- **Git backend** — jj stores data in git format. `jj git push/pull` interoperates with any git remote. The operator's GitHub workflow doesn't change.
- **Rust-native API** — `jj-lib` is explicitly designed to be used as a library (documented in their architecture page). commit, merge, rebase, workspace operations all have typed Rust APIs.

### What makes it risky

- **927 dependencies** — jj-lib pulls in gix (gitoxide), protobuf, tokio, and more. Our current omegon binary has ~1000 deps; this would nearly double it.
- **Pre-1.0 API** — version 0.30.0, breaking changes between releases. We'd be coupling to an unstable API.
- **Operator must have jj installed** — or we vendor it. Today we require git; adding a jj requirement is a new install step. Unless we use jj-lib purely as a library (no jj CLI needed).
- **Co-located mode complexity** — to keep git compatibility, jj runs in "co-located" mode with both .jj/ and .git/ directories. This adds filesystem complexity and potential confusion.
- **agentic-jujutsu crate exists** — someone already built an "AI agent" wrapper around jj. But it's a v0.1.0 wrapper, not production-grade.

### The key question: what problem does jj solve that git2 + CLI doesn't?

1. **Lock-free concurrent operations** — YES. This is real. Cleave children fighting over .git/index.lock is a recurring pain point. But we already solved it with worktrees (each child gets its own working copy).

2. **First-class rebase** — YES. Our cleanup_and_merge cherry-pick loop is a hack. jj's rebase is native and handles edge cases. But we could also just use `git cherry-pick` CLI and it works.

3. **Working copy as commit** — INTERESTING but unnecessary. RepoModel already tracks the working set. Making it a jj commit doesn't add value — it adds a conceptual layer.

4. **Better merge** — jj has first-class conflict representation in its data model. Git treats conflicts as markers in files. For our use case (automated merges with conflict detection), git2's approach is sufficient.

### Verdict

jj-lib is architecturally superior for what it does, but it's **overkill for our needs and too heavy to adopt**:
- 927 deps for a problem we've already solved with git2 (121 deps) + targeted CLI calls
- Pre-1.0 API instability
- Adds a conceptual layer (jj's operation log, change IDs, etc.) that our users don't need
- The compelling features (lock-free, native rebase) are already addressed by worktrees and cherry-pick

**The right answer is what we already have**: git2 for the operations it handles well (status, commit, merge, branch), CLI git for the gaps (worktree add, submodule init, cherry-pick). The CLI calls aren't failures — they're the right tool for operations where git CLI is more reliable than any library reimplementation.

### Reassessment — jj-lib as harness backbone, not just VCS wrapper

**The previous assessment evaluated jj as a git replacement. The operator is asking about it as a structural backbone for the harness itself.**

### The mapping that changes everything

| Harness concept | Current implementation | jj-lib native equivalent |
|----------------|----------------------|--------------------------|
| **Design tree node** | Markdown file + status enum | `ChangeId` — first-class immutable change identity |
| **Working set** | `RepoModel.working_set: HashSet<String>` | Working copy IS a commit — no side structure needed |
| **Feature branch** | git branch + ceremony commits | `jj new` — anonymous change, no branch overhead |
| **Cleave child isolation** | git worktree + submodule init | `Workspace` — native concurrent workspace, shared objects |
| **Squash on merge** | `squash_merge()` in git2 | `jj squash` — first-class operation, handles edge cases |
| **Ceremony dropping** | `cleanup_and_merge` cherry-pick loop | `jj rebase --skip` — native, handles conflicts |
| **Checkpoint commits** | `chore(cleave): checkpoint` forced commits | **Eliminated** — working copy is always a commit, no dirty tree |
| **Lifecycle batching** | `pending_lifecycle: HashSet` queue | Changes are mutable until committed — just keep editing |
| **Operation history** | Session memory episodes | `Operation` log — built-in, immutable, undo-capable |
| **Dirty tree preflight** | 300 lines of TS classification | **Eliminated** — no staging area, no dirty tree concept |

### The killer features for our use case

1. **No dirty tree problem.** The entire dirty-tree-preflight system (300+ lines of TS, the checkpoint flow, the stash flow) exists because git has a working tree that can be dirty. In jj, the working copy IS a change. There's nothing to "checkpoint" — every state is already captured.

2. **Lock-free concurrent children.** Cleave children currently need isolated worktrees because git's index lock prevents concurrent operations. jj's operation-based model allows concurrent operations on the same repo without locks. Children could operate on different `Workspace`s in the same repo.

3. **Native rebase solves ceremony dropping.** Our `cleanup_and_merge` (python → sed → cherry-pick) becomes `rebase_commit` with a filter. First-class, tested, handles conflicts.

4. **Transaction model matches harness lifecycle.** `repo.start_transaction()` → make changes → `tx.commit()`. This is exactly how the harness thinks: open a logical unit of work, make edits, commit when done. The transaction is the boundary we've been trying to build with RepoModel.

5. **Change IDs are permanent.** Unlike git commits (which change SHA on rebase), jj change IDs survive rewriting. A design tree node could be bound to a change ID that persists through rebases and squashes. This is structural integration, not just file tracking.

6. **Operation log for session history.** Every jj operation is logged immutably. Session start = operation. Each tool call that modifies the repo = operation. Session end = operation. This gives us structured undo and session history for free.

### The real cost

- **927 deps** — but replaces git2 (121), our RepoModel, merge.rs, worktree.rs, the TS dirty-tree preflight. Net new deps is more like +600.
- **Pre-1.0 API** — version 0.30 to 0.39 in the last few months. Active development. Google-backed (originally a Google project).
- **Co-located .jj/ + .git/** — operators keep their git workflow (push, PR, CI). jj manages the local state.
- **Learning curve** — the operator needs to understand jj's model. But the operator is us. The agent doesn't need to understand it — the harness abstracts it.
- **Not yet evaluated: submodule support in jj** — jj has a `default_submodule_store` but submodule support may be incomplete.

### What this means for the architecture

If we adopt jj-lib, the git-harness-integration epic changes from "add RepoModel + commit tool + squash-merge" to **"replace the entire VCS layer with jj-lib and map harness concepts to jj concepts natively."**

The `omegon-git` crate we just built would be rewritten to use jj-lib internally. The public API (RepoModel, commit, merge, worktree) stays similar but the implementation becomes dramatically simpler because jj's model already handles:
- Working set tracking (working copy is a change)
- Lifecycle batching (changes are mutable until committed)
- Squash merge (native `jj squash`)
- Ceremony dropping (native `jj rebase` with filter)
- Concurrent workspaces (native, no worktree-init dance)
- Operation history (native, immutable log)

### jj-lib blocker: submodule support is absent

**Hard blocker: jj ignores git submodules entirely.**

- `local_working_copy.rs` has `eprintln!("ignoring git submodule at {path:?}")` in multiple code paths
- `FileType::GitSubmodule => panic!("git submodule cannot be written to store")`
- Issue #494 (open since 2022) tracks submodule support — still unresolved
- Community consensus: "JJ doesn't support submodules. It will just ignore them."

The omegon repo uses `core` as a submodule. This is not optional — it's where the entire Rust codebase lives. Any VCS layer that ignores submodules is unusable for us today.

**Revised verdict:** jj-lib's model is architecturally compelling and the concept mapping (changes → design nodes, workspaces → cleave children, operation log → session history) is genuinely powerful. But the submodule blocker is absolute. We cannot adopt jj-lib until issue #494 is resolved.

**The path forward:** Keep the git2 + CLI architecture for now. If/when jj adds submodule support, the `omegon-git` crate's public API is the right abstraction boundary — we can swap the implementation from git2 to jj-lib without changing callers. This is the value of having a typed wrapper crate rather than raw CLI calls scattered everywhere.

**Action item:** Watch jj-vcs/jj#494. When submodule support lands, reassess adoption. The design tree node mapping alone would justify the migration cost.

## Decisions

### Decision: Use git2 as the primary git library, shell out to git CLI only for gaps

**Status:** decided
**Rationale:** git2 covers all 7 operation categories we need (discovery, status, index, branch, worktree, submodule, stash). gix is missing checkout, worktree create, and stash entirely. git2 is battle-tested (cargo uses it), adds 121 deps (vs 400+ for gix), and the C dependency (libgit2-sys) is acceptable — omegon already compiles native code via reqwest and rusqlite. Shell out to git CLI only where git2's API is awkward or where we need exact CLI flag parity (e.g., git worktree add with specific branch tracking options). Long-term, gix may catch up and could replace git2, but it's not ready today for our critical paths.

### Decision: Defer jj-lib adoption until submodule support lands — maintain omegon-git as the abstraction boundary for future swap

**Status:** decided
**Rationale:** jj-lib's model maps naturally to harness concepts (changes → design nodes, workspaces → cleave children, operation log → sessions, mutable working copy → no dirty tree). This is genuinely transformative, not just a VCS wrapper. But jj ignores submodules entirely (panics on write, eprintln-skips on read) and our core/ submodule is non-negotiable. The omegon-git crate is the right abstraction boundary: callers use typed Rust APIs regardless of whether the implementation uses git2, CLI git, or eventually jj-lib. When jj-vcs/jj#494 lands, swap the implementation. Until then, git2 for what it does well, CLI git for the gaps, and no reinventing what git already does reliably.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon-git/Cargo.toml` (new) — New crate: git2 dep, re-exports RepoModel and git operations
- `core/crates/omegon-git/src/lib.rs` (new) — Crate root — re-exports repo, status, commit, submodule, worktree modules
- `core/crates/omegon-git/src/repo.rs` (new) — RepoModel struct — discovery, branch, head SHA, submodule map, working set tracking
- `core/crates/omegon-git/src/status.rs` (new) — Status queries — dirty files, staged files, submodule state via git2 statuses API
- `core/crates/omegon-git/src/commit.rs` (new) — Commit operations — stage paths, create commit with conventional message, submodule two-level dance
- `core/crates/omegon-git/src/worktree.rs` (new) — Worktree operations — create, remove, list via git2 + CLI fallback for edge cases
- `core/crates/omegon-git/src/merge.rs` (new) — Merge operations — squash-merge, conflict detection, merge-base resolution
- `core/Cargo.toml` (modified) — Add omegon-git to workspace members
- `core/crates/omegon/Cargo.toml` (modified) — Add omegon-git dependency
- `core/crates/omegon/src/cleave/worktree.rs` (modified) — Replace Command::new(git) calls with omegon-git API, add squash-merge

### Constraints

- git2 is the primary library — shell out to git CLI only for operations git2 doesn't cover well
- RepoModel must be Send + Sync for use across async tasks
- Working set tracks files touched by edit/write tools — reset on commit
- Submodule map populated at init, refreshed on submodule operations
- Squash-merge is the default for cleave child branches
