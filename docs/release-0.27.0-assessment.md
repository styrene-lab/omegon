+++
id = "release-0-27-0-assessment"
kind = "document"
title = "0.27.0 release assessment — 2026-07-01"
status = "exploring"
tags = ["release", "0.27.0", "assessment", "hardening"]
aliases = ["0.27.0 assessment"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# 0.27.0 release assessment — 2026-07-01

Initial releasability assessment of the pending 0.27.0 line, taken on `main` at `2f24e82d` (`chore(release): bump rc version to 0.27.0-rc.6`). Companion to [release-0.27.0-exploration](release-0.27.0-exploration.md), which captured the 2026-06-13 posture; this document captures what has drifted since.

## Verdict (preliminary)

**Not releasable as-is — but the blockers are bookkeeping, not code.** The workspace test suite passes in full (3,957 tests), the auth-store integrity risk from the exploration doc has a closing fix trail on main, and rc.3–rc.6 stabilization has been active. What blocks the cut is release-mechanics incoherence: a phantom dated changelog section, a three-weeks-stale release branch, a missing milestone entry, and unpushed history.

## Evidence — repo state

| Fact | Evidence |
|------|----------|
| Latest published tag | `v0.26.16` — no `v0.27.*` tag exists locally or on `origin` |
| Main version | `0.27.0-rc.6` (Cargo.toml, workspace-shared) |
| Commits since v0.26.16 | 758 on main |
| Unpushed commits on main | 27 ahead of `origin/main`, 0 behind |
| `release/0.27` branch | version `0.27.0`, last commit 2026-06-10, **664 commits behind main, 0 unique** |
| `release/0.27-ui-polish` branch | 6 TUI commits (2026-06-13) **never merged to main**: inline row affordance layout, engine footer row alignment, details affordance centralization |
| Working tree | clean |

## Blocking findings

### B1. Version/changelog incoherence: `[0.27.0] - 2026-06-11` already exists in CHANGELOG.md

`CHANGELOG.md` carries a dated `[0.27.0] - 2026-06-11` section, but no `v0.27.0` tag was ever cut. Main then continued for ~664 commits versioned as `0.27.0-rc.N` — which sorts *before* the already-dated 0.27.0 section content. 575 lines of `[Unreleased]` now sit on top of the stale dated section.

Consequence: when 0.27.0 actually ships, either the 2026-06-11 section is a lie (wrong date, missing ~575 lines of content) or the release notes must be rebuilt by folding `[Unreleased]` into a corrected `[0.27.0]` section. AGENTS.md makes the changelog mandatory release memory — this must be reconciled before tagging.

### B2. `just preflight` fails on main

```
✗ Release preflight failed:
  - workspace role must be 'release' for release cuts (currently: unset)
  - Workspace version 0.27.0-rc.6 is not a stable release version
```

Consistent with the AGENTS.md model: stable tags belong to `release/X.Y` branches, main owns nightly. But the only `release/0.27` branch is three weeks stale (B3). Preflight failing on main is *expected*; the problem is there is no branch on which it would pass.

### B3. `release/0.27` is stale and cannot be the release vehicle without a re-cut

The branch predates ~664 commits of stabilization including the rc.3–rc.6 hardening line, control-runtime refactors, secret-response ownership guards, and guided-menu fixes. It has zero unique commits, so it can be fast-forwarded or deleted and re-cut from main (`just branch-release`) with no loss. Shipping it as-is would ship a June-10 snapshot while calling it the stabilized line.

### B4. Milestone ledger has no 0.27.0 entry

`.omegon/milestones.json` contains no `0.27.0` milestone. `just release` calls `milestone-update.sh release $VERSION` as part of the cut, and the rc bumps that produced rc.3–rc.6 were evidently made without milestone tracking. Additionally three stale milestones were never closed: `0.15.12` (open), `0.18.4` (rc), `0.24.0` (open).

## Non-blocking findings (decide-or-defer)

### D1. `release/0.27-ui-polish` orphaned commits

Six TUI commits (fixes + one feature) exist only on that branch. Decision needed: merge to main before the re-cut, or explicitly abandon. Silently dropping fix commits is the worst outcome.

### D2. Four completed OpenSpec changes unarchived

`knowledge-quadrant-lifecycle` (8/8), `plan-refinement` (44/44), `provider-route-state-machine` (47/47), `splash-systems-integration` (29/29) are all task-complete but not archived to baseline. Per the OpenSpec lifecycle these should be verified (`/assess spec`) and archived before the release closes them out implicitly.

### D3. `styrene-identity-secrets` at 5/9 tasks

Remaining tasks (2.3, 2.4, 3.1, 3.2) are the feature-gated Styrene Identity backend and mesh-lookup work — documented as future scope in store.rs. Decision needed: formally defer to post-0.27.0 (split the change or annotate tasks.md) rather than leaving the change ambiguously half-open across a release boundary.

### D4. `omega-daemon-runtime-v1` is proposal-only

No tasks.md. Clearly post-release; no action needed beyond not letting it creep in.

### D5. `[Unreleased]` section structure is malformed

Category headings repeat (`### Added` ×3, `### Fixed` ×5, `### Changed` ×6) from block-appended entries over time. Keep-a-Changelog wants one heading per category per version. Must be normalized when folding into the 0.27.0 section (the exploration doc notes a similar normalization was done once on the mechanics-validation branch and has since regressed on main).

## Verification results

- **Full workspace test suite: PASS.** `just test-rust` (`cargo test --workspace`) run on `2f24e82d`, 2026-07-01. Direct evidence covers the final stage (all doctest targets ok); since cargo test is fail-fast across targets, reaching doctests implies all preceding test binaries passed. 3,957 tests enumerated across the workspace.
- Release warning gate (`RUSTFLAGS="-D warnings" cargo check`) not independently re-run — it exceeded the tool timeout while the test build held the target-dir lock. Low risk (preflight runs it as part of the cut) but should be confirmed on the release branch.

## Recommended sequence (draft, pending operator decisions)

1. Decide D1 (ui-polish commits): merge or abandon.
2. Reconcile CHANGELOG: fold `[Unreleased]` into a corrected, normalized `[0.27.0]` section with the actual release date; fix B1/D5 together.
3. Archive the four completed OpenSpec changes (D2); formally defer styrene-identity-secrets remainder (D3).
4. Create the 0.27.0 milestone entry; close/annotate stale milestones (B4).
5. Push the 27 unpushed main commits.
6. Re-cut `release/0.27` from main (`just branch-release` — fast-forward is clean since the branch has 0 unique commits), set role=release, promote rc.6 → 0.27.0 there.
7. `just preflight` + `just release` on the release branch; `just merge-release-forward` after tagging.

## Open questions

- [assumption] The intent is to ship 0.27.0 from current main (rc.6), not from the June-10 `release/0.27` snapshot. Inferred from the rc.3–rc.6 stabilization cadence on main; not confirmed by the operator.

## Resolved questions

- **Remote tags**: `git ls-remote --tags origin` shows only `v0.27.0-nightly.20260611` — no stable `v0.27.0` was ever pushed. The version number remains usable; no 0.27.1 reframe needed.
- **Auth-store integrity** (exploration doc finding): main carries a closing fix trail since v0.26.16 — `9f66aa96 preserve auth store on credential writes`, `0dd72645 prevent credential store key loss`, `3c326e5b normalize provider credential keys`, plus hydration/refresh hardening. The exploration-era credential-disappearance risk appears addressed on main (not on the stale `release/0.27` branch — one more reason B3 blocks shipping that snapshot).
