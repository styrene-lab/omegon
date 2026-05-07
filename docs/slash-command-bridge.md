+++
id = "26ed668d-2ea6-46a8-a345-2a17606e20fa"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Slash Command Bridge

> Structured executor for slash commands — enables agent-callable commands with typed results, side-effect classification, and confirmation gating.

## What It Does

The SlashCommandBridge provides a structured interface between the agent tool layer (`execute_slash_command`) and Omegon's slash commands. Each bridged command declares:

- **`agentCallable`**: Whether the agent can invoke it (false for interactive-only commands like `/dash`)
- **`sideEffectClass`**: read, write, or destructive — enables confirmation gating
- **`requiresConfirmation`**: Whether the agent must pass `confirmed: true`
- **`structuredExecutor`**: Function returning typed `SlashCommandResult` with summary, effects, and human-readable text

Currently bridged: `/assess` (cleave/diff/spec subcommands), `/dash`, `/dashboard`, and select OpenSpec commands. Gap: `/opsx:*` commands are registered via `pi.registerCommand()` but not bridged — available via `openspec_manage` tool instead.

## Key Files

| File | Role |
|------|------|
| `extensions/lib/slash-command-bridge.ts` | Bridge singleton, `getSharedBridge()`, `buildSlashCommandResult()`, registration API |
| `extensions/cleave/bridge.ts` | `/assess` bridge registration |
| `extensions/dashboard/index.ts` | `/dash`, `/dashboard` bridge registration (interactive-only) |

## Design Decisions

- **Shared singleton bridge**: `getSharedBridge()` returns a module-scoped instance; all extensions register against the same bridge.
- **Side-effect classification**: Commands declare their effect class so the harness can gate destructive operations behind confirmation.
- **Interactive-only registration**: Commands like `/dash` register with `agentCallable: false` so the agent gets a structured refusal ("interactive only") instead of a silent "not registered" error.
- **`buildSlashCommandResult` helper**: Standardized result envelope with `ok`, `summary`, `humanText`, and `effects` — consistent across all bridged commands.

## Behavioral Contracts

See `openspec/baseline/harness/slash-command-bridge.md` and `openspec/baseline/harness/slash-commands.md` for Given/When/Then scenarios.

## Constraints & Known Limitations

- Only `/assess` is fully agent-callable via the bridge; `/opsx:*` commands use `openspec_manage` tool as a workaround
- The bridge is a Omegon concept — pi core's `execute_slash_command` tool delegates to it
- Commands not registered with the bridge return an "unknown command" error from `execute_slash_command`

## Related Subsystems

- [Cleave](cleave.md) — `/assess` bridged for code review and spec verification
- [OpenSpec](openspec.md) — `/opsx:*` commands available via tool but not bridge
- [Dashboard](dashboard.md) — `/dash` registered as interactive-only
