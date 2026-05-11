+++
id = "960efacc-4a8b-41c4-a379-d1cefbec0876"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Contributing to Omegon

Guidelines for branching, merging, and collaborating on this repository.

## Development Setup

```bash
just bootstrap --check
just build
just test-rust
```

The repository is a Cargo workspace rooted at this directory. The main binary is `core/crates/omegon`, and `cargo` commands are run from the repo root unless a recipe says otherwise.

`just link` installs the local build for development by writing `~/.omegon/dev-alias.sh` and wiring the current shell profile. Source that file in the current shell if you need the alias immediately:

```bash
source ~/.omegon/dev-alias.sh
```

It deliberately does not overwrite `/usr/local/bin`, `/opt/homebrew/bin`, or package-manager-owned binaries.

## Development Model

**Trunk-based development** on `main`. Direct commits for small, self-contained changes. Feature branches for multi-file or multi-session work.

### When to Branch

| Scenario | Approach |
|---|---|
| Single-file fix, typo, config tweak | Commit directly to `main` |
| Multi-file feature or refactor | `feature/<name>` or `refactor/<name>` branch |
| Multi-session work (spans days) | Feature branch, push regularly |
| Cleave-dispatched parallel tasks | Automatic `cleave/*` worktree branches (ephemeral) |

### Branch Naming

Follow `<type>/<short-description>` per the [git skill](skills/git/SKILL.md):

```
feature/design-tree
fix/memory-zombie-resurrection
refactor/rename-diffuse-to-render
chore/bump-dependencies
```

### Merging

- **Merge commits** (not squash, not rebase) for feature branches — preserves full history
- **Fast-forward** is fine for single-commit branches
- **Never rebase branches that touch `facts.jsonl`** — see [Memory Sync](#memory-sync) below
- Delete the branch after merge (local and remote)

## Commits

[Conventional Commits](https://www.conventionalcommits.org/) required. See [git skill](skills/git/SKILL.md) for the full spec.

```
feat(project-memory): add union merge strategy for facts.jsonl
fix(cleave): bare /assess runs adversarial session review
docs: add contributing guide and branching policy
```

Commit messages explain *why*, not just *what*. Include the motivation in the body when the subject line isn't self-evident.

## Memory Sync

The project memory system uses a three-layer architecture for cross-machine portability:

```
facts.db (SQLite)     ← runtime working store (local, .gitignored)
facts.jsonl (JSONL)   ← transport format (git-tracked, union merge)
content_hash (SHA256) ← dedup key (idempotent import)
```

### How It Works

1. **Session start**: `facts.jsonl` is always imported into `facts.db`. Dedup by `content_hash` makes this safe to run every session — existing facts get reinforced, new ones inserted, archived/superseded ones skipped.

2. **Session shutdown**: Active facts, edges, and episodes are exported from `facts.db` to `facts.jsonl`, overwriting the file.

3. **Git merge**: `.gitattributes` declares `merge=union` for `facts.jsonl`. On merge, git keeps all lines from both sides, removing only exact duplicates. Redundant lines are harmlessly deduplicated at next import.

### Rules

| Rule | Reason |
|---|---|
| Never manually edit `facts.jsonl` | Machine-generated; manual edits will be overwritten on next session shutdown |
| Never rebase across `facts.jsonl` changes | `merge=union` only works with merge commits; rebase replays one side's version, losing the other's facts |
| Never `git checkout -- facts.jsonl` to resolve conflicts | Use `merge=union` (automatic) or manual union: keep all lines from both sides |
| Don't track `*.db` files | Binary, machine-local, rebuilt from JSONL on session start |

### .gitignore / .gitattributes

```
# ai/.gitignore — exclude runtime DB files
memory/*.db
memory/*.db-wal
memory/*.db-shm

# .gitattributes — union merge for append-log JSONL
ai/memory/facts.jsonl merge=union
```

## Cleave Branches

The [cleave extension](extensions/cleave/) creates ephemeral worktree branches for parallel task execution:

```
cleave/<childId>-<label>    # e.g., cleave/a1b2c3-fix-imports
```

These branches are:
- Created automatically by `cleave_run`
- Merged back to the parent branch sequentially
- Worktree directories cleaned up after merge
- **Branches preserved on merge failure** for manual resolution

### Cleanup

After cleave completes successfully, worktree directories are pruned but branches may linger. Clean up periodically:

```bash
# Delete local branches already merged into main
git branch --merged main | grep 'cleave/' | xargs git branch -d

# Prune remote tracking refs for deleted remote branches
git fetch --prune
```

## Repository Hygiene

### Stale Branches

Delete remote branches after merge. Don't accumulate tracking refs:

```bash
# List remote branches merged into main
git branch -r --merged origin/main | grep -v 'main$'

# Delete a stale remote branch
git push origin --delete <branch-name>
```

### Protected Files

Files that should never cause merge conflicts due to their nature:

| File | Strategy | Notes |
|---|---|---|
| `ai/memory/facts.jsonl` | `merge=union` | Append-log, deduped at import |
| `*.db`, `*.db-wal`, `*.db-shm` | `.gitignore` | Binary, machine-local |
| `ai/memory/` directory | Partial ignore | Only `facts.jsonl` tracked |

### What Gets Tracked

See `.gitignore` (repo root) and `ai/.gitignore` (memory directory) for the authoritative ignore rules. Key principle: `facts.jsonl` is tracked, `*.db` files are not.

Lifecycle artifacts under `docs/` and `openspec/` are also treated as durable project records and should be version controlled by default. These files are not scratch space — they are part of the human-readable design, planning, and verification history for the repo.

By contrast, transient cleave runtime artifacts such as machine-local workspaces and worktrees remain optional and should live outside the durable lifecycle paths. If something is experimental or disposable, do not leave it under `docs/` or `openspec/`.

The standard validation path enforces this policy:

```bash
just lint
just test-rust
```

If it reports untracked lifecycle artifacts, either:
- `git add` the durable files under `docs/` / `openspec/`, or
- move transient scratch material elsewhere.

## Rust Workspace

| Crate | Purpose |
|---|---|
| `omegon` | Main binary: TUI, agent loop, providers, tools, ACP, daemon/control plane |
| `omegon-codescan` | Code scanning helpers |
| `omegon-extension` | Extension SDK and protocol types |
| `omegon-git` | Git and worktree operations |
| `omegon-memory` | Project memory runtime |
| `omegon-opsx` | OpenSpec/lifecycle engine |
| `omegon-secrets` | Secret resolution and redaction |
| `omegon-traits` | Shared protocol and event types |
| `omegon-web` | Web dashboard and web-facing support code |

Use focused validation while developing:

```bash
just test-crate omegon-memory
just test-filter "vault_sync"
cargo test -p omegon-extension
```

Before landing a code change, run:

```bash
just lint
just test-rust
```

For documentation-only changes, run the relevant site checks instead of the full Rust suite when the code was untouched:

```bash
cd site
npm test
npm run build
```

## Release Process

Omegon uses a **release candidate** flow. All releases go through RC builds before stable.

### Channels

| Channel | Cadence | Version format | Example |
|---|---|---|---|
| **Stable** | When ready | `X.Y.Z` | `0.19.5` |
| **RC** | Per-feature batch | `X.Y.Z-rc.N` | `0.19.6-rc.2` |
| **Nightly** | Daily (planned) | `X.Y.Z-nightly.YYYYMMDD` | `0.19.6-nightly.20260510` |

### Commands

| Step | Command | What it does |
|---|---|---|
| **Cut RC** | `just rc` | Bump version → test → commit → tag → build → sign → update milestones |
| **Install locally** | `just link` | Write dev aliases for the newest local binary and install bundled skills/catalog |
| **Sign (YubiKey)** | `just sign` | Sign and optionally notarize the local macOS validation binary with Apple Developer ID |
| **Ship stable** | `just release` | Strip `-rc.N` → test → commit → tag → build → close milestone → open next cycle |
| **Publish** | `just publish` | Push refs → trigger CI release/site workflows → build docs locally → link local binary → smoke test |
| **Quick dev build** | `just update` | Pull → build dev-release profile → no version bump |

### RC flow

```
just rc          # 0.19.5 → 0.19.6-rc.1 (or rc.1 → rc.2)
just link        # install locally, verify
# ... test, iterate, fix ...
just rc          # 0.19.6-rc.2
just link
# ... satisfied ...
just release     # 0.19.6-rc.2 → 0.19.6 (stable)
just publish     # push to GitHub, trigger CI
```

Package publishing is CI-owned. `just sign` signs the local macOS validation binary on the operator workstation; `just publish` pushes the release refs and verifies the local install path; downstream package surfaces such as Homebrew update from the published GitHub release artifacts rather than from workstation-side scripts. The distributable archives that packages consume are built and signed in CI, not copied from the locally YubiKey-signed binary.

### Milestone tracking

`.omegon/milestones.json` is automatically maintained by `just rc` and `just release` via `scripts/milestone-update.sh`. Each milestone tracks:

- **status**: `open` → `rc` → `released`
- **channel**: `stable` or `nightly`
- **rc_version / rc_count**: current RC and iteration count
- **notes**: auto-collected feat/fix/refactor commit subjects
- **timestamps**: `opened`, `last_rc`, `released`

The `/milestone` TUI command also reads this file for operator-facing release scope management.

### Version identity

The binary's `--version` output includes the git SHA and build date:

```
omegon 0.19.6-rc.1 (660e1ef 2026-05-10)
```

The build.rs script computes `OMEGON_NEXT_VERSION` (displayed in the TUI footer):
- RC build `0.19.6-rc.1` → next milestone is `0.19.6`
- Stable `0.19.6` → next milestone is `0.19.7`

### Pre-flight checks

`just rc` and `just release` both refuse to run with uncommitted changes in `core/` or `.omegon/milestones.json`. The `just smoke` recipe verifies post-merge invariants (binary works, test count floor, provider count, tool count, key file line counts, no SubprocessBridge).

## Scaling Notes

This policy is designed for a small team (1–3 contributors) working with agent-assisted development. If the contributor count grows:

- Enable branch protection on `main` (require PR, at least 1 review)
- Add CI validation for conventional commits (`commitlint`)
- Consider a `develop` branch if release cadence requires staging
- Monitor `facts.jsonl` size — if it exceeds ~10K lines, evaluate archival rotation or LFS
