# Omegon

**The AI agent harness with enough nerve to act like an operating system for software work.**

Single binary. Native Rust. Real memory. Parallel worktrees. Design and spec lifecycles built in.

Omegon is not a chatbot in your terminal. It is a systems engineering harness for operators who build: a terminal-native agent that can read and edit code, run commands, manage project memory across sessions, decompose work into parallel children, and track design intent as a first-class artifact.

[![docs](https://img.shields.io/badge/docs-omegon.styrene.dev-2ab4c8)](https://omegon.styrene.dev)
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

RC channel one-shot:

```sh
CHANNEL=rc curl -fsSL https://omegon.styrene.io/install.sh | sh
```

Or with Homebrew:

```sh
brew tap styrene-lab/tap
brew install omegon                # stable
brew install styrene-lab/tap/omegon-rc  # rc
```

Docs: <https://omegon.styrene.dev/docs/install>

---

## Get working fast

### Fastest interactive path

```sh
omegon login openai-codex
om
```

Omegon now installs two standard entrypoints from the same binary:

- `om` — slim, copy-friendly, familiar terminal mode
- `omegon` — full harness mode

You can move between them interactively at runtime:

- `/warp` — toggle slim ↔ full
- `/shackle` — force slim (`om`) mode
- `/unshackle` — force full (`omegon`) mode

Flags still override the entrypoint default when you need the opposite posture:

```sh
om --full
omegon --slim
```

### API-key path

```sh
export ANTHROPIC_API_KEY=sk-ant-...
om
```

### Local path

```sh
ollama pull qwen3:32b
om
```

If you want the shortest happy path instead of a full tour, start here:
- <https://omegon.styrene.dev/docs/get-started>

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
It is a project memory system with recall, persistence, and lifecycle-aware usage.

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

### Benchmarking and signal shaping

Omegon now ships with an in-repo token-efficiency comparison harness under `scripts/benchmark_harness.py` and `ai/benchmarks/`.

Use it for two things:

1. **compare harnesses honestly** — same task, same acceptance, different agent surface
2. **shape Omegon’s signal profile with evidence** — inspect totals, wall clock, and Omegon’s `sys/tools/conv/mem/hist/think` buckets before changing prompt, history, or tool-surface behavior

Current stance:

- `om` is the de-facto comparison profile for mainstream CLI coding agents
- default `omegon` is the premium harness mode when richer systems-engineering behavior is worth extra token cost
- `omegon --slim` and `om --full` remain valid overrides when you want the opposite posture from the entrypoint default
- the benchmark harness stays **in-repo for now** because it is still tightly coupled to Omegon internals (`--usage-json`, `omegon_context`, auth/provider behavior, and clean-room runtime mechanics)

Design notes:

- [[docs/design/evidence-driven-signal-shaping|Evidence-Driven Signal Shaping]]
- [[docs/design/signal-shaping-profiles|Signal Shaping Profiles]]
- [[docs/design/signal-classes-and-retention-policy|Signal Classes and Retention Policy]]


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
- git operations
- date/time resolution
- memory management
- design-tree queries and mutations
- OpenSpec lifecycle management
- codebase retrieval
- model control
- background services

### 11 inference providers

Current surfaces include:
- Anthropic/Claude
- OpenAI API
- OpenAI/Codex
- OpenRouter
- Groq
- xAI
- Mistral
- Cerebras
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
    omegon-git/             Git and worktree operations
    omegon-memory/          Project memory runtime
    omegon-secrets/         Secret resolution and redaction
    omegon-traits/          Shared protocol and event types
site/                       Public docs site
openspec/                   Spec-driven lifecycle artifacts
docs/                       Durable architecture and design docs
skills/                     Markdown skill packs
themes/                     Alpharius theme assets
```

---

## Build from source

```sh
cd core
cargo build --release
```

Release binary:

```text
core/target/release/omegon
```

Type-check and test path for TypeScript and Rust changes lives in repo-local commands and release validation.

---

## Configuration

Mutable user state lives under:

```text
~/.config/omegon/
```

Common files:
- `auth.json` — provider credentials
- `settings.json` — user settings
- `AGENTS.md` — global directives

Project-local control surfaces include:
- `AGENTS.md`
- `.omegon-version`
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
- <https://omegon.styrene.dev/docs/install>

---

## Read the docs

Start here:
- <https://omegon.styrene.dev/docs/>
- <https://omegon.styrene.dev/docs/quickstart>
- <https://omegon.styrene.dev/docs/providers>
- <https://omegon.styrene.dev/docs/tui>
- <https://omegon.styrene.dev/docs/tutorial>

---

## License

[BSL 1.1](LICENSE) — © 2024–2026 Black Meridian, LLC

In practical terms: if you are not trying to offer Omegon itself as a competing hosted agent service, the license is unlikely to be the part that ruins your day.
