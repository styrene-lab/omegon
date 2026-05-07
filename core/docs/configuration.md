+++
id = "92b1c76e-099f-48ee-b928-51ec15faacea"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Configuration

## Project profile

Settings persist per-project in `<repo-root>/.omegon/profile.json` (resolved at the repository root even when you launch Omegon from a nested subdirectory). The profile is automatically updated when you change settings via slash commands. If no project profile exists, Omegon falls back to the user-level profile at `~/.omegon/profile.json`.

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

Create `~/.config/omegon/AGENTS.md` for directives that apply to all sessions:

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

## Codex Integration

Omegon integrates with [Codex](https://codex.styrene.io) vaults for knowledge management, design tree visualization, and bidirectional memory sync.

### Auto-Detection

If a `.codex/config.toml` exists at the project root, Omegon automatically enables vault integration with default settings.

### Explicit Configuration

Create `.codex/omegon-integration.toml` or `.omegon/codex.toml`:

```toml
enabled = true  # master switch

[vault]
path = "."  # vault root relative to project root

[memory]
materialize_on_session_end = true   # write facts to vault as markdown
import_on_session_start = true      # import Codex-authored facts
reinforce_references = true          # anchor facts referenced by notes
max_episodes = 20

[design_tree]
enabled = true
vault_subdir = "design"

[agent]
model = "anthropic:claude-sonnet-4-6"
posture = "fabricator"
```

### Memory Sync

On session end, Omegon:
1. Imports facts authored in Codex (`kind = "memory_fact"`)
2. Reinforces facts referenced by vault notes (`related_facts` in frontmatter)
3. Materializes all memory sections to `{vault}/ai/memory/{section}.md`
4. Writes session episodes to `{vault}/ai/memory/episodes/{date}.md`

Facts referenced by vault documents get their decay timer reset — they won't fade as long as the note exists.

### Design Tree Export

Design nodes are exported to `{vault}/design/{node-id}.md` with TOML frontmatter compatible with Codex's entity system. Codex displays them in the knowledge graph with status icons and dependency edges.

## Configuration Schemas (Pkl)

All configuration surfaces are validated by [Pkl](https://pkl-lang.org/) schemas in the `pkl/` directory:

| Schema | Validates | File |
|--------|-----------|------|
| `AgentManifest.pkl` | Catalog agent bundles | `catalog/*/agent.pkl` |
| `PluginManifest.pkl` | Plugin manifests | `plugin.toml` |
| `TriggerConfig.pkl` | Daemon triggers | `.omegon/triggers/*.toml` |
| `RouteMatrix.pkl` | Model routing matrix | `data/route-matrix.json` |
| `SkillManifest.pkl` | Skill frontmatter | `skills/*/SKILL.md` |
| `Profile.pkl` | User profile | `profile.json` |
| `CodexIntegration.pkl` | Codex vault config | `.codex/omegon-integration.toml` |
| `McpConfig.pkl` | MCP server declarations | `.omegon/mcp.toml` |
| `ExtensionManifest.pkl` | Extension manifests | `manifest.toml` |
| `TaskSpec.pkl` | Bounded task specs | `task.toml` for `omegon run` |

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

## Kubernetes Workloads

Omegon runs as any k8s workload type. The agent manifest (ConfigMap) declares identity; the k8s resource type declares lifecycle.

| Resource | Command | Use Case |
|----------|---------|----------|
| Job | `omegon run task.toml` | One-shot bounded tasks |
| CronJob | `omegon run task.toml` | Recurring bounded tasks |
| Deployment | `omegon serve` | Long-lived agent with vox/triggers |
| StatefulSet | `omegon serve` | Agent with persistent workspace |
| DaemonSet | `omegon serve` | Per-node agent |

### Health Probes

```yaml
livenessProbe:
  httpGet:
    path: /api/healthz
    port: 7842
  initialDelaySeconds: 5

readinessProbe:
  httpGet:
    path: /api/readyz
    port: 7842
  initialDelaySeconds: 10
```

### Example Job

```yaml
apiVersion: batch/v1
kind: Job
spec:
  template:
    spec:
      containers:
      - name: omegon
        image: ghcr.io/styrene-labs/omegon:0.16.0
        command: ["omegon", "run"]
        args: ["--prompt", "Review open PRs", "--output", "/output/result.json"]
        env:
        - name: ANTHROPIC_API_KEY
          valueFrom:
            secretKeyRef:
              name: llm-credentials
              key: anthropic-api-key
      restartPolicy: Never
```

See `docs/design/k8s-workload-matrix.md` for the full implementation status matrix.
