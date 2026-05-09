+++
id = "b8d23f71-5e9a-4c12-8f3d-6a1b4d7e9c82"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Flynt Integration

Omegon is the default agent backend for [Flynt](https://flynt.styrene.io), Styrene's native desktop IDE. Flynt communicates with omegon via ACP (Agent Client Protocol) over stdin/stdout.

## Binary Resolution

Flynt locates the omegon binary through this resolution chain:

1. `omegon_bin_override` in Flynt's local runtime config (explicit path)
2. `OMEGON_BIN` environment variable
3. Channel-matched version in `~/.omegon/versions/` (versioned install layout)
4. `omegon` on `PATH` or well-known locations (`/usr/local/bin`, `~/.local/bin`)

To pin a specific version:

```bash
export OMEGON_BIN=$HOME/.omegon/versions/v0.19.2/omegon
```

## Configuration

Flynt stores ACP preferences in `.flynt/operator-settings.json` at the project root:

```json
{
  "acpConfig": {
    "thinking": "low",
    "posture": "fabricator",
    "model": "anthropic:claude-opus-4-7"
  }
}
```

These are applied as defaults when Flynt spawns omegon. The user can override them at runtime via the config dropdowns in Flynt's agent panel.

## Session Modes

Flynt's agent panel exposes omegon's four session modes:

| Mode | Posture | Description |
|------|---------|-------------|
| **Code** | Fabricator | Balanced coding — direct execution (default) |
| **Architect** | Architect | Plans, delegates to local models, reviews |
| **Ask** | Explorator | Read-only exploration, lean |
| **Agent** | Devastator | Maximum force, deep reasoning |

## Host Delegation

Flynt does **not** currently advertise file system or terminal capabilities. This means omegon executes all file reads, writes, and shell commands locally — the same behavior as the TUI.

When Flynt adds capability advertisement (planned), omegon will automatically delegate:
- File reads/writes to Flynt's diff view
- Terminal commands to Flynt's terminal panel
- Permission prompts to Flynt's approval dialog

No omegon-side changes are needed — the delegation is capability-driven.

## Settings Surface

Flynt has access to the full ACP settings surface via ext_method RPC:

**Skills:** `skills/list`, `skills/get`, `skills/create`, `skills/update`, `skills/delete`, `skills/install`

**Extensions:** `extensions/list`, `extensions/get`, `extensions/install`, `extensions/remove`, `extensions/update`, `extensions/enable`, `extensions/disable`, `extensions/search`, `extensions/config_get`, `extensions/config_set`, `extensions/secret_set`, `extensions/secret_delete`

**Personas:** `personas/list`, `personas/get`, `personas/create`, `personas/update`, `personas/delete`

**Catalog:** `catalog/list`, `catalog/get`, `catalog/install`, `catalog/remove`

**Control:** `control/stats`, `control/persona_list`, `control/persona_switch`, `control/context_status`, `control/secrets_view`, `control/provider_status`, and 15+ more operational commands.

## Plan Updates

When omegon decomposes work (cleave, phased skills), Flynt receives `SessionUpdate::Plan` notifications with live progress for each child task. These render as a task list in Flynt's agent panel.

## Session Titles

Omegon derives a session title from the first user prompt and sends it as a `SessionInfoUpdate`. Flynt displays this in the session list instead of a generic "Session N".

## Concurrent Use

Running Flynt and the TUI simultaneously in the same repo is safe. Each instance gets its own workspace lease under `.omegon/runtime/{mode}-{pid}/`. Shared state (profile, skills, extensions, secrets) uses advisory file locking to prevent corruption.

## Debugging

Launch Flynt with omegon tracing enabled:

```bash
RUST_LOG=info open dist/Flynt.app --stderr /tmp/flynt-trace.log
```

Then monitor:

```bash
tail -f /tmp/flynt-trace.log | grep omegon
```

For omegon-specific debug output, set the `OMEGON_BIN` override with logging args:

```bash
# Create a wrapper script
cat > /tmp/omegon-debug.sh << 'EOF'
#!/bin/sh
exec omegon acp --log-file /tmp/omegon-flynt.log --log-level debug "$@"
EOF
chmod +x /tmp/omegon-debug.sh
export OMEGON_BIN=/tmp/omegon-debug.sh
open dist/Flynt.app
```
