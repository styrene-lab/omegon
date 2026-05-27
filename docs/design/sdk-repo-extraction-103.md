---
title: Standalone Rust Extension SDK Repo (#103)
status: seed
tags: [sdk, repo, extraction, extensions]
---

# Standalone Rust Extension SDK Repo (#103)

## Problem

`omegon-extension` is becoming the public SDK contract for extension authors and downstream SDK ports, but it still lives inside the Omegon monorepo.

## Goal

Eventually extract it to:

```text
styrene-lab/omegon-extension
```

without breaking first-party extensions or downstream SDKs.

## Dependency chain

This is blocked on [[sdk-lockstep-contract-102]].

Recommended sequence:

1. Stabilize 0.25 UI/resource schemas inside the monorepo.
2. Publish SDK contract artifact.
3. Make Python/TypeScript ports validate against contract.
4. Use example extensions as conformance fixtures.
5. Extract Rust SDK repo.
6. Update Omegon and first-party extensions to depend on the standalone crate/tag.

## Decisions

### Decision: No big-bang extraction

Do not move the SDK before contract validation exists. Repo extraction without lockstep would amplify drift.

## Open questions

- [assumption] Crate version remains aligned with Omegon through 0.x.
- Should the standalone SDK publish independently or tag lockstep with host releases?
- How do bundled examples consume local path dependencies during host development?
- Which CI owns cross-repo conformance after extraction?

## Acceptance

- Standalone repo exists with history or clean import.
- Omegon depends on the standalone crate without losing local development ergonomics.
- First-party extensions build against the standalone crate.
- Downstream SDK ports validate against the same contract artifact.

## Links

- [[0.25-roadmap-extension-surfaces]]
- [[sdk-lockstep-contract-102]]
