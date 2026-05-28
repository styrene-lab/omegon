---
title: Extension SDK Contract Artifact for 0.25
status: exploring
tags: [extension-sdk, contract, schema, 0.25]
parent: extension-sdk-standalone-0.25-roadmap
issue: 103
---

# Extension SDK Contract Artifact for 0.25

## Purpose

Define a machine-readable contract artifact for the current in-repo `omegon-extension` Rust SDK before any repository split.

This artifact is the anchor for:

- Rust SDK public constants and schema examples.
- Python/TypeScript SDK lockstep validation.
- Example extension conformance tests.
- Host compatibility checks.
- Future standalone `styrene-lab/omegon-extension` releases.

## Proposed files

```text
core/crates/omegon-extension/schema/sdk-contract.json
core/crates/omegon-extension/schema/sdk-contract.pkl
```

JSON is the minimum required artifact because every downstream SDK can consume it without language-specific tooling. Pkl is useful if it becomes the generated source of truth, but it should not block the first slice if Pkl generation adds friction.

## Proposed Rust API

```rust
pub const SDK_CONTRACT_VERSION: &str = "0.25";
```

Potential supporting constants:

```rust
pub const PROTOCOL_VERSION: &str = "2";
pub const HOST_ACTION_TERMINAL_CREATE_V1: &str = "terminal.create@1";
```

## Contract contents

Minimum JSON shape:

```json
{
  "sdk_contract_version": "0.25",
  "protocol_version": "2",
  "rpc_methods": {
    "get_tools": "get_tools",
    "bootstrap_secrets": "bootstrap_secrets",
    "bootstrap_config": "bootstrap_config",
    "execute_tool_prefix": "execute_",
    "actions_execute": "actions/execute"
  },
  "capabilities": ["tools", "widgets", "mind", "vox", "resources", "prompts", "sampling", "elicitation", "streaming", "voice", "ui_contributions", "host_actions", "host_action_execution"],
  "host_actions": ["terminal.create@1"],
  "host_action_statuses": ["completed", "needs_approval", "denied", "unsupported", "invalid"],
  "tool_result_meta_keys": ["omegon/hostActions"],
  "manifest_fragments": ["capabilities", "permissions.host_actions", "ui"]
}
```

## Required tests

- Rust `SDK_CONTRACT_VERSION` equals `sdk_contract_version` in JSON.
- Public `Capabilities` field names match contract capability list.
- `HostActionStatus` serializes to exactly the status strings listed in the contract.
- HostAction type constants match contract `host_actions`.
- MCP/metadata key constants match contract `tool_result_meta_keys`.
- Manifest examples using `capabilities`, `permissions.host_actions`, and `ui` parse successfully.

## Decisions

- Decision: `sdk-contract.json` is the first required artifact because it is language-neutral and easy for Python/TypeScript to consume.
- Decision: `SDK_CONTRACT_VERSION` should represent the contract surface, not the crate patch version.
- Decision: Contract conformance tests live in `omegon-extension` while the crate remains in-tree.

## Open questions

- [assumption] Contract version should be `0.25`, not `0.25.0`, so patch releases can fix SDK implementation without changing the contract.
- Should the contract artifact include full JSON Schemas for tool definitions/results, or only constants plus examples in the first slice?
- Should Pkl generate JSON, or should JSON be committed directly first and Pkl follow?

## First implementation slice

1. Add `SDK_CONTRACT_VERSION`.
2. Add `schema/sdk-contract.json`.
3. Add conformance tests for version, capabilities, HostAction statuses, and `terminal.create@1`.
4. Update changelog.
5. Open PR linked to #103.
