# pi-kit

A batteries-included extension package for the [pi coding agent](https://github.com/nicolecomputer/pi-coding-agent). Adds persistent project memory, local LLM inference, image generation, web search, task decomposition, and quality-of-life tools — all loadable with a single install.

```bash
pi install https://github.com/cwilson613/pi-kit
```

## Architecture

![pi-kit Architecture](docs/img/architecture.png)

pi-kit extends the pi agent with **20 extensions**, **6 skills**, and **4 prompt templates** — loaded automatically on session start.

## Extensions

### 🧠 Project Memory

Persistent, cross-session knowledge stored in SQLite. The agent accumulates architectural decisions, constraints, patterns, and known issues — and retrieves them semantically each session.

- **11 tools**: `memory_store`, `memory_recall`, `memory_query`, `memory_supersede`, `memory_archive`, `memory_connect`, `memory_compact`, `memory_episodes`, `memory_focus`, `memory_release`, `memory_search_archive`
- **Background extraction**: Auto-discovers facts from tool output without interrupting work
- **Episodic memory**: Generates session narratives at shutdown for "what happened last time" context
- **Git sync**: Exports to JSONL for version-controlled knowledge sharing across machines

![Memory Lifecycle](docs/img/memory-lifecycle.png)

### 🤖 Local Inference

Delegate sub-tasks to locally running LLMs via Ollama — zero API cost.

- Auto-discovers available models on session start
- Tools: `ask_local_model`, `list_local_models`
- Commands: `/local-models`, `/local-status`

### 🔌 Offline Driver

Switch the driving model from cloud to a local Ollama model when connectivity drops or for fully offline operation.

- Tool: `switch_to_offline_driver`
- Auto-selects best available model (Nemotron, Devstral, Qwen3)

### 🎨 Render

Generate images and diagrams directly in the terminal.

- **FLUX.1 image generation** via MLX on Apple Silicon — `generate_image_local`
- **D2 diagrams** rendered inline — `render_diagram`
- **Excalidraw** JSON-to-PNG rendering — `render_excalidraw`

### 🔍 Web Search

Multi-provider web search with deduplication.

- Providers: Brave, Tavily, Serper (Google)
- Modes: `quick` (single provider), `deep` (more results), `compare` (fan out to all)
- Tool: `web_search`

### 🪓 Cleave

Recursive task decomposition, code assessment, and OpenSpec lifecycle integration.

- **Tools**: `cleave_assess` (complexity evaluation), `cleave_run` (parallel dispatch in git worktrees)
- **Commands**: `/cleave <directive>`, `/assess cleave`, `/assess diff`, `/assess spec`, `/assess complexity`
- **OpenSpec integration**: When `openspec/` exists, uses `tasks.md` as the split plan, enriches child tasks with design.md decisions and spec acceptance criteria, writes back task completion, and guides through verify → archive
- **Session awareness**: Surfaces active OpenSpec changes with task progress on session start

### 💰 Model Budget

Switch model tiers to match task complexity and conserve API spend.

- Tool: `set_model_tier` — opus / sonnet / haiku
- Downgrade for routine edits, upgrade for architecture decisions

### 🔐 Secrets

Resolve secrets from environment variables, shell commands, or system keychains — without storing values.

- Declarative `@secret` annotations in extension headers
- Supports `env:`, `cmd:`, `keychain:` sources

### 🌐 MCP Bridge

Connect external MCP (Model Context Protocol) servers as pi tools.

- Bridges MCP tool schemas into pi's native tool registry
- Stdio transport for local MCP servers

### 🔧 Utilities

| Extension | Description |
|-----------|-------------|
| `chronos` | Authoritative date/time from system clock — eliminates AI date math errors |
| `whoami` | Check auth status across git, GitHub, AWS, k8s, OCI registries |
| `view` | Inline file viewer — images, PDFs, docs, syntax-highlighted code |
| `distill` | Context distillation for session handoff (`/distill`) |
| `session-log` | Append-only structured session tracking |
| `status-bar` | Severity-colored context gauge with memory usage and turn counter |
| `terminal-title` | Dynamic tab titles for multi-session workflows |
| `spinner-verbs` | Warhammer 40K-themed loading messages |
| `style` | Verdant design system reference (`/style`) |
| `defaults` | Auto-configures theme on first install |

## Skills

Skills provide specialized instructions the agent loads on-demand when a task matches.

| Skill | Description |
|-------|-------------|
| `cleave` | Task decomposition, code assessment, OpenSpec lifecycle integration |
| `git` | Conventional commits, semantic versioning, branch naming, changelogs |
| `oci` | Container and artifact best practices |
| `python` | Project setup, pytest, ruff, mypy, packaging, venv management |
| `rust` | Cargo, clippy, rustfmt, Zellij WASM plugin development |
| `style` | Verdant color system, typography, spacing — shared across all visual output |

## Prompt Templates

Pre-built prompts for common workflows:

- **assess** — Adversarial assessment of session work (see also `/assess` command)
- **cleave** — Task decomposition, assessment, and OpenSpec lifecycle
- **new-repo** — Scaffold a new repository
- **oci-login** — OCI registry authentication

## Requirements

- [pi coding agent](https://github.com/nicolecomputer/pi-coding-agent) (v1.0+)
- **Optional**: [Ollama](https://ollama.ai) — for local inference, offline mode, and semantic memory search
- **Optional**: [d2](https://d2lang.com) — for diagram rendering
- **Optional**: [mflux](https://github.com/filipstrand/mflux) — for FLUX.1 image generation on Apple Silicon
- **Optional**: API keys for web search (Brave, Tavily, or Serper)

## License

ISC
