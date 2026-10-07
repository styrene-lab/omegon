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

## Your First Contribution

You do not need to learn Omegon's agent-only lifecycle systems to contribute. Design Tree, OpenSpec, Workbench reconciliation, release signing, and the full local test matrix are maintainer workflows unless a maintainer explicitly asks you to use them.

A good first contribution is deliberately small:

- clarify a confusing sentence or example;
- add a regression test for an existing bug;
- fix a focused issue with a clear reproduction;
- improve an error message without redesigning the surrounding subsystem.

If you are unsure whether an idea fits, open a feature request first. An incomplete report with a concrete observation is more useful than a large speculative patch.

### 1. Fork and clone

Fork the repository on GitHub, then clone your fork and create a branch:

```bash
git clone https://github.com/<your-name>/omegon.git
cd omegon
git switch -c fix/short-description
```

You may use another branch name. The important boundary is that community contributions arrive through a pull request; direct commits to `main` described later in this guide are for maintainers with repository access.

### 2. Check your environment

Omegon's supported development environments are macOS, Linux, and Linux processes running under WSL2. Native Windows process semantics are not currently supported.

Run the read-only prerequisite check:

```bash
just bootstrap --check
```

It reports missing tools without installing or changing them. The usual prerequisites are:

- Git;
- a Rust stable toolchain installed through `rustup`;
- `rustfmt` and Clippy;
- `just`;
- Python 3 for repository developer scripts.

Pkl is optional unless your change touches Pkl schemas or custom posture/agent configuration. Provider credentials are not required to compile the workspace or run ordinary unit tests.

To install missing prerequisites and build/link Omegon automatically, run `just bootstrap`. The script may install system or user-level tools, so inspect `scripts/bootstrap.sh` first if you do not want that automation.

### 3. Make and validate a focused change

Use the narrowest check that covers your change:

| Change | While iterating | Before opening the pull request |
|---|---|---|
| Markdown-only documentation | inspect the rendered diff | `git diff --check` |
| Rust formatting or compile fix | `cargo fmt --all` and `just check-changed` | `just test-commit` and `just clippy-changed` |
| Focused Rust behavior | `just test-filter "test_name"` or `just test-crate <crate>` | `just test-commit` and `just clippy-changed` |
| Developer scripts | run the affected Python test | `just test-dev-scripts` |
| Public docs site | relevant site test | `cd site && npm test && npm run build` |

You are not expected to run `just test-rust` for every first contribution. It is the serialized full-workspace CI/release gate and can be expensive. CI and maintainers will run broader checks when needed.

Do not update `CHANGELOG.md` for a trivial typo or internal test cleanup. Do add an `[Unreleased]` entry when your pull request changes operator-visible behavior, public documentation, tooling, packaging, API behavior, or contributor workflow. A maintainer can help classify the entry during review.

### 4. Open the pull request early

Use the repository pull request template. A draft pull request is welcome when you want feedback before polishing the implementation. Include:

- the problem and why the change is useful;
- the smallest reproduction or before/after example you have;
- the exact validation commands you ran;
- anything you could not verify locally.

Maintainers own repository-wide integration, release gates, lifecycle bookkeeping, and final release notes. Review may ask for a focused test or a smaller scope, but it should not require you to reverse-engineer hidden process.

### What review should feel like

Expect maintainers to explain blocking requests, distinguish required changes from suggestions, and help with project-specific mechanics. You may disagree with feedback and ask for the underlying constraint. No prior issue, design document, or permission is required for a small pull request.

## Development Setup

This repository owns its Rust toolchain through `flake.nix`; Cargo is not
expected to be installed globally. With `direnv` installed and hooked into your
shell, authorize the repository once:

```bash
direnv allow
just bootstrap --check
just build
```

Without direnv, enter the same environment explicitly:

```bash
nix develop
```

The `Rust build` CI job uses the Rust version selected by `flake.nix` and
`flake.lock`, including its Clippy checks with `-D warnings`. Read that version
with `nix eval --raw .#packages.x86_64-linux.rust-toolchain.version`.
`stable.latest` in the flake means the latest stable release in the **locked**
Rust overlay, not the current upstream release. Toolchain baseline updates must
therefore be deliberate lockfile changes, validated with the same build and lint
checks. The other Rust test jobs continue to use upstream stable for forward
compatibility. A passing local baseline does not replace those CI tests.

The repository is a Cargo workspace rooted at this directory. The main binary is `core/crates/omegon`, and `cargo` commands are run from the repo root unless a recipe says otherwise. Use the focused validation table above while iterating; reserve `just test-rust` for broad or release-hardening gates.

`just link` installs the local build for development by writing `~/.omegon/dev-alias.sh` and wiring the current shell profile. Source that file in the current shell if you need the alias immediately:

```bash
source ~/.omegon/dev-alias.sh
```

It deliberately does not overwrite `/usr/local/bin`, `/opt/homebrew/bin`, or package-manager-owned binaries.

## Development Model

Omegon uses trunk-based development, but `main` is no longer a casual working branch. All implementation work should start from a focused branch and land through a pull request unless it falls under the direct-to-main exception policy below.

### Branch Policy

External contributors should work from a fork and open a pull request. Maintainers with repository access should also use a topic branch for ordinary work.

| Scenario | Approach |
|---|---|
| Ordinary bug fix, feature, refactor, docs update, or workflow change | Create `fix/<name>`, `feature/<name>`, `refactor/<name>`, `docs/<name>`, or `chore/<name>` and open a PR to `main` |
| Multi-session work | Push a topic branch regularly and keep the PR draft until ready |
| Cleave-dispatched parallel tasks | Use automatic `cleave/*` worktree branches; merge through the parent topic branch |
| Stable-line hardening | Branch from and target `release/X.Y` |
| Release preparation or version-state changes | Use a dedicated release-prep branch; see [Version and Release Authority](#version-and-release-authority) |

Do not start maintainer or agent implementation work directly on `main`. Pull `main`, create the focused branch, then edit.

### Direct-to-main Exceptions

Direct commits to `main` are reserved for exceptional maintainer operations:

- reverting a bad merge when a PR would prolong a broken trunk;
- emergency repository/CI repair that blocks all PR validation;
- administrative metadata that cannot practically be routed through PR;
- explicitly authorized post-merge housekeeping.

Even then, keep the commit focused, use a Conventional Commit message, run the narrowest relevant validation, and follow up with a PR if the emergency commit leaves policy, docs, tests, or release state incomplete.

### Branch Naming

Use `<type>/<short-description>`:

```text
feature/design-tree
fix/memory-zombie-resurrection
refactor/rename-diffuse-to-render
docs/contributing-branch-policy
chore/open-0.30-dev-line
release-prep/v0.29.0
```

### Pull Requests and Merging

- Open a PR for ordinary work, including maintainer-authored work.
- Keep PRs focused; split generated state, rustfmt-only churn, and release/version state into separate commits when possible.
- Fill in the validation section honestly. "Not run" is acceptable when paired with a reason.
- Merge commits, squash merges, and rebase merges are all enabled by GitHub; choose the method that preserves the useful review/history shape. Do not rebase branches that touch union-merged memory transport files.
- Delete topic branches after merge unless they are long-lived release branches.

### Happy Development Loop (Maintainers and Large Changes)

The full loop below describes changes made by maintainers and contributors working on large or architecturally significant changes. First-time contributors can use the shorter path above; maintainers own any required lifecycle reconciliation and release integration.

Use this loop for substantial changes to Omegon itself:

1. **Inspect** — establish the current repository, runtime, and Workbench state from evidence before changing anything.
2. **Design** — identify the smallest coherent change, record durable decisions when the work has architectural consequences, and surface unresolved assumptions instead of coding through them.
3. **Implement** — make bounded edits against code already read; keep interfaces and ownership explicit.
4. **Test** — run focused checks while iterating, then the required landing gates. For isolated Rust behavior changes, use the crate-specific gate plus `just clippy-changed`; use `just test-commit` for multi-crate/shared-contract changes and reserve `just lint` plus serialized `just test-rust` for broad or high-risk work and release hardening.
5. **Commit** — update `[Unreleased]` for operator-visible behavior or workflow changes and create a focused Conventional Commit.
6. **Rebuild and install** — run `just link` so the executable and bundled assets used by the development environment match the committed source.
7. **Exercise the real harness** — launch the installed/current Omegon through the normal operator path and verify the behavior in its actual TUI, process, and tool environment rather than relying only on unit tests.
8. **Reconcile state** — align Workbench plans, OpenSpec tasks, design status, validation evidence, and git state with what actually landed.
9. **Hand off cleanly** — stop transient processes, leave no stale active plan, confirm the worktree state, and report the commit, verification performed, and any explicit remaining limitation.

In short:

```text
inspect → design → implement → test → commit → rebuild/install
        → run the real harness → reconcile state → hand off cleanly
```

A timeout or cancellation must terminate the full process group, not only its immediate shell. Build and validation workflows routinely spawn `cargo`, `rustc`, test binaries, and other descendants; leaving those descendants alive corrupts subsequent iterations and makes repository state appear nondeterministic.

The final runtime exercise complements automated validation; it does not replace it. If the harness cannot be exercised in the current environment, record that limitation explicitly rather than treating a successful build as equivalent evidence.

## Version and Release Authority

SemVer and release state must come from a branch whose name and target make release intent explicit. Do not edit release authority files directly on `main`.

Release authority files are:

- workspace-version declarations in root and crate `Cargo.toml` files
- the workspace-version updates propagated into `Cargo.lock`
- `.omegon/milestones.json`
- versioned release sections in `CHANGELOG.md`
- release manifests or packaging metadata when a release workflow explicitly consumes them

Ordinary dependency changes may update dependency entries in `Cargo.toml` and `Cargo.lock` on a normal focused `chore/` or dependency-update branch. They become release-authority changes only when they alter the workspace/package version or release-generated metadata.

### Trunk Version

`main` carries the active development line, such as `0.29.0-dev`. Normal feature and fix PRs should not change the workspace version. Operator-visible behavior, public docs, tooling, packaging, API behavior, or contributor workflow changes should update `[Unreleased]` in `CHANGELOG.md` instead.

### Release Branches

Stable releases are cut from `release/X.Y` branches. Release branches own stable tags for that line; trunk owns normal development and nightly/dev builds.

### Nightly cutoff and merge policy

Omegon automatically cuts one nightly from `main` every day at **07:17 UTC**. The immutable cutoff is the `main` commit checked out when the scheduled workflow starts. A pull request is included when its merge commit is reachable from that checkout; a pull request merged after the cutoff enters the following nightly.

Required branch-protection checks and reviews are the complete pre-cut quality gate. There is no nightly-specific merge freeze, hold label, observation window, or manual release approval. Do not rush or bypass a required PR gate to catch a nightly cutoff, and do not move an existing nightly tag to include a late merge.

Nightlies are integration builds and may contain functional bugs. A nightly is considered mechanically broken only when the standard release pipeline cannot build, sign, notarize, package, describe, publish, install, or launch its artifacts. Failed candidates are fixed forward on `main`; they are not reconstructed by mutating an immutable tag. Stable releases add deliberate release judgment and stronger validation beyond the nightly contract.

The scheduled workflow generates release metadata in a detached release commit derived from the cutoff commit, pushes an immutable nightly tag, and explicitly dispatches the standard release workflow. It does not commit generated version state back to `main`.

Use the existing release helpers rather than hand-rolling branch/version mechanics:

```bash
just branch-release
just merge-release-forward
```

Release hardening fixes target `release/X.Y` first, then merge forward to `main` with `just merge-release-forward` so trunk receives the stable-line fix without pulling version state backward.

### Release-prep PRs

Version bumps and release-state changes should be isolated in a dedicated PR. Use a branch such as:

```text
release-prep/vX.Y.Z
chore/release-vX.Y.Z
chore/open-X.Y-dev-line
```

A release-prep PR may move `[Unreleased]` entries into an exact version section, update release authority files, refresh `Cargo.lock`, and adjust release metadata. It should not bundle unrelated feature work.

### Opening the Next Development Line

After cutting `release/X.Y`, trunk must open the next development line through a PR, for example `chore/open-0.30-dev-line`. This keeps `origin/main` from advertising an older version than the active release branch and matches the invariant enforced by the release scripts.

### Stable Hotfixes

Stable hotfixes should:

1. branch from `release/X.Y`;
2. target the PR to `release/X.Y`;
3. update the exact release changelog section as needed;
4. pass release-appropriate validation;
5. merge forward to `main` after the release branch PR lands.

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

The broad validation path enforces this policy:

```bash
just lint
just test-rust
```

Focused changes may use the narrower landing gates described under [Rust Workspace](#rust-workspace).

If it reports untracked lifecycle artifacts, either:
- `git add` the durable files under `docs/` / `openspec/`, or
- move transient scratch material elsewhere.

## Rust Workspace

### Shipped content packs

Shipped skills, prompts, personas, tones, workflows, and catalog data are content-pack assets, not kernel Rust. Edit the source asset under `skills/`, `prompts/`, `personas/`, `tones/`, `workflows/`, or `catalog/`, then regenerate and check the canonical manifest:

```bash
python3 scripts/content_pack_manifest.py
python3 scripts/content_pack_manifest.py --check
python3 scripts/check_no_embedded_content.py
```

Content metadata can request content capabilities or external paths. It cannot grant tools, host effects, executable trust, trusted paths, or persistent permissions. Changes that add executable assets belong in the dynamic contribution admission path, not the shipped content pack.

| Crate | Purpose |
|---|---|
| `omegon` | Main binary: TUI, agent loop, providers, tools, ACP, daemon/control plane |
| `omegon-codescan-contracts` | Portable codescan request, response, and status types |
| `omegon-codescan` | Code and knowledge indexing |
| `omegon-git` | Repository, commit, merge, worktree, and submodule operations |
| `omegon-memory` | Fact storage, decay, search, injection, and vault synchronization |
| `omegon-opsx` | OpenSpec and design lifecycle state machine |
| `omegon-rbac` | Omegon capability vocabulary mapped onto Styrene RBAC |
| `omegon-secrets` | Secret resolution, redaction, and tool guards |
| `omegon-skills` | Skill parsing, inventory, activation, and suggestion policy |
| `omegon-traits` | Shared protocol, feature, command, tool, and event contracts |
| `omegon-web` | Web search and content extraction |
| `styrene-work-model` | Provider-neutral work-item contracts |
| `styrene-work-runtime` | Work-source refresh and immutable aggregate snapshots |

First-party native extensions live under `extensions/`. The release-coupled
`extensions/omegon-codescan` process owns SQLite, indexing, and BM25. The host
depends only on `omegon-codescan-contracts`. Run `just test-codescan-extension`
after changing its protocol, engine, or process lifecycle.

Use focused validation while developing:

```bash
just test-crate omegon-memory
just test-filter "vault_sync"
just test-secrets
```

Before landing a focused single-crate code change, run the crate-specific gate plus changed-crate Clippy; use broader gates when the change crosses crate or contract boundaries:

```bash
just test-crate <crate>
just clippy-changed
# For multi-crate/shared-contract changes:
just test-commit
```

Reserve `just lint` and serialized `just test-rust` for broad or high-risk changes and release hardening.

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
| **Nightly** | Daily at 07:17 UTC | `X.Y.Z-nightly.YYYYMMDD` | `0.19.6-nightly.20260510` |

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
