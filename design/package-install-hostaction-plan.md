# package.install@1 HostAction implementation plan

## Issue

GitHub issue #113: `package.install@1` HostAction for trusted package providers.

## Existing framework fit

Omegon already has the required HostAction framework:

- candidate parsing and manifest gating in `extensions/host_actions.rs`;
- runtime/manual approval via `prepare_host_action_for_approval` and `process_host_action_candidate_with_approval_decision`;
- executor registry pattern via `HostActionExecutorRegistry`;
- terminal backend registry and real terminal execution for `terminal.create@1`;
- declarative HostAction processing for extension tool results and MCP results;
- manifest allowlist gate through `ExtensionManifest::allows_host_action_type`.

The implementation should add a second typed executor beside `terminal.create@1`, not a new package-manager command surface.

## Design stance

`nex_capability` remains read-only. Any package mutation must happen through HostAction approval and host-owned execution.

The extension proposes intent:

```json
{
  "type": "package.install@1",
  "params": {
    "provider": "omegon-nex",
    "tool": "micro",
    "package": "micro",
    "scope": "user",
    "capability": "terminal-editor",
    "may_require_privilege": true
  }
}
```

The host derives execution from policy. The extension never sends command/env/cwd/sudo data.

## Contract

Action type:

```text
package.install@1
```

Params:

```rust
struct PackageInstallParams {
    provider: String,
    tool: String,
    package: String,
    scope: String,
    capability: Option<String>,
    may_require_privilege: bool,
}
```

Initial policy constants:

```text
provider: omegon-nex
tools/packages: micro -> micro, hx -> helix, nvim -> neovim
scope: user
execution_mode: managed_terminal
require_operator_approval: true
allow_privilege_escalation: true
```

The issue text listed package ids equal to tool ids. For host-owned policy, use an explicit map and decide whether the provider's accepted package ids are:

```text
micro -> micro
hx -> hx
nvim -> nvim
```

or canonical Nix ids:

```text
micro -> micro
hx -> helix
nvim -> neovim
```

First pass should preserve issue acceptance literally unless upstream `omegon-nex` emits canonical package ids. That means allow exact tool/package matches initially and leave alternate mapping as an obvious future extension.

## Execution strategy

Use managed terminal first.

Derived command for approved install:

```text
nex install --nix <package>
```

But do not execute through shell. Represent it as command + args internally and render it as a reviewed command string only for terminal display.

Recommended first executor output:

- If approval is unavailable/required: `needs_approval` before execution.
- If approved: open managed terminal session running the derived command.
- Completion status can be `completed` once the terminal session is created, with `terminal_id` in result. It does not need to wait for package-manager completion in MVP.

## Implementation steps

### 1. Extend executor registry

File:

```text
core/crates/omegon/src/extensions/host_actions.rs
```

Add:

```rust
const PACKAGE_INSTALL_V1: &str = "package.install@1";
```

Extend `HostActionExecutorRegistry`:

```rust
package_install_policy: Option<PackageInstallPolicy>
```

Default supported should include both:

```text
terminal.create@1
package.install@1
```

Real executor registry should configure terminal backend and package policy.

### 2. Add policy model

Add small internal structs/enums:

```rust
struct PackageInstallPolicy {
    enabled: bool,
    allowed_providers: BTreeSet<String>,
    allowed_tools: BTreeMap<String, PackageToolPolicy>,
    allowed_scopes: BTreeSet<String>,
    allow_privilege_escalation: bool,
    require_operator_approval: bool,
    execution_mode: PackageInstallExecutionMode,
}

struct PackageToolPolicy {
    package: String,
    command_package: String,
}

enum PackageInstallExecutionMode {
    ManagedTerminal,
}
```

Default policy:

```text
enabled = true
allowed_providers = ["omegon-nex"]
allowed_tools = { micro, hx, nvim }
allowed_scopes = ["user"]
allow_privilege_escalation = true
require_operator_approval = true
execution_mode = ManagedTerminal
```

Issue says host policy can enable/disable; first pass can expose constructors for tests and hardcode default until config wiring exists.

### 3. Validate params

Validation order should match issue:

1. type is exact `package.install@1`
2. provider allowlisted
3. tool allowlisted
4. package matches provider/tool policy
5. scope allowlisted
6. privilege policy allows `may_require_privilege`
7. execution mode available

Return structured `HostActionOutcome` statuses:

- disabled/denied: `HostActionStatus::Denied`, code `package_install_denied`
- invalid params/mismatch: `HostActionStatus::Invalid`, code `package_tool_mismatch` or specific codes
- unsupported execution mode: `HostActionStatus::Unsupported`

### 4. Approval handling

Do not bypass existing approval path.

Because `package.install@1` must require approval initially, ensure:

```rust
action_requires_manual_approval(...)
```

already returns true for `Manual`, `None`, and `AutoIfAllowed` unless all auto gates are true.

Tests should assert declarative host actions without an approval channel produce `needs_approval` from the context path, or denied/unavailable from non-context synchronous paths depending on existing semantics.

### 5. Execute managed terminal

Reuse the terminal backend seam if possible.

Options:

A. Convert package install into an internal `terminal.create@1` action and call `execute_terminal_create_with_registry`.

B. Add a package executor that directly calls the real terminal backend with a prepared terminal request.

Prefer A only if terminal request types are easy to reuse without JSON roundtripping. Otherwise B is clearer.

Terminal metadata/result should include:

```json
{
  "provider": "omegon-nex",
  "tool": "micro",
  "package": "micro",
  "scope": "user",
  "execution_mode": "managed_terminal",
  "terminal_id": "..."
}
```

### 6. Manifest and MCP policy

Native extensions must declare:

```toml
[host_actions]
allowed = ["package.install@1"]
```

MCP origin uses existing MCP host_action policy allowlist; no special path should be needed beyond executor registry support.

### 7. Tests

Add tests in `extensions/host_actions.rs`:

- unsupported when executor registry lacks `package.install@1`
- denied when manifest does not allow type
- invalid unknown provider
- invalid unknown tool
- invalid package mismatch
- invalid unknown scope
- denied privilege disallowed
- needs approval when context approval is absent/unavailable on declarative processing path
- approved managed-terminal path returns completed outcome with reviewed fields

Use a fake terminal backend, as terminal.create tests already do.

### 8. Docs/changelog

- Update CHANGELOG `[Unreleased]`.
- Add implementation notes to this plan or a host action design doc.
- Comment on issue #113 with validation evidence after release or merge.

## Cut line

MVP does not:

- add `omegon package`;
- add slash commands;
- execute package managers directly in extension process;
- support arbitrary commands/env/cwd;
- support system scope;
- wait for terminal package-manager completion;
- integrate persistent config for host action policy.

## Release acceptance

- HostAction executor registry supports `package.install@1`.
- Package install params are typed and validated.
- Policy rejects unknown/mismatched providers/tools/packages/scopes.
- Approval path blocks mutation without operator approval.
- Approved path creates a managed terminal with host-derived command.
- Tests cover denied, invalid, approval-required, and managed-terminal paths.
- `cargo test -p omegon host_actions -- --nocapture` passes.
- `cargo test -p omegon command_palette -- --nocapture` passes to prove no slash surface was added.
- `cargo clippy -p omegon --all-targets -- -D warnings` passes.
