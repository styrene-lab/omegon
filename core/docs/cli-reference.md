# CLI Reference

## Usage

```
omegon [OPTIONS] [COMMAND]
```

Without a subcommand, launches the interactive TUI agent session.

## Global options

| Flag | Default | Description |
|------|---------|-------------|
| `-c, --cwd <PATH>` | `.` | Working directory |
| `--bridge <PATH>` | — | Path to LLM bridge script (Node.js fallback) |
| `--node <PATH>` | `node` | Node.js binary path |
| `-m, --model <MODEL>` | `anthropic:claude-sonnet-4-6` | Model identifier (provider:model) |
| `-p, --prompt <TEXT>` | — | Prompt for headless mode |
| `--prompt-file <PATH>` | — | Read prompt from file |
| `--max-turns <N>` | `50` | Maximum turns (0 = unlimited) |
| `--max-retries <N>` | `3` | Retries on transient LLM errors |
| `--resume [ID]` | — | Resume a session (latest or by prefix) |
| `--no-session` | `false` | Disable session auto-save |
| `--no-splash` | `false` | Skip splash screen animation |
| `--log-level <LEVEL>` | `info` | Log level: error, warn, info, debug, trace |
| `--log-file <PATH>` | — | Write logs to file |
| `--version` | — | Print version |

## Subcommands

### `interactive`

Launch the interactive TUI session (same as bare `omegon`).

### `login [PROVIDER]`

Authenticate with an LLM provider via OAuth.

```bash
omegon login              # Anthropic (default)
omegon login openai       # OpenAI
```

### `migrate [SOURCE]`

Import settings from another CLI agent tool.

```bash
omegon migrate            # auto-detect all tools
omegon migrate claude-code
omegon migrate aider
```

Supported: claude-code, pi, codex, cursor, aider, continue, copilot, windsurf.

### `cleave`

Run a parallel task decomposition.

```bash
omegon cleave \
  --plan plan.json \
  --directive "implement feature X" \
  --workspace /tmp/cleave-work \
  --max-parallel 4 \
  --timeout 900 \
  --idle-timeout 180 \
  --max-turns 50
```

| Flag | Default | Description |
|------|---------|-------------|
| `--plan <PATH>` | — | Path to plan JSON file |
| `--directive <TEXT>` | — | Task description |
| `--workspace <PATH>` | — | Worktree and state directory |
| `--max-parallel <N>` | `4` | Maximum parallel children |
| `--timeout <SECS>` | `900` | Per-child wall-clock timeout |
| `--idle-timeout <SECS>` | `180` | Per-child idle timeout |
| `--max-turns <N>` | `50` | Max turns per child |

## Slash commands (interactive)

| Command | Description |
|---------|-------------|
| `/model [name]` | View or switch model |
| `/think [level]` | Set reasoning: off, low, medium, high |
| `/context` | Toggle 200k ↔ 1M context |
| `/sessions` | List saved sessions |
| `/compact` | Trigger context compaction |
| `/clear` | Clear display |
| `/detail` | Toggle compact/detailed tool cards |
| `/migrate [source]` | Import settings from other tools |
| `/web` | Launch web dashboard |
| `/help` | Show all commands |
| `/quit` | Exit |
