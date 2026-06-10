+++
id = "f7e7cbad-864c-4dc5-8d4e-48c6b0c21339"
kind = "document"
title = "Release process hygiene — close the gaps between version bumps, git tags, and CI releases"
status = "implemented"
tags = ["release", "git", "ci", "versioning", "process", "0.15.1"]
aliases = ["release-process-hygiene"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "chore"
open_questions = []
priority = "1"
+++

# Release process hygiene — close the gaps between version bumps, git tags, and CI releases

## Overview

The release process has multiple disconnected steps that are manually executed and routinely forgotten: Cargo.toml version bump, git tag creation, CI release trigger, and switch registry coherence. The result is version drift (git describe says one thing, Cargo.toml says another), missing tags (rc.3, rc.11, rc.16, rc.17 never tagged), and omegon switch pointing at stale GitHub releases. Need a single `just release` command that does all steps atomically.

## Research

### Current state: 5 disconnected manual steps

The release process today is:

1. **Edit root `Cargo.toml`** — bump the workspace `version` field manually
2. **Commit** — `git commit -m "chore(release): bump to X.Y.Z-rc.N"`
3. **Tag** — `git tag vX.Y.Z-rc.N` (FREQUENTLY FORGOTTEN)
4. **Push tag** — `git push origin vX.Y.Z-rc.N` (triggers CI release)
5. **Build locally** — `just build` (for dev use)

What actually happens: steps 1-2 happen, step 3 is forgotten half the time, step 4 never happens for RC builds (they're dev-only), and step 5 happens independently. The result:

- `omegon --version` shows `0.15.1-rc.18` (from Cargo.toml)
- `git describe` shows `v0.14.1-rc.15-125-gad5428c` (last tag was 125 commits ago)
- The `--version` output includes BOTH, confusing operators
- `omegon switch --latest-rc` fetches from GitHub releases which only exist for tagged+pushed versions
- Missing tags: rc.3, rc.11, rc.16, rc.17 were bumped in Cargo.toml but never tagged

The build.rs bakes both Cargo.toml version AND git describe into the binary. When they disagree, the output looks broken.

### Proposed: `just release` and `just rc` atomic commands

**`just rc`** — cut a release candidate (dev workflow):
1. Read current version from Cargo.toml
2. Bump the RC number (or create rc.1 from a release version)
3. Write Cargo.toml
4. `cargo build --release`
5. `cargo test`
6. Commit with conventional message: `chore(release): X.Y.Z-rc.N`
7. Tag: `vX.Y.Z-rc.N`
8. Print: "RC ready. `git push origin vX.Y.Z-rc.N` to publish."

**`just release`** — cut a stable release:
1. Strip `-rc.N` from current version (or bump minor/patch)
2. Write Cargo.toml
3. `cargo build --release`
4. `cargo test`
5. Commit: `chore(release): X.Y.Z`
6. Tag: `vX.Y.Z`
7. Print: "Release ready. `git push origin vX.Y.Z` to publish."

Both commands refuse to run with uncommitted changes. Both commands create the tag atomically with the version bump commit. The push is still manual — that's the operator's decision gate.

**Build.rs simplification**: When `git describe` matches the Cargo.toml version (because we always tag), only show one version line. When they disagree (dirty dev build), show both so the operator knows.

**`omegon switch` coherence**: The switch command fetches GitHub releases. RCs only appear there if the tag was pushed. The `just rc` command makes tagging automatic; the push is the explicit "publish this RC" decision. This is correct — not every RC needs to be published for `switch`.

## Decisions

### Decision: Atomic `just rc` / `just release` that bump, build, test, commit, and tag in one command

**Status:** decided
**Rationale:** The root cause of version drift is that version bump and tag creation are separate manual steps. Making them atomic — one command does both — eliminates the gap. The push remains manual because not every RC should be published to GitHub releases. Build and test are included so the tag is never created on broken code.

### Decision: keep `main` as trunk and use `release/X.Y` only for stabilization

**Status:** decided
**Rationale:** Operator-facing channels should stay simple: `nightly` follows `main`, `stable` follows the latest stable semver tag, and exact `vX.Y.Z` tags remain available for pins. Release branches are internal hardening branches, not automatic install channels. They exist to stabilize or patch a version line without exposing operators to another channel choice.

The target model is:

1. Normal feature, refactor, and product work lands on `main`; `main` is the source for nightly builds.
2. When a version needs hardening, run `just branch-release` from `main` to create/push `release/X.Y` and switch the working copy to it.
3. Land only release-hardening work on `release/X.Y`: bug fixes, tests, release docs, packaging fixes, and validation tooling needed for that release. Broad features/refactors stay on `main` unless explicitly approved as release blockers.
4. Continuously run `just merge-release-forward` after release-branch hardening commits. The helper merges into `main`, preserves `main`'s version-state files, pushes `main`, and switches back to the release branch.
5. Tag stable releases from `release/X.Y` as `vX.Y.Z` after the release branch is green.
6. After every stable or patch tag, run `just merge-release-forward` again before considering the release operationally complete.
7. Delete or retire old release branches once their patch window closes.

Channel ownership:

- `main` owns nightly tags and is the trunk for ongoing development.
- `release/X.Y` owns stable tags for that X.Y line while it is active, but it is not an operator-selectable automatic channel.
- `stable` resolves to the latest stable semver tag.
- `nightly` resolves to `main`.
- Release hardening fixes flow forward from `release/X.Y` to `main`; normal development does not flow backward into release branches unless deliberately backported.

Invariant: a release tag is not complete until the tagged release branch state has been merged forward to `main` and `main` is green. This prevents `nightly` from falling behind stable fixes while keeping the operator-facing channel set limited to `stable`, `nightly`, and explicit tags.

## Open Questions

*No open questions.*
