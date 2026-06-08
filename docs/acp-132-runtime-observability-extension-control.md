---
id: acp-132-runtime-observability-extension-control
title: "Issue 132: ACP runtime observability and extension control surfaces"
status: exploring
tags: [acp, issue-132, flynt, runtime-observability, extensions, providers, packages]
open_questions:
  - "[assumption] ACP clients will call underscore-prefixed methods and Omegon will continue routing them through the existing ExtRequest bridge."
  - "[assumption] P0 can preserve existing extensions/list callers by adding fields rather than replacing the response shape."
  - "[assumption] Generic extension invocation can be limited to loaded, enabled extension subprocesses and does not need to respawn disabled/not-loaded extensions."
dependencies:
  - acp-128-turn-control-telemetry
related:
  - docs/acp-surface.md
  - docs/flynt-integration.md
---

# Issue 132: ACP runtime observability and extension control surfaces

## Overview

Upstream issue #132 asks Omegon to expose stable ACP surfaces for runtime/session status, extension inventory and generic extension RPC invocation, provider readiness, and package inventory. The driver is Flynt: ACP transport/session readiness works, but Flynt currently cannot verify the loaded Flynt extension contract without trial-and-error probing extension method names.

The design should not add a Flynt-only readiness endpoint. It should make ACP clients able to distinguish these states:

- ACP transport/session is connected.
- Omegon runtime is initialized with a known cwd/profile/model/memory scope.
- A named extension is installed, enabled, loaded, and invocable.
- Provider authentication/model readiness is structured.
- Secret setup/status is exposed through safe ACP surfaces so low-tech-savviness operators can configure credentials from a settings/panel UI instead of editing shell environment variables.
- Package inventory is discoverable.
- Failures are structured enough for UI remediation.

## Current evidence

- `core/crates/omegon/src/acp.rs` routes underscore-prefixed ACP methods through `route_ext_method`, stripping the leading `_`. Therefore `_extensions/list` maps to `extensions/list`, and `_runtime/status` maps to `runtime/status`.
- `OmegonAcpAgent::handle_ext_method` already implements `extensions/list`, `extensions/get`, extension config, extension secret, and secrets surfaces.
- `extensions/list` currently inventories installed manifest/config/secrets state but does not expose loaded state, load diagnostics, capability provenance, or metadata in the issue #132 response shape.
- `OmegonAcpAgent` stores `extension_metadata`, but not a general live map from extension id to an invocable `ExtensionPollingHandle`.
- `setup.rs` discovers and spawns extensions and already returns `extension_metadata`, widget receivers, vox polling handles, and voice polling handles. This is the right place to also assemble runtime extension diagnostics and generic RPC handles.
- `core/crates/omegon/src/extensions/mod.rs` exposes `ExtensionPollingHandle::rpc_call`, which can invoke arbitrary JSON-RPC methods on a live extension subprocess. The missing interface is wiring that handle into ACP by extension id.

## Goals

### P0

1. `_runtime/capabilities`
   - Advertise supported runtime surfaces and feature flags.
   - Avoid client trial-and-error probing.

2. `_runtime/status`
   - Return one structured runtime/session snapshot: binary, cwd, version, ACP protocol/session, current model/thinking/posture, memory scope.

3. `_extensions/list`
   - Return installed/enabled/loaded status, path/source, capabilities, metadata, and last error where available.
   - Preserve existing `extensions/list` fields for backwards compatibility.

4. `_extensions/call`
   - Generic extension RPC bridge for enabled and loaded extensions.
   - Request shape: `{ "extension": "flynt", "method": "initialize", "params": {} }`.
   - Response shape: `{ "extension": "flynt", "method": "initialize", "result": ... }`.
   - Structured error codes for not installed, disabled, not loaded, method not found, method failed, and policy denied.

5. `_provider/status`
   - Structured provider/auth/model readiness for the active model.

6. `_secrets/capabilities`, `_secrets/list`, `_secrets/set_value`, `_secrets/set_recipe`, `_secrets/check`
   - Expose secret management as a first-class ACP capability for settings panels and low-tech-savviness operators.
   - Values are write-only: clients can set a credential, list configured secret names/sources, and check whether a secret resolves, but never read secret values back.
   - Prefer keyring-backed `set_value` for operator-entered API keys/tokens. Recipes remain available for advanced users and automation.
   - This is both easier and safer than asking operators to edit shell profile files, launch-agent env vars, or per-app settings by hand.

7. `_packages/list`
   - Structured installed package inventory sufficient for Flynt/operator UI. Full package manager semantics can remain future work.

### Deferred P1/P2

- `_session/status`, `_session/config`, `_tools/list`, `_runtime/health`, `_permissions/status`, `_diagnostics/recent`, `_errors/last` should be designed as compatible future additions but not required for first closure unless implementation cost is trivial.

## Non-goals

- No Flynt-specific ACP method.
- No direct exposure of secret values.
- No generic secret deletion/rotation workflow in the first patch; write/check/list is enough for credential setup and diagnostics.
- No automatic extension respawn from `_extensions/call` in the first patch.
- No guarantee that every extension RPC method is safe; policy gating should be explicit before methods with host-action side effects are exposed.

## Proposed architecture

### 1. Runtime surface lives in ACP bridge

Keep the public ACP surface in `core/crates/omegon/src/acp.rs` because the transport already sends `_...` methods there. Add explicit `handle_ext_method` match arms:

- `runtime/capabilities`
- `runtime/status`
- `provider/status`
- `packages/list`
- `secrets/capabilities`
- `secrets/list`
- `secrets/set_value`
- `secrets/set_recipe`
- `secrets/check`
- `extensions/call`

This avoids introducing a parallel ACP router and preserves the existing method translation contract.

### 2. Extension registry passed from setup to ACP

Introduce a small ACP-facing extension registry assembled during setup:

```rust
pub(crate) struct AcpExtensionRegistryEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub loaded: bool,
    pub path: PathBuf,
    pub source: Option<String>,
    pub capabilities: ExtensionCapabilitySummary,
    pub metadata: serde_json::Value,
    pub last_error: Option<String>,
    pub rpc: Option<ExtensionPollingHandle>,
}
```

The exact type should live where ownership is least cyclic. Likely options:

- `extensions/mod.rs`: if it depends only on extension types and serde/path.
- `acp.rs`: if setup only needs to construct JSON-ish entries.
- New `acp/runtime_surfaces.rs`: cleaner if the first patch grows.

The registry should be held by `OmegonAcpAgent` as a map keyed by manifest extension name/id. `setup.rs` should populate it while discovering extensions.

### 3. Installed vs loaded distinction

`_extensions/list` should merge two data sources:

- Installed inventory from `extensions_dir()/manifest.toml` and `ExtensionState`.
- Loaded/runtime inventory from setup-spawn results and metadata/handles.

For each installed extension:

- `enabled`: from `ExtensionState`.
- `loaded`: true only if setup spawned and registered a live handle/metadata entry.
- `last_error`: state stability last error or setup spawn error.
- `capabilities.tools`: true if the manifest/handshake reports one or more tools.
- `capabilities.resources/prompts`: false until extension SDK supports these clearly.

For runtime-only metadata without an installed manifest, include a defensive entry with `installed: false`, `loaded: true`, and metadata, but this should be uncommon.

### 4. Structured error contract

Use JSON-RPC errors for transport-level failures where possible, with stable `data.code` for client branching. `_extensions/call` successful transport but failed extension method should return a JSON-RPC error, not `{ error: "..." }` hidden in a success payload.

Suggested codes in `error.data.code`:

- `extension_not_installed`
- `extension_disabled`
- `extension_not_loaded`
- `extension_method_not_found`
- `extension_method_failed`
- `extension_policy_denied`
- `invalid_request`

If the existing `ext_method` wrapper still collapses errors into `{ "error": "..." }`, refactor only enough to let new surfaces return proper ACP JSON-RPC errors. Avoid changing legacy extension config behavior unless tests prove clients depend on it.

### 5. Provider status scope

For first patch, provider status should be honest but bounded:

- Infer provider from active model.
- Report active provider/model and whether credentials are likely configured using existing auth/env helpers where available.
- Do not force OAuth refreshes or trigger keychain prompts from a status call.
- Model availability can be `true`, `false`, or `unknown` if no cheap check exists.

### 6. Secrets capability scope

Secrets should be a first-class part of the runtime capabilities surface, not hidden behind extension-specific setup. The existing ACP method set already includes `secrets/list`, `secrets/set_value`, `secrets/set_recipe`, `secrets/check`, and `extensions/secret_set`, so issue #132 should formalize and version these methods instead of leaving clients to discover them by convention.

The operator-facing contract should be:

- `secrets/capabilities` returns supported secret operations, storage backends, and safety rules.
- `secrets/list` returns names and non-sensitive source metadata only.
- `secrets/check` returns whether a named secret resolves, without returning the value.
- `secrets/set_value` stores an operator-entered value in keyring-backed storage and returns only status/source.
- `secrets/set_recipe` stores an advanced resolver recipe for operators who know what they are doing.
- `extensions/secret_set` remains useful for extension-scoped setup because it validates that the secret name is declared by the extension manifest.

Recipes are not secret values. Evidence: `omegon-secrets/src/recipes.rs` defines recipes as persisted resolution instructions and states they are never the actual secret values; `resolve.rs` treats recipes as authoritative indirections over fallback env vars. That means the ACP secrets surface should not treat recipes as leaked plaintext credentials.

The right safety boundary is metadata exposure, not value exposure:

- `env:NAME` exposes only an environment variable name. It is still an indirection, not the value, and env vars are fallback/legacy integration plumbing rather than the preferred secret store.
- `keyring:NAME` / `keychain:NAME` exposes only the keychain entry name.
- `vault:path#key` exposes Vault coordinate metadata, not the Vault value.
- `file:/path` exposes a local path that may be sensitive operational metadata.
- `cmd:...` exposes an executable resolver command that may reveal operational details and can have side effects if evaluated.

Therefore `_secrets/list` should return recipe descriptors by default when the caller asks for operator diagnostics, but it should classify recipe kind and avoid resolving them unless explicitly requested. A future low-detail mode may omit payloads for `file` and `cmd`, but the default design should respect that recipes are the abstraction layer operators need to understand and repair their setup.

### 7. Package inventory scope

First patch should expose a basic `_packages/list` from existing local package/extension/Nex metadata if readily available. If package state is not centralized, return a clearly versioned empty inventory with `supported: true` and a `source: "not_yet_indexed"` diagnostic. That satisfies discovery without fabricating package truth.

## Security and policy

- `_extensions/call` is a control surface. It must not let arbitrary ACP clients bypass permission policy for host actions.
- Extension RPC calls that trigger `actions/execute` still flow through existing host-action processing and approval/policy.
- Add an allow/deny layer if extension manifests declare RPC method policy in the future. For first patch, expose the surface only to currently connected ACP clients, and preserve existing host-action gates.
- Never include resolved secret values in runtime, provider, package, extension, or secrets list/check surfaces.
- Recipe descriptors are safe to expose as configuration metadata when needed for diagnostics, but they should be classified by kind (`env`, `keyring`, `vault`, `file`, `cmd`) so UI can choose low-detail vs expert display.
- Secret write methods should treat values as write-only and should not echo them in responses, logs, diagnostics, or test failure output.
- Secret status methods should prefer bounded diagnostics such as `list_recipe_diagnostics`; do not make a settings panel trigger keychain prompts, Vault reads, file reads, or arbitrary `cmd:` recipe execution merely by rendering status.

## Acceptance tests

1. ACP request `_runtime/capabilities` returns versioned surface entries for all implemented P0 surfaces.
2. ACP request `_runtime/status` returns runtime name/version, ACP connected/session fields, current model/thinking/posture, and cwd.
3. ACP request `_extensions/list` includes installed extension enabled/loaded fields and metadata when present.
4. ACP request `_extensions/call` returns structured `extension_not_installed` for an unknown extension.
5. ACP request `_extensions/call` returns structured `extension_disabled` for a disabled installed extension.
6. ACP request `_extensions/call` returns structured `extension_not_loaded` for enabled but not spawned extension.
7. ACP request `_extensions/call` can invoke a fixture extension method and return its result.
8. ACP request `_provider/status` returns active provider/model readiness without prompting for secrets.
9. ACP request `_secrets/capabilities` advertises write-only value storage, recipe indirection support, list/check support, and redaction guarantees.
10. ACP request `_secrets/list` returns configured names, recipe descriptors, recipe kinds, and bounded status without resolved secret values.
11. ACP request `_secrets/set_value` stores a value and returns only status/source, never the value.
12. ACP request `_packages/list` returns a stable JSON object, even if inventory is empty.

## Implementation plan

### Phase 1 — Surface skeleton and tests

- Add helper builders for runtime capabilities/status/provider/package/secrets JSON.
- Add tests that call `handle_acp_request_result` for `_runtime/capabilities`, `_runtime/status`, `_provider/status`, `_packages/list`, and `_secrets/capabilities`.
- Keep outputs conservative and stable.

### Phase 2 — Secrets capability hardening

- Formalize underscore aliases for existing `secrets/*` and `extensions/secret_set` ACP methods.
- Add `secrets/capabilities` so panels can discover safe setup affordances.
- Prefer `SecretsManager::list_recipe_diagnostics()` over raw `list_recipes()` for UI status, because it already provides bounded recipe diagnostics and treats Vault recipes as deferred.
- Add an explicit non-resolving recipe descriptor path for settings panels. `list_recipe_diagnostics()` is useful for CLI-style checks, but it resolves non-Vault recipes today; ACP settings/status should instead describe recipe kind/payload/status metadata without executing `cmd:`, reading `file:`, touching keyring, or reading Vault unless the operator explicitly requests a check.
- Add tests for list/check/set_value behavior that assert resolved values are never echoed.

### Phase 3 — Extension diagnostics

- Define ACP-facing extension registry/diagnostic structures.
- Populate registry from setup discovery results.
- Extend `OmegonAcpAgent::new_with_extension_metadata` or add a richer constructor that accepts metadata plus registry.
- Expand `extensions/list` with `id`, `loaded`, `path`, `source`, `capabilities`, `metadata`, and `last_error` fields while preserving existing fields.

### Phase 4 — Generic extension call

- Wire `_extensions/call` to the live registry RPC handle.
- Add structured error helpers.
- Add fixture tests for success and missing/disabled/not-loaded failures.

### Phase 5 — Validation and release memory

- Add/update `CHANGELOG.md` under `[Unreleased]`.
- Run targeted ACP tests, then `cargo test -p omegon` or the project validator.
- Build/link after implementation per project directive.
