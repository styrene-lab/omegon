# Configuration

## Project profile

Settings persist per-project in `.omegon/profile.json`. The profile is automatically updated when you change settings via slash commands.

```json
{
  "model": "anthropic:claude-opus-4-6",
  "thinking": "high",
  "max_turns": 50
}
```

### Settings

| Setting | Default | Description |
|---------|---------|-------------|
| `model` | `anthropic:claude-sonnet-4-6` | Provider and model ID |
| `thinking` | `medium` | Reasoning level: off, low, medium, high |
| `max_turns` | `50` | Maximum turns before forced stop |

## AGENTS.md directives

### Global directives

Create `~/.pi/agent/AGENTS.md` for directives that apply to all sessions:

```markdown
# Global Operator Directives

## Attribution Policy
NO Co-Authored-By trailers for AI systems in git commits.

## Interaction Model
Ask the operator to make decisions, not perform menial tasks.
```

### Project directives

Create `AGENTS.md` in the project root for project-specific instructions:

```markdown
# Project Directives

## Contributing
This repo uses trunk-based development on main.
Conventional commits required.

## Testing
All changes must include tests.
Use `cargo test --all` before committing.
```

## Environment variables

| Variable | Description |
|----------|-------------|
| `ANTHROPIC_API_KEY` | Anthropic API key (alternative to OAuth login) |
| `OPENAI_API_KEY` | OpenAI API key |
| `ANTHROPIC_BASE_URL` | Override Anthropic API endpoint |
| `OPENAI_BASE_URL` | Override OpenAI API endpoint |
| `LOCAL_INFERENCE_URL` | Ollama endpoint (default: `http://localhost:11434`) |
| `RUST_LOG` | Log level override (e.g. `debug`, `omegon=trace`) |

## Project convention detection

Omegon auto-detects project type from config files and adjusts guidance:

| File | Convention |
|------|-----------|
| `Cargo.toml` | Rust — cargo test, clippy, rustfmt |
| `tsconfig.json` | TypeScript — tsc, vitest/jest |
| `pyproject.toml` | Python — pytest, ruff, mypy |
| `go.mod` | Go — go test, go vet |
| `package.json` | Node.js — npm test |

## Plugins

Omegon supports TOML-manifest plugins that register HTTP-backed tools and context providers.

Create `.omegon/plugins/<name>.toml`:

```toml
[plugin]
name = "my-tool"
version = "0.1.0"

[[tools]]
name = "my_custom_tool"
description = "Does something useful"
endpoint = "http://localhost:8080/tool"

[tools.parameters]
type = "object"
properties.query = { type = "string", description = "Query to process" }
required = ["query"]
```
