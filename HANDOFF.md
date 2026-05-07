+++
id = "e04d1982-d009-4be2-be63-e799b4029d25"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omegon Handoff — 2026-05-06

## Where you're picking up

I (the Flynt-side agent) just landed **`bbcd1231`** on `main`. It bundles every omegon change Flynt's 0.7.0 polish required. Read its commit body — it's the source of truth for the *why*. Quick recap of the surface area touched:

```
core/crates/omegon/src/acp.rs            (+ build_config_options now reads from worker)
core/crates/omegon/src/acp_worker.rs     (SetX requests carry oneshot acks; settings exposed on WorkerHandle)
core/crates/omegon/src/behavior.rs       (nudge wording rebalanced)
core/crates/omegon/src/extensions/mod.rs (FLYNT_VAULT / CODEX_VAULT in SAFE_INHERIT_ENVS)
core/crates/omegon/src/loop.rs           (dead-mouse + first-turn nudges rephrased + Q&A guards)
```

Everything compiles `cargo check -p omegon` and `cargo build --release -p omegon`.

## What's currently deployed

- `/Users/wilson/.omegon/versions/0.18.5/omegon` — built from `bbcd1231`, **adhoc signed** (`codesign -fs -`). Suitable for local dev only. The directory name (`0.18.5`) does NOT match the binary (`0.18.6` per Cargo.toml). Real users on the published 0.18.5 are unaffected.
- The user runs Flynt against this; both repos sit side-by-side under `~/workspace/styrene-labs/`.

## Open items, in priority order

### 1. Push `bbcd1231` to origin
The commit isn't pushed yet. Confirm with the user before pushing — I didn't authorize it.

### 2. Cut a proper 0.18.6 release
- Versions dir naming convention says `~/.omegon/versions/<version>/omegon`. Currently `0.18.5/` holds a 0.18.6 binary. Either rename the dir, bump the install/upgrade tooling, or roll the version forward.
- Replace adhoc signing with the real Developer ID signature. Notarization if that's part of the release flow.
- Whatever release workflow exists in `.github/workflows/` — verify it picks up the new commit.

### 3. ~280 stale `.md` modifications in working tree
Pre-existing dirty state when I arrived. NOT touched by me. Sample:
```
M CONTRIBUTING.md
M EXTENSIONS.md
M README.md
... (hundreds of .md files across catalog/, core/, design/, docs/, etc.)
```
Looks like a global rename pass or formatting sweep that was started and never committed. Needs a decision: commit / revert / investigate provenance with `git diff <file>` on a few samples.

### 4. Audit the rest of `behavior.rs` for write-bias nudges
My rebalance was reactive — I caught the ones that fired during Flynt Q&A testing (`continuation_pressure_message`, `evidence_sufficiency_message`, `om_local_first_message`, dead-mouse, first-turn-execution, execution-pressure). Probably more in:
- `BehavioralTier::Constrained` arms I didn't touch
- `controller.observe_turn` paths and their downstream nudges
- `should_inject_execution_pressure` heuristics
- The `cleave/test_architect.rs` write-bias is intentional (code-test gen) — leave it.

Test: in a fresh Flynt session, ask for "rundown / overview / explain / give me a summary" of any document and confirm zero file writes happen. The `Rust Game Engine Tech` note in `~/workspace/black-meridian` is a useful target — substantial enough to trigger nudges at >2 tool calls.

### 5. Generalize `FLYNT_VAULT` → `OMEGON_PROJECT_ROOT`
I added `FLYNT_VAULT` and `CODEX_VAULT` to `SAFE_INHERIT_ENVS` as the surgical fix. Cleaner long-term: have omegon set a canonical `OMEGON_PROJECT_ROOT` env var on every spawned extension, derived from `--cwd`. Extensions read that instead of app-specific names. Removes the per-embedder special-casing.

## Architecture notes you'll need

ACP runs on **two threads**. Easy to forget.

- **Transport thread** (main): owns `OmegonAcpAgent`, holds `RefCell<Option<WorkerHandle>>`, runs the LocalSet for the ACP I/O. Types are `!Send`. This is where `new_session`, `set_session_config_option`, `prompt` arrive.
- **Worker thread** (`acp_worker::worker_loop`): owns the agent setup, runs `omegon::r#loop::run_agent_loop`, processes `WorkerRequest::Prompt`/`SetModel`/`SetThinking`/`SetPosture`. Has its own tokio runtime.
- Communication: `mpsc::Sender<WorkerRequest>` for control, `broadcast::Sender<WorkerEvent>` for streamed events.

`SharedSettings = Arc<Mutex<Settings>>` is created in `spawn_worker` and **shared between both threads**. ACP reads via `WorkerHandle.settings.lock()`; worker writes via `s.set_model(...)` etc. inside the `WorkerRequest::SetX` handlers. The ack channel exists because the channel send returns immediately — without it, the ACP transport reads the lock before the worker has even popped the message.

## What you don't need to deal with

- Flynt 0.7.0 release prep, panel UI, `ui-state.json`, the Justfile bundle path. All landed in flynt repo (commits `2c37050`, `44fdbfa`, `c5f03a3`, `6a85b5d`, `6476212`).
- The `crates/flynt-agent/flynt-agent` binary in the flynt repo. That's an embedder concern.
- The dev tooling that ad-hoc-signs binaries. The Flynt agent dealt with that as a workaround.

## Pointers
- Flynt's ACP client: `~/workspace/styrene-labs/flynt/crates/flynt-app/src/acp.rs`. Useful when reasoning about what messages clients actually consume vs. discard.
- ACP schema: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/agent-client-protocol-schema-0.11.4/`.
- Trace flag for live debugging from a Flynt run: `RUST_LOG=info open dist/Flynt.app --stderr /tmp/flynt-trace.log` from the flynt repo, then `tail -f /tmp/flynt-trace.log | grep omegon`. Faster than instrumenting a unit test.
