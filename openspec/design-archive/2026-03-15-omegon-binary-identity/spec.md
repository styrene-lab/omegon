+++
id = "fb8c8b1a-2b22-470e-bfc9-ee290bf04ea2"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omegon binary identity — eliminate direct product exposure as `pi` — Design Spec

## Scenarios

### Scenario 1 — Canonical launch path is Omegon-owned

Given a fresh Omegon install
When the operator follows the documented happy path
Then they launch the tool with `omegon`
And the process enters through an Omegon-owned executable boundary
And no required operator step depends on invoking `pi` directly

### Scenario 2 — Update and restart handoff stay inside the Omegon boundary

Given an installed or dev-mode Omegon environment
When the operator runs `/update`
Then the flow verifies active executable/runtime ownership through Omegon-controlled checks
And the completion message instructs the operator to restart or relaunch `omegon`
And the flow does not present direct `pi` invocation as the required next step

### Scenario 3 — Legacy `pi` users do not bypass lifecycle control

Given a migration period where `pi` still exists as a compatibility alias
When an operator invokes `pi`
Then the invocation resolves to the same Omegon-owned entrypoint and lifecycle checks as `omegon`
And it does not create an alternate startup or update path that bypasses Omegon policy

## Falsifiability

- Fail if README, install docs, or update/restart prompts instruct the operator to run `pi` as the primary command.
- Fail if package/bin wiring allows `pi` to reach a different entrypoint than `omegon`.
- Fail if update/install verification only proves the `pi` alias without proving Omegon-owned executable control.
- Fail if the migration removes `pi` without a deliberate compatibility or deprecation strategy for existing users.

## Constraints

- Preserve the singular-package runtime contract already shipped: Omegon remains the owning runtime root in both vendor/dev and installed/npm modes.
- Treat direct `pi` usage as compatibility debt to be contained, not as the supported lifecycle entrypoint.
- Keep restart semantics explicit; do not replace the restart handoff with in-process hot swap.
