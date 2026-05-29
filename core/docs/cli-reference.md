+++
id = "4419783e-c84a-4c55-99ab-7ab582c4b219"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

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

### `omegon run`

Bounded headless task execution. Designed for k8s Jobs/CronJobs, CI pipelines, and scripted automation.

```
omegon run task.toml
omegon run --prompt "Review open PRs" --max-turns 10
omegon run task.toml --model anthropic:claude-opus-4-6
```

**Task spec format** (`task.toml`):
```toml
[task]
prompt = "Review open PRs and summarize blockers"

[bounds]
max_turns = 30
timeout_secs = 600

[agent]
model = "anthropic:claude-sonnet-4-6"

[output]
path = "/output/result.json"
```

**Options:**
| Flag | Description | Default |
|------|-------------|---------|
| `--prompt` | Inline task prompt | — |
| `--prompt-file` | Task prompt from file | — |
| `--output` | JSON result output path (default: stdout) | stdout |
| `--max-turns` | Maximum agent turns | 30 |
| `--timeout` | Wall-clock timeout (seconds) | 600 |
| `--token-budget` | Total token budget (input + output) | — |
| `--manifest` | Agent manifest (Pkl) | — |

**Exit codes:**
| Code | Meaning |
|------|---------|
| 0 | Completed successfully |
| 1 | Error |
| 2 | Upstream provider exhausted |
| 3 | Wall-clock timeout |

### Slash Commands (Interactive)

#### Session & Navigation
| Command | Description |
|---------|-------------|
| `/help` | Show available commands |
| `/exit` or `/quit` | Exit omegon |
| `/new` | Start a new session |
| `/sessions` | List saved sessions |
| `/stats` | Show token and turn statistics |
| `/status` | Show harness status |

#### Model & Provider
| Command | Description |
|---------|-------------|
| `/model [name]` | View or switch model |
| `/think [level]` | Set thinking level (off/minimal/low/medium/high) |
| `/auth status` | Show authentication status |

#### UI & Display
| Command | Description |
|---------|-------------|
| `/ui full` | Show dashboard + instruments + footer |
| `/ui lean` | Conversation-only mode (default) |
| `/ui show\|hide\|toggle <surface>` | Control dashboard/instruments/footer |
| `/ui detail <lean|compact|detailed|verbose>` | Set tool-card output density |
| `/auth login|logout <provider>` | Manage provider authentication |
| `/notes add <text>` / `/notes checkin` | Capture or triage pending notes |
| `/focus` | Enter focus mode (timeline navigation) |
| `/mouse on\|off` | Enable/disable mouse capture |

#### Context & Memory
| Command | Description |
|---------|-------------|
| `/context [status\|compact\|clear]` | Manage context window |
| `/memory` | Memory management |
| `/copy [raw\|plain\|session]` | Copy segment or full session to clipboard |

#### Lifecycle & Design
| Command | Description |
|---------|-------------|
| `/tree [list\|...]` | Design tree operations |
| `/notes add <text>` | Add investigation note |
| `/notes [clear]` | Show or clear notes |
| `/notes checkin` | Triage workflow |
| `/milestone` | Milestone tracking |

#### Workspace & Tools
| Command | Description |
|---------|-------------|
| `/workspace` | Workspace info and management |
| `/skills [list\|install]` | Manage bundled skills |
| `/plugin [list\|install\|remove]` | Manage plugins |
| `/secrets [list\|set\|get\|delete]` | Manage secrets |
| `/update` | Check for updates |

#### Agent Operations
| Command | Description |
|---------|-------------|
| `/delegate` | Delegate tasks to worker agents |
| `/cleave` | Parallel task decomposition |
| `/persona [name]` | Switch persona |

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| Up/Down | Scroll conversation |
| Ctrl+Up/Down | Recall previous prompts |
| Shift+Up/Down | Scroll conversation (3 lines) |
| PageUp/PageDown | Scroll conversation (20 lines) |
| Ctrl+R | Reverse history search |
| Ctrl+C | Cancel current turn / quit |
| Ctrl+Y | Copy selected segment to clipboard |
| Mouse wheel | Scroll conversation (works without mouse capture) |
| `/focus` | Enter focus mode (single segment view) |
| `c` (focus mode) | Copy focused segment to clipboard |
| Esc (focus mode) | Exit focus mode |
