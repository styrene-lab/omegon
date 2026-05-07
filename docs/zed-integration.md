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

Omegon integrates with Zed via the [Agent Client Protocol (ACP)](https://agentclientprotocol.com/).

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

> **Tip:** Run `/editor zed` in omegon's TUI to get the config snippet with your exact binary path.

## Modes

Omegon exposes four modes in Zed's Agent Panel:

| Mode | Posture | Description |
|------|---------|-------------|
| **Code** | Fabricator | Balanced coding agent (default) |
| **Architect** | Architect | Plans, delegates to local models, reviews |
| **Ask** | Explorator | Read-only exploration, lean |
| **Agent** | Devastator | Maximum force, deep reasoning |

Switch modes in Zed's Agent Panel mode selector.

## Model Configuration

By default, omegon uses the model from your profile (`~/.omegon/profile.json`).
Override with:

```json
"args": ["acp", "--model", "ollama:qwen3:32b"]
```

## How It Works

1. Zed spawns `omegon acp` as a subprocess
2. Communication happens via JSON-RPC over stdin/stdout
3. Omegon runs its full agent loop (tools, memory, lifecycle) 
4. File reads/writes and shell commands execute directly on the filesystem
5. Sessions persist across editor restarts
