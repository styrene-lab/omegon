# HostActions Core Runtime Design

## TDD strategy

Issue #75 must land as a sequence of red-green commits. Each phase introduces failing tests that describe one runtime seam, then the smallest implementation that makes them pass.

## Phase A — Manifest and capability substrate

### Red tests

- `manifest_host_action_capabilities_parse`: manifest with `[capabilities] host_actions = true` and `host_action_execution = true` exposes both flags.
- `manifest_host_action_capabilities_default_false`: legacy manifest omitting these flags loads with both false.
- `manifest_host_action_permissions_parse`: `[permissions.host_actions] allowed = ["terminal.create@1"]` parses.
- `manifest_terminal_create_policy_parse`: terminal policy fields parse exactly.

### Implementation target

- `core/crates/omegon-extension/src/manifest.rs`
- possibly `core/crates/omegon/src/extensions/mod.rs` for runtime exposure

### Exit criteria

- Manifest model can represent capabilities and permissions.
- Existing manifest tests still pass.
- No action execution is added.

## Phase B — Structured tool-result envelope extraction

### Red tests

- Raw JSON output still becomes visible content.
- Existing `{content:[...]}` output maps to `ContentBlock` without stringifying the entire envelope.
- Valid `actions` are extracted into structured details separate from content.
- Malformed `actions` produce invalid/ignored outcomes while preserving content.

### Implementation target

- Add small parser module, likely under `core/crates/omegon/src/extensions/host_actions.rs` or `core/crates/omegon/src/extensions/tool_result.rs`.
- Update `ExtensionFeature::execute` in `core/crates/omegon/src/extensions/mod.rs` to call the parser instead of always `output.to_string()`.

### Exit criteria

- Backward compatibility is proven by tests.
- Declarative HostActions are detectable without executing anything.
- Malformed `content` arrays fall back to visible legacy text instead of producing empty output.
- Image content accepts both SDK/internal `media_type` and MCP-style `mediaType`.

## Phase C — Validation and policy pipeline

### Red tests

- Missing `id`, missing `type`, malformed `params` => `invalid` outcome.
- Unknown action type/version => `unsupported` outcome.
- Manifest disallows action type => `denied` outcome.
- Allowed action type reaches executor registry seam but does not spawn anything unless an executor is registered.
- `auto_if_allowed` remains non-auto unless all policy gates are explicitly true.

### Implementation target

- New core HostAction pipeline module with:
  - origin model: `native_extension`, `mcp`, `internal`
  - scoped action identity
  - validation result / outcome model
  - manifest permission policy check
  - audit event hook

### Exit criteria

- One public pipeline function handles declarative and imperative candidates.
- Policy denial and validation failures are typed and auditable.

## Phase D — Imperative `actions/execute` handling

### Red tests

- Extension-origin `actions/execute` request reaches the same pipeline as declarative actions.
- A manifest-denied action is denied through `actions/execute`.
- Unsupported action is returned as typed `unsupported`.
- Invalid action is returned as typed `invalid`.

### Implementation target

- Extension process reader/router in `core/crates/omegon/src/extensions/mod.rs` or a split module.
- Avoid any separate imperative-only policy path.

### Exit criteria

- Imperative execution cannot bypass declarative policy.

## Phase E — Rendering/headless exposure

### Red tests

- Tool results expose ordinary content and HostAction outcomes separately in `ToolResult.details` or a typed host-action detail structure.
- Headless mode produces deterministic JSON-ish details rather than requiring TUI interaction.
- TUI/ACP can render action summaries without parsing raw extension JSON.

### Implementation target

- Result detail schema first; polished TUI action cards can be deferred if details are stable.

### Exit criteria

- Declarative HostActions are visible separately from ordinary content.
- No accidental execution in non-interactive contexts.

## Minimal implementation pass order

1. Phase A tests + manifest implementation.
2. Phase B tests + parser implementation.
3. Phase C tests + pipeline implementation with no-op/unsupported executors.
4. Phase D tests + `actions/execute` routing.
5. Phase E tests + rendering/detail exposure.

## Risk controls

- Never use shell strings for command actions; all tests should assert argv-only shapes.
- Treat all extension/MCP fields as untrusted until validation succeeds.
- Keep malformed actions isolated from ordinary content.
- Keep executor registry empty until issue #76 adds `terminal.create@1` execution.
