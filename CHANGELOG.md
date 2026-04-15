# Changelog

All notable changes to Omegon are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/). Versioning: [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.15.24] - 2026-04-15

### Added

- **Daemon trigger configs** — `.omegon/triggers/*.toml` defines scheduled and event-driven prompt dispatch. Scheduled triggers support preset schedules (`hourly`, `daily`, `weekdays`, `weekly`) and interval durations (`30s`, `5m`, `1h`). Event triggers match inbound `DaemonEventEnvelope` by source and trigger_kind, rendering prompt templates with `{{payload.field}}` interpolation.
- **Daemon session router** — per-caller session multiplexing for daemon mode. Inbound messages are keyed by `(source_user, source_channel, source_thread)` and routed to dedicated sessions. `Arc<Semaphore>` bounds concurrent turns (default 8). Idle sessions are parked after a configurable timeout. Events without identity metadata route to a default session, preserving single-session backward compatibility.
- **Spawned daemon turns** — daemon command loop now spawns turns as tokio tasks via `spawn_best_effort_result` instead of awaiting inline, keeping the dispatch loop responsive during long-running LLM calls. Applies to user prompts, vox events, auto-dispatch turns, and scheduled triggers.
- **Daemon control plane** — `execute_daemon_control()` routes control requests (model, auth, secrets, skills, plugins) in daemon mode. Non-canonical slash commands dispatch as agent prompts instead of being rejected.
- **Vox caller identity propagation** — `DaemonEventEnvelope` carries `source_user`, `source_channel`, and `source_thread` identity fields from vox bridge messages. All fields are `Option<String>` with `serde(default)` for backward-compatible deserialization.
- **Vox extension bridge** — bidirectional bridge between vox (Discord/Slack) and the daemon agent loop. Includes extension CLI and secret CLI for runtime configuration.
- **Trust-level prompt framing** — operator messages get direct instruction framing; user messages get XML containment with prompt injection defense. Trust classification is transport-specific (Discord roles, Slack usergroups).
- **Nix flake with composable container toolset profiles** — declarative container builds with selectable tool profiles.
- **Homebrew RC channel** — `brew tap styrene-lab/tap && brew install styrene-lab/tap/omegon-rc` installs the latest RC build. The `omegon-rc` formula in the tap is updated automatically by CI on every RC release. Switch back to stable with `brew unlink omegon-rc && brew install omegon`.
- **`just cut-rc` developer command** — cuts an RC from the main workspace without manual setup. Validates that `main` is clean and pushed, clones a fresh release workspace from GitHub (correct origin, no stale state), runs `just rc`, and pulls the resulting commit + tag back into local `main`.
- **Brew-managed upgrade guard** — `is_homebrew_managed()` detects when the running binary lives in a Homebrew Cellar path and refuses in-place upgrade, redirecting the operator to `brew upgrade omegon` or `brew upgrade styrene-lab/tap/omegon-rc` as appropriate. Prevents Homebrew version tracking corruption.
- **Typed control promotion across transport surfaces** — operator-facing control families now route through canonical typed requests instead of bespoke slash-only handlers. Recent promotions include `skills/plugin`, `secrets/vault`, and the minimal `cleave/delegate` status surface, with matching TUI, IPC, and WebSocket routing.
- **Minimal cleave/delegate typed status surface** — `cleave status`, `cleave cancel <label>`, and `delegate status` are now first-class typed control requests. Cleave execution remains feature-owned and continues to route through the orchestration bus by design.
- **Linux release ABI validation** — CI gates every release on a 3-distro ABI matrix (ubuntu-22.04, rockylinux-9, amazonlinux-2023) using Docker. The release job cannot publish if any validation fails. Linux binaries are built with `cargo-zigbuild` to widen the glibc compatibility floor.
- **TUI attachment-token word navigation** — Meta/Alt word motion and word deletion now treat inline attachment placeholders like `[image0]` as atomic tokens instead of stepping into projected placeholder text. This fixes cursor lockups and broken editor navigation introduced with inline attachment token rendering.

### Changed

- **omegon-extension dual-licensed MIT/Apache-2.0** — extension SDK crate is now dual-licensed for crates.io compatibility.
- **Daemon loop `Cell` → `AtomicBool`** — `loop.rs` stream idle timeout flag converted from `Cell<bool>` to `AtomicBool` (Relaxed ordering) to make the `run()` future `Send`-compatible for spawned turn tasks.
- **Secrets and vault control normalization** — `/secrets` and `/vault` no longer depend on the old bespoke runtime path. Secret view/set/get/delete and vault status/configuration flows now run through shared control responders, and transport policy is explicit and conservative.
- **Homebrew formula auto-update** — the `homebrew.yml` CI workflow now correctly pushes stable updates to `styrene-lab/homebrew-tap` (the tap users actually read) and RC updates to `omegon-rc.rb`. Previously it was writing to the wrong file in the wrong repo.
- **Release assets no longer dropped on immutable release** — `release.yml` now creates the GitHub Release as a draft, uploads all assets (archives, sha256, cosign `.sig`/`.pem` sidecars, SBOM), then publishes. Previously the release was published before upload completed, making it immutable and causing all uploads to fail.
- **RC lifecycle doctor compile removed** — `just rc` no longer runs a blocking `cargo run -p omegon -- doctor` when no milestone-scoped design nodes exist. The check is warning-only for empty milestones; the compile added several minutes of wall time for zero diagnostic value.
- **Release validation split** — `just rc` cuts and ships; `just rc-validate` runs the full local test suite. Previously both were mixed into the same recipe, making every RC cut pay for a full test run even when CI would catch failures faster.

### Fixed

- **omegon-extension accepts numeric JSON-RPC IDs** — extension RPC layer now accepts both string and numeric `id` fields per JSON-RPC 2.0 spec.
- **Daemon `--model` flag passed to daemon process** — `serve` command now forwards the `--model` flag instead of hardcoding the anthropic provider.
- **Vox daemon event drain** — serve dispatch loop now drains vox daemon events correctly instead of dropping them.
- **Path traversal and multi-instance isolation** — hardened secret CLI and multi-instance file paths against directory traversal.
- **TUI panel rendering artifacts** — panel area is now fully cleared before re-rendering instruments, eliminating stale content bleed-through on resize or content change (#36).
- **TUI table body trailing pipe** — table rows that omit the trailing `|` character are now parsed and rendered correctly (#37).
- **Linux Homebrew install honesty** — install and distribution docs now explicitly warn that Homebrew on Linux does not solve host glibc ABI mismatches for Omegon release binaries. Users hitting `GLIBC_2.38` / `GLIBC_2.39` runtime errors are directed toward compatible distro/container baselines.
- **Release-line correction** — `v0.15.11-rc.2` was published from a mistaken version-line advance after `0.15.10` had not actually closed cleanly. The active candidate line remains the `0.15.10` RC series. See `docs/release-line-correction-0-15-10.md`.

## [0.15.22] - 2026-04-14

### Fixed

- **Delegate children ignore parent session provider** — delegate workers defaulted to a hardcoded provider candidate list (with `openai-codex:gpt-5.4` first) instead of inheriting the parent session's active model. Children now inherit the parent model via `TurnEnd` event tracking; the candidate list is only used as a last-resort fallback and now respects `OMEGON_MODEL`, `automation_safe_model()`, and puts API-key providers ahead of consumer subscription routes.
- **Anthropic prefill rejection after compaction/decay** — `build_llm_view()` could produce a conversation ending with an assistant message after decay or repair stripped surrounding messages. Anthropic rejects this with "This model does not support assistant message prefill." A trailing user continuation is now appended when the final message is assistant-role.
- **Cleave model fallback ignores operator environment** — cleave config fell back to hardcoded `anthropic:claude-sonnet-4-6` when `OMEGON_MODEL` was unset, ignoring configured API-key providers. Now checks `automation_safe_model()` before the hardcoded default.

## [0.15.11] - 2026-04-14

### Fixed

- **Full-mode tool surfaces disabled after `/unshackle`** — `apply_operator_tool_profile` placed delegate, auth-status, harness-settings, persona, and memory tools in the always-disabled base block instead of the slim-only block. `/unshackle` and `/warp` switched the UI to full mode but those tools remained suppressed. Delegate, persona, auth, harness settings, and memory lifecycle/connect/archive surfaces are now only disabled in slim mode and fully available after `/unshackle`.

## [0.15.10] - 2026-04-05

### Added

- **Anthropic subscription ToS compliance** — Omegon now enforces Anthropic's Consumer Terms restriction on automated use of subscription (Claude.ai / Claude Pro) credentials. Affected paths (`--prompt`, `--prompt-file`, `--smoke`) are hard-blocked at startup with a clear error citing the ToS URL. Interactive TUI sessions are fully permitted.
- **Subscription-aware cleave fallback routing** — When only an Anthropic subscription credential is present, cleave workers are automatically rerouted to the best available automation-safe provider (OpenAI API key → OpenAI/Codex OAuth → OpenRouter → Ollama) rather than failing. The TUI shows a toast with the fallback model. If no fallback exists, a clear block message lists concrete options to fix it.
- **`AnthropicCredentialMode` enum and helpers** — `providers.rs` now exports `AnthropicCredentialMode` (`ApiKey` / `OAuthOnly` / `None`), `anthropic_credential_mode()`, and `automation_safe_model()` for credential-aware routing decisions across the codebase.
- **Tutorial orientation mode** — `/tutorial` now calls `tutorial_gate()` to detect auth state and presents an orientation-only tour (Tab steps, no agent AutoPrompt) when no Victory-tier cloud model is available. `/tutorial consent` upgrades to Interactive mode when an Anthropic subscription is detected.
- **Ollama Cloud provider path** — Omegon now models hosted Ollama as a first-class provider (`ollama-cloud`) instead of overloading local `ollama` semantics. Runtime routing, provider catalogs, and auth surfaces preserve the distinction between local Ollama and the hosted API.
- **Self-service provider-key UX for hosted providers** — operator-facing auth flows now support API-key-backed providers such as OpenAI API, OpenRouter, and Ollama Cloud through `/login` and `/secrets`, instead of requiring environment variables as the only setup path.
- **Provider documentation refresh** — `docs/anthropic-subscription-tos.md` and the site provider/install/command guides now document the real automation boundary, hosted Ollama path, and secrets-driven provider setup.
- **Archived design-tree lifecycle** — design nodes now support an explicit archived state and archive action, with filtering/reporting surfaces updated to distinguish archived work from active lifecycle states.
- **Provider runtime degradation surfacing** — runtime state now carries degraded-provider information so the TUI and status surfaces can distinguish authentication problems from upstream reliability degradation.
- **Release manifest for downstream packaging** — release CI now emits a canonical `release-manifest.json` describing version, channel, commit, assets, checksums, signatures, and release URLs. Homebrew automation consumes this manifest instead of ad-hoc checksum scraping.
- **Scripted release preflight** — stable release gating is now enforced by `scripts/release_preflight.py`, checking branch cleanliness, RC/stable version coherence, changelog readiness, install-doc placeholder policy, and manifest-based packaging wiring.

### Changed

- **Footer subscription badge** — The subscription credential label now reads "subscription · interactive only" instead of just "subscription", making the interactive-only constraint continuously visible.
- **`/tutorial consent` acknowledgment** — Consent message now includes the automation restriction note alongside the quota usage warning.
- **`/cleave` guard** — Changed from a flat block to a smart dispatch: routes to fallback when available, blocks only when no automation-safe provider exists.
- **Startup gate is model-aware** — The Anthropic subscription gate now only fires when the requested `--model` is Anthropic. A child process explicitly running `--model ollama:llama3` is not blocked even when `ANTHROPIC_OAUTH_TOKEN` is set.
- **OpenAI/Codex provider naming** — Operator-facing surfaces now use `OpenAI/Codex` and `Anthropic/Claude` as canonical labels instead of mixed branding.
- **Engine footer limit wording** — The footer now labels Codex upstream quota telemetry as `limit` and prefixes model-family bucket names as buckets, reducing confusion between selected model and provider quota metadata.
- **Operator-first split footer engine panel** — the left engine panel now prioritizes provider, model, runtime posture, session totals, and optional limit telemetry. Bucket/version/path noise was removed from the default visible row stack.
- **TUI footer/runtime honesty** — provider/status surfaces now separate auth failures from degraded provider recency and keep runtime identity explicit across footer, status, bootstrap, and dashboard flows.
- **Embedded web identity parity** — The local web control plane now mirrors the canonical Omegon instance descriptor in startup and state payloads so browser consumers can see the same instance identity model as IPC consumers.
- **Package publishing ownership** — `just publish` no longer mutates Homebrew/tap state from a workstation. Downstream packaging is CI-owned and derived from published GitHub release artifacts.
- **Install docs version policy** — versioned install and verification examples are now explicitly documented as placeholders to avoid stale RC-by-RC doc churn.
- **Session journal path** — session narrative logging moved from `.session_log` to `.omegon/agent-journal.md`.

### Fixed

- **Tutorial test infinite loop** — `Tutorial::with_context()` was changed to call `tutorial_gate()`, which returned `OrientationOnly` (no API keys in test env) and caused tests looping for Command/AutoPrompt triggers to spin forever. Reverted: `with_context()` is now gate-free; `tutorial_gate()` is the TUI layer's responsibility.
- **Hosted Ollama message parsing** — Ollama Cloud now preserves native thinking/tool-call parsing instead of dropping hosted-Ollama-specific message structure on the floor.
- **ChatGPT/Codex models missing from `/model`** — `ModelCatalog` now keeps the OpenAI/Codex OAuth route visible and executable for GPT-family model selection instead of treating generic OpenAI auth and Codex auth as the same thing.
- **Upstream stall handling in the agent loop** — retries and idle timeout behavior were hardened across the 0.15.9 RC line: provider-specific upstream errors are classified into explicit recovery classes, persistent stalls now exhaust cleanly instead of hanging, and OpenAI/Codex idle timeout behavior was raised to align with real upstream streaming behavior.
- **Codex incomplete/heartbeat stream handling** — Codex SSE parsing now handles `response.incomplete`, treats unhandled heartbeat traffic as liveness, and avoids poisoning partial-content state on incomplete responses.
- **Bash tool TUI robustness** — interactive commands are prevented from wedging the TUI, terminal control noise is stripped from bash output, and `cd`-prefixed tool summaries are rendered more honestly.
- **Settings/profile persistence scope** — root profile persistence is anchored at the repo level instead of drifting by invocation path.
- **CI/Homebrew detached-HEAD publishing** — formula update automation was fixed to push correctly even when running from detached release contexts.
- **Release validation hygiene** — tracked Python bytecode artifacts were removed and `__pycache__/` / `*.pyc` are now ignored so Python-based release validation no longer dirties the tree.
- **OAuth login port held after browser cancel** — if the user closed the browser or switched accounts without completing the OAuth redirect, `listener.accept()` blocked indefinitely and held the callback port open. A second `/login` attempt failed with an OS address-in-use error and required killing Omegon. The accept is now wrapped in a 5-minute timeout; on expiry the listener drops, the port is freed, and a clear retry message is shown.

## [0.15.7] - 2026-04-03

### Fixed

- **ChatGPT/Codex models missing from `/model`** — `ModelCatalog` had no `openai-codex` section; users authenticated via ChatGPT/Codex OAuth saw an empty model picker. GPT-5.4 and GPT-5.4 mini now appear under "ChatGPT / Codex" when an `openai-codex` token is present.
- **"LLM bridge may have crashed" false-positive on Codex** — three bugs in `parse_codex_stream` caused the agent loop to surface this error spuriously:
  1. `try_send` for terminal events (`Done`/`Error`) could silently drop on a full channel (cap 256). Terminal events are now sent with `.send().await` after `process_sse` returns, guaranteeing delivery.
  2. When the Codex SSE stream closed cleanly without emitting `response.completed` (network drop, server restart), no signal was sent to the consumer. Partial content now synthesises a `Done`; an empty stream surfaces a clear `Error`.
  3. Some Codex endpoint variants emit `response.done` instead of `response.completed`. Both are now handled.

## [0.15.6] - 2026-04-01

### Added

- **Extension widget system** — stateful tab panels and ephemeral modals for Rust-native extensions. Schema-aware rendering supports `timeline`, `table`, and `tree` layouts. `Alt+N` / `Alt+P` cycle tabs. Action prompts accept numeric key selection. Widgets auto-fetch initial data on extension spawn.
- **BYOM (Bring Your Own Mind) — Phases 1–3** — extensions can declare a custom inference mind in `manifest.toml`; manifest types, state management, and persistence are fully wired. Extensions that supply their own inference layer are isolated from the global model selector.
- **`omegon-extension` SDK** — first-party Rust crate for third-party extension authors. Typed RPC primitives, manifest schema, and widget contracts published as a stable API surface.
- **Scribe Rust-native extension** — reference implementation: timeline widget emits formatted session events; manifest declares a `timeline` widget; RPC sidecar integration replaces the previous TypeScript bridge.
- **Bootstrap secrets RPC** — the extension IPC protocol now delivers required secrets via a `bootstrap_secrets` RPC call at spawn, not through process environment variables. Extensions receive only the secrets they declare in `manifest.toml`; the values never appear in `argv` or `environ` of the subprocess.
- **Extension secret preflight** — at startup, manifests are scanned for `required_secrets`; those names are added to the preflight set so vault/keyring-backed secrets are warmed before any extension subprocess spawns.
- **Vault integration at startup** — `VAULT_ADDR` + `VAULT_ROLE_ID`/`VAULT_SECRET_ID` (AppRole) or `VAULT_TOKEN` are detected at startup; vault-recipe secrets are batch-resolved in the preflight phase so both extensions and MCP plugins receive their tokens without per-request vault calls.
- **Plugin MCP env template preflight** — `collect_plugin_secret_requirements()` scans `~/.omegon/plugins/*/plugin.toml` and `.omegon/mcp.toml` for `{VAR_NAME}` references and adds them to the preflight set, so vault-backed secrets used in MCP server `env` blocks are available before plugins connect.
- **Session-long token counters in footer** — cumulative session input and output tokens shown in the engine block; compact `k`/`M` formatting prevents overflow on narrow terminals.
- **`/context` subcommand interface** — `SharedContextMetrics` provides real-time token composition; `/context clear` and `/context compact` are exposed as slash commands with a deadlock-free implementation.

### Fixed

- **Dual macOS Keychain prompts at startup** — the original code called `keyring::get_password()` separately for each requested secret, triggering one OS dialog per secret. Secrets are now batch-resolved through the session cache; a single "Always Allow" covers the entire preflight batch.
- **Web auth secret in preflight** — `OMEGON_WEB_AUTH_SECRET` was included in the startup preflight even though web search auth is only needed on-demand. Removed from preflight; resolved lazily on first web tool call.
- **Keyring recipes shadowed by environment variables** — `resolve()` checked `std::env::var` before the keyring, making it impossible to override a leaked env value with a properly stored keyring secret. Order is now: session cache → keyring → env → recipe fallback.
- **Redactor rebuilt per-secret** — the HMAC redactor was rebuilt after every individual secret resolution. It is now rebuilt once after the full preflight batch completes.
- **`/context clear` deadlock** — the clear handler held the conversation lock while dispatching a TUI command that re-acquired it. Lock scope tightened; clear and compact commands now complete reliably.
- **Footer token display overflow** — session input/output token counts used full decimal formatting (`1,234,567`); replaced with compact `format_tokens()` (`1.2M`).
- **Context bar breakdown heuristics** — `cached_tokens` / `input_tokens` / `output_tokens` from the provider response are now used directly; the old `chars/4` character-count estimate is gone.
- **Footer sync on compaction and clear** — `FooterData` was not updated after `/compact` or `/clear`; turn counter and token totals now reset correctly.
- **Extension spawn blocked when required secrets absent** — extensions that declare `required_secrets` are refused spawn (with a clear error) if any declared secret cannot be resolved. Previously the extension spawned with missing env vars and failed silently.

### Changed

- **Scribe-rpc crate removed from workspace** — the TypeScript-bridge `scribe-rpc` crate is replaced by the Rust-native scribe extension. The workspace is smaller; the extension binary is self-contained.
- **Legal surface** — Terms of Use, Privacy Policy, and `THIRD_PARTY_NOTICES` added. Contact address updated to `admin@styrene.io`.
- **CI release workflow** — `workflow_dispatch` trigger added to `release.yml`; `RELEASE_TAG` env var used throughout for consistency. SBOM and `THIRD_PARTY_NOTICES` integrated into release artifacts.
- **Site CI** — direct git push to vanderlyn on deploy; nginx location blocks for `/terms` and `/privacy`.
- 1073 tests.

## [0.15.5] - 2026-03-31

### Added

- **Speculative sandbox tools** — `speculate_start` / `speculate_check` / `speculate_commit` / `speculate_rollback`. Creates a git checkpoint before exploratory changes; commit to keep or rollback to discard. Replaces the pattern of ad-hoc `git stash` in agent sessions.
- **Tool groups in `manage_tools`** — predefined named capability clusters: `memory-advanced`, `delegate`, `cleave`, `lifecycle-advanced`, `model-control`. `enable_group` / `disable_group` / `list_groups` actions let operators collapse entire capability surfaces in one call. Groups don't change default state — they're a batch toggle for managing schema surface.
- **Ollama model warmup** — before streaming starts, cold Ollama models are pre-warmed with a no-op request. Progress surfaces in the TUI so the operator sees the model loading rather than a silent hang.
- **Unified braille context bar** — replaced the `≋ ≈ ∿ ·` character ramp with a braille-density bar backed by actual provider token counts (not a character-count heuristic). Bucket legend identifies all composition zones.
- **Per-turn token stats row** — the instruments panel shows last-turn input/output tokens immediately below the context bar.
- **Session token totals in footer** — cumulative session input/output tokens shown in the footer engine block.
- **Auto-ingest lifecycle decisions to memory** — `design_tree_update(add_decision)` and status transitions to `resolved` / `decided` / `implementing` automatically persist to the `Decisions` memory section via `BusRequest::AutoStoreFact`. Previously declared intent (`memory_ingest_lifecycle`) now has a real call path.
- **Auto-stored session episodes** — at session close, a template episode (title, turn count, tool calls, duration, tagged `auto`) is written to the memory backend. Searchable via `memory_episodes` in future sessions.
- **Segment copy** — `Ctrl+Y` copies the currently selected conversation segment as plain text to the system clipboard.
- **Dynamic Ollama catalog** — available local models are fetched at startup and surfaced in the model selector; unavailable cloud providers are filtered from the selector unless authenticated.

### Fixed

- **Spurious end-of-turn commit nudge** — `update_from_tools("commit")` now clears `files_modified` and `commit_nudged` is persisted across TUI `run()` invocations (was a local variable reset each message). The `[System: You made file changes but did not run git commit]` injection no longer fires after a successful commit.
- **`manage_tools` schema leak** — `tool_defs` was captured once before the turn loop; disabled tools were filtered from execution routing but still appeared in the schema sent to the LLM. Tool definitions are now refreshed from `bus.tool_definitions()` at the top of every turn.
- **Actual provider token counts end-to-end** — `input_tokens` from Anthropic / OpenAI / Codex API responses are wired through `LlmEvent::Done` → `AssistantMessage.provider_tokens` → `AgentEvent::TurnEnd` → TUI context bar.
- **`SessionEnd` never emitted in production** — the agent loop emitted `AgentEnd` but not `SessionEnd`, so `session_log.append_entry()` and all `SessionEnd` feature handlers were dead code. Fixed; `SessionEnd` now carries `turns` / `tool_calls` / `duration_secs`.
- **Post-loop `AutoStoreFact` dropped** — late-arriving or `SessionEnd`-triggered auto-store requests were silently discarded at the post-loop drain site. They now execute via `bus.execute_tool`.
- **Mouse on by default; `Esc` no longer silently disables** — mouse capture is enabled at startup; `Esc` closes popups/unpins segments only. `Ctrl+M` is the explicit mouse toggle.
- **Context bar memory fill estimate** — corrected the memory-fill fraction computation in the context bar breakdown.
- **`/context` slash command** — was parsing `ContextMode` (200k/1M) instead of `ContextClass` (squad/maniple/clan/legion); the command now matches what the selector shows.
- **Splash screen overflow** — content height was miscalculated (logo + 4 instead of actual content rows), causing overflow on terminals shorter than ~30 lines. Content-sized grid layout eliminates terminal-proportional whitespace.
- **Ambiguous-width Unicode cell advancement** — `⊙`, `◎`, `✦` and similar glyphs are 2-cell wide in most terminals; the footer and segment renderers now use `unicode-width` for correct cell advancement.
- **Session resume with missing fields** — tolerates unknown/missing fields in saved session snapshots rather than failing to deserialize.
- **Ollama stream flakiness** — `extra_body` injected into `StreamOptions` for provider-specific fields; model label display corrected.

### Changed

- **Tool schema surface −650 tokens/request** — stripped redundant `description` fields from optional properties in the four heaviest feature schemas (`design_tree_update`, `delegate`, `lifecycle_doctor`, `openspec_manage`). `file_scope` simplified to `items: {type: object}`.
- **Feature tool output capped at 16 000 chars** — universal safety net applied at the `dispatch_tools` level. Truncated blocks append `[truncated: N chars dropped — limit 16000]`.
- All provider model catalogs updated to current 2026 IDs (Anthropic, OpenAI, Groq, xAI, Mistral, OpenRouter). Route matrix includes gpt-5 family. MLX removed as a dedicated provider — use Ollama instead.
- `SessionEnd` is now emitted after every agent loop regardless of exit reason, enabling post-session hooks in features.
- 1050 tests.

## [0.15.5-rc.3] - 2026-03-30

### Added

- **Tool groups** — predefined named sets (`memory-advanced`, `delegate`, `cleave`, `lifecycle-advanced`, `model-control`) in `manage_tools`. Operators can enable/disable an entire capability cluster in one call. Groups don't change default state — they're a batch toggle mechanism for managing schema surface.
- **Auto-ingest lifecycle decisions to memory** — `BusRequest::AutoStoreFact` variant wired from `LifecycleFeature` through all bus drain sites to `memory_store`. When `design_tree_update(add_decision)` or `set_status(resolved|decided|implementing)` runs, the decision is automatically persisted to the `Decisions` memory section. The previously declared `memory_ingest_lifecycle` tool had no automatic call path; this replaces that intent correctly.

### Fixed

- **Spurious end-of-turn commit nudge** — `update_from_tools("commit")` now clears `files_modified`, so the `[System: You made file changes but did not run git commit]` injection no longer fires after the agent already committed. Previously, `files_modified` accumulated on every `edit`/`write` call and was never cleared, causing the nudge to fire spuriously on every session that used the `commit` tool.
- **`manage_tools` enable/disable had no effect on LLM schema** — `tool_defs` was captured once before the turn loop; disabled tools were filtered from execution routing but not from the schema sent to the LLM each turn. Tool definitions are now refreshed from `bus.tool_definitions()` at the top of every turn, so schema reflects current enabled state immediately.
- **Context bar used `chars/4` heuristic** — actual `input_tokens` from Anthropic/OpenAI/Codex API responses are now wired end-to-end: `LlmEvent::Done` → `AssistantMessage.provider_tokens` → `AgentEvent::TurnEnd` → TUI `context_percent`. The bar now shows what the provider actually billed, not a character-count estimate.

### Changed

- **Tool schema surface reduced ~650 tokens/request** — stripped redundant `description` fields from optional properties in the 4 heaviest feature tool schemas: `design_tree_update` (−168 tok), `delegate` (−268 tok), `lifecycle_doctor` (−102 tok), `openspec_manage` (−115 tok). `file_scope` nested object schema in `design_tree_update` simplified to `items: {type: object}` — field validation is at the Rust handler level.
- **Feature tool output capped at 16,000 chars** — all tool text blocks are truncated after secret redaction in `dispatch_tools`. Catches unbounded feature tool responses (`memory_query` listing all facts, `design_tree list` with 267 nodes, etc.). Native tools (bash 50KB, read 2000 lines) already self-limit; this is a universal safety net. Truncated blocks append `[truncated: N chars dropped — limit 16000]`.
- All provider model catalogs updated to current 2026 IDs (Anthropic, OpenAI, Groq, xAI, Mistral, OpenRouter). Route matrix includes gpt-5 family.
- 1050 tests.

## [0.15.4] - 2026-03-29

### Added

- **Headless OAuth login** — `omegon auth` now detects SSH sessions and Linux environments without a display server (`$DISPLAY`/`$WAYLAND_DISPLAY`) and falls back to a paste-back flow: prints a numbered instruction block, prompts the user to copy the callback URL from their browser's address bar, and parses `code` + `state` from it. The TUI Enter handler delivers the pasted URL directly to the waiting login coroutine via a oneshot channel. Both Anthropic and OpenAI Codex providers use the same path. Previously the login command hung indefinitely on headless machines waiting for a TCP callback that never arrived.
- **Auspex native IPC server** — native Unix socket (`$PWD/.omegon/ipc.sock`) with typed MessagePack framing, versioned handshake, capability negotiation, full state snapshots, filtered event subscriptions, and single-controller enforcement. Auspex clients can now connect directly without HTTP/WebSocket. Full contract defined in `docs/auspex-ipc-contract.md`.
- **Web control-plane startup contract** — machine-readable JSON line on stdout at startup (`omegon.startup` event) with `http_base`, `control_port`, `pid`, and schema version. External tools and CI scripts can now reliably discover the running instance.
- **Dashboard web auth endpoints** — `/api/startup`, `/api/healthz`, `/api/readyz` with resolved auth state (OAuth token, API key, or unauthenticated), enabling Auspex to attach without operator intervention.
- **Unified TUI footer console** — redesigned three-zone operations bar: engine block (provider/model/route/version), inference panel (context composition with bucket legend), and live tools strip. Replaces the old split footer design.
- **Context composition inference panel** — segmented bar showing cached/input/output/reasoning token distribution with a compact legend row. Activity overlay with a "thinking" pulse for extended reasoning turns.
- **Live tool runtimes in footer** — real elapsed time per tool from `ToolStart`/`ToolEnd` events, fixed-width duration field, decay/history strip on the right.
- **Segment copy to clipboard** — `Ctrl+Y` copies the currently selected conversation segment as plain text. `Ctrl+Y` in terminal copy mode copies the selection.
- **Dim segment header timestamps** — every conversation segment shows a muted timestamp in its header, making turn sequencing readable at a glance.
- **Durable tag-link release workflow** — `just link-tag <version>` reuses an already-built tagged binary without a rebuild. Detached-HEAD release cuts are now blocked at the tool layer.

### Fixed

- **TUI — mouse interaction at startup** — mouse capture was declared enabled in state but `EnableMouseCapture` was never emitted to the terminal. Mouse events now work from the first frame.
- **TUI — conversation streaming scroll jank** — streaming chunks no longer trigger excessive relayout. Manual scroll position is preserved during live streaming; auto-scroll only applies when the viewport was already at the bottom.
- **TUI — wrapped editor cursor alignment** — cursor position is now computed against the top border of the editor block, not the terminal origin. Cursor no longer drifts above the editor on multi-line input.
- **TUI — arrow navigation scope** — `↑`/`↓` in the composer navigate history, not the conversation panel. Horizontal arrow keys (`←`/`→`) never steal focus from the conversation. The two navigation contexts are now fully separated.
- **TUI — terminal copy as default** — terminal-native text selection is now on at startup; mouse scroll mode is the non-default opt-in, reversing the previous incorrect default.
- **TUI — inference panel** — replaced placeholder glyph palette with semantically accurate Unicode; memory counts are no longer swallowed by the wave animation; bucket legend labels identify all composition zones.
- **TUI — tool card rendering** — `change`, `read`, `edit` tool cards no longer leave stale trailing glyphs after path text shrinks. Instrument rows are cleared before each redraw. Status language (running/ok/error glyphs) is now consistent between the tool cards and the tools instrument strip.
- **TUI — segment reasoning/answer labels** — thinking blocks are labelled `reasoning` and response content is labelled `answer`; both show full text live during streaming.
- **TUI — input history separation** — scroll fallback no longer bleeds into composer history recall; the two are independently tracked.
- **TUI — engine block layout** — reorganized as aligned label/value rows, home path compacted to `~/…/project`.
- **TUI — startup memory counts** — the splash screen was silently discarding `HarnessStatusChanged` events while draining the broadcast buffer. All three mind slot counts (project / working / episodes) now populate correctly on the first frame instead of showing zero until the next turn completes.
- **Memory — harness status refresh** — after any memory update (store, archive, supersede) the harness status panel is invalidated and redrawn within the same event cycle.
- **Status — nested runtime crash** — `startup_memory_probe` no longer spawns a nested Tokio runtime inside an async context, fixing a panic on startup when memory state was probed before the main runtime was fully initialized.
- **Web — stdout contamination** — log lines no longer leak into stdout alongside the startup JSON contract.
- **Release — detached-head blocking** — `just rc` and `just release` now verify `git branch --show-current` is non-empty before proceeding.
- **CI — ghost publish workflow** — removed a stale publish workflow that was re-triggering on every push and failing silently.

### Changed

- TUI footer is now a unified console; the previous split inference widget and tool sidebar are removed.
- Operator input area defaults to terminal-native selection mode; mouse scroll is toggled with `Ctrl+M`.
- IPC is started automatically alongside the TUI — no separate server process or flag required.
- 1259 tests (up from 983 in 0.15.3).

## [0.15.3] - 2026-03-27

### Added

- **Codebase search** — shipped the `omegon-codescan` crate plus `codebase_search` / `codebase_index` tools for ranked concept search across code and project knowledge.
- **Lifecycle doctor** — design-drift auditing surfaced as an operator tool for catching suspicious lifecycle state before release.
- **Diagnostics and session observability** — startup preflight and child-environment diagnostics, session-log tool exposure, auto-written session narratives, provider usage/rate-limit capture, and RC-channel self-update verification.
- **TUI input and conversation upgrades** — multiline operator editor with wrapped rendering, cursor navigation, visible blinking cursor, Shift+Enter support, copy-mode improvements, soft-card assistant responses, and clearer operator/assistant identity.

### Fixed

- **Cleave/provider routing hardening** — separated OpenAI API routing from Codex OAuth, repaired cross-provider model routing, passed warmed session secrets into children, reset internal workspaces more reliably, and simplified child finalization/cleanup.
- **Secrets and startup behavior** — aligned preflight with the active model, avoided duplicate keychain reads, hydrated configured API keys into the environment, and unified the macOS keychain service name.
- **TUI correctness** — fixed wrapped editor growth, cursor alignment/overflow, manual conversation scroll preservation, dashboard scroll routing, context-window synchronization, memory failure surfacing, and wrapped tool/card height stability.
- **Release/install pipeline** — restored valid nightly/RC automation, tightened `just` release behavior, fixed asset naming and POSIX install compatibility, added signature verification, and now require branch-attached release cuts from `main`.
- **Loop/provider robustness** — hardened LLM call handling, improved 429 overflow compaction behavior, sanitized tool IDs, and omitted invalid unsigned thinking blocks in Anthropic message assembly.

### Changed

- Release workflow now treats RCs as first-class milestones with automated milestone tracking, cleaner nightly draft handling, and stricter branch discipline.
- Session behavior now defaults to auto-resume with a clearer fresh-session escape hatch.
- The TUI status/inference surfaces now emphasize real context, memory, and tool-state telemetry over ornamental noise.

## [0.15.2] - 2026-03-25

### Added

- **Serve tool** — long-lived background process manager for dev servers, watchers, MCP servers. Start, stop, list, logs, check. Auto-cleanup on session exit. Path traversal protection. Zombie prevention.
- **Update checker** — background GitHub Releases API check at startup, toast notification, `/update` command with release notes.
- **Headless smoke tests** (`omegon --smoke`) — 4 scripted tests through the LLM bridge validating response content and tool usage.
- **SegmentMeta rendering** — assistant responses show dim header tag: model, provider, tier, thinking level, active persona.
- **Editor improvements** — placeholder text, dynamic height (3-8 rows), model shortname in prompt, contextual keybinding hints.
- **Ctrl+D sidebar navigation** — navigate the design tree with arrow keys/hjkl, Enter to focus a node, Esc to exit.
- `/tree` slash command — operator access to design tree summary (list, frontier, ready, blocked).
- `/update` slash command — check for and display available updates.
- `just publish` recipe — end-to-end release: pre-flight, push+tags, docs build, link, smoke test.
- `just build-linux-amd64` / `just build-linux-arm64` — local cross-compilation via cargo-zigbuild (zig linker, no containers).
- `just package` — archive all targets with SHA-256 checksums.
- Homebrew formula (`homebrew/Formula/omegon.rb`) with auto-update CI workflow.
- Apple notarization pipeline — async submission via `xcrun notarytool`, Developer ID signing via YubiKey.

### Fixed

- **True single binary** — vendored libgit2 + OpenSSL. Zero runtime dependencies beyond OS system libraries. macOS: 19 MB, Linux: 25 MB.
- **Border consistency** — all TUI panels use `BorderType::Rounded`. No square corners.
- **Ctrl+O segment expansion** — pinned-segment model replaces Tab. Expand and lock a tool card visible.
- **JSON pretty-print** — tool results detected as JSON are formatted with `serde_json::to_string_pretty`.
- **`/focus` collision** — lifecycle bus commands renamed to `design-focus`/`design-unfocus` to avoid shadowing the TUI instrument panel toggle.
- **Squash merge restoration** — Ctrl+D sidebar navigation and `/focus` dedup lost in squash merge re-applied.

### Changed

- Binary size 15 MB to 19 MB (macOS) due to vendored libgit2/OpenSSL — worth the zero-dependency guarantee.
- Tool count 48 to 49 (added `serve`).
- 883 tests (up from 874 in 0.15.1).

### Documentation

- Complete site overhaul for public release: 23 pages (was 13).
- 10 new pages: providers, tutorial, TUI, plugins, sessions, security, contributing, FAQ, migration guide.
- All pages rewritten with current reality — commands, stats, features.
- 4 D2 diagrams: three-axis model, OpenSpec lifecycle, provider routing, cleave architecture.
- Opinionated FAQ: Claude memory vs real memory, personas, license, migration from Claude Code/Codex/Cursor.
- Cleave vs subagents comparison table.
- All `omegon-core` links fixed to `omegon`. All pi references purged. License corrected (MIT conversion, not Apache).
- Landing page with hero, feature grid, install snippet, brew alternative.

## [0.15.1] - 2026-03-25

### Added

- **Provider routing engine** (`routing.rs`) — CapabilityTier (Leaf/Mid/Frontier/Max), ProviderInventory, scored `route()` function, BridgeFactory, per-child cleave routing.
- **OllamaManager** (`ollama.rs`) — structured Ollama server interaction with hardware profiling.
- **OpenAICompatClient** — generic Chat Completions client covering Groq, xAI, Mistral, Cerebras, HuggingFace, Ollama.
- **CodexClient** — OpenAI Responses API client for ChatGPT OAuth JWT tokens with full SSE parsing.
- **10/10 provider matrix**: Anthropic, OpenAI, OpenAI Codex, OpenRouter, Groq, xAI, Mistral, Cerebras, HuggingFace, Ollama.
- **SegmentMeta** — per-segment metadata (provider, model, tier, thinking level, turn, tokens, context%, persona) captured at creation time.
- **Glyph+label tool names** in instrument panel — 48 tools mapped to compact domain-grouped glyphs.
- **Signal-density bar characters** — tool bars degrade ≋ ≈ ∿ · as recency fades.
- `--tutorial` CLI flag for demo overlay activation.
- `read_credential_extra()` and `extract_jwt_claim()` in auth.rs.

### Changed

- **Node.js dependency removed.** SubprocessBridge, `--bridge`, and `--node` CLI flags deleted. The binary is fully self-contained — native Rust clients for all providers.
- **Segment refactored** from flat enum to `Segment { meta: SegmentMeta, content: SegmentContent }`.
- `auto_detect_bridge()` unified: uses `resolve_provider()` for both primary and fallback with priority ordering.
- `intensity_color` uses alpharius teal ramp (was CIE L* with green/olive mid-range).
- Glitch fills both context bar rows during thinking.
- Rounded borders on all panels (instruments, dashboard, tool cards, footer).
- Tutorial text: "AI" → "Omegon" / "the agent" throughout.
- `/tutorial` always starts overlay; legacy lessons via `/tutorial lessons` only.
- Dashboard auto-opens on leaving the "Web Dashboard" tutorial step.

### Fixed

- Tool card separator uses error color (red) when `is_error` is true.
- Tutorial demo choice passes `--tutorial` to exec'd process.
- Tutorial "My Project" choice advances past blank step 0.
- Corrupted design tree titles (exponential backslash doubling).

### Removed

- **SubprocessBridge** — 214 lines of Node.js subprocess management.
- **`--bridge` and `--node` CLI flags** — no longer needed.
- 3 stale feature branches, 11 stale stashes, 3 stale remote tracking branches.

## [0.15.1-rc.76] - 2026-03-25

### Added

- **CodexClient** — OpenAI Responses API client for ChatGPT Pro/Plus OAuth JWT tokens. 350 lines covering: JWT resolution, token refresh, Responses API wire format, SSE parsing for 12 event types, compound tool call IDs, retry with backoff. 7 unit tests.
- **OpenAICompatClient** — generic OpenAI Chat Completions client covering Groq, xAI, Mistral, Cerebras, HuggingFace, Ollama. 6 unit tests.
- 6 missing providers restored to `auth::PROVIDERS`: openai-codex, groq, xai, mistral, cerebras, ollama.
- `read_credential_extra()` and `extract_jwt_claim()` made public in auth.rs.
- Tutorial: `--tutorial` CLI flag activates demo overlay in exec'd processes.
- Tutorial: demo choice auto-advances to Welcome step on "My Project" selection.
- Tool card separator uses error color (red) when `is_error` is true.

### Changed

- Provider matrix: 10/10 complete (was 3/10 after branch restore).
- `auto_detect_bridge()` uses `resolve_provider()` for both primary and fallback, eliminating duplicated client construction.
- CodexClient default model aligned with routing.rs: `codex-mini-latest`.
- Removed dead `provider_inventory` field from App (CleaveFeature probes on demand).
- `/tutorial` always starts the overlay; legacy lessons require explicit `/tutorial lessons`.
- Dashboard opens when operator presses Tab to LEAVE the "Web Dashboard" step.

## [0.15.1-rc.70] - 2026-03-25

### Added

- **SegmentMeta** — every conversation segment now carries rich metadata: timestamp, provider, model_id, tier, thinking_level, turn number, est_tokens, context_percent, persona, branch, duration_ms. Populated from harness state on segment creation.
- **Glyph+label tool names** in instrument panel — 48 tools mapped to compact domain-grouped glyphs (e.g. `▲ d.tree↑` instead of `design_tree_update`).
- **Signal-density bar characters** — tool bars degrade `≋ ≈ ∿ ·` as recency fades (three visual channels: length × color × density).
- **Tutorial auto-opens web dashboard** — the "Web Dashboard" step now fires `StartWebDashboard` on advance instead of telling the operator to type `/dash` (input is locked during tutorial).
- 6 missing providers restored to `auth::PROVIDERS`: openai-codex, groq, xai, mistral, cerebras, ollama.

### Changed

- **Segment refactored** from flat enum to `Segment { meta: SegmentMeta, content: SegmentContent }`. All construction sites migrated to use convenience constructors.
- `intensity_color` replaced CIE L* ramp (green/olive mid-range) with sqrt-perceptual teal ramp matching alpharius primary (#2ab4c8).
- Glitch fills both context bar rows during thinking with row-offset hash for visual variance.
- Tutorial text: all 13 "AI" references replaced with "Omegon" or "the agent".
- Rounded borders on instrument panels and dashboard sidebar (matches tool cards and footer).
- `just link` picks newest binary (release vs dev-release).

### Fixed

- **Provider model mismatch** — `routing.rs` mapped 10 providers but `auth.rs` only listed 9 and `resolve_provider` only handled 3. Restored missing provider entries; `resolve_provider` now explicitly documents unimplemented providers.
- **`provider_inventory` restored on App** — was dropped during branch restore; now populated after splash probes.
- **Lost Justfile recipes** — `rc`, `release`, `sign`, `setup-signing` restored from git history.

## [0.15.1-rc.62] - 2026-03-25

### Added

- **Provider routing engine** (`routing.rs`) — `CapabilityTier` (Leaf/Mid/Frontier/Max), `ProviderInventory`, `ProviderEntry`, scored `route()` function, and `BridgeFactory` for cached bridge instances. Providers are ranked by tier match, cost, and local preference. 8 unit tests.
- **OllamaManager** (`ollama.rs`) — structured Ollama server interaction: `is_reachable()`, `list_models()`, `list_running()`, `hardware_profile()` with Apple Silicon unified memory detection. 5 unit tests.
- **Per-child cleave routing** — `CleaveConfig.inventory` and `ChildState.provider_id` enable scope-aware provider assignment. Children with ≤2 files get Leaf tier, 3–5 get Mid, 6+ get Frontier. Falls back to global model if no inventory or route() returns empty.
- **`auto_detect_bridge()` routing fallback** — when the requested provider is unavailable, fallback now uses the routing engine's scored candidates before the legacy static provider list.
- **Startup inventory probing** — `ProviderInventory::probe()` runs after splash, checking env vars and auth.json for credential availability. Stored on `App` for downstream use.

### Changed

- `resolve_provider()` in `providers.rs` is now `pub` (was crate-private) for `BridgeFactory` access.
- `auth.json` writes now set `0600` permissions on Unix (owner-only read/write).

### Fixed

- **Credential probe bug** — `ProviderInventory::probe()` was reporting all providers as credentialed (checked provider registry instead of actual env vars / auth.json). Fixed to check `env_vars` and `read_credentials()`.
- **Async safety** — replaced `blocking_read()` with `read().await` in cleave dispatch loop to avoid potential deadlock in tokio context.
- **Corrupted design titles** — `startup-systems-check` and `memory-task-completion-facts` had exponential backslash doubling in YAML frontmatter. Replaced with clean titles.
- **Dead code warnings** — suppressed unused `model_for_redetect` variable and `resolve_secret` sync function.
- **90 clippy warnings** resolved via autofix (collapsible-if, map_or simplification, late initialization, format!).

### Removed

- 3 stale feature branches (orchestratable-provider-model, splash-systems-integration, tutorial-system) — all work merged to main.
- 3 stale remote tracking branches pruned from origin.
- 11 stale git stashes referencing dead branches.

## [0.15.0] - 2026-03-21

### Added

- **Interactive tutorial overlay** — 4-act, 10-step onboarding guide compiled into the binary. Four acts: Cockpit (passive UI tour), Agent Works (AutoPrompt — watch the agent read the project and explore a design node), Lifecycle (live cleave demonstration), Ready (wrap-up and power tools). Triggered by `/tutorial` or shown automatically on first run.
  - `Trigger::AutoPrompt` — new trigger type that sends a prompt to the agent automatically on Tab press, then advances the overlay when the agent's turn completes. Operator watches real work happen while the overlay narrates.
  - `Highlight::Dashboard` — positions overlay in the center of the conversation area when demonstrating the sidebar, leaving the design tree fully visible.
  - Large overlay during AutoPrompt steps covers conversation chaos while the agent works; footer instruments remain visible for telemetry.
  - Tab advances, Shift+Tab / BackTab goes back, Esc dismisses. All other keys swallowed while tutorial is active.
  - Auto-dismissed permanently via `.omegon/tutorial_completed` marker.

- **Dashboard sidebar overhaul** — full rewrite using `tui-tree-widget`. Layout: header with inline status badges and pipeline funnel → focused node panel → interactive tree (fills remaining height, scrollable) → OpenSpec changes. Activated via Ctrl+D.
  - Per-node rich text: `status_icon node-id ?N P1 ◈` with color-coded status badges.
  - Parent-child hierarchy, sorted by actionability (implementing → blocked → decided → exploring → seed → deferred). Implemented nodes filtered by default.
  - Degraded nodes (parse failures, missing IDs) shown at top with ⚠ error-colored italic styling. Header badge shows count. Enter on degraded node shows diagnostic info.
  - Pipeline funnel across all 8 statuses with live counts.
  - Periodic rescan every 10 seconds picks up external changes (other Omegon instances, git pull, manual edits).

- **Terminal responsive degradation** — 5-tier progressive layout collapse:
  - Tier 1 (≥120w, ≥30h): sidebar + full 9-row footer
  - Tier 2 (<120w or <30h): full footer, no sidebar
  - Tier 3 (<24h): compact 4-row footer (model+tier+ctx%, session+facts)
  - Tier 4 (<18h): conversation + editor only
  - Tier 5 (<10h or <40w): centered "terminal too small" message
  - Focus mode override always wins; `compute_footer_height()` is a testable function.

- **Theme calibration** — `/calibrate` command with live HSL transform layer over `alpharius.json`:
  - Three parameters: gamma (lightness curve), saturation multiplier, hue shift (degrees).
  - `CalibratedTheme` pre-computes all 23 color fields at construction — zero HSL calculations per frame.
  - Persisted to project profile (`profile.json`) — calibration is per-project, not global.
  - `/calibrate reset` restores identity (1.0, 1.0, 0°).

- **`ai/` directory convention** — unified home for all agent-managed content:
  - `ai/docs/` — design tree markdown documents
  - `ai/openspec/` — OpenSpec lifecycle changes
  - `ai/memory/` — facts.db and facts.jsonl
  - `ai/lifecycle/` — opsx-core state.json
  - `ai/milestones.json`
  - Centralized path resolution in `paths.rs` with fallback chain: `ai/` → legacy (`docs/`, `openspec/`, `.omegon/`) → `.pi/` compat. New writes go to `ai/`; existing projects with legacy layout continue working.

- **`/init` command** — project scanner and migration assistant:
  - Detects: Claude Code (CLAUDE.md), Codex (codex.md), Cursor (.cursor/rules, .cursorrules), Windsurf (.windsurfrules), Cline (.clinerules), GitHub Copilot (.github/copilot-instructions.md), Aider, and pi artifacts (.pi/memory/).
  - Auto-migrates: instructions → `AGENTS.md`, memory → `ai/memory/`, lifecycle state → `ai/lifecycle/`, milestones → `ai/`, auth.json → `~/.config/omegon/`.
  - `/init migrate` moves `docs/` → `ai/docs/` and `openspec/` → `ai/openspec/` with `fs::rename` (same-mount safe).

- **Conversation visual identity** — agent text is plain flowing prose; operator messages get an accent bar + bold. Thinking blocks are dimmed. Tool cards show recency bars and elapsed time. Ctrl+O expands tool card detail.

- **opsx-core crate** — lifecycle FSM with TDD enforcement:
  - `Specs → Testing → Implementing` gate: first-class Testing state between Planned and Implementing; test stubs required before work begins.
  - FSM validates all state transitions before markdown is written. opsx-core is the state guardian; markdown is the content store.
  - JSON file store with atomic writes (write-then-rename). Schema versioning with forward migration stubs.

- **Scanner hardening** — 256 KB file size cap, 1000 files per directory, 128 char ID limit, symlinks skipped. `ScanResult` returns parse failures alongside nodes for degraded node detection without redundant file re-reads.

- **User config path migration** — `~/.config/omegon/` replaces `~/.pi/agent/` for auth tokens, sessions, logs, visuals. Fallback reads from legacy locations for backward compat. Writes always go to primary.

### Changed

- Footer height reduced from 12 → 9 rows; `compute_footer_height()` extracted as testable pure function.
- Dashboard panel width increased from 36 → 40 columns.
- Tab is now the universal "interact with active widget" key (tutorial advance, command completion). Ctrl+O expands tool cards. Shift+Tab / BackTab navigates backward.
- Ctrl+D toggles sidebar navigation mode; arrow keys navigate the tree; Enter focuses selected node via `design-focus` bus command.
- `auth_json_path()` split into read path (legacy fallback) and `auth_json_write_path()` (always primary). All three credential write functions updated.
- `sessions_dir()` split into read (legacy fallback) and `sessions_dir_write()` (always primary).

### Fixed

- Tutorial overlay: uses `card_bg` as surface color, preventing terminal default color bleed-through. Every cell gets explicit bg + fg.
- Tutorial Shift+Tab / BackTab now correctly goes back. `crossterm` sends `KeyCode::BackTab`; the previous code only matched `Tab` + SHIFT modifier.
- Tutorial key events swallowed while overlay is active — previously leaked to sidebar navigator and editor.
- Dashboard step overlay centered in conversation area instead of pinned to x=2 (far left wall).
- Focus mode now collapses footer to 0 rows (was allocating 12 empty rows in focus mode).
- Context bar reduced to 1 row; duplicate context gauge removed from engine panel.
- Lifecycle rescan uses single Mutex lock acquisition — previous double-lock could deadlock.
- Tool card expand moved to Ctrl+O; Tab freed for tutorial and command completion only.

## [0.9.0] - 2026-03-22

### Added
- **CIC Instrument Panel**: Submarine-inspired footer redesign with split-panel layout and four simultaneous fractal instruments providing ambient system awareness.
  - **Split-panel layout**: Engine/memory state (left 40%) + system telemetry (right 60%) replacing the old 4-card footer
  - **Perlin sonar instrument**: Context health monitoring with organic noise patterns responding to token utilization and context pressure
  - **Lissajous radar instrument**: Tool activity visualization using parametric curves that trace call patterns and execution state
  - **Plasma thermal instrument**: Thinking state display with fluid dynamics responding to reasoning intensity and model temperature
  - **CA waterfall instrument**: Memory operations visualization using 1D cellular automata with per-mind columns, CRT noise glyphs, and state-driven evolution rules
  - **Unified navy→teal→amber color ramp**: Perceptual CIE L* color progression from idle navy through stormy teal to amber at maximum intensity across all instruments
  - **Focus mode toggle**: Hide instruments completely for full-height conversation when concentration is needed
  - **Fractal header removal**: Dashboard header collapses as fractal visualization moves to system panel, freeing space for design tree
  - Footer grows from 4 rows to 10-12 rows with conversation absorbing the height loss
- **Per-mind independent CA columns**: Each active memory mind gets its own waterfall column with independent cellular automaton state
- **CRT noise texture**: Waterfall instrument uses authentic terminal glyphs (`▓`, `▒`, `░`) to simulate CRT monitor noise patterns
- **State-driven CA rules**: Cellular automaton evolution rules change dynamically based on memory operation types (injection, compaction, retrieval)
- **Operator-tuned telemetry defaults**: All instrument sensitivity curves hand-tuned for practical submarine operation feel
- **Context caps and error visualization**: Context utilization hard-capped at 70% with amber+red border treatment for error states

### Changed
- Footer layout completely redesigned from horizontal 4-card layout to vertical split-panel with instrument grid
- Color language unified across all instruments using single navy→teal→amber perceptual ramp instead of per-instrument color schemes
- Dashboard header space reallocation provides more room for design tree navigation and git branch topology
- Memory waterfall replaces Clifford attractor for more actionable memory operation feedback

### Fixed
- Perceptual color linearization ensures visible feedback starts at 10% intensity and reaches amber by 80%
- Instrument color distribution rebalanced so amber state gets half the ramp length for better visual distinctness
- Memory event feedback now shows "hotter" activity during injection and compaction operations
- Tool state differentiation with distinct visual patterns for different tool execution phases

## [0.8.0] - 2026-03-17

### Added
- **Mind-per-directive lifecycle**: `implement` forks a scoped memory mind from `default`; all fact reads/writes auto-scope to the directive. `archive` ingests discoveries back to `default` and cleans up. Zero-copy fork with parent-chain inheritance — no fact duplication, parent embeddings and edges are reused.
- **Substance-over-ceremony lifecycle gates**: `set_status(decided)` checks for open questions and recorded decisions instead of artifact directory existence. Design specs are auto-extracted from doc content and archived — no manual scaffolding ceremony.
- **Auto-transition seed → exploring**: `add_research` and `add_decision` on seed nodes automatically transition to exploring and scaffold the design spec.
- **Branch↔mind consistency check**: session start detects if the active directive mind doesn't match the current git branch and surfaces a context message.
- **Dashboard directive indicator**: raised footer shows `▸ directive: name ✓` (branch match) or `▸ directive: name ⚠ main` (mismatch) when a directive mind is active.
- **Multi-layer testing directive**: AGENTS.md "Testing Standards" section, cleave child contract, task file contract, and system prompt guideline all enforce test-writing as a mandatory part of code changes.
- **Design exploration**: directive-branch-lifecycle, multi-instance coordination, lifecycle gate ergonomics, test coverage directive gap, and omegon directive authority design nodes.

### Fixed
- Design tree footer no longer lists decided/implemented/resolved nodes individually — shows only actionable work (exploring, seed, blocked, implementing).
- Context card model/thinking line no longer overflows to `...` — width-aware rendering drops provider prefix and abbreviates thinking in narrow cards.
- Memory card `~30...` truncation fixed — compact separators, width-aware stat selection, `k` suffix for token counts.
- Models card `Driver claude-...` truncation fixed — very compact mode drops role label.
- `getFactsBySection` dedup was backwards (kept parent, discarded child shadow) — fixed to match `getActiveFacts` chain-index pattern.
- `extractAndArchiveDesignSpec` preserves existing scaffold files (tasks.md) in archive.
- Actionable error messages follow `⚠ what → how` pattern with specific commands to run.

## [0.7.8] - 2026-03-17

### Fixed
- Bridged `/assess spec` no longer times out — uses in-session follow-up pattern instead of fragile 120s subprocess. Removes ~150 lines of dead subprocess code.
- Anthropic OAuth login on headless machines no longer fails with `invalid_grant` — token exchange now always uses the localhost `redirect_uri` matching the authorization request.
- Kitty theme ownership marker aligned with generated file content.

## [0.7.7] - 2026-03-16

### Fixed
- Restart script no longer runs `reset` before exec'ing the new process — `reset` outputs terminfo init strings to stdout which the new TUI interprets as keyboard input, causing stray characters ("j") and double "press any key" prompts. RIS via `/dev/tty` + `stty sane` is sufficient.

## [0.7.6] - 2026-03-16

### Fixed
- `/restart` and `/update` restart handoff no longer corrupt the terminal with visible ANSI escape sequences — RIS reset now writes directly to `/dev/tty`, bypassing the TUI layer

## [0.7.5] - 2026-03-16

### Fixed
- Splash auto-dismiss no longer bypasses press-any-key gate

## [0.7.1] - 2026-03-16

### Added
- Glitch-convergence ASCII logo animation on startup with tiered rendering (full sigil on tall terminals, compact wordmark on mid-size, skip on short)
- `/splash` easter egg command to replay the logo animation
- Startup notifications gated behind press-any-key dismissal

### Fixed
- Terminal reset during `/update` restart uses RIS hard reset
- Splash render lines truncated to terminal width
- Splash extension registered in package.json manifest

## [0.6.35] - 2026-03-16

### Fixed
- ANSI escape sequence leakage into editor input
- `/update` recovers from detached HEAD before pulling

## [0.6.27] - 2026-03-15

### Fixed
- Pop kitty keyboard protocol before restart to prevent ANSI barf
- Dashboard compact footer hints moved to base row
- Dashboard raised layout lifecycle artifacts finalized
- Memory facts transport export made explicit

## [0.6.26] - 2026-03-15

### Fixed
- Dashboard 3-column wide layout and compact model badges

## [0.6.25] - 2026-03-15

### Fixed
- Remove duplicate vault dependency entry

## [0.6.24] - 2026-03-15

### Added
- HashiCorp Vault provider for auth status checking

### Fixed
- Remove dead heartbeat, add Vault error patterns
- Use HashiCorp apt repo for vault CLI install on Linux
- Stream install output live and pin permanently

## [0.6.23] - 2026-03-15

### Fixed
- Restart handoff terminal corruption and stale test
- `@mariozechner/clipboard` added as direct optionalDependency for platform-correct native binary
- `--version`/`-v` now reports Omegon version instead of pi-coding-agent's

## [0.6.22] - 2026-03-15

### Fixed
- Brew fallback for all deps, auto-select by available package manager

## [0.6.21] - 2026-03-15

### Added
- HashiCorp Vault provider for auth status checking

## [0.6.20] - 2026-03-15

### Fixed
- Detect ostree read-only root, guide user through nix prereqs

## [0.6.19] - 2026-03-15

### Fixed
- Remove invalid `--init none` flag from nix installer

## [0.6.18] - 2026-03-15

### Fixed
- Restart via detached script to avoid TUI collision

## [0.6.17] - 2026-03-15

### Fixed
- Nix `--init none` for immutable distros, readable failure output

## [0.6.16] - 2026-03-15

### Fixed
- Clean terminal reset before restart, use shell exec

## [0.6.15] - 2026-03-15

### Fixed
- Proactively patch PATH for nix/cargo at module load

## [0.6.14] - 2026-03-15

### Fixed
- Nix install `--no-confirm` for headless, skip nix in runtime health check

## [0.6.13] - 2026-03-15

### Added
- Auto-restart after `/update`, add `/restart` command
- Nix as universal package manager, suppress pi resource collisions

### Fixed
- Clipboard diagnostic uses correct default export and sendMessage API
- Shared-state test import path updated after module relocation
- Merge consecutive `say()` calls; ASCII emoji fallback for legacy Windows console

## [0.6.11] - 2026-03-15

### Fixed

- **Orphaned subprocess elimination** — Cleave child processes spawned with `detached: true` now have three layers of cleanup defense: (1) `process.on('exit')` handler that SIGKILLs all tracked children synchronously when the parent exits for any reason, (2) PID file tracking in `$TMPDIR` with startup scan that kills orphans from dead parents, (3) SIGKILL escalation timer no longer `.unref()`'d so it actually fires during shutdown. Previously, if the parent process crashed or was killed, `session_shutdown` never fired and detached children survived indefinitely.
- **Nested cleave prevention** — Cleave extension now exits immediately when `PI_CHILD=1` is set, preventing child processes from registering cleave tools or spawning nested subprocesses. Previously, every cleave child loaded the full cleave extension, creating a vector for exponential process growth.
- **Lifecycle batch ingest contention** — `ingestLifecycleCandidatesBatch` no longer wraps the full batch in a single transaction, reducing SQLite write-lock hold time and SQLITE_BUSY errors when concurrent processes share the database.

## [0.6.9] - 2026-03-15

### Fixed

- **Cleave subprocess lifecycle** — Cleave child dispatch and spec-assessment subprocesses now spawn with `detached: true`, are tracked in a shared process registry, and are killed by process group (`-pid`). A `session_shutdown` handler sweeps all tracked processes with SIGTERM→SIGKILL escalation, preventing orphaned `pi` processes from accumulating and causing runaway CPU/thermal issues.

## [0.6.7] - 2026-03-15

### Fixed

- **Memory injection budget discipline** — project-memory now uses a tighter routine-turn budget and only adds structural filler, episodes, and global facts on higher-signal turns, reducing repeated prompt overhead while keeping high-priority working memory first.
- **Node runtime guardrails** — Omegon now declares Node.js 20+ at the root package boundary and fails early during install on unsupported runtimes instead of crashing later on Unicode `/v` regex parsing in bundled pi-tui.
- **Design assessment stability** — `/assess design` no longer depends on a nested subprocess successfully loading a second extension graph to produce a result.
- **Cleave volatile runtime hygiene** — `.pi/runtime/operator-profile.json` is treated as volatile runtime state instead of blocking cleave dirty-tree preflight.

## [0.6.6] - 2026-03-15

### Fixed

- **Internal subprocess boundary hardening** — Cleave child dispatch, bridged assess subprocesses, and project-memory subprocess fallback now re-enter Omegon explicitly through the canonical Omegon-owned entrypoint instead of depending on PATH resolution of the legacy `pi` alias.
- **Memory search stability** — FTS-backed fact search now tolerates apostrophes and preserves useful recall for technical identifier/path-like queries while continuing to surface unrelated operational storage failures instead of silently returning empty results.

## [0.6.0] - 2026-03-11

### Added

- **Dashboard: raised view horizontal split layout** — The `/dash` raised view is now a proper full-height multi-zone panel:
  - **Git branch tree** (full-width, top) — unicode tree rooted at repo name (`─┬─`, `├─`, `└─`) with current branch highlighted, branches color-coded by prefix, and design node annotations (`◈ title`) for branches matched to active design nodes
  - **Two-column split** (at ≥120 terminal columns) — Design Tree full-width above; Recovery+Cleave left, OpenSpec right, separated by `│`
  - **No line cap** — raised mode renders as much content as needed; the 10-line holdover from compact-first thinking is gone
  - **Narrow stacked layout** (<120 cols) — all sections top-to-bottom with the branch tree at the top
  - Branch inline in footer suppressed when raised (tree above covers it, no duplication)
- **`render-utils.ts`** — Shared column-layout primitives built on `visibleWidth()` from `@mariozechner/pi-tui`: `padRight`, `leftRight`, `mergeColumns`. Eliminates all hand-rolled ANSI-stripping width calculations. Correctly handles OSC 8 hyperlink sequences that the old regex approach missed, fixing the column misalignment visible in the previous raised view.
- **`git.ts`** — `readLocalBranches(cwd)` reads `.git/refs/heads/` recursively without shell spawning. `buildBranchTreeLines()` renders the unicode branch tree with sort order (main/master → feature → refactor → fix → rest) and design node annotations.
- **Design tree dashboard state** — `nodes[]` now includes `branches: string[]` so the branch tree can annotate branches with their linked design node titles.

### Fixed

- **Cleave wave progress** — Progress messages now show both wave position and child position: `Wave 3/3 (child 4/4): dispatching footer-layout`. Previously "Wave 3/3" while the dashboard showed "3/4 children done" — same numbers, different meanings.
- **README: broken pi dependency link** — `nicolecomputer/pi-coding-agent` (404) replaced with `badlogic/pi-mono`.
- **README: 9 additional corrections** — Extension count (23→27), skill count (7→12), missing extensions (dashboard, tool-profile, vault, version-check), missing skills (typescript, pi-extensions, pi-tui, security, vault), duplicate Model Budget section, fabricated OpenAI model names in effort tier table, missing prompt templates (init, status), `shared-state` removed from utilities (internal lib).

## [0.5.4] - 2026-03-10

### Fixed

- **Dashboard: suppress `T0` turn counter at session start** — The context gauge no longer renders `T0` before the first assistant turn completes. The turn prefix appears naturally from `T1` onward.
- **Dashboard: replace unintelligible memory audit labels** — `"Memory audit: no injection snapshot"` (shown before the first injection) replaced with `"Memory · pending first injection"`. Injection mode `"full"` renamed to `"bulk"` throughout (`MemoryInjectionMode`, dashboard audit line, tests) — `full` read as "memory is full" rather than "all-facts dump".

## [0.5.3] - 2026-03-10

### Fixed

- **Dashboard Ctrl+Shift+D shortcut shadowed by pi-tui debug handler** — Toggle binding moved to `Ctrl+Shift+B`; pi-tui hardcodes `Ctrl+Shift+D` as a global debug key, intercepting it before any extension shortcut could fire.

## [0.5.2] - 2026-03-10

### Added

- **Design doc lifecycle and reference documentation** — Implemented three-stage close-out pipeline: design exploration journals archived to `docs/design/`, distilled reference pages generated in `docs/`, and pointer facts ingested into project memory. 15 subsystem reference pages covering dashboard, cleave, model routing, error recovery, operator profile, design tree, OpenSpec, project memory, slash command bridge, quality guardrails, view, render, tool profiles, secrets, and local inference.
- **`/migrate` command** — Detects completed design docs in `docs/` and archives them to `docs/design/` via `git mv`. Interactive confirmation with preview. Bridged via `SlashCommandBridge` for agent access. Session-start hint notifies when migration is available.
- **`/init` migration hint** — The `/init` prompt template now checks for unmigrated design docs and surfaces a `/migrate` hint in the project orientation summary.

## [0.5.1] - 2026-03-10

### Added

- **Image zoom and scale controls** — `/view` now accepts scale arguments (`compact`, `normal`, `large`, `full`, `2x`, `3x`) to control rendered image size. `/zoom` opens the last viewed image in a fullscreen overlay at terminal-filling size. The `view` tool accepts a numeric `scale` parameter for agent-driven rendering. Tab completions provided for both commands.

### Fixed

- **Secrets configure no longer shows pasted values** — `/secrets configure` now reads secret values from the clipboard instead of displaying them in the TUI input field. Copy the value first, confirm, and the extension reads it via `pbpaste`/`xclip`/`xsel`/`wl-paste`. Falls back to direct input with a warning only if no clipboard command is available.

## [0.5.0] - 2026-03-10

### Added

- **Upstream error recovery and fallback signaling** — Omegon now classifies upstream provider failures into structured recovery events, applies bounded retry or failover, and surfaces recovery state to the dashboard and agent.
  - Failure taxonomy in `extensions/lib/model-routing.ts`: `retryable-flake`, `rate-limit`, `backoff`, `auth`, `quota`, `tool-output`, `context-overflow`, `invalid-request`, `non-retryable`.
  - Same-model retry bounded to one attempt per request fingerprint; retry ledger clears on next successful turn.
  - Rate limits and explicit backoff trigger candidate cooldown and failover through existing routing.
  - Non-transient failures (auth, quota, malformed output, context overflow) are never generic-retried.
  - Extension-driven retry fallback for structured error codes (e.g. Codex JSON `server_error`) that pi core's regex misses.
  - Recovery state visible in dashboard shared state (`latestRecoveryEvent`, `recovery`).
- **Invalid request error classification** — oversized image errors (>8000px), `invalid_request_error`, and other 400-class API rejections are now classified as `invalid-request` with actionable operator guidance instead of surfacing as raw JSON.
- **Slash command bridge for all commands** — all Omegon slash commands are now registered with a shared `SlashCommandBridge` singleton, so the agent can invoke them via `execute_slash_command`.
  - 7 OpenSpec commands bridged as agent-callable: `/opsx:propose`, `/opsx:spec`, `/opsx:ff`, `/opsx:status`, `/opsx:verify`, `/opsx:archive`, `/opsx:apply`.
  - `/dashboard` and `/dash` bridged with `agentCallable: false` — returns structured refusal instead of opaque "not registered" error.
  - Shared bridge via `getSharedBridge()` in `extensions/lib/slash-command-bridge.ts` (Symbol.for global singleton).
  - Side-effect metadata: `read` for status/verify/apply, `workspace-write` for propose/spec/ff/archive.
- **Cleave child progress emission** — `emitCleaveChildProgress()` in `extensions/cleave/dispatcher.ts` now updates shared state and emits `DASHBOARD_UPDATE_EVENT` so the terminal title and dashboard footer reflect child progress in real time.

### Changed

- OpenSpec commands converted from plain `pi.registerCommand()` to bridge-registered with `structuredExecutor` and `interactiveHandler` separation.
- Cleave `/assess` now uses the shared bridge instance instead of creating a local one.
- Operator fallback logic extended with cooldown tracking and alternate candidate resolution for rate-limited providers.

### Fixed

- Terminal tab title now updates dynamically as cleave child progress changes (was static after initial render).
- Assess spec bridge tests no longer depend on a real active OpenSpec change — tests scaffold a temporary fixture and clean up after themselves.
- Dashboard footer recovery section renders safely when recovery state is absent or partially rolled out.

## [0.4.1] - 2026-03-09

### Fixed

- **Raised dashboard footer cleanup** — wide raised mode now stays vertically stacked instead of rendering Design Tree, OpenSpec, and Cleave as a single bleeding cross-row status strip.
- Raised dashboard truncation now applies against full-width rows, so long design and OpenSpec labels remain recognizable instead of getting mangled by the split layout.

## [0.4.0] - 2026-03-09

### Added

- **Operator capability profiles** — `.pi/config.json` can now persist operator-visible capability intent and fallback policy, with public roles (`archmagos`, `magos`, `adept`, `servitor`, `servoskull`), explicit thinking ceilings, and runtime cooldown state kept separate from durable preferences.
- **Allowlisted slash-command bridge** — the harness can now invoke approved slash commands through a structured, machine-readable bridge.
  - Added generic bridge primitives in `extensions/lib/slash-command-bridge.ts`.
  - Bridged `/assess spec`, `/assess diff`, `/assess cleave`, and `/assess complexity` while keeping bare `/assess` interactive-only in v1.
- **OpenSpec assessment lifecycle authority** — each active change now persists its latest structured lifecycle assessment in `openspec/changes/<change>/assessment.json`.
  - `/opsx:verify` now reuses current persisted assessments or prompts refresh when the implementation snapshot has drifted.
  - `/opsx:archive` now fails closed on missing, stale, ambiguous, or reopened assessment state.
  - Post-assess reconciliation now persists structured lifecycle assessment results for later gates.

### Changed

- OpenSpec, Cleave, and Assess now compose around structured assessment records instead of relying on operator memory or prose-only review output.
- Operator profile schema was finalized around canonical candidate fields:
  - `source: "upstream" | "local"`
  - `weight: "light" | "normal" | "heavy"`
- Dashboard compact/raised views now truncate more cleanly and use a wider deep view layout.

### Fixed

- Dashboard footer layout no longer wastes horizontal space in deep view.
- Operator profile parsing now normalizes legacy `frontier` source values and numeric weight inputs.
- Structured lifecycle assessment metadata now survives the `/assess` bridge path consistently.

## [0.3.2] - 2026-03-09

### Changed

- **Provider-aware model control copy** — `/local`, `/haiku`, `/sonnet`, `/opus`, and `set_model_tier` now describe provider-neutral capability tiers instead of sounding Anthropic-only.
  - Model-switch notifications now include the resolved concrete provider/model so routing decisions are visible at runtime.
  - Effort startup and tier-switch notifications also report the resolved provider/model.
- **Dashboard compact footer cleanup** — compact mode now renders a single dashboard-first line instead of duplicating footer metadata into extra lines.
  - Compact mode still shows the active model inline on wide terminals for at-a-glance provider awareness.

### Fixed

- **Last-used driver persistence** — Omegon now persists the last successfully selected concrete driver model in `.pi/config.json` and restores it on session start before falling back to effort-tier defaults.
- Compact dashboard footer no longer looks like the built-in footer is still leaking through.

## [0.3.1] - 2026-03-09

### Changed

- **Dashboard overlay openability UX** — openable rows are now visibly marked and the overlay selects the first openable item instead of the non-openable summary row.
  - `extensions/dashboard/overlay.ts` adds a `↗` marker for rows with `openUri`, lets `Enter` open non-expandable items, and surfaces inline status feedback when a row cannot be opened.
  - Footer copy now accurately describes open behavior and no longer implies every row is clickable.
- **Design tree context summary clarity** — the generic design-tree session summary now reports implemented and implementing counts instead of implying only `decided` nodes matter.
  - `extensions/design-tree/index.ts` now emits summaries like `implemented — implementing — decided — exploring — open questions`.

### Fixed

- Dashboard open behavior no longer appears broken when focus starts on the summary row.
- Design-tree summary text no longer hides implemented nodes.

## [0.3.0] - 2026-03-08

### Added

- **Post-assess lifecycle reconciliation** — assessment outcomes can now feed back into lifecycle state instead of leaving OpenSpec and design-tree artifacts stale after review/fix cycles.
  - `extensions/openspec/reconcile.ts` adds explicit post-assess outcomes: preserve verifying, reopen implementing conservatively, append implementation-note deltas, and emit ambiguity warnings.
  - `openspec_manage` now supports `reconcile_after_assess` so assessment/review loops can refresh lifecycle state programmatically.
  - Design-tree implementation notes can now absorb follow-up file-scope and constraint deltas discovered during post-assess fixes.
- **Reusable design-tree dashboard emitter** — `extensions/design-tree/dashboard-state.ts` centralizes dashboard-state emission so lifecycle reconciliation can refresh the design-tree view without duplicating logic.
- **Lifecycle artifact tracking guard** — `npm run check` now fails if durable lifecycle artifacts under `docs/` or `openspec/` are left untracked.
  - Added `extensions/openspec/lifecycle-files.ts` and tests for git-status parsing, durable artifact classification, and actionable failure messaging.
- **New baseline lifecycle specs**
  - `openspec/baseline/lifecycle/post-assess.md`
  - `openspec/baseline/lifecycle/versioning.md`

### Changed

- OpenSpec lifecycle guidance now treats post-assess reconciliation as a required checkpoint before archive, not an operator memory task.
- Repository contribution policy now explicitly distinguishes durable lifecycle documentation (`docs/`, `openspec/`) from transient cleave runtime artifacts.

### Fixed

- Archiving lifecycle changes now remains compatible with the new durability guard because archive outputs and baseline files are committed as part of the release-ready workflow.
- Assessment/review loops no longer leave verifying changes misleadingly closed when follow-up implementation work is still required.

## [0.2.0] - 2026-03-07

### Added

- **Effort Tiers extension** (`extensions/effort/`) — single global knob controlling local-vs-cloud inference ratio across the entire harness. Seven named tiers from fully local to all-cloud: Servitor (0% cloud) → Average → Substantial → Ruthless → Lethal → Absolute → Omnissiah (100% cloud). Inspired by Space Marine 2 difficulty levels.
  - `/effort <name>` — switch tier mid-session; applies immediately to next decision point
  - `/effort cap` — lock current tier as ceiling; agent cannot upgrade past it
  - `/effort uncap` — remove ceiling lock
  - Each tier controls: driver model + thinking level, extraction model, compaction routing, cleave child floor/preferLocal, and review model
  - Cap derives ceiling from `capLevel` via `tierConfig()` — survives subsequent `/effort` switches without breaking
  - Tiers 1–5 use local extraction and local compaction; tiers 6–7 escalate to cloud

- **Local model registry** (`extensions/lib/local-models.ts`) — single source of truth for all local model preferences. Edit one file; all consumers (offline-driver, effort, cleave, project-memory) update automatically.
  - `KNOWN_MODELS` — metadata (label, icon, contextWindow, maxTokens) for 30+ models
  - `PREFERRED_ORDER` — general orchestration, quality-first: 70B → 32B → MoE-30B → 14B → 8B → 4B → sub-3B
  - `PREFERRED_ORDER_CODE` — code-biased ordering for cleave leaf workers
  - `PREFERRED_FAMILIES` — prefix catch-alls for `startsWith` matching (catches quantization-tagged variants)
  - Full hardware spectrum: 64GB (72B/70B), 32GB (32B), 24GB (MoE-30B/14B), 16GB (8B), 8GB (4B)

- **New models in registry**: `qwen3-coder:30b` (MoE, 30B total/3.3B active, ~18GB at Q4, 262K context, SWE-Bench trained — best local code-agent at its size), `devstral:24b` (current canonical Ollama tag, 53.6% SWE-Bench verified), plus full 8B/14B/4B tiers for smaller hardware.

- **Local-first extraction** — `project-memory` now routes extraction to Ollama via direct HTTP (`runExtractionDirect`) instead of spawning a pi subprocess, bypassing the `--no-extensions` limitation. Falls back to cloud Sonnet only if Ollama is unreachable.

- **Local-first compaction** — `compactionLocalFirst: true` by default; `session_before_compact` intercepts and routes to local Ollama. Cloud is fallback only. `applyEffortToCfg()` re-applies tier overrides at call-time so mid-session `/effort` switches take effect immediately.

- **Scope-based cleave autoclassification** — `classifyByScope()` in `dispatcher.ts`: ≤3 non-test files → local, 4–8 → sonnet, 9+ → opus. Test files (`.test.ts`, `.test.js`, `.spec.ts`, `.spec.js`) excluded from count. Layered under explicit annotations and effort floor.

- **Rich terminal tab titles** (`extensions/terminal-title/`) — tab bar shows active tool chain, cleave progress, turn count, and model tier.

### Changed

- `offline-driver` expanded with full model registry spanning 8GB–128GB hardware. `PREFERRED_ORDER` and `PREFERRED_ORDER_CODE` re-exported from `lib/local-models.ts`.
- `project-memory` default `extractionModel` changed from `claude-sonnet-4-6` to `devstral-small-2:24b`.
- Cleave child local model selection uses `PREFERRED_ORDER_CODE` preference list instead of `models[0]` (non-deterministic). Prefers `qwen2.5-coder:32b` → `qwen3-coder:30b` → `devstral:24b` → ... → `qwen3:4b`.
- `/effort` slash commands (`/opus`, `/sonnet`, `/haiku`) now enforce the effort cap — no silent bypass.
- `AbortSignal.any()` gracefully falls back on Node.js < 20.3 (was a hard crash).
- Duplicate cloud model string extracted to `EFFORT_EXTRACTION_MODELS` constant in project-memory.

### Fixed

- **Cap ceiling bug** — `checkEffortCap` now derives ceiling from `capLevel` via `tierConfig()`, not `effort.driver`. Cap survived tier switches incorrectly before this fix.
- **Tier matrix divergence** — Ruthless (4) and Lethal (5) corrected to `extraction: "local"` and `compaction: "local"` per design matrix (cleave child implemented them with cloud extraction).
- **Average ≠ Servitor** — Average tier differentiated: `thinking: "minimal"`, `cleavePreferLocal: false` (scope-based local bias, not forced-local). Was byte-for-byte identical to Servitor.
- **`isLocalModel()` heuristic** — replaced fragile `startsWith("claude-")` check with `CLOUD_MODEL_PREFIXES` allowlist (GPT, Gemini, etc. no longer misclassified as local).
- **Dead code** — `COMPLEX_FILE_PATTERNS` array defined but never used removed from `dispatcher.ts`.
- `tierConfig()` docstring corrected (was "Frozen", returns shared reference).
- `capLevel` non-null assertion replaced with proper guard in effort status display.
- Dead `haiku` key removed from `MODEL_PREFIX` in effort extension (haiku is not a valid driver tier).

## [0.1.3] - 2026-03-07

### Added

- **Non-capturing dashboard overlay** — new `panel` mode renders the dashboard as a persistent side panel that doesn't steal keyboard input, using pi 0.57.0's `nonCapturing` overlay API. `focused` mode enables interactive navigation within the panel.
- **4-state dashboard cycle** — `/dashboard` now cycles through `compact → raised → panel → focused`. Direct subcommands: `/dashboard panel`, `/dashboard focus`, `/dashboard open` (legacy modal).
- **Tab completions** for `/dashboard` subcommands (`compact`, `raised`, `panel`, `focus`, `open`).
- **Footer `/dashboard` hint** — compact footer now shows `/dashboard` for discoverability.

### Changed

- Dashboard keybind changed from `ctrl+shift+b` to `` ctrl+` `` — the previous binding was intercepted by Kitty terminal's default keymap (`move_window_backward`) and never reached pi.
- Upgraded `@mariozechner/pi-coding-agent` and `@mariozechner/pi-ai` to `^0.57.0`.

### Fixed

- Dashboard keybind was silently non-functional due to Kitty terminal default keymap collision.

## [0.1.2] - 2026-03-07

### Added

- **Version-check extension** — polls GitHub releases on session start and hourly. Notifies operator to run `pi update` when a newer release exists. Respects `PI_SKIP_VERSION_CHECK` and `PI_OFFLINE` env vars.

### Fixed

- Test command glob now includes root-level `extensions/*.test.ts` files (were silently missed by `**` glob).

### Changed

- README documents main-branch tracking limitation with link to [#5](https://github.com/cwilson613/pi-kit/issues/5).

## [0.1.1] - 2026-03-07

### Added

- **Scenario-first task generation** — cleave child tasks are now matched to spec scenarios using 3-tier priority: spec-domain annotations (`<!-- specs: domain -->`) → file scope matching → word-overlap fallback. Prevents cross-cutting spec scenarios (e.g., RBAC enforcement) from falling between children when tasks are split by file layer.
- **Orphan scenario auto-injection** — any spec scenario matching zero children is automatically injected into the closest child with a `⚠️ CROSS-CUTTING` marker for observability.
- **`TaskGroup.specDomains`** — parsed from `<!-- specs: ... -->` HTML comments in tasks.md group headers for deterministic scenario-to-child mapping.
- **`matchScenariosToChildren`** — exported function for pre-computing scenario assignments across all children with orphan detection.

### Fixed

- Domain matching is now path-segment-aware (`relay` no longer matches `relay-admin/permissions`).
- Scope matching uses word-boundary regex instead of substring (prevents `utils.py` matching "utility").
- `ChildPlan.specDomains` normalized to required `string[]` (was optional, causing type inconsistency with `TaskGroup`).

### Changed

- `buildDesignSection` in workspace.ts uses pre-computed scenario assignments instead of per-child word-overlap heuristic.
- `skills/openspec/SKILL.md` updated with scenario-first grouping guidance and annotation examples.
- `skills/cleave/SKILL.md` updated with annotation syntax and orphan behavior documentation.

## [0.1.0] - 2026-03-07

Initial public release.

### Added

- **OpenSpec extension** — spec-driven development lifecycle: propose → spec → design → tasks → verify → archive. Given/When/Then scenarios as acceptance criteria. Delta-spec merge on archive. API contract derivation from scenarios (`api.yaml`).
- **Design Tree extension** — structured design exploration with persistent markdown documents. Frontmatter-driven status tracking, open question syncing, branching from questions, and OpenSpec bridge (`/design implement` scaffolds change from decided node).
- **Cleave extension** — recursive task decomposition with parallel execution in git worktrees. Complexity assessment, OpenSpec integration (tasks.md as split plan, design context enrichment, task completion writeback). Code assessment: `/assess cleave` (adversarial + auto-fix), `/assess diff` (review), `/assess spec` (validate against scenarios + API contract), `/assess complexity`.
- **Project Memory extension** — persistent cross-session knowledge in SQLite+WAL. 11 tools for store/recall/query/supersede/archive/connect/compact/episodes/focus/release/search-archive. Semantic retrieval via Ollama embeddings (FTS5 fallback). Background fact extraction. Episodic session narratives. JSONL export/import with `merge=union` for git sync.
- **Local Inference extension** — delegate sub-tasks to Ollama models at zero API cost. Auto-discovers available models on session start.
- **Offline Driver extension** — switch driving model from cloud to local Ollama when connectivity drops. Auto-selects best available model (Nemotron, Devstral, Qwen3).
- **Model Budget extension** — switch model tiers (opus/sonnet/haiku) and thinking levels (off/minimal/low/medium/high) to match task complexity and conserve API spend.
- **Render extension** — FLUX.1 image generation via MLX on Apple Silicon, D2 diagram rendering, Excalidraw JSON-to-PNG.
- **Web Search extension** — multi-provider search (Brave, Tavily, Serper) with quick/deep/compare modes and deduplication.
- **MCP Bridge extension** — connect external MCP servers as pi tools via stdio transport.
- **Secrets extension** — resolve secrets from env vars, shell commands, or system keychains via declarative `@secret` annotations.
- **Auth extension** — authentication status, diagnosis, and refresh across git, GitHub, GitLab, AWS, k8s, OCI registries.
- **Chronos extension** — authoritative date/time from system clock, eliminates AI date calculation errors.
- **View extension** — inline file viewer for images, PDFs, documents, and syntax-highlighted code.
- **Auto-compact extension** — context pressure monitoring with automatic compaction.
- **Defaults extension** — auto-deploys AGENTS.md and theme on first install with content-hash guard to prevent overwrites.
- **Distill extension** — context distillation for session handoff.
- **Session Log extension** — append-only structured session tracking.
- **Status Bar extension** — severity-colored context gauge with memory usage and turn counter.
- **Terminal Title extension** — dynamic tab titles for multi-session workflows.
- **Spinner Verbs extension** — themed loading messages.
- **Style extension** — Verdant design system reference.
- **Shared State extension** — cross-extension state sharing.
- **Skills**: openspec, cleave, git, oci, python, rust, style.
- **Prompt templates**: new-repo, oci-login.
- **Global directives**: attribution policy (no AI co-author credit), spec-first development methodology, API contract requirement (OpenAPI 3.1 derived from scenarios), runtime validation middleware guidance, completion standards, memory sync rules, branch hygiene.
- **Documentation**: README with architecture diagram, spec pipeline diagram, memory lifecycle diagram. CONTRIBUTING.md with branching policy, memory sync architecture, and cleave branch cleanup.

### Security

- Path traversal hardening in view and render extensions.
- Command injection prevention in cleave worktree operations.
- Design tree node ID validation.
