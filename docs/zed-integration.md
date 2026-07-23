+++
id = "0571cf78-6ba8-461b-9aa6-805d0cb05162"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Zed Integration

Omegon integrates with Zed via the [Agent Client Protocol (ACP)](https://agentclientprotocol.com/). Zed's agent panel acts as the ACP client; omegon runs as the ACP server.

## Setup

Add omegon to your Zed settings (`~/.config/zed/settings.json`):

```json
{
  "agent_servers": {
    "Omegon": {
      "type": "custom",
      "command": "omegon",
      "args": ["acp"],
      "env": {}
    }
  }
}
```

If omegon is not on your PATH, use the full path:

```json
"command": "/Users/you/.local/bin/omegon"
```

> **Tip:** Run `/editor zed` in omegon's TUI to auto-generate this config with your exact binary path.

Then open Zed, click the **+** button in the Agent Panel, and select **Omegon**.

## Modes

Omegon exposes four modes in Zed's Agent Panel:

| Mode | Posture | Description |
|------|---------|-------------|
| **Code** | Fabricator | Balanced coding agent (default) |
| **Architect** | Architect | Plans, delegates to local models, reviews |
| **Ask** | Explorator | Read-only exploration, lean |
| **Agent** | Devastator | Maximum force, deep reasoning |

Switch modes via the mode selector in Zed's Agent Panel.

## Configuration Dropdowns

Four portable config dropdowns appear at the bottom of the Agent Panel:

- **Model** — LLM provider and model. Auto-detects local Ollama models alongside authenticated cloud-provider options.
- **Thinking Level** — off, minimal, low, medium, high. Controls extended thinking budget.
- **Profile** — applies a named project/user Omegon profile and refreshes all resulting controls.
- **Context Window** — compact, standard, extended, or massive requested context policy.

Posture is represented by Zed's first-class **Mode** selector rather than a duplicate config dropdown. ACP also assigns semantic categories and descriptions to these controls so clients can place model and thinking selectors natively.

## Host Delegation

When Zed advertises file system and terminal capabilities (which it does by default), omegon delegates operations to Zed instead of executing locally:

- **File reads/writes** appear in Zed's diff view with checkpoint/restore
- **Terminal commands** run in Zed's terminal panel
- **Permission prompts** appear in Zed's approval dialog

If Zed does not advertise a capability, omegon falls back to direct local execution.

## Slash Commands

Type these in the Zed chat input:

| Command | Description |
|---------|-------------|
| `/model <provider:model>` | Switch LLM model |
| `/thinking <level>` | Set thinking level |
| `/posture <name>` | Set behavioral posture |
| `/skills` | Manage skills (list, get, create, delete) |
| `/extension` | Manage extensions (list, install, enable, search) |
| `/persona` | Manage personas (list, create, switch) |
| `/catalog` | Browse agent catalog (list, install, remove) |
| `/secrets` | View configured secret names and recipes; values are never printed |
| `/status` | Session status |
| `/help` | List all commands |

## Settings Management (RPC)

Full CRUD is available via ACP ext_method RPC calls (prefix with `_`):

- `skills/list`, `skills/get`, `skills/create`, `skills/update`, `skills/delete`
- `extensions/list`, `extensions/get`, `extensions/install`, `extensions/remove`, `extensions/enable`, `extensions/disable`, `extensions/search`
- `personas/list`, `personas/get`, `personas/create`, `personas/update`, `personas/delete`
- `catalog/list`, `catalog/get`, `catalog/install`, `catalog/remove`
- `secrets/list`, `secrets/set_value`, `secrets/set_recipe`, `secrets/check`, `secrets/delete`

Secret values should be sent only through operator UI fields intended for secrets. `secrets/check` reports whether a value resolves; it does not return the value.

## MCP Server Forwarding

Zed can provide MCP servers via the `mcp_servers` field in the session request. Omegon connects them to its tool bus, making their tools available alongside built-in tools.

## Concurrent Use

Running omegon in Zed while also running it in the TUI (or another editor) in the same repository is safe. Each instance gets its own workspace lease under `.omegon/runtime/{mode}-{pid}/`. Shared config (profile, extension state) uses advisory file locking.

## Model Override

Override the default model at launch:

```json
"args": ["acp", "--model", "ollama:qwen3:32b"]
```

## Debugging

Enable debug logging:

```json
"args": ["acp", "--log-file", "/tmp/omegon-zed.log", "--log-level", "debug"]
```

Then `tail -f /tmp/omegon-zed.log` in a terminal.

## How It Works

1. Zed spawns `omegon acp` as a subprocess
2. ACP handshake via JSON-RPC over stdin/stdout
3. Omegon advertises capabilities (image support, session list/close)
4. Zed creates a session — omegon spins up a worker thread with full agent loop
5. File I/O and terminal delegated to Zed when capabilities are advertised
6. Plan updates stream as decomposition progresses
7. Tool output streams progressively during execution
8. Session title derived from first prompt
