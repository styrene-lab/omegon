---
id: ts-rust-cleave-split
title: "TS/Rust cleave split — experimental Rust backend, open-sourced TS harness, clean migration boundary"
status: implemented
parent: rust-agent-loop
tags: [architecture, migration, cleave, open-source, strategic]
open_questions: []
issue_type: epic
priority: 1
---

# TS/Rust cleave split — experimental Rust backend, open-sourced TS harness, clean migration boundary

## Overview

The TS→Rust migration is half-done. Cleave currently dispatches TS children (node bin/omegon.mjs) that hit the same provider bugs the Rust binary has already fixed. Rather than keep patching both codebases in lockstep, create an explicit fork in the road: the Rust binary becomes an opt-in experimental cleave backend (where it should be as a maturing implementation), the TS project gets open-sourced for the pi community who already knows it, and the pure-Rust path is there as an easter egg for anyone who discovers experimental cleave. This cleanly separates the two lifecycles instead of keeping them entangled.

## Research

### The entanglement problem

**Current state**: The TS Omegon (installed at /opt/homebrew/lib/node_modules/omegon) is the running harness for interactive sessions. It dispatches cleave children as `node bin/omegon.mjs --mode rpc --no-session`. These TS children make their own LLM API calls through the TS provider stack, which has the same thinking signature bug the Rust binary just fixed. The Rust binary (core/target/release/omegon) has working native providers, TUI, agent loop, and cleave orchestrator — but it's only used when you explicitly run `omegon` from the Rust build.

**The problem**: every provider-level fix now needs to be applied twice — once in Rust (providers.rs) and once in TS (wherever the pi-mono provider code lives). The TS codebase is a fork of an upstream project with its own provider abstraction. The Rust codebase has clean, purpose-built providers. Keeping them in sync is wasted effort.

**The installed binary gap**: `/opt/homebrew/bin/omegon` is version 0.10.7 (ancient). The Rust 0.13.0 binary exists but isn't installed or used by default. The TS harness doesn't know about the Rust binary's capabilities.

**What actually works in Rust today**: interactive TUI, native Anthropic + OpenAI streaming, agent loop with tool dispatch, session save/load/resume, cleave orchestration (child dispatch, worktree management, merge), memory system, lifecycle engine (design tree + openspec), all 32 registered tools, splash screen, web dashboard. It's not experimental in quality — it's experimental only in deployment.

### The two-product strategy

**Product 1: pi (TypeScript, open-sourced)**
- The familiar pi/omegon TS harness, based on the pi-mono fork
- Open-sourced for the community that already knows it
- Extensions, themes, TUI components — the JS ecosystem toolchain people expect
- No longer the primary development target — receives stability patches, not new features
- Published as @styrene-lab/pi-coding-agent or similar

**Product 2: omegon (Rust, the forward path)**
- Single static binary, zero runtime dependencies
- Native providers, native TUI (ratatui), native cleave orchestrator
- All the lifecycle tooling (design tree, openspec, memory) built in
- The "easter egg" — if you use experimental cleave or install from omegon.styrene.dev, you're running Rust
- This is where all new development happens

**The bridge**: The TS harness can optionally dispatch cleave children to the Rust binary instead of to itself. A flag like `--experimental-backend rust` or an env var `OMEGON_CLEAVE_BACKEND=native` switches the child dispatch from `node bin/omegon.mjs` to the Rust `omegon` binary. This is the discovery path — operators who try experimental cleave get faster, more reliable children, and eventually realize the Rust binary does everything.

**Migration endpoint**: When the Rust binary handles all the use cases the TS harness does (including the extensions that matter), the TS version becomes a compatibility shim that launches the Rust binary. Eventually it's just `npm install -g omegon` downloading a platform binary — no Node.js at all.

### What needs to happen concretely

**Immediate (unblocks the current session)**:
1. Add Rust backend option to TS cleave dispatcher — when configured, dispatch children as `omegon --prompt-file task.md --no-session --cwd worktree/` instead of `node bin/omegon.mjs -p`
2. Build and install the Rust 0.13.0 binary locally so cleave can find it
3. Wire the experimental flag through cleave_run params or environment

**Short-term (open-source TS)**:
4. Clean up the pi-mono fork — remove proprietary extensions, add LICENSE, README for community
5. Publish as a standalone repo (styrene-lab/pi or similar)
6. Final TS release with a note pointing to Rust omegon as the successor

**Medium-term (Rust as default)**:
7. Make Rust the default cleave backend, TS as fallback
8. Install script (`omegon.styrene.dev/install.sh`) already delivers the Rust binary
9. `npm install -g omegon` downloads the platform binary (no Node.js child needed)

**The Vault work we were doing**: continues in the Rust crate, cleave'd using Rust children. The TS side doesn't need Vault — it's a feature for the Rust product.

## Decisions

### Decision: TS version reverts to pi branding — Omegon is the Rust product

**Status:** decided
**Rationale:** The community knows it as pi. Keeping the names distinct creates a clean identity boundary: pi is the TypeScript agent harness (open-sourced, not in active development, a learning tool). Omegon is the Rust native successor (actively developed, where all new work happens). The open-source pi README makes clear it's an educational/experimental project, not a maintained product.

### Decision: Experimental cleave calls the Rust binary's cleave subcommand directly — TS becomes a thin invoker

**Status:** decided
**Rationale:** The Rust cleave subcommand already owns worktree management, child dispatch, merge, and guardrails. The TS cleave_run tool just needs to: (1) write the plan JSON to a temp file, (2) invoke `omegon cleave --plan plan.json --directive "..." --workspace ws/`, (3) read the report from the process output. No TS orchestration code runs. This eliminates the dual-provider bug class entirely — Rust children use the fixed Rust providers.

### Decision: Minimum switchover: build + install Rust binary, add native backend flag to TS cleave_run tool

**Status:** decided
**Rationale:** Three concrete steps: (1) cargo build --release and install the 0.13.0 binary, (2) add a `backend` param to the TS cleave_run tool — when set to "native", invoke the Rust cleave subcommand instead of the TS dispatcher, (3) pass through the plan, directive, and workspace path. The TS orchestration layer needs only a thin shim — the Rust binary does all the work. No changes to the Rust side needed, it already has the cleave subcommand.

## Open Questions

*No open questions.*
