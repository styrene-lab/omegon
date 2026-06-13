---
id: acp-132-0-26-9-completion
title: "0.26.9: Complete issue 132 P0 ACP runtime surfaces"
status: implemented
tags: [acp, issue-132, 0.26.9, runtime-observability, extensions, providers]
open_questions:
  - "[assumption] 0.26.9 should complete only P0 and already-advertised capability surfaces, not P1/P2 diagnostics."
  - "[assumption] Existing underscore ACP routing remains the public wire path for these runtime surfaces."
  - "[assumption] Extension RPC invocation can target only loaded/enabled extension subprocesses for 0.26.9."
dependencies:
  - acp-132-runtime-observability-extension-control
related:
  - docs/acp-132-runtime-observability-extension-control.md
---

# 0.26.9: Complete issue 132 P0 ACP runtime surfaces

## Overview

0.26.9 should close the remaining upstream issue #132 P0 contract by making the surfaces advertised in `_runtime/capabilities` real, stable, and test-covered. 0.26.8 shipped the secrets slice and capability discovery skeleton; 0.26.9 is the contract-completion release.

The target is deliberately bounded: P0 plus already-advertised surfaces only. P1/P2 "everything ever" runtime observability moves into separate follow-up design nodes.

## 0.26.9 scope

### Must implement

1. `_runtime/status`
   - Runtime: name, version, commit, binary, cwd.
   - ACP: protocol version, transport, connected, session id, session cwd when known.
   - Agent: current model, thinking, posture.
   - Memory: scope/root if cheaply available; otherwise explicit `unknown`/`unavailable` fields rather than omission that forces inference.

2. `_provider/status`
   - Active provider/model and readiness.
   - Provider list with cheap auth/session state.
   - Must not force OAuth refresh, keychain prompts, or network calls from a status render.

3. `_extensions/list` diagnostics
   - Preserve existing response fields for compatibility.
   - Add `id`, `loaded`, `path`, `source`, `capabilities`, `metadata`, and `last_error`.
   - Merge installed manifest/state with runtime metadata where available.

4. `_extensions/call`
   - Request: `{ "extension": "flynt", "method": "initialize", "params": {} }`.
   - Response: `{ "extension": "flynt", "method": "initialize", "result": ... }`.
   - Invoke only loaded/enabled extension subprocesses in 0.26.9.

5. Structured errors for `_extensions/call`
   - Stable error codes:
     - `extension_not_installed`
     - `extension_disabled`
     - `extension_not_loaded`
     - `extension_method_not_found`
     - `extension_method_failed`
     - `extension_policy_denied`
     - `invalid_request`
   - Errors should be JSON-RPC errors where possible, with `data.code` for client branching.

6. Capability truthfulness
   - `_runtime/capabilities` must only advertise surfaces that work.
   - If a surface is deferred, remove it from capabilities until implemented.

### Already complete from 0.26.8

- `_runtime/capabilities` skeleton.
- `_secrets/capabilities`.
- `_secrets/list` with non-resolving recipe descriptors.
- `_secrets/set_value`, `_secrets/set_recipe`, `_secrets/check` via existing routes.
- `_packages/list` exists as `crate::packages::list()`.

## Explicit non-scope for 0.26.9

- `_session/status`
- `_session/config`
- `_tools/list`
- `_runtime/health`
- `_permissions/status`
- `_diagnostics/recent`
- `_errors/last`

These are valuable but are not required to close the P0 issue #132 target.

## Implementation phases

### Phase 1 — Runtime/provider basics

- Implement `runtime/status` and `provider/status` match arms in `OmegonAcpAgent::handle_ext_method`.
- Add tests through `handle_acp_request_result` for stable shape and no-prompt/no-network assumptions where feasible.

### Phase 2 — Extension runtime registry

- Add an ACP-facing extension runtime registry populated during setup discovery.
- Registry entries should carry installed/enabled/loaded/path/source/capabilities/metadata/last_error and an optional invocable handle.
- Keep ownership simple; prefer an `Arc`/cloneable handle map already derived from `ExtensionPollingHandle` rather than routing through the event bus.

### Phase 3 — Extension diagnostics list

- Expand `extensions/list` using the registry and installed manifest state.
- Preserve the current `extensions` array and existing per-entry fields.
- Add tests for installed-only, disabled, loaded, and metadata-present entries.

### Phase 4 — Extension call and errors

- Implement `extensions/call` using the registry handle.
- Add fixture-backed success test if feasible.
- Add error-shape tests for not installed, disabled, not loaded, and method failure.

### Phase 5 — Release prep

- Changelog `0.26.9` section.
- Version bump to 0.26.9.
- `just test-rust`, targeted site tests if docs/site changed, `just build && just link`.

## Acceptance criteria

- ACP clients can call `_runtime/capabilities` and every advertised P0 surface works.
- ACP clients can call `_runtime/status` and get structured runtime/session/agent/memory data.
- ACP clients can call `_provider/status` and get structured active provider/model readiness without side effects.
- ACP clients can call `_extensions/list` and distinguish installed/enabled/loaded/error states.
- ACP clients can call `_extensions/call` against a loaded extension and receive a stable success envelope.
- Extension call failures return stable structured error codes.
- Flynt can verify the Flynt extension/deployment contract without trial-and-error probing method names.
