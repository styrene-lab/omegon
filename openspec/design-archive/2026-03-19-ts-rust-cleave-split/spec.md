# TS/Rust cleave split — experimental Rust backend, open-sourced TS harness, clean migration boundary — Design Spec (extracted)

> Auto-extracted from docs/ts-rust-cleave-split.md at decide-time.

## Decisions

### TS version reverts to pi branding — Omegon is the Rust product (decided)

The community knows it as pi. Keeping the names distinct creates a clean identity boundary: pi is the TypeScript agent harness (open-sourced, not in active development, a learning tool). Omegon is the Rust native successor (actively developed, where all new work happens). The open-source pi README makes clear it's an educational/experimental project, not a maintained product.

### Experimental cleave calls the Rust binary's cleave subcommand directly — TS becomes a thin invoker (decided)

The Rust cleave subcommand already owns worktree management, child dispatch, merge, and guardrails. The TS cleave_run tool just needs to: (1) write the plan JSON to a temp file, (2) invoke `omegon cleave --plan plan.json --directive "..." --workspace ws/`, (3) read the report from the process output. No TS orchestration code runs. This eliminates the dual-provider bug class entirely — Rust children use the fixed Rust providers.

### Minimum switchover: build + install Rust binary, add native backend flag to TS cleave_run tool (decided)

Three concrete steps: (1) cargo build --release and install the 0.13.0 binary, (2) add a `backend` param to the TS cleave_run tool — when set to "native", invoke the Rust cleave subcommand instead of the TS dispatcher, (3) pass through the plan, directive, and workspace path. The TS orchestration layer needs only a thin shim — the Rust binary does all the work. No changes to the Rust side needed, it already has the cleave subcommand.

## Research Summary

### The entanglement problem

**Current state**: The TS Omegon (installed at /opt/homebrew/lib/node_modules/omegon) is the running harness for interactive sessions. It dispatches cleave children as `node bin/omegon.mjs --mode rpc --no-session`. These TS children make their own LLM API calls through the TS provider stack, which has the same thinking signature bug the Rust binary just fixed. The Rust binary (core/target/release/omegon) has working native providers, TUI, agent loop, and cleave orchestrator — but it's only used …

### The two-product strategy

**Product 1: pi (TypeScript, open-sourced)**
- The familiar pi/omegon TS harness, based on the pi-mono fork
- Open-sourced for the community that already knows it
- Extensions, themes, TUI components — the JS ecosystem toolchain people expect
- No longer the primary development target — receives stability patches, not new features
- Published as @styrene-lab/pi-coding-agent or similar

**Product 2: omegon (Rust, the forward path)**
- Single static binary, zero runtime dependencies
- Native provi…

### What needs to happen concretely

**Immediate (unblocks the current session)**:
1. Add Rust backend option to TS cleave dispatcher — when configured, dispatch children as `omegon --prompt-file task.md --no-session --cwd worktree/` instead of `node bin/omegon.mjs -p`
2. Build and install the Rust 0.13.0 binary locally so cleave can find it
3. Wire the experimental flag through cleave_run params or environment

**Short-term (open-source TS)**:
4. Clean up the pi-mono fork — remove proprietary extensions, add LICENSE, README for co…
