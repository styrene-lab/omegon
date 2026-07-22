+++
id = "e4f7a912-3c5d-4b8e-a1d6-9e2f8c3b7a54"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# VS Code Integration

Omegon integrates with VS Code via the [vscode-acp](https://github.com/formulahendry/vscode-acp) extension.

## Setup

### 1. Install the ACP extension

The extension is not on the VS Code marketplace. Install from source:

```bash
git clone https://github.com/formulahendry/vscode-acp.git /tmp/vscode-acp
cd /tmp/vscode-acp
npm install
npx vsce package
code --install-extension acp-client-*.vsix
```

### 2. Configure omegon as an agent

Add to your VS Code settings (`Cmd+,` → Open Settings JSON):

```json
{
  "acp.agents": {
    "Omegon": {
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

### 3. Open the ACP panel

The extension adds its own panel to the activity bar (left sidebar). Look for the **ACP Client** icon, or run `Cmd+Shift+P` → **"ACP: Open Chat Panel"**.

> **Important:** The ACP chat panel is separate from VS Code's built-in Chat panel. Omegon will not appear in the built-in Chat model dropdown — use the ACP panel instead.

## Using Omegon in VS Code

Once connected (status bar shows "ACP: Omegon Agent"):

- **Send prompts** in the ACP chat panel
- **Switch modes** via `Cmd+Shift+P` → "ACP: Set Agent Mode" (Code, Architect, Ask, Agent)
- **Switch models and settings** through ACP session selectors when rendered by the installed extension; `/model`, `/thinking`, `/profile`, and `/context` remain portable fallbacks
- **Attach files** via `Cmd+Shift+P` → "ACP: Attach File to Prompt"
- **View protocol traffic** via `Cmd+Shift+P` → "ACP: Show Protocol Traffic"

## Portable Session Controls

Omegon publishes semantically categorized ACP selectors for model and thinking level, plus Omegon-specific profile and context-window selectors. Applying a profile refreshes the complete selector set because a profile may change model, thinking, and context together. Posture is represented through ACP's first-class mode selector rather than duplicated as a settings dropdown.

The community extension may not render every optional ACP selector in every release. The corresponding slash commands remain available as a compatibility fallback.

## Secrets

Omegon exposes generic operator secret methods over ACP:

- `secrets/list`
- `secrets/set_value`
- `secrets/set_recipe`
- `secrets/check`
- `secrets/delete`

Use `secrets/set_value` only from UI fields intended for secret/password input. `secrets/check` reports whether a value resolves and never returns the value. Extension-scoped secret setup remains available through extension methods, but operator-owned values such as `VAULT_ROOT_TOKEN` should use `secrets/*`.

## Extension Settings

| Setting | Default | Description |
|---------|---------|-------------|
| `acp.agents` | (built-in agents) | Agent configurations — add Omegon here |
| `acp.autoApprovePermissions` | `"ask"` | Permission handling: `"ask"`, `"always"`, `"never"` |
| `acp.defaultWorkingDirectory` | (workspace root) | Working directory for agent sessions |
| `acp.logTraffic` | `true` | Log JSON-RPC traffic for debugging |

## Modes

| Mode | Description |
|------|-------------|
| **Code** | Balanced coding — direct execution, delegates larger tasks |
| **Architect** | Orchestrator — plans, delegates to local models, reviews |
| **Ask** | Read-only exploration — lean, no file mutations |
| **Agent** | Maximum force — deep reasoning, large context |

## Debugging

Enable traffic logging in settings:

```json
{
  "acp.logTraffic": true
}
```

View logs via `Cmd+Shift+P` → "ACP: Show Log" or "ACP: Show Protocol Traffic".

Add omegon-side logging:

```json
"args": ["acp", "--log-file", "/tmp/omegon-vscode.log", "--log-level", "debug"]
```

## Concurrent Use

Running omegon in VS Code while also running it in the TUI or Zed in the same repository is safe. Each instance isolates its runtime state. Shared configuration (skills, extensions, secrets) is unified across all instances.

## Limitations

- VS Code's built-in Chat panel does not show ACP agents — use the ACP extension's own panel
- File delegation requires the ACP extension to advertise `fs.readTextFile`/`fs.writeTextFile` capabilities (check extension version)
- The ACP extension is community-maintained; check [formulahendry/vscode-acp](https://github.com/formulahendry/vscode-acp) for updates
