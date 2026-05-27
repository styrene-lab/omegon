---
title: SDK Lockstep Contract (#102)
status: exploring
tags: [sdk, contract, 0.25, extensions, python, typescript]
---

# SDK Lockstep Contract (#102)

## Problem

Rust, Python, and TypeScript extension SDKs can drift silently. Recent examples include stale TypeScript declarations and Python defaults lagging the host contract.

## Goal

Publish a canonical SDK contract artifact owned by the Rust SDK and consumed/validated by downstream SDK ports.

## Candidate artifact

```text
core/crates/omegon-extension/schema/sdk-contract.json
```

or:

```text
core/crates/omegon-extension/schema/sdk-contract.pkl
```

## Contract contents

- SDK contract version
- protocol versions
- RPC methods
- tool definition schema
- tool result schema
- error codes
- capability flags
- manifest schema fragments
- HostAction schemas
- UI contribution schemas
- resource/open schemas after #83

## Decisions

### Decision: Rust SDK remains canonical until extraction

Downstream Python/TypeScript SDKs validate against Rust-owned contract data.

### Decision: Do not extract repo before contract exists

This issue blocks [[sdk-repo-extraction-103]].

## Open questions

- [assumption] JSON Schema is better for Python/TypeScript validation; Pkl remains useful for host config validation.
- Should SDK ports generate types or validate handwritten types?
- What version granularity: `0.25`, `0.25.0`, or independent `contract_version = 1`?

## Acceptance

- Rust SDK exports a versioned contract artifact.
- Rust tests assert artifact matches current SDK constants/types where practical.
- Python/TypeScript SDK repos can validate their exposed constants/types against it.
- Release checklist requires contract update/check for SDK changes.

## Links

- [[0.25-roadmap-extension-surfaces]]
- [[extension-ui-contributions-101]]
- [[sdk-repo-extraction-103]]
