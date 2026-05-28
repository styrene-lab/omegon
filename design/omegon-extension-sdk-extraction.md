+++
id = "omegon-extension-sdk-extraction"
kind = "design_node"
title = "Omegon Extension SDK Extraction"
status = "implementing"
tags = ["extensions", "sdk", "rust", "contract"]
aliases = ["omegon-extension-rs"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "feature"
open_questions = []
parent = "rust-native-extension-boundary"
priority = "1"
+++

# Omegon Extension SDK Extraction

## Overview

Extract `core/crates/omegon-extension` from the Omegon host repository into a standalone Rust SDK repository/check-out (`omegon-extension-rs`) that sits alongside `omegon-extension-py`, `omegon-extension-ts`, and `omegon-example-extension` in the extension ecosystem workspace.

The extraction is not a pure file move. The Rust crate is currently the de facto contract for all extension SDKs. Before moving it, the shared SDK contract must be made explicit so Python and TypeScript inherit from a versioned protocol artifact rather than from Rust implementation details.

## Demarcation boundary

### Standalone Rust SDK owns

- Extension-facing protocol constants: `SDK_CONTRACT_VERSION`, `PROTOCOL_VERSION`.
- JSON-RPC message structs and error code mapping.
- Initialize params/result and capability flags.
- Tool result/content helper types.
- HostAction declaration types and declarative result embedding.
- Manifest fragments extension authors need to write and validate manifests.
- Extension author runtime ergonomics: `Extension`, `serve`, `serve_v2`, `HostProxy`.
- Contract artifacts under `schema/`, initially `sdk-contract.json`.
- Contract tests that fail when Rust types drift from the artifact.

### Omegon host owns

- Extension install/enable/disable lifecycle.
- Manifest loading policy, permission enforcement, and capability compatibility decisions.
- Process supervision and timeout policy.
- HostAction execution, sandboxing, and operator approval.
- TUI/Cockpit/UI rendering, slash-command routing, and Armory registry integration.
- Host compatibility matrix for exact/older/newer/missing SDK contract versions.

### Other SDKs own

- Language-native author ergonomics for Python and TypeScript.
- Validation that their exposed constants, errors, capabilities, and HostAction shapes match `sdk-contract.json`.
- Runtime implementations that speak the same wire contract.

## Decisions

1. The first implementation step is to add an explicit SDK contract artifact inside the current Omegon repo clone, not to move the crate immediately.
2. Rust exports `SDK_CONTRACT_VERSION` as a contract compatibility version separate from Cargo package version.
3. The initial contract covers only already-public stable surfaces: protocol version, error codes, capability flags, initialize shape, and core JSON-RPC method names.
4. Host compatibility policy remains host-side. The SDK artifact describes versions and shapes; it does not decide whether a host should accept an extension.

## Implementation plan

1. Add `SDK_CONTRACT_VERSION` to `omegon-extension` and re-export it.
2. Add `schema/sdk-contract.json` to the standalone `omegon-extension-rs` crate.
3. Add Rust tests asserting the contract artifact matches `SDK_CONTRACT_VERSION`, `PROTOCOL_VERSION`, `ErrorCode`, and `Capabilities` serialization/defaults.
4. Update the test extension to advertise `SDK_CONTRACT_VERSION` instead of a stale hard-coded SDK version.
5. Validate with `cargo test -p omegon-extension`.
6. After contract tests pass, split/copy to `omegon-extension-rs` and convert path consumers in a separate commit.
