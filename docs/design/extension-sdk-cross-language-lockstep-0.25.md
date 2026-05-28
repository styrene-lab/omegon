---
title: Extension SDK Cross-Language Lockstep for 0.25
status: exploring
tags: [extension-sdk, python, typescript, lockstep, 0.25]
parent: extension-sdk-standalone-0.25-roadmap
issues: [102, 103]
---

# Extension SDK Cross-Language Lockstep for 0.25

## Purpose

Keep Python and TypeScript extension SDK ports aligned with the Rust `omegon-extension` contract before extracting the Rust SDK into a standalone repo.

Related upstream issue: [#102 Keep Python and TypeScript extension SDK ports in lockstep with Rust SDK](https://github.com/styrene-lab/omegon/issues/102).

## Direction

Rust remains the canonical contract source for 0.25. Python and TypeScript consume or validate against the contract artifact.

## Required SDK behavior

Python SDK:

- Exposes `SDK_CONTRACT_VERSION`.
- Sets extension metadata or manifest `sdk_version` from the contract version.
- Validates capabilities, error codes, HostAction statuses, and metadata keys against `sdk-contract.json` in CI.

TypeScript SDK:

- Exposes `SDK_CONTRACT_VERSION`.
- Emits SDK version in generated declarations or runtime extension metadata.
- Validates capabilities, error codes, HostAction statuses, and metadata keys against `sdk-contract.json` in CI.

## Contract consumption options

1. Vendor committed `sdk-contract.json` into each SDK repo.
2. Fetch pinned contract artifact from `styrene-lab/omegon-extension` once extracted.
3. Use a shared generated package later.

For the first 0.25 slice, vendoring/pinning is simplest and reviewable.

## Decisions

- Decision: Python/TypeScript SDKs validate against the Rust contract; they do not define it.
- Decision: Lockstep validation should fail CI when a contract field is missing, renamed, or serialized differently.
- Decision: Cross-language examples should use the same logical extension fixture names and tool payloads.

## Open questions

- [assumption] Python and TypeScript SDK repos already exist or will be created before standalone Rust extraction.
- Which CI should own cross-repo validation before extraction: Omegon host repo, SDK repos, or example-extension repo?
- Should the contract artifact include enough JSON Schema to generate Python/TypeScript types, or should initial ports hand-code types and validate examples?

## Required tests

- Python SDK contract version equals Rust artifact version.
- TypeScript SDK contract version equals Rust artifact version.
- Capability field sets match exactly.
- HostAction status strings match exactly.
- `terminal.create@1` params/result examples round-trip in all languages.
- ToolResult actions metadata matches `_meta["omegon/hostActions"]` expectations.
