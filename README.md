# Omegon

**Native AI agent harness — for agents, by agents.**

Single binary. Zero dependencies. Full autonomy.

Omegon is a Rust-native AI coding agent that runs entirely in your terminal. It connects to Anthropic and OpenAI models, manages project memory across sessions, decomposes complex tasks into parallel workers, and tracks design decisions in a persistent knowledge graph — all from a ~19MB binary with no runtime dependencies.

[![omegon.styrene.dev](https://img.shields.io/badge/docs-omegon.styrene.dev-2ab4c8)](https://omegon.styrene.dev)

---

## Install

```sh
curl -fsSL https://omegon.styrene.dev/install.sh | sh
```

This downloads the correct binary for your platform and adds it to your PATH.

### Supported Platforms

| Platform | Architecture |
|----------|-------------|
| macOS    | arm64 (Apple Silicon) |
| macOS    | x86_64 (Intel) |
| Linux    | x86_64 |
| Linux    | arm64 / aarch64 |

### Version Pinning

Pin a project to a specific Omegon version by creating an `.omegon-version` file:

```sh
echo "0.14.1" > .omegon-version
```

Omegon detects this file from the current directory upward and warns if the active version doesn't match.

---

## Quick Start

```sh
# Launch Omegon in your project directory
omegon

# Set your API key on first run (or export ANTHROPIC_API_KEY)
export ANTHROPIC_API_KEY=sk-ant-...
```

Omegon opens a terminal UI where you interact with the agent through natural language. It can read and edit files, run commands, search the web, manage git, and orchestrate multi-step workflows.

---

## Key Features

### Three-Axis Model Routing

Omegon routes every request through three independent axes:

- **Capability tier** — `local` → `retribution` → `victory` → `gloriana` (cheapest to most capable)
- **Thinking level** — `off` / `minimal` / `low` / `medium` / `high` (controls reasoning budget)
- **Context class** — routes based on task type and provider availability

The agent automatically adjusts tier and thinking level based on task complexity. You can also set them manually with `/set-tier` and `/set-thinking`.

### Project Memory

Facts persist across sessions in a local SQLite database:

- **Architecture** — system structure, component relationships
- **Decisions** — choices made and their rationale
- **Constraints** — hard limits and invariants
- **Known Issues** — tracked problems and workarounds
- **Patterns & Conventions** — coding standards and project norms

Memory supports semantic search (vector embeddings), episodic recall (session narratives), and a knowledge graph with typed relationships between facts. Stale facts decay automatically.

### Design Tree

A persistent knowledge graph for tracking design exploration:

- Create nodes for features, bugs, tasks, and epics
- Track status: `seed` → `exploring` → `resolved` → `decided` → `implementing` → `implemented`
- Record research findings, open questions, and design decisions
- Branch child nodes from open questions
- Declare dependencies between nodes
- Query for `ready` (decided + deps met) or `blocked` nodes

### Parallel Task Execution (Cleave)

Decompose complex work into parallel subtasks:

1. **Assess** — score task complexity against patterns
2. **Plan** — split into independent children with file scopes
3. **Execute** — each child runs in an isolated git worktree on its own branch
4. **Merge** — automatic conflict detection and branch integration

```
/cleave "add authentication with JWT tokens, rate limiting, and audit logging"
```

### Spec-Driven Development (OpenSpec)

Every non-trivial change follows a lifecycle:

```
propose → spec → plan → implement → verify → archive
```

Specs use **Given/When/Then** scenarios to define what must be true *before* code is written. After implementation, `/assess spec` validates the code against the scenarios.

### Code Assessment

```
/assess cleave     # Adversarial review of recent commits with auto-fix
/assess diff       # Review changes since last commit
/assess spec       # Validate implementation against OpenSpec scenarios
/assess design     # Evaluate design node readiness
```

### CIC Instrument Panel

Submarine-inspired real-time system state visualization in the terminal footer:

- **Split-panel layout** — Engine/memory state (left 40%) + system telemetry (right 60%)
- **Four simultaneous fractal instruments** — Perlin sonar (context health), Lissajous radar (tool activity), Plasma thermal (thinking state), CA waterfall (memory operations with per-mind columns)
- **Unified navy→teal→amber color ramp** — Perceptual CIE L* color progression from idle to maximum intensity
- **Focus mode toggle** — Hide instruments for full-height conversation when needed
- **Ambient awareness** — Pattern recognition across all four instruments provides situational awareness without text reading

### Skills

Markdown-based skill definitions that inject domain expertise into the agent context:

| Skill | Domain |
|-------|--------|
| `git` | Conventional commits, semantic versioning, branch naming |
| `typescript` | Strict typing, async patterns, node:test |
| `rust` | Cargo, clippy, Zellij plugin development |
| `python` | pyproject.toml, pytest, ruff, mypy |
| `openspec` | Spec lifecycle, Given/When/Then scenarios |
| `cleave` | Task decomposition, parallel execution |
| `security` | Input escaping, injection prevention, secrets |
| `oci` | Container builds, multi-arch, registries |
| `style` | Alpharius color system, visual consistency |
| `vault` | Obsidian-compatible markdown conventions |

### Built-in Tools

Omegon exposes tools to the agent model:

| Tool | Purpose |
|------|---------|
| `read` / `write` / `edit` | File operations |
| `bash` | Shell command execution |
| `web_search` | Multi-provider search (Brave, Tavily, Serper) |
| `memory_*` | Store, recall, query, connect, archive facts |
| `design_tree` / `design_tree_update` | Query and mutate the design graph |
| `cleave_assess` / `cleave_run` | Task decomposition and parallel execution |
| `openspec_manage` | Spec lifecycle management |
| `chronos` | Authoritative date/time (no hallucinated dates) |
| `set_model_tier` / `set_thinking_level` | Runtime model routing control |
| `ask_local_model` | Delegate to local Ollama for zero-cost inference |

---

## Project Structure

```
core/                       Rust workspace
  crates/
    omegon/                 Main binary — TUI, tools, agent loop
    omegon-git/             Git operations
    omegon-memory/          Memory system (SQLite, vectors, decay)
    omegon-secrets/         Secret resolution, redaction, tool guards
    omegon-traits/          Shared trait definitions
  site/                     omegon.styrene.dev landing page
design/                     Design exploration tree (markdown nodes)
docs/                       Architecture docs and design decisions
graphics/                   Logo and icon assets
openspec/                   Spec-driven development artifacts
prompts/                    Prompt templates
skills/                     Markdown skill definitions
themes/                     Alpharius terminal theme
```

---

## Building from Source

```sh
cd core
cargo build --release
```

The release binary is at `core/target/release/omegon`.

For faster iteration during development (thin LTO, ~90% of release performance):

```sh
cargo build --profile dev-release -p omegon
```

---

## Configuration

Omegon stores mutable state under `~/.pi/agent/`:

| Path | Contents |
|------|----------|
| `~/.pi/agent/auth.json` | API keys and provider credentials |
| `~/.pi/agent/settings.json` | User preferences |
| `~/.pi/agent/AGENTS.md` | Global operator directives (apply to all projects) |

Project-level configuration lives in the repository:

| Path | Contents |
|------|----------|
| `AGENTS.md` | Project-specific agent directives |
| `.omegon-version` | Pinned Omegon version |
| `.pi/memory/facts.jsonl` | Project memory (tracked in git) |

---

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `ANTHROPIC_API_KEY` | Anthropic API key (Claude models) |
| `OPENAI_API_KEY` | OpenAI API key |
| `BRAVE_API_KEY` | Brave Search API |
| `TAVILY_API_KEY` | Tavily Search API |
| `SERPER_API_KEY` | Serper (Google) Search API |

---

## Legacy

The original TypeScript/pi-based harness is archived at [omegon-pi](https://github.com/styrene-lab/omegon-pi). The Rust native rewrite is the active development target.

## License

[BSL 1.1](LICENSE) — © 2024–2026 Black Meridian, LLC

BSL 1.1 means the source is fully visible and unrestricted for personal and production use. The one restriction: you cannot use Omegon to offer a competing hosted agent service. If that is not you, BSL 1.1 is functionally identical to MIT for your purposes.
