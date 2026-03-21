# Unified auth surface — single /login command, agent-callable, all backends

## Intent

Auth is fragmented across 6 mechanisms with 3 different UX paths: CLI-only for LLM providers (`omegon login`), TUI-only for Vault (`/vault login`), and nothing for MCP remote OAuth or secrets store unlock. The operator has no single place to see what's authenticated, what's expired, and what needs attention.\n\nGoal: one `/auth` slash command + one `auth` agent tool + one `omegon auth` CLI subcommand that covers all backends uniformly.

See [design doc](../../../docs/unified-auth-surface.md).
