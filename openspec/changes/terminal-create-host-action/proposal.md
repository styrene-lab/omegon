# terminal.create@1 HostAction Executor

## Intent

Implement the first concrete HostAction executor: `terminal.create@1`, allowing extensions such as `omegon-reader` to request a host-managed interactive terminal session running an argv command while Omegon owns policy, lifecycle, and terminal substrate.

## Scope

- Validate `terminal.create@1` params before spawn.
- Enforce manifest command/cwd/env policy.
- Keep argv-only command launch; no shell-string variant.
- Reuse existing PTY-backed terminal session machinery where possible.
- Return typed HostAction outcomes with stable terminal result data and warnings/degradations.
- Return `unsupported` or `denied` rather than panicking in unsupported/headless contexts.

## Non-goals

- Bookokrat-specific argument construction.
- MCP metadata mapping (#77).
- Rich TUI/ACP action cards beyond existing outcome details.
