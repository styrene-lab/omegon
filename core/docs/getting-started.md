# Getting Started

## Installation

### One-liner (recommended)

```bash
curl -fsSL https://omegon.styrene.dev/install.sh | sh
```

This installs the `omegon` binary to `/usr/local/bin` (or `$INSTALL_DIR`).

### Manual download

Download a release from [GitHub Releases](https://github.com/styrene-lab/omegon-core/releases) and place the binary on your `$PATH`.

### From source

```bash
git clone https://github.com/styrene-lab/omegon-core.git
cd omegon-core
cargo build --release
cp target/release/omegon ~/.local/bin/
```

## Authentication

Omegon needs an API key from at least one LLM provider.

### Anthropic (default)

```bash
# OAuth login (recommended — no API key needed)
omegon login

# Or set an API key directly
export ANTHROPIC_API_KEY=sk-ant-...
```

### OpenAI

```bash
omegon login openai

# Or set an API key
export OPENAI_API_KEY=sk-...
```

## First session

```bash
cd your-project
omegon
```

This launches the interactive TUI. Type a prompt and press Enter.

### Headless mode

```bash
omegon --prompt "add error handling to src/main.rs"
```

### Key commands

| Command | Description |
|---------|-------------|
| `/model` | Switch LLM provider/model |
| `/think` | Adjust reasoning level (off/low/medium/high) |
| `/context` | Toggle 200k ↔ 1M context window |
| `/sessions` | List saved sessions |
| `/help` | Show all commands |
| `Ctrl+C` | Cancel current operation / quit |
| `Ctrl+R` | Search command history |

## Configuration

Omegon auto-detects project conventions from config files (Cargo.toml, tsconfig.json, pyproject.toml, go.mod) and adjusts its behavior accordingly.

### Project profile

Settings persist per-project in `.omegon/profile.json`:

```bash
# These are saved automatically when you use /model or /think
omegon --model anthropic:claude-opus-4-6
```

### Global directives

Create `~/.pi/agent/AGENTS.md` with directives that apply to all sessions across all projects.

### Project directives

Create `AGENTS.md` in your project root for project-specific instructions.
