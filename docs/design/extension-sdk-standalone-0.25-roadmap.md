---
title: Extension SDK Standalone Roadmap for 0.25.x
status: exploring
tags: [extension-sdk, sdk-contract, 0.25, roadmap]
issue: 103
---

# Extension SDK Standalone Roadmap for 0.25.x

## Context

Upstream issue: [#103 Extract omegon-extension into standalone Rust SDK repo after contract stabilization](https://github.com/styrene-lab/omegon/issues/103).

The target is to extract `core/crates/omegon-extension` into a standalone first-party Rust SDK repository, but not as a big-bang move. The extraction must happen after the SDK contract boundary is explicit and downstream Python/TypeScript validation exists.

## Goal

Make 0.25.x the contract-stabilization line for the extension SDK, with enough design and validation to support a later standalone repository split.

## Non-goals for the first 0.25 slice

- Do not move `omegon-extension` to a new repo yet.
- Do not require all first-party extensions to migrate in the first PR.
- Do not invent host rendering behavior for UI contributions as part of contract extraction.
- Do not make Python/TypeScript SDKs authoritative over the Rust contract.

## Required design nodes

This roadmap is the parent tracking node. Required child/internal nodes:

1. [[extension-sdk-contract-artifact-0.25]] — define the canonical SDK contract artifact and Rust conformance tests.
2. [[extension-sdk-host-compatibility-policy-0.25]] — define host behavior for exact, older, newer, missing SDK contract versions.
3. [[extension-sdk-cross-language-lockstep-0.25]] — define Python/TypeScript SDK lockstep validation against the Rust contract.
4. [[extension-sdk-conformance-suite-0.25]] — define example-extension protocol smoke and cross-language fixture coverage.
5. [[extension-sdk-standalone-repo-extraction-0.25]] — define the eventual `styrene-lab/omegon-extension` repository contents, history strategy, and consumer migration plan.
6. [[extension-ui-contributions-contract-0.25]] — define declarative UI/TUI contributions as a contract surface without binding host rendering implementation.

## Sequencing

### Phase 1 — Contract artifact in current repo

- Add `SDK_CONTRACT_VERSION` to `omegon-extension`.
- Add `core/crates/omegon-extension/schema/sdk-contract.json`.
- Add `core/crates/omegon-extension/schema/sdk-contract.pkl` if Pkl validation is part of the build gate.
- Add tests that compare artifact contents to Rust constants and public schema expectations.

### Phase 2 — Host compatibility policy

- Add host-side compatibility rules for extension-advertised SDK contract versions.
- Classify exact, older compatible, older unsupported, newer unknown, and missing contract versions.
- Surface warnings/refusals through extension startup diagnostics.

### Phase 3 — Cross-language lockstep

- Make Python and TypeScript SDK ports pin/consume the contract artifact.
- Export equivalent `SDK_CONTRACT_VERSION` constants.
- Validate capabilities, error codes, protocol metadata, and manifest fragments in CI.

### Phase 4 — Conformance examples

- Use example Rust/Python/TypeScript extensions as protocol smoke fixtures.
- Assert metadata, tool definitions, tool results, capabilities, HostActions, and UI contribution examples.

### Phase 5 — Standalone extraction

- Create `styrene-lab/omegon-extension` with `src/`, `schema/`, README, changelog, license, and CI.
- Prefer `git filter-repo`/subtree history preservation from `core/crates/omegon-extension`.
- Update Omegon host and first-party extensions to depend on a released or git-tagged standalone SDK.

## Decisions

- Decision: 0.25.x will stabilize the SDK contract before extracting the crate.
- Decision: Rust `omegon-extension` remains the canonical contract source until extraction.
- Decision: Python/TypeScript SDK ports validate against the contract; they do not define it.
- Decision: Host compatibility policy belongs in Omegon host, not in the SDK crate.

## Open questions

- [assumption] `sdk-contract.json` is sufficient as the first machine-readable artifact; Pkl can be added in the same PR only if low-cost.
- [assumption] Existing protocol smoke tests can be extended rather than replaced by a new conformance harness.
- Which first-party extension should be the primary migration canary: `omegon-reader`, `omegon-voice`, or `omegon-example-rust`?
- Should `SDK_CONTRACT_VERSION` be semver-major/minor only (`0.25`) or full patch (`0.25.0`)?
- Where should generated JSON live after extraction: committed artifact only, generated-from-Pkl, or generated-from-Rust build script?

## Acceptance criteria for this roadmap

- Child design nodes exist for each required area.
- #103 is linked from this roadmap.
- 0.25 milestone scope is explicit: contract first, extraction later.
- First implementation slice is identified as the SDK contract artifact, not the repo split.
