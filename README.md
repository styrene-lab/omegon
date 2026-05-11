+++
id = "30b48db5-e615-4544-a4a9-634872a27a77"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omegon

**Terminal-native agent harness for serious software work.**

Single Rust binary. Persistent project memory. Multiple inference providers. Parallel git worktrees. Design and spec lifecycles built in.

Omegon is a systems engineering harness, not a transcript viewer. It can read and edit code, run commands, manage project memory across sessions, expose project-specific REST APIs as agent tools from OpenAPI specs, decompose work into isolated child worktrees, run bounded headless tasks, run long-lived Sentry automation, and operate as a local daemon or ACP agent server for editor integrations.

[![docs](https://img.shields.io/badge/docs-omegon.styrene.io-2ab4c8)](https://omegon.styrene.io/docs/)
[![license](https://img.shields.io/badge/license-BSL%201.1-344858)](LICENSE)

---

## Why Omegon exists

Most “coding agents” are thin wrappers around an API call, a file picker, and some hopeful marketing.

Omegon takes the opposite position.

Software work is a live system with:
- state
- memory
- lifecycle artifacts
- source control boundaries
- multiple inference backends
- cost and quota pressure
- operator intent that must survive more than one session

So Omegon treats the agent as one subsystem inside a larger harness.

That means you get:
- a **real terminal UI** instead of a transcript dump
- **provider honesty** instead of vague “smart routing” slogans
- **persistent project memory** instead of amnesia between runs
- **design-tree and OpenSpec lifecycles** instead of planning in random markdown scraps
- **parallel git worktree execution** instead of praying one giant prompt does the right thing

---

## Install

```sh
curl -fsSL https://omegon.styrene.io/install.sh | sh
```

Preview channel one-shot:

```sh
curl -fsSL https://omegon.styrene.io/install.sh | sh -s -- --channel=rc
```

Or with Homebrew:

```sh
brew tap styrene-lab/tap
brew install omegon                # stable
brew install styrene-lab/tap/omegon-rc  # preview / rc
```

Stable docs: <https://omegon.styrene.io/docs/install>
Preview docs: <https://omegon.styrene.dev/docs/install>

---

## Get working fast

### Fastest interactive path

```sh
omegon auth login openai-codex
om
```

Omegon now installs two standard entrypoints from the same binary:

- `om` - slim mode: prompt, edit, validate. The agent sees the core coding tools by default. Memory works normally. `/help` shows the essentials; `/help all` reveals the full command set.
- `omegon` - full harness mode: design tree, OpenSpec lifecycle, cleave orchestration, delegation, richer dashboard surfaces, and the same underlying binary.

Start with `om`. When you're ready for more, shift up:

- `/warp` — toggle slim ↔ full
- `/help all` — see every command without switching mode

Flags override the entrypoint default when you need the opposite posture:

```sh
om --full
omegon --slim
```

### API-key path

```sh
omegon secret set ANTHROPIC_API_KEY --stdin
om
```

### Local path

```sh
ollama pull qwen3:32b
om
```

If you want the shortest happy path instead of a full tour, start here:
- <https://omegon.styrene.io/docs/get-started>
- Preview / staging guidance: <https://omegon.styrene.dev/docs/get-started>

---

## What it does

### 1. Runs as a native terminal harness

Omegon is a Rust-native binary with no Node.js runtime dependency.

It gives you:
- structured conversation rendering
- collapsible tool cards
- wrapped multiline editor
- live engine, memory, and system telemetry
- primary browser surface launch via `/auspex open`
- local browser compatibility/debug surface via `/dash`
- ACP server mode for editors that speak Agent Client Protocol
- `serve` mode for persistent local daemon/control-plane use

### 2. Keeps provider identity honest

Omegon distinguishes concrete runtime backends instead of hiding them behind mushy branding.

Examples:
- **Anthropic/Claude**
- **OpenAI API**
- **OpenAI/Codex**
- **Ollama (Local)**
- **Ollama Cloud**

The footer separates:
- **provider** → concrete backend identity
- **model** → selected runtime model
- **limit** → upstream quota/bucket telemetry when available

That sounds small until you’ve spent an hour debugging a system that lied about what model path it was actually using.

### 3. Persists project memory across sessions

Omegon stores durable facts under typed sections:
- Architecture
- Decisions
- Constraints
- Known Issues
- Patterns & Conventions
- Specs

This is not “chat history with search.”
It is a project memory system with recall, persistence, and lifecycle-aware usage. Semantic recall uses Ollama embeddings when available and can fall back to a local ONNX sentence-transformer model in builds compiled with `local-embeddings`; if neither backend is available, memory degrades to FTS5 keyword search instead of failing.

### 4. Tracks design and specification as first-class work

Omegon ships with two durable lifecycle systems:

- **Design Tree** — explore questions, record research, dependencies, and decisions
- **OpenSpec** — write Given/When/Then specs, generate plans, verify implementation, archive change history

This makes the agent better at long-running work because intent is written down in forms the harness can query.

### 5. Executes parallel work in real git worktrees

For larger tasks, Omegon can assess complexity and split work into child tasks with isolated scopes.

That means:
- separate worktrees
- concrete file boundaries
- merge and conflict detection
- better operator control over risky multi-file changes

### 6. Runs outside the TUI when needed

The same binary supports scripted and service-oriented use:

```sh
omegon run task.toml
omegon run --prompt "Review this repository" --max-turns 10
omegon sentry --config sentry.toml
omegon serve
omegon acp
```

`omegon run` is bounded and exits with structured status codes: `0` completed, `1` error, `2` upstream exhausted, `3` timeout. `omegon sentry` runs the autonomous task executor with triggers, budgets, optional auto model routing, and a local control plane. `omegon serve` exposes a local daemon/control-plane with health probes. `omegon acp` runs an Agent Client Protocol server over stdio by default, or WebSocket with `--listen`.

### Benchmarking and signal shaping

Omegon ships with an in-repo token-efficiency comparison harness under `scripts/benchmark_harness.py` and `ai/benchmarks/`.

Use it to compare harnesses against the same task and acceptance criteria, then inspect totals, wall clock, and Omegon's `sys/tools/conv/mem/hist/think` buckets before changing prompt, history, or tool-surface behavior.

Current stance:

- `om` is the comparison profile for mainstream CLI coding agents.
- `omegon` is the full systems-engineering harness profile.
- `omegon --slim` and `om --full` remain valid overrides.
- the benchmark harness stays in-repo while it is coupled to `--usage-json`, `omegon_context`, auth/provider behavior, and clean-room runtime mechanics.


## Quick example

Launch Omegon in a repo:

```sh
om
```

Then prompt it normally:

```text
Read README.md and summarize the architecture.
```

Or make it do real systems work:

```text
Inspect the auth flow, identify the weakest boundary, propose a minimal fix, add tests, and commit it.
```

Or lean into lifecycle mode:

```text
We need to refactor the session model. Explore the design space, surface assumptions, write a spec, and then implement it.
```

---

## Core capabilities

### Three-axis inference control

Omegon treats inference as three separate controls:

- **Capability tier** — `local` → `retribution` → `victory` → `gloriana`
- **Thinking level** — `off` / `minimal` / `low` / `medium` / `high`
- **Context class** — `squad` / `maniple` / `clan` / `legion`

This is the right model because “which model?” is not the only question that matters.
Capability, reasoning effort, and context budget are different levers.

### Built-in tools

Omegon exposes structured tools for:
- file reads/writes/edits
- shell execution
- web search
- OpenAPI-backed project tools from `.omegon/openapi.toml`
- git operations
- date/time resolution
- memory management
- design-tree queries and mutations
- OpenSpec lifecycle management
- codebase retrieval
- model control
- background services

### Inference providers

- Anthropic/Claude
- OpenAI API
- OpenAI/Codex
- OpenRouter
- Groq
- xAI (Grok)
- Mistral AI
- Cerebras
- Google Gemini
- Google Antigravity
- OpenCode Go
- Hugging Face
- Ollama (Local)
- Ollama Cloud

### Tutorial that actually does work

`/tutorial` is an interactive overlay, not a static lesson reader.
It can read code, store memory, create lifecycle artifacts, and walk an operator through real work.

---

## Project structure

```text
core/                       Rust workspace
  crates/
    omegon/                 Main binary — TUI, agent loop, tools, web surface
    omegon-codescan/        Code scanning helpers
    omegon-extension/       Extension SDK crate
    omegon-git/             Git and worktree operations
    omegon-memory/          Project memory runtime
    omegon-opsx/            OpenSpec/lifecycle engine
    omegon-secrets/         Secret resolution and redaction
    omegon-traits/          Shared protocol and event types
    omegon-web/             Web and ACP-adjacent surfaces
site/                       Public docs site
openspec/                   Spec-driven lifecycle artifacts
docs/                       Durable architecture and design docs
skills/                     Markdown skill packs
themes/                     Alpharius theme assets
```

---

## Build from source

```sh
cargo build --release
```

Release binary:

```text
target/release/omegon
```

Common development commands:

```sh
just build
just test-rust
just lint
just link
```

`just link` writes a sourceable dev alias file at `~/.omegon/dev-alias.sh` and wires the current shell profile so `omegon` and `om` point at the newest local build. It does not overwrite package-manager owned binaries.

---

## Configuration

Mutable user state lives under:

```text
~/.config/omegon/      settings and provider auth
~/.omegon/             installed skills, agents, extensions, dev aliases
```

Common files:
- `auth.json` — provider credentials
- `settings.json` — user settings
- `AGENTS.md` — global directives

Project-local control surfaces include:
- `AGENTS.md`
- `.omegon-version`
- `.omegon/openapi.toml`
- `ai/memory/facts.jsonl`
- `openspec/`
- `docs/`

---

## Release hygiene

Omegon ships signed releases with:
- SHA-256 checksums
- cosign signatures
- CycloneDX SBOMs
- GitHub build attestations

Install/update docs:
- <https://omegon.styrene.io/docs/install>
- Preview / RC docs: <https://omegon.styrene.dev/docs/install>

---

## Read the docs

Start here:
- <https://omegon.styrene.io/docs/>
- <https://omegon.styrene.io/docs/quickstart>
- <https://omegon.styrene.io/docs/providers>
- <https://omegon.styrene.io/docs/tui>
- <https://omegon.styrene.io/docs/tutorial>

Preview / staging docs:
- <https://omegon.styrene.dev/docs/install>
- <https://omegon.styrene.dev/docs/get-started>

---

## License

[BSL 1.1](LICENSE) — © 2024–2026 Black Meridian, LLC

In practical terms: if you are not trying to offer Omegon itself as a competing hosted agent service, the license is unlikely to be the part that ruins your day.
