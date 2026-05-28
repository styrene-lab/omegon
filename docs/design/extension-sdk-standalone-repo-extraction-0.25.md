---
title: Extension SDK Standalone Repo Extraction for 0.25
status: exploring
tags: [extension-sdk, repo-extraction, release, 0.25]
parent: extension-sdk-standalone-0.25-roadmap
issue: 103
---

# Extension SDK Standalone Repo Extraction for 0.25

## Purpose

Define the eventual extraction of `core/crates/omegon-extension` into `styrene-lab/omegon-extension` after contract stabilization.

## Target repository

```text
styrene-lab/omegon-extension
```

Initial contents:

```text
Cargo.toml
src/
schema/
README.md
CHANGELOG.md
LICENSE-APACHE
LICENSE-MIT
.github/workflows/ci.yml
```

## History strategy

Preferred:

```text
git filter-repo or subtree split from core/crates/omegon-extension
```

Fallback:

```text
copy current crate contents and document original source commit in README/CHANGELOG
```

## Consumer migration

Known first-party consumers:

- `omegon-browser`
- `omegon-reader`
- `omegon-voice`
- `aether`
- `omegon-example-rust`
- Omegon host repo itself where shared SDK protocol types are used

Migration target:

```toml
omegon-extension = "0.25"
```

Temporary target if crates.io release timing is constrained:

```toml
omegon-extension = { git = "https://github.com/styrene-lab/omegon-extension", tag = "v0.25.0" }
```

## Extraction prerequisites

- Contract artifact exists and has tests.
- Host compatibility policy exists and has tests.
- Python/TypeScript lockstep plan is implemented or has active PRs.
- Example conformance suite exists for Rust and at least one non-Rust SDK.
- UI contribution contract is represented if #101 is included in 0.25.

## Decisions

- Decision: Do not extract until consumers have a migration path and a known-good contract tag.
- Decision: Avoid long-term local shim crates in the Omegon host repo.
- Decision: If a temporary re-export crate is needed, it should last for one release window only.

## Open questions

- [assumption] `omegon-extension` will be published to crates.io under Styrene ownership.
- Should the standalone SDK use dual Apache/MIT licensing even though the host repo is BUSL?
- Should host repo retain any generated schema artifacts for offline install validation?
- Should extraction happen before or after UI contribution host rendering is implemented?

## Acceptance criteria

- New repo exists.
- CI passes in new repo.
- Versioned SDK tag exists.
- Omegon host builds against standalone SDK.
- First-party extension migration PRs exist or are merged.
- Tracking config can replace path anchors with release/git anchors.
