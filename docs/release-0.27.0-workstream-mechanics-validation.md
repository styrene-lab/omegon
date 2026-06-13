+++
id = "release-0-27-0-workstream-mechanics-validation"
kind = "document"
title = "0.27.0 workstream — release mechanics and validation hygiene"
status = "exploring"
tags = ["release", "0.27.0", "workstream", "validation", "release-mechanics"]
aliases = ["0.27 mechanics validation", "release mechanics validation workstream"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# 0.27.0 workstream — release mechanics and validation hygiene

## Owner

Primary owner: **current Omegon session / operator pair**.

## Branch

`release/0.27-mechanics-validation`

Branch created from current `main` HEAD: `07027739 fix(auth): prefer refreshable oauth credentials`.

## Mission

Make the 0.27.0 release process boring: clean release notes, clear version/tag state, repeatable validation gates, and explicit release-vs-patch decisioning.

This workstream owns release hygiene, not broad product changes.

## Inputs

- [[release-0.27.0-exploration|0.27.0 release exploration]]
- `CHANGELOG.md`
- `Cargo.toml`
- `core/release.toml`
- `justfile`
- `scripts/release_preflight.py`
- `scripts/release_branch.py`
- release-related tests under `tests/`

## Current findings

- Workspace version is `0.27.0`. No local `v0.27.*` tag was present after upstream fetch, so this checkout is in pre-tag `0.27.0` stabilization posture.
- `core/release.toml` uses shared versioning and `v{{version}}` tags.
- `just release` expects a clean tree, `just preflight`, stable semver, milestone update, release commit, tag, and release build.
- Release branch helpers exist: `just branch-release` and `just merge-release-forward`.
- `CHANGELOG.md` previously had duplicated `[Unreleased]` subsection groups after recent hardening edits; this checkout now has one canonical Added/Changed/Fixed sequence.
- Recent `just link` hardening should be included in validation: installed binary must not lag source HEAD.

## Scope

### In scope

- Consolidate `[Unreleased]` changelog headings into canonical Keep-a-Changelog order.
- Ensure release-hardening entries are correctly classified under Added/Changed/Fixed.
- Verify whether this is pre-tag `0.27.0` stabilization or should become `0.27.1` patch hardening.
- Run or repair release validation gates:
  - `just test-commit`
  - `just lint`
  - `just preflight`
  - `just link`
  - optional: `just test-rust` if time permits
- Document any gate failures with concrete file/test ownership.
- Confirm `omegon --version` after `just link` reflects the current build.
- Produce the final release decision note: tag 0.27.0 vs patch 0.27.1.

### Out of scope

- Provider auth implementation fixes except where needed to unblock release gates.
- TUI layout or footer polish.
- Extension SDK compatibility remediation unless it blocks `just preflight` or release packaging.
- New product features.

## Acceptance criteria

- `[Unreleased]` in `CHANGELOG.md` has a single canonical section sequence.
- Release mechanics decision is recorded: pre-tag 0.27.0 stabilization or 0.27.1 patch.
- Required validation gates are run and results recorded.
- If a validation gate fails, the failure is either fixed or assigned to another release workstream with evidence.
- `just link` has been run successfully after hardening patches, and installed binary freshness is verified.
- Workstream commits use conventional commit messages.

## Suggested task breakdown

1. Inspect release state:
   - `git status --short`
   - `git tag --list 'v0.27*'`
   - `git branch --show-current`
   - `grep '^version = ' Cargo.toml`
2. Normalize `CHANGELOG.md`.
   - Status: done in this checkout; duplicated `[Unreleased]` Added/Fixed sections were consolidated into one canonical Added/Changed/Fixed sequence. Auth-implementation changelog text from the sibling auth-integrity checkout was intentionally not imported because its code change is outside this workstream.
3. Run focused release-hygiene validation:
   - `just test-commit`
   - `just lint`
   - `just preflight`
4. Run `just link`; verify `omegon --version`.
5. Write release decision result back into [[release-0.27.0-exploration]].
6. Commit the mechanics/validation workstream.

## Risks

- Full `just lint`/`just test-rust` may expose unrelated long-standing failures. Do not silently absorb those into this workstream; record and route them.
- If `v0.27.0` already exists remotely, do not attempt to retag. Reframe as `0.27.1` patch hardening.
- Release scripts may require clean tree; coordinate with the other workstreams before running mutation-producing release commands.

## Coordination notes

- Coordinate with `release/0.27-auth-integrity` before final release validation because auth-store hardening is currently the most release-relevant bug fix.
- Coordinate with `release/0.27-ui-polish` only for release notes and validation smoke; UI polish should not block mechanics unless it exposes route truthfulness regressions.


## Release decision

Current decision: **pre-tag 0.27.0 stabilization**.

Evidence:

- `Cargo.toml` declares `version = "0.27.0"`.
- `core/release.toml` declares shared versioning and `v{{version}}` tags.
- `git tag --list 'v0.27*'` returned no local `0.27` tags after upstream fetch.

If a remote/published `v0.27.0` tag appears before final release work, do not retag; convert remaining hardening to `0.27.1` patch work.

## Validation log

- `just test-commit`: **passed**. The changed-path detector found no affected Rust crates for the docs/changelog-only mechanics slice, so cargo tests were skipped.
- `just lint`: **failed outside this workstream scope**. `cargo fmt --all --check` and `cargo check --workspace` passed, then clippy reported:
  - `core/crates/omegon/src/tui/active_tool_stream.rs:132`: `clippy::too_many_arguments` on `append_visible_tail`; route to the UI polish workstream.
  - `core/crates/omegon/src/auth.rs:2498`: `clippy::await_holding_lock` in `resolve_with_refresh_prefers_persisted_oauth_over_oauth_env`; route to the auth-integrity workstream.
- `just source-clean`: **passed after commit**; source tree clean.
- `just preflight`: **failed by branch/workspace role policy**, not by release content: current branch is `release/0.27-mechanics-validation`, while preflight only accepts `main` or `release/X.Y`, and this checkout has no release workspace role set. Rerun from `release/0.27`/`main` with release role when cutting.
- `just link`: **passed**. It rebuilt `target/release/omegon`, linked aliases to that binary, installed bundled skills/catalog, and printed `omegon 0.27.0 (e7234c1 2026-06-13)`.
- Binary freshness check: **passed** via `target/release/omegon --version` and an interactive shell sourcing `~/.omegon/dev-alias.sh`; both reported `omegon 0.27.0 (e7234c1 2026-06-13)`.

Validation status: mechanics normalization and binary freshness are complete. Final release readiness remains blocked on the UI/auth clippy failures or an explicit release-owner waiver, plus rerunning `just preflight` from an accepted release branch/workspace role.
