---
title: Extension SDK Conformance Suite for 0.25
status: exploring
tags: [extension-sdk, conformance, testing, 0.25]
parent: extension-sdk-standalone-0.25-roadmap
issues: [102, 103]
---

# Extension SDK Conformance Suite for 0.25

## Purpose

Define a cross-language extension conformance suite that validates Rust, Python, and TypeScript SDK behavior against the same protocol contract.

## Candidate fixtures

- `omegon-example-rust`
- Python example extension
- TypeScript example extension

Each fixture should expose the same logical capabilities where language support exists:

- `get_tools`
- `execute_echo`
- `execute_host_action_dry_run`
- `execute_ui_contribution_fixture` if UI contribution contract is included
- optional notification fixture for voice/event adapter behavior

## Smoke expectations

Host-driven protocol smoke should verify:

- Extension starts and responds within timeout.
- `get_tools` returns SDK-style schemas accepted by the host.
- Tool execution returns content and structured details.
- ToolResult actions preserve HostAction candidates.
- Capabilities serialize consistently.
- SDK contract metadata is present.
- Invalid method returns contract error shape.

## Decisions

- Decision: The conformance suite should run against built extension binaries/processes, not only in-language unit tests.
- Decision: Rust example should be the first canary because it uses the canonical SDK crate.
- Decision: Python/TypeScript fixtures can initially validate protocol shape without full feature parity.

## Open questions

- [assumption] Existing `core/crates/omegon-extension/tests/protocol_smoke.rs` can be extended rather than replaced.
- Should conformance fixtures live in the SDK repo after extraction or in a separate `omegon-extension-examples` repo?
- How many features are required before an SDK port is considered 0.25-compatible?

## Required tests

- Rust protocol smoke passes against contract version 0.25.
- Python protocol smoke passes against contract version 0.25.
- TypeScript protocol smoke passes against contract version 0.25.
- Host rejects or warns on mismatched fixture SDK contract versions according to compatibility policy.
