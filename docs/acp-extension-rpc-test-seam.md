---
id: acp-extension-rpc-test-seam
title: "ACP extension RPC test seam"
status: implemented
tags: [acp, extensions, testing, issue-132-followup]
open_questions:
  - "[assumption] ExtensionPollingHandle should remain the production transport wrapper while ACP depends on a small trait for invocation tests."
  - "[assumption] A fake in-memory RPC invoker is sufficient for ACP request/response tests; end-to-end subprocess coverage can remain in extension integration tests."
dependencies:
  - acp-132-0-26-9-completion
related:
  - docs/acp-extension-control-plane-hardening.md
---

# ACP extension RPC test seam

## Overview

The 0.26.9 ACP `_extensions/call` path can validate missing-extension and invalid-request behavior without spawning a real extension, but successful invocation is hard to unit-test because `ExtensionPollingHandle` directly wraps process stdin/stdout handles.

Introduce a narrow trait seam so ACP can test successful calls with an in-memory fake while production continues to use `ExtensionPollingHandle`.

## Proposed interface

```rust
#[async_trait]
pub trait ExtensionRpcInvoker: Send + Sync {
    fn extension_name(&self) -> &str;
    async fn rpc_call(&self, method: &str, params: serde_json::Value) -> anyhow::Result<serde_json::Value>;
}
```

Production adapter:

```rust
#[async_trait]
impl ExtensionRpcInvoker for ExtensionPollingHandle { ... }
```

ACP storage changes from:

```rust
BTreeMap<String, ExtensionPollingHandle>
```

to:

```rust
BTreeMap<String, Arc<dyn ExtensionRpcInvoker>>
```

## Test strategy

- Unit-test `_extensions/call` success with a fake invoker that records method/params and returns deterministic JSON.
- Unit-test `method_failed` with a fake invoker returning an error.
- Keep current missing/not-loaded validation tests.
- Leave subprocess protocol coverage in extension integration tests.

## Implementation

Implemented in `core/crates/omegon/src/acp/extension_rpc.rs`. ACP stores production handles unchanged, while the call helper accepts any `ExtensionRpcInvoker` implementation. Unit tests use an in-memory fake to verify method/parameter forwarding and `method_failed` error mapping without spawning an extension process.

## Acceptance criteria

- ACP `_extensions/call` has success-path unit coverage without spawning a process.
- ACP `_extensions/call` method-failure mapping has unit coverage without spawning a process.
- Production extension handle plumbing remains behaviorally unchanged.
- The trait is scoped to ACP control-plane invocation and does not become a general extension lifecycle abstraction.
