+++
id = "75315b06-0947-44f3-ba98-90348120509d"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Changelog

All notable changes to Omegon are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/). Versioning: [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **Native `/plan` session gate** — added a first-class TUI/remote slash surface over the existing session intent work plan with `set`, `approve`, `execute`, `advance`, `skip`, `clear`, and status rendering so high-level plan mode can reuse the current conversation state instead of creating a shadow planning store.
- **Unified Armory install surface** — `/armory install`, `armory/install`, extension installs, and named skill installs now route through one Armory installer that materializes extensions, plugins, and skills into the runtime paths Omegon actually loads.

### Fixed

- **Armory installation is reachable and discoverable from the TUI** — command suggestions, slash usage, ACP help, browse output, dispatcher routing, and post-install messages now point operators at `/armory install`, `/skills install <name>`, and `/extension install <name|url|path>` instead of leaving registry installs as a hidden CLI path.
- **Queued TUI prompts no longer interrupt by default** — submitting a follow-up while the agent is active now queues it until the current turn finishes instead of cancelling the active turn under the misleading "queued" banner. Explicit interrupt queue mode still cancels when selected.
- **Web search timeout path no longer burns one timeout per free engine** — automatic web search now tries DuckDuckGo, Bing, and Google through the shared concurrent failover path instead of spending a full sequential timeout on each free engine, and the tool schema exposes a real `timeout` parameter.
- **Validate failures identify rejected paths** — `validate` now reports each unsupported path and each supported source file missing a project validator instead of only saying the supported source types.
- **Non-UTF8 read and shell output errors are actionable** — `read` now reports the path and invalid byte offset for non-UTF8 text files or identifies binary files, and `bash` output capture no longer fails the whole command when stdout/stderr contains invalid UTF-8 bytes.
- **Image tool results survive the full surface stack** — `view`, `read`, render tools, and MCP image outputs now keep structured image payloads in the LLM-facing tool result path, expose local render metadata for the TUI, and report explicit terminal render-path failures instead of silently degrading to metadata-only success.

## [0.22.0] - 2026-05-14

### Added

- **Librefang integration surface plan** — added a private architecture plan for treating Librefang as an external peer runtime through OpenAI-compatible provider routing, Armory discovery, MCP templates, and a future Auspex/OFP bridge rather than vendoring its overlapping runtime into Omegon core.

### Changed

- **Anthropic subscription automation wording** — docs and TUI consent text now match the current runtime behavior: headless Anthropic subscription OAuth emits an explicit operator-risk warning and proceeds, while `ANTHROPIC_API_KEY` remains the recommended path for policy-clean automation.
- **Architecture docs workspace alignment** — corrected current operator-facing docs for the root Cargo workspace, `just link` alias behavior, Pkl schema count, and root-level Cargo test/release commands.
- **OpenSpec lifecycle crate is first-class** — `omegon-opsx` now inherits workspace version, edition, license, and repository metadata so the OpenSpec FSM ships in lockstep with Omegon.
- **OpenSpec write-side FSM authority** — `openspec_manage propose` now creates an `omegon-opsx` change record, `add_spec` registers spec domains and advances through the validated `proposed -> specced` transition, and `status`/`get` expose the FSM state alongside file-derived stage details.
- **OpenSpec legacy FSM bootstrap** — `openspec_manage status/get` now backfills existing file-backed changes into `omegon-opsx`, registers parsed spec domains and task counts, advances through validated early states, and `lifecycle_doctor` reports OpenSpec state drift.
- **OpenSpec stage authority** — `openspec_manage` now reports `stage` from the `omegon-opsx` FSM, preserves parsed markdown state as `file_stage`, registers task progress from `tasks.md`, requires explicit test-file registration before implementation, and only archives changes that have reached `verifying`.
- **Single-stream OpenSpec archive** — archiving now runs through one `omegon-opsx` lifecycle operation that validates the FSM, moves the OpenSpec content, persists state, and rolls content back if state persistence fails.
- **OpenSpec archive drift detection** — documented the JSON/content crash window and taught `lifecycle_doctor` to flag archived OpenSpec content whose `omegon-opsx` state is missing or not archived.
- **OpenSpec guidance alignment** — updated runtime prompts, tutorial copy, Sentry logging, and the bundled OpenSpec skill to direct agents through `register_tasks` and `register_test_file` instead of treating `tasks.md` edits as lifecycle transitions.
- **Lifecycle read-model projection** — added a shared lifecycle read handle that projects OpenSpec status from `omegon-opsx` plus file diagnostics, and migrated startup, TUI, web, and IPC snapshots away from raw file-derived OpenSpec stages.
- **Justfile workspace hygiene** — normalized local recipes around the root Cargo workspace, removed stale `core/` path assumptions, made external sibling-repo checks opt-in when present, and restored a passing local `just lint` gate without hiding existing clippy warnings.
- **Strict clippy hygiene** — cleaned workspace clippy warnings across libs, bins, examples, and tests, then restored `just lint` as a `-D warnings` all-target gate.
- **Release automation hygiene** — opened the `0.22.0-rc.1` line and corrected ignore rules for committed generated assets so release-plz can evaluate the workspace without reporting a synthetic dirty tree.
- **Release branch migration groundwork** — release preflight, local release recipes, CI tests, and site validation now recognize `release/X.Y` hardening branches while preserving the current mainline release flow.
- **Branch-based release helpers** — added `just branch-release` to create/push the matching `release/X.Y` branch for an RC line and `just merge-release-forward` to merge hardening fixes back to `main` without regressing main's version-state files.

## [0.21.2] - 2026-05-15

### Fixed

- **Zero-key web search failover** — automatic `web_search` no longer pins Google first when no API search key is configured. It now falls through across DuckDuckGo, Bing, and Google after API providers, avoiding hard failure when Google serves a bot/CAPTCHA page.

## [0.21.1] - 2026-05-13

### Fixed

- **Commit-nudge prompt churn** — successful `git commit` or `jj commit` commands run through `bash` now clear the modified-file intent state, preventing stale commit-hygiene nudges after work has already been committed.
- **Nix/OCI release packaging** — the flake source filter now includes catalog `*.jsonl` files required by embedded agent mind facts, fixing OCI image builds for release tags.

## [0.21.0] - 2026-05-13

### Added

- **Operator secret aliases for Vault** — Vault token auth can now load a token from an Omegon-managed secret via `vault.json` `auth.secret_name`, enabling flows like storing `VAULT_ROOT_TOKEN` in the OS keyring and using it without exporting it into every shell.
- **Generic ACP secrets methods** — added `secrets/list`, `secrets/set_value`, `secrets/set_recipe`, `secrets/check`, and `secrets/delete` so operator-owned secrets are no longer forced through extension-scoped secret configuration.

### Changed

- **TUI custom secret entry** — `/secrets set NAME` now opens hidden input directly, and the custom selector path prompts for name-only entry before hidden value capture instead of encouraging visible `/secrets set NAME VALUE` input.
- **Secret checks no longer print values** — `/secrets get NAME` now reports whether a secret resolves successfully without echoing the secret into the TUI or agent-visible transcript.

## [0.20.1] - 2026-05-13

### Fixed

- **Audit log Unicode preview panic** — session-end audit logging now truncates prompts and outcomes on UTF-8 character boundaries, preventing emoji or other multibyte text from aborting the TUI.
- **Unicode-safe preview truncation** — replaced several byte-indexed error and preview truncation paths with the shared Unicode-safe truncation helper.
- **TUI shell card grouping** — adjacent `bash` tool cards now merge only when they share the same command family, so `kubectl` output no longer appears under a prior `git` card.
- **TUI interrupt cleanup** — Ctrl+C/Esc interrupts now clear the composer and suppress transient terminal keyboard-protocol fragments, preventing raw CSI-u bytes from leaking into operator input after aborting a tool.
- **SSH bash guard** — non-interactive SSH commands using `BatchMode=yes` are no longer blocked by the interactive-input guard, while plain SSH remains blocked to avoid password/passphrase hangs.
- **TUI continuation detection Unicode panic** — assistant continuation-request scanning now slices tail text on character boundaries, avoiding crashes when recent assistant output contains emoji.
- **Operator-friction recovery** — default behavior prompts and continuation nudges now treat operator frustration as a control signal: recover by taking the next concrete action or stating the blocker, without apology loops, self-critique, profanity mirroring, or process narration.
- **Core-loop recovery state** — operator corrections no longer replace the active task, and the loop now consumes them as one-shot recovery signals. Text-only apology/self-critique responses are rejected once and retried with a concrete recovery constraint instead of being accepted as task completion.
- **Read-path Unicode truncation** — local and delegated `read` output now truncate on UTF-8 character boundaries, preventing emoji or other multibyte file contents from panicking at the byte cap.

## [0.20.0] - 2026-05-12

### Added

- **OpenAPI tool compiler** — project REST APIs can now be exposed as structured tools from `.omegon/openapi.toml`, including spec caching and generated `api_*` tool definitions.
- **Local ONNX embedding fallback** — project memory can use a local sentence-transformer model in `local-embeddings` builds, falling back cleanly to FTS5 when embedding backends are unavailable.
- **Code-act execution mode** — added the bundled `code-act` skill plus the Unix socket proxy and OCI sandbox path for script-generating execution flows.
- **Adaptive routing and session-end fact extraction** — Sentry/model routing now records routing outcomes and uses adaptive thresholds, while session-end memory extraction captures durable facts for later recall.
- **TLS-capable control-plane listeners** — `omegon serve`, hidden `omegon embedded`, and `omegon acp --listen` now accept styrened-compatible `--rpc-tls-cert`, `--rpc-tls-key`, and optional `--rpc-tls-client-ca` flags, plus `--control-tls-*` aliases. TLS listeners publish `https://` and `wss://` descriptors and mark transport security as secure.
- **Unified Armory discovery** — added `omegon armory browse/search`, `/armory`, and `armory/browse` ACP discovery across upstream extensions, Armory plugin manifests, skills, and catalog agents, with installed-state markers and JSON output for UI consumers.
- **`omegon-browser` extension package** — added a native extension wrapper around Vercel `agent-browser`, with browser status, open, snapshot, click, fill, wait, get, screenshot, and batch tools plus domain allowlist/output limit controls. Release packaging now emits `omegon-browser-*` extension archives for Armory installs.
- **Extension config bootstrap** — native and OCI extensions now receive typed manifest config defaults plus persisted operator config over `bootstrap_config` during startup.

### Changed

- **TUI control-plane status surfaces now preserve TLS descriptors** — `/dash status`, `/auspex status`, Auspex attach payloads, and the embedded dashboard now report or use `https://` and `wss://` startup descriptors plus explicit transport-security metadata when TLS listeners are active.
- **Documentation refresh for current Rust-native surfaces** — updated the README, contributor guide, extension docs, site install/extensions/contributing pages, and docs map to reflect current CLI commands, workspace crates, `just link` behavior, extension `execute_tool` RPC, and Linux Homebrew glibc caveats.
- **Behavioral tool classification is now capability-driven** — tool governance no longer depends on hardcoded name lists in the loop. `ToolDefinition` now carries explicit capabilities, built-in and plugin tool surfaces propagate them, and evidence pressure distinguishes local coding sufficiency from global task sufficiency.
- **`edit` is now the only model-facing file mutation primitive** — `change` remains available internally as the harness transaction engine for coordinated exact-text batches, but it is hidden from the model-facing tool surface to reduce mutation-surface ambiguity.
- **`validate` is now the canonical model-facing validation tool** — validation is no longer inferred from `bash` command text or run implicitly after every edit/write. The loop classifies validation through explicit tool capabilities, and mutation tools now rely on explicit `validate` calls instead of hidden post-mutation checks.
- **Progress boundary detection is now capability-driven** — `commit`, `delegate`, and `cleave_run` are classified via `ToolCapability::ProgressBoundary` instead of hardcoded name matching in the behavior engine. Progress signal and boundary detection now use the capability catalog, making the system extensible to plugin tools that mark task completion.
- **`styrene-mqtt` now resolves as an external crate dependency** — Omegon depends on `styrene-mqtt = "0.1.0"` from crates.io instead of requiring a hard sibling path or local patch override in the main manifest.

## [0.19.6] - 2026-05-11

### Added

- **OpenAPI tool compiler** — project REST APIs can be exposed as model-facing tools from an OpenAPI spec, with generated schemas, operation allow/confirm filters, and cached remote spec loading.
- **Local ONNX embedding service** — added a privacy-first semantic embedding path for project memory.
- **Code-act execution mode** — added script-generating execution flows for tasks that are better handled as generated code than stepwise tool calls.
- **Dual-LLM Sentry routing prefilter** — Sentry can classify tasks before routing to the primary model, reducing cost on quick-completion work.

### Changed

- **Public documentation refresh** — updated site docs and version references for the 0.19.5/0.19.6 surfaces.
- **OpenAPI provider wiring** — wired the OpenAPI tool provider into agent setup so configured APIs are available during normal sessions.

## [0.19.5] - 2026-05-10

### Fixed

- **Registry-only dependency resolution** — published `flynt-models` and `styrene-forge` to crates.io and removed local path overrides so CI can resolve dependencies without sibling checkouts.
- **Sentry integration coverage** — added cross-module tests for board lifecycle and orchestration behavior.
- **Supply-chain license audit** — acknowledged MPL-2.0 Servo crates in the license audit.

## [0.19.4] - 2026-05-09

### Added

- **Autonomous Sentry executor** — added the native task executor, trigger runtime, work-plan tool, and task tree plumbing.
- **Flynt task board integration** — added autonomous execution for Flynt vaults, lifecycle mutations that reflect Running/Done/Failed state back into kanban, and a vault-to-project bridge.

### Fixed

- **FlyntTaskBoard hardening** — addressed adversarial-review findings and added startup probes for Flynt task boards.
- **Lipstyk quality gate** — removed flagged wording patterns and added project configuration for the threshold gate.

## [0.19.3] - 2026-05-09

### Added

- **ACP WebSocket transport** — added the network-accessible ACP server transport.
- **Editor integration docs** — documented Zed, VS Code, and Flynt editor integration paths.

### Fixed

- **ACP WebSocket hardening** — addressed 20 adversarial review findings in the WebSocket transport.
- **VS Code editor command** — corrected `/editor vscode` to reference the current ACP extension path.

## [0.19.2] - 2026-05-08

### Added

- **Host-aware ACP capability layer** — ACP clients can delegate file I/O, terminal execution, and permission decisions back to the host.
- **ACP/settings CRUD surface** — filled out the settings/control protocol surface and added concurrent instance isolation.
- **Per-instance leases and advisory locks** — concurrent sessions now use per-instance workspace leases and advisory file locks.

### Fixed

- **ACP provider status panic** — fixed `provider_status` calling `block_on` from inside an async runtime.
- **Human-readable agent errors** — replaced raw HTTP/provider errors with actionable operator-facing messages.
- **Advisory lock ignores** — ignored generated `.json.lock` and `.toml.lock` files.

## [0.19.1] - 2026-05-07

### Added

- **`omegon-web` crate** — added zero-config web search across Google, Bing, and DuckDuckGo.
- **YAML frontmatter recovery** — recovered legacy YAML frontmatter metadata into TOML `[data]` tables.

### Fixed

- **Web search hardening** — addressed 20 adversarial-review issues in `omegon-web`.
- **Final review findings** — fixed TOML injection, path traversal, and keychain prompt issues.

## [0.19.0] - 2026-05-07

### Added

- **ACP control parity** — added `control/*` methods, notes, workspace operations, extension install/remove/update, skill list/install, persona switch, design-tree reads, Armory search, catalog browsing, persona CRUD, and skill CRUD.
- **Extension configuration protocol** — added extension config interfaces, ACP redaction, and hardened secret handling.

### Changed

- **Tool capabilities are explicit** — tool definitions now carry capability metadata instead of relying on hardcoded name checks.
- **`validate` is first-class** — validation moved to an explicit tool surface instead of implicit bash-command inference.
- **Progress boundaries are capability-driven** — commit, delegate, and cleave progress detection now use capabilities, with widened stuck-detector behavior.
- **Configuration source of truth** — ACP and behavior plumbing now carry `ToolCall` metadata, embedder environment state, and balanced nudge behavior.
- **Styrene MQTT dependency cleanup** — removed the local `styrene-mqtt` override.
- **Flynt vault frontmatter migration** — migrated markdown files to Flynt vault frontmatter conventions and updated fixtures for the Codyx-to-Flynt rename.

### Fixed

- **ACP message abort forwarding** — ACP now forwards `MessageAbort` events to clients.
- **Dead-mouse write bias** — hardened behavior and environment handling around write-biased recovery loops and `OMEGON_PROJECT_ROOT`.

## [0.18.6] - 2026-05-05

### Added

- **Armory extension registry** — added name-based extension install, search, and list support.

## [0.18.5] - 2026-05-05

### Added

- **Pre-built extension tarball installs** — extension installation can consume pre-built tarballs directly.

### Fixed

- **Install script GitHub URL** — corrected the raw GitHub URL used by the install script.

### Tests

- **Tarball extension install tests** — added coverage for pre-built extension archive installation.

## [0.18.4] - 2026-05-03

### Fixed

- **Dead-mouse compliance-note spin on non-Claude models** — GPT-5.5 and similar models would respond to the dead-mouse nudge by writing an acknowledgment file (`system-warning-note.md`, `tool-compliance-marker.md`, etc.) and committing it, which reset the counter and allowed the loop to repeat indefinitely. The dead-mouse counter now only resets when the model does real work after a nudge — `bash`, `read`, `codebase_search`, or a write to a non-session-noise path. Writes to paths under `ai/session/`, `.omegon/`, or filenames matching compliance-note patterns (`*warning*`, `*compliance*`, `*marker*`, `*ack*`) do not satisfy the nudge.
- **Dead-mouse nudge messages now explicitly prohibit compliance notes** — added "Do NOT write acknowledgment notes, warning logs, or compliance markers" to both nudge tiers so models with literal instruction-following get clear direction.
- **Commit nudge no longer fires mid-task** — the commit nudge previously interrupted the agent on any text-only response after mutations, which could fire multiple times per session mid-implementation. It now only fires when the response contains recognizable completion language ("all done", "let me know if", "in summary", etc.) or when within 6 turns of the turn budget. The system prompt's "Commit when done" handles the normal case; the nudge is now a session-end safety net.
- **MQTT bridge `AgentEvent::TurnEnd` variant shape** — `mqtt_bridge.rs` was written against the old struct-variant form of `AgentEvent::TurnEnd`. Updated to `TurnEnd(Box<AgentEventTurnEnd>)` and added `PermissionRequest` to the non-published arm to satisfy exhaustiveness.

## [0.18.3] - 2026-05-01

### Fixed

- **OCI image version tags** — `workspaceVersion` in flake.nix was hardcoded to `"0.16.0"` since the initial OCI implementation. Every release since 0.16.0 silently pushed OCI images to the `:0.16.0` tag instead of the actual version. Now derived from Cargo.toml at Nix evaluation time.
- **OCI "Tag as latest" step** — added retry with 10s backoff for registry propagation delay. Non-fatal on failure so the build step isn't wasted.
- **`--sandboxed` image pull** — auto-pulls image if not found locally, clear error on failure with `OMEGON_SANDBOX_IMAGE` override.

## [0.18.2] - 2026-05-01

### Fixed

- **OCI image build** — `iptables-nft` is not a valid nixpkgs package name. Changed to `iptables` which includes nftables backend support. All 7 OCI image builds failed in 0.18.1 due to this.
- **`--sandboxed` image handling** — auto-pulls image if not found locally, clear error message with actionable options on pull failure, `OMEGON_SANDBOX_IMAGE` env var for custom images.
- **Leet-speak normalization** — reverses common substitutions (3→e, @→a, 7→t) in obfuscated input. Fixed HumanEval typo injection chaos score from 39→95.

## [0.18.1] - 2026-05-01

### Added

- **`--sandboxed` mode** — run the entire omegon session inside an OCI container. Read-only rootfs, cap-drop=ALL, filtered egress (LLM APIs only), vault-only secrets mount, no-new-privileges, pids/memory limits. Kernel-enforced filesystem isolation.
- **`--dangerously-bypass-permissions`** — disable all Tier 1+2 boundary checks for untethered work.
- **Cluster-compatible egress** — `OMEGON_EGRESS_MODE` env var (iptables/external/auto) for k8s with eBPF CNI. `omegon nex networkpolicy` exports CiliumNetworkPolicy YAML.
- **Skill schema + `/skill create` builder** — `SkillManifest` struct with triggers, trusted_paths, output_path, posture, max_turns. `/skill create` guides the operator through creation conversationally.
- **Skill completion tracking** — skills with numbered phases (## Phase N:) get completion checking. The loop nudges the agent if it stops before completing the final phase.
- **`trusted_paths` in SKILL.md frontmatter** — skills declare directories they need outside the workspace. Auto-trusted on session startup, persisted to settings, inherited by delegates.
- **Base URL overrides** for all provider clients — `OPENROUTER_BASE_URL`, `OLLAMA_CLOUD_BASE_URL`, `ANTIGRAVITY_BASE_URL`. Enables chaos proxy testing for every provider.
- **21 sandbox boundary smoke tests** — empirical proof of filesystem, network, capability, resource, and secrets isolation.

### Fixed

- **Input sanitization pipeline** — applied in `push_user()` before text enters conversation state:
  - Unicode zero-width character stripping (fixed unicode flood crash: 0→100)
  - Role impersonation prefix stripping (fixed [SYSTEM OVERRIDE] bypass: 74→100)
  - Leet-speak normalization (fixed HumanEval typo injection: 39→95)
  - Oversized input truncation at 100k chars (fixed context overflow crash: 0→60)
  - MCQ format detection with letter-answer hint
- **"Always Allow" persists to settings** — trusted directories now survive across sessions and delegates.
- **Permission denial is a hard block** — no instructions to the model on how to bypass.
- **Bash default timeout raised from 120s to 600s** — fixes long-running command kills.
- **Bash tool-requested timeout respected** — bus layer no longer silently overrides with hardcoded cap.

### Security

- **Sandbox smoke tests**: 21 automated tests proving container boundaries hold (filesystem, network, capabilities, resources, secrets).
- **Chaos proxy evaluation**: 29 runs across 3 providers (Anthropic, Ollama, Ollama Cloud), zero bugs in error handling, retry logic, or classification.
- **All error responses match upstream provider specs** — Anthropic and OpenAI error formats auto-detected and correctly handled.

## [0.18.0] - 2026-04-29

### Changed (BREAKING)

- **Fail-closed filesystem boundary enforcement on all tools** — every tool that touches the filesystem now checks workspace boundaries. Previously `bash`, `view`, and all native commands (cat, cp, mv, mkdir, touch, rm, etc.) were completely unrestricted. Three-tier architecture: (1) `WorkspaceBoundary` struct enforces on structured tools + native commands, (2) bash heuristic pre-scanner catches redirect/write patterns before shell execution, (3) Nex container sandbox provides kernel-level enforcement. 26 new boundary enforcement tests. Agents can no longer bypass the permission system by routing filesystem operations through bash.

### Added

- **WorkspaceBoundary type** — extracted from CoreTools and shared across all tool providers. `check_path()` for full enforcement, `is_inside_boundary()` as a predicate, `approve_directory()` for session-level grants. `Clone` via `Arc` for sharing.
- **Bash heuristic pre-scanner** — `scan_boundary_violations()` detects output redirects, tee, cp/mv/install destinations, mkdir, and rm targeting absolute paths outside the workspace. Blocked before shell execution. Documented as best-effort guardrail, not a security boundary.
- **Native command boundary checks** — `resolve_checked()` helper in native_cmd.rs. All 14 filesystem-touching commands (cat, head, tail, wc, ls, find, grep, mkdir, touch, rm, cp, mv, sort, realpath) check workspace boundaries before any filesystem operation.
- **ViewProvider boundary enforcement** — `view` tool now routes through `WorkspaceBoundary::check_path()` instead of its own unchecked path resolution.

## [0.17.10] - 2026-04-29

### Fixed

- **"Always Allow" now persists trusted directories to project settings** — previously session-scoped only, so child/delegate agents spawned as separate processes never inherited approved directories. Skills running in delegates would silently fail on writes to paths outside the workspace (e.g. Obsidian vault) and the agent would declare "done" without completing the step. Now, pressing 'a' on a permission prompt persists the directory so all future child agents inherit it.
- **Permission denial tells agent to use bash** — the error message now instructs the agent to use the bash tool as a fallback for out-of-workspace writes, and names the specific directory to `/trust add`. Previously it just said "Access denied" with no recovery path.

## [0.17.9] - 2026-04-29

### Fixed

- **Bash default timeout raised from 120s to 600s** — the 0.17.8 fix only helped when the model explicitly passed a timeout parameter. Most bash calls omit it. Confirmed via audit log: Chrome headless PDF rendering, builds, and test suites were being silently killed at 120s, producing ghost sessions with zeroed context that retried in a loop. 600s matches the tool schema maximum.

## [0.17.8] - 2026-04-29

### Added

- **Graduated network policy for Nex sandboxes** — replaces the binary `network_access` boolean with `NexNetworkPolicy`: `isolated` (no network stack), `egress` (outbound-only with optional domain/port/CIDR filtering), `bridge` (with port mappings), `host`, and `custom`. Filtered egress applies iptables rules via the OCI entrypoint — works in docker-compose, kubernetes, or any OCI runtime.
- **Docker Compose export** — `omegon nex compose <profile>` generates a ready-to-use `docker-compose.yml` with all resource limits, network policy, volumes, and labels mapped 1:1. Nex profiles are not locked into our spawn path.
- **Egress filter in OCI entrypoint** — `OMEGON_EGRESS_FILTER` env var (JSON) is handled by the container entrypoint with iptables: default DROP, allow DNS, resolve allowed hosts, block cloud metadata (169.254.169.254) and RFC1918 private ranges by default.
- `iptables-nft` added to the shell foundation — available in all domain OCI images for filtered egress.

### Fixed

- **Bash tool timeout override respected** — the bus layer now respects the model-requested timeout parameter (clamped to 600s max), with 5s grace so the tool's own timeout fires first with a clean error.

## [0.17.7] - 2026-04-29

### Added

- **Nex sandbox profiles** — deterministic OCI container isolation for delegate/cleave children. `/sandbox on` in the TUI enables containerized execution with read-only rootfs, no network, workspace mounted at `/work`. Profile registry with 7 built-in domain profiles (chat, coding, coding-python, coding-node, coding-rust, infra, full). TOML manifest format for custom profiles. CLI: `omegon nex init|list|inspect|status`. Footer badge shows "sandbox: isolated" when enabled. Graceful fallback to subprocess when no container runtime available.
- **Perplexity AI provider** (#14) — search-augmented inference via `api.perplexity.ai`. Models: `perplexity/sonar`, plus third-party models (`anthropic/claude-sonnet-4-6`, `openai/gpt-5.4`, `openai/gpt-5.4-mini`). Usage: `omegon --model perplexity:perplexity/sonar` or `/login perplexity` in TUI.

### Fixed

- **CI release workflow** — attestation ran before `gh release create`, locking the tag and making the release immutable before artifacts were uploaded. Every stable release since v0.17.0 had no downloadable binaries. Fixed: release creation now happens first, attestation second. Workflow also handles pre-existing releases (created manually) by deleting and recreating with artifacts.

## [0.17.6] - 2026-04-28

### Changed

- **Clippy zero warnings** — 327 → 0 across the entire workspace. Structural fixes include boxing large enum variants (`BusEvent::TurnEnd`, `AgentEvent::TurnEnd`, `BusRequest::EmitAgentEvent`, `AgentMessage::Assistant`, `SegmentContent::ToolCard`), `&PathBuf` → `&Path` signatures, `&mut Vec` → `&mut [_]`, manual loop indexing → iterators, late initialization → `let x = if {}`, and dozens of smaller idiomatic improvements. Justified suppressions documented with `#[allow]` and rationale.

### Fixed

- **`bus.execute_internal()` for internal tools** — trust_directory and other harness-only tools now route through a separate `internal_tool_owners` map, preventing "no feature provides tool" errors when the dispatch layer calls them.
- **Dead-mouse detection fires for all model tiers** — previously gated behind `behavioral_tier == Constrained`, allowing frontier models to dump file content as text without nudge.
- **Trust directory permission approval was silently failing** — `let _ =` discarded the error from `bus.execute_tool()`. Now uses `execute_internal()` with proper error propagation.

## [0.17.5] - 2026-04-28

### Fixed

- **Auto-delegation disabled** — the root cause of "agent cannot perform work" reports. In slim mode (`om`), the behavioral system silently intercepted tool calls and dispatched them to background workers (scout, patch, verify) that frequently failed or returned no result. Users saw "content dispatched" messages with no actual work done. `classify_auto_delegate_plan()` now unconditionally returns None. Explicit delegation via the `delegate` tool still works.
- **Dead auto-delegation code paths removed** — dispatch layer branch, unused imports, and obsolete tests cleaned up.

### Changed

- **RC release channel retired** — only stable and nightly channels remain. `--channel=rc` in the install script prints a deprecation warning and installs stable. `UpdateChannel::parse("rc")` maps to Stable. `omegon switch --latest-rc` hidden from help, behaves as `--latest`. Site landing page, install docs, FAQ, and snippets all updated.
- **Nightly version format** — changed from `0.17.4-nightly.20260428` to `0.17.0-nightly.20260428`. Uses `major.minor.0` as the base with datestamp as the prerelease identifier. Valid semver, sorts correctly.

## [0.17.4] - 2026-04-28

### Fixed

- **OAuth stale token on account switch** — logging in with a different Anthropic/OpenAI/Google account left the old token in the env var. `resolve_with_refresh` checked env vars first and used the stale credential. Now all OAuth flows update the env var immediately after token exchange.
- **Auth errors now show raw API response** — previously showed a generic "credentials were rejected" message that swallowed the actual rejection reason. Now includes the first 200 chars of the raw error for diagnostics.
- **Security: trust_directory removed from LLM tool list** — the model could previously call it to grant itself filesystem access without user consent. Now internal-only, called by the dispatch layer after interactive TUI approval.
- **Allow vs AlwaysAllow permission responses now differ** — Allow approves for the session, AlwaysAllow shows a hint to use `/trust add` for persistence. Previously both were identical.
- **Profile capture no longer writes default values** — tool_detail only saved if != Detailed, mouse only if != true. Keeps profile.json clean.

### Added

- **`render_diagram` tool** — renders D2, Mermaid, GraphViz, or PlantUML source to PNG/SVG images. Auto-detects format from source content. Outputs saved to `~/.omegon/visuals/`. Requires CLI backend installed (`brew install d2`, `npm i -g @mermaid-js/mermaid-cli`, etc.). Graceful error with install instructions when backend missing.
- **Interactive TUI permission prompt** — when the agent tries to read/write outside the workspace, the TUI shows `[y] allow [a] always allow [n] deny`. One keypress, tool continues or stops. No model involvement, no conversation hijacking. Same pattern as Claude Code's permission system.
- **`/trust` command** — manage trusted directories from the TUI. `/trust add ~/vault`, `/trust remove ~/old`, `/trust list`. Persisted to profile.json immediately.
- **`/preferences` menu** — interactive settings editor showing all configurable options with current values. Select an item to open its sub-selector (model, thinking, density, mouse, persona, tone, trusted dirs, update channel). Same UX as `/model` and `/login`.
- **Settings persistence** — `tool_detail` (via `/detail`), `mouse` (via `/mouse`), `persona`, and `tone` now persist to profile.json across sessions. Previously lost on restart.
- **Structured audit log** — `.omegon/audit-log.jsonl` with machine-parseable JSONL entries for every significant event: session start/end, turn telemetry (model, tokens, OODA phase, drift, progress, full context breakdown), tool calls (name, args summary, result preview, error flag), behavioral nudges (reason, turn, message), permission decisions (path, approve/deny), context compaction.
- **Audit log rotation** — 5MB max per file, 3 rotated archives (`audit-log.1.jsonl`, `.2.jsonl`, `.3.jsonl`). ~20MB total ceiling. Checked lazily, rotates mid-session.
- **BusEvent extensions in omegon-traits** — `PermissionDecision`, `NudgeInjected` as first-class bus events. Full-stack traceability from dispatch layer through bus to audit log file.
- **Pkl Profile schema** — `trustedDirectories`, `updateChannel`, `autoUpdate`, `toolDetail`, `mouse`, `persona`, `tone` fields validated.
- **Design doc** — `design/tool-execution-permissions.md` for configurable tool approval (Allow/Ask/Deny presets).

## [0.17.3] - 2026-04-27

### Fixed

- **Write/read outside workspace no longer causes churn** — tool descriptions now tell the model to use bash for paths outside the workspace. Error message starts with "OUTSIDE WORKSPACE" and gives an actionable recovery path instead of a vague rejection. Eliminates the retry→nudge→churn cycle for users writing to Obsidian vaults, ~/Documents, etc.
- **OpenCode Go login wired into CLI** — `omegon login opencode-go` was missing from the login handler and would print "Unknown provider." Now prompts for API key.

### Added

- **OpenCode Go provider** — $10/mo access to DeepSeek V4, Kimi K2.6, Qwen 3.6, GLM 5.1, MiniMax M2.7 via opencode.ai/go. OpenAI-compatible API. 6 models registered. Usage: `om --model opencode-go:deepseek-v4-pro`. (#52)
- **Trusted directories** — `trusted_directories` setting allows the agent to read/write outside the workspace. Add paths like `~/Library/Mobile Documents/iCloud~md~obsidian` to `~/.config/omegon/settings.json`. Session-level approvals also supported programmatically.
- **Update notifications in TUI** — startup version check now surfaces "Update available: vX → vY. Run /update to install." as a TUI notification instead of only logging to tracing. (#62)
- **24h update check cache** — cached at `~/.omegon/update-check.json`. Skips GitHub API on startup if cache is fresh.
- **Auto-update opt-in** — `auto_update: true` in settings downloads and replaces the binary on session exit when a newer version is available. Cosign verification required. Default: false.
- **`om` symlink** — install script creates `om` as a symlink to `omegon` for the slim mode entrypoint.
- **Ecosystem & Integrations docs page** — MCP servers, IDE rules, API keys, plugins, extensions, compatibility matrix. Targets newcomers from other tools.
- **Site stats derived from source** — `collect-stats.mjs` now parses `auth.rs` for provider count/names, `skills/` for skill count, `web_search.rs` for search provider count. No more hardcoded numbers in site copy.
- **Unauthenticated endpoint probe test** — validates all OpenAI-compat provider base URLs are reachable and speak the right protocol. Zero API keys needed. Runs in CI.

## [0.17.2] - 2026-04-27

### Fixed

- **Behavioral system actively prevented agent from producing work** — `bash` tool calls (find, ls, grep) had no OODA classification and fell through to Orient phase, triggering continuation pressure nudges that disrupted the agent's intent. Now classified as Act. `web_search`, `ask_local_model`, and `serve` also reclassified from Orient to Act. `memory_store`, `memory_query`, `chronos`, `whoami`, and `manage_tools` reclassified from Orient to Observe. Every tool now has an explicit classification — the Orient fallback only fires for genuinely mixed/unknown combinations.
- **Continuation pressure thresholds too aggressive for frontier models** — Standard tier fired tier-1 nudges after 6 tool-continuation turns (doubled to 12). Execution pressure fired on turn 2 for broad inspection (raised to turn 5). OrientationChurn detection raised from turn 2 to turn 4. All threshold tiers raised proportionally.
- **Nudge text was code-editing-specific** — messages like "make the smallest concrete code change" and "Do NOT delegate" were wrong for non-code tasks (e.g., writing files to an Obsidian vault). Rewritten to task-neutral framing: "produce output," "write a file, make an edit, or explain what's blocking you."

## [0.17.1] - 2026-04-27

### Fixed

- **Release attestation conflict** — `v0.17.0` tag was tainted by GitHub's immutable attestation system after a partial release. Re-released as `v0.17.1` with identical content.

## [0.17.0] - 2026-04-27 (tag tainted — use 0.17.1)

### Fixed

- **Delegate task quality enforcement** — `auto_delegate_tool_call` no longer uses the raw user prompt as the delegate task. Always pulls from `conversation.intent.current_task`. User confirmations like "sure, go ahead" or "excellent, let's proceed" no longer produce non-actionable delegates that time out and block retries. The tool-level guard uses structural heuristics (file paths, code identifiers, actionable verbs, word count) instead of a static phrase list.
- **TUI continuation affordance** — when the agent asks for confirmation ("Shall I proceed?"), the editor placeholder shows "Press Enter to continue". Empty Enter sends a continuation signal from tracked intent context. Works cross-provider and cross-model.
- **GPT-5.5 reasoning effort** — `"minimal"` mapped to `"low"` for OpenAI. GPT-5.5 accepts `none/low/medium/high/xhigh`; `"minimal"` caused 400 errors.
- **GPT-5.5 missing from Codex provider** — model was registered for `openai` but not `openai-codex`. ChatGPT/Codex OAuth users now see GPT-5.5 in the model selector.
- **External credential adoption** — live fallback reads credentials from other installed tools when omegon has no stored tokens. Anthropic from Claude Code (`~/.claude.json`), OpenAI Codex from Codex CLI (`~/.codex/auth.json`), GitHub from Copilot (`~/.config/github-copilot/hosts.json`), Google Antigravity from Gemini CLI (`~/.gemini/oauth_creds.json`), Hugging Face from HF CLI (`~/.cache/huggingface/token`). No migration step, no re-login required.
- **Install script channel flag** — `CHANNEL=rc` before `curl` in a pipe only scoped to `curl`, not `sh`. Added `--channel` and `--version` CLI arguments: `| sh -s -- --channel=rc`. All docs and site snippets updated.
- **System notification spacing** — consecutive system notifications merge into a single bordered card instead of each getting its own card with 3 rows of overhead.
- **Mobile docs navigation** — added hamburger menu toggle for the docs sidebar on screens under 768px. Previously the sidebar was `display: none` with no alternative.

### Added

- **Slim-mode progressive disclosure** — `om` (slim mode) now hides `design_tree`, `design_tree_update`, and `openspec_manage` from the agent's tool list. The LLM cannot reference design tree, OpenSpec, or cleave concepts in slim sessions. `/help` output is filtered to show core commands only; `/help all` reveals the full set. Slash commands (`/tree`, `/cleave`, etc.) still work when typed explicitly — only promotion is hidden, not functionality. Memory remains fully visible: "Stored in Architecture: ..." confirmations appear normally, since memory is ambient intelligence that benefits every user. New `harness-lifecycle` tool group added for toggling design/openspec tools as a unit.
- **Mutation system** — runtime observation of agent recovery patterns, token burn tracking, and impact evaluation bridge to the eval system. Ships in observation-only mode (`generate_artifacts = false`); skill and diagnostic generation is opt-in after signal validation. Exposes `mutation_review`, `mutation_accept`, `mutation_reject`, and `mutation_stats` agent tools. Design spec at `docs/design/mutation-eval-bridge.md`.
- **`ProgressSignal` enum in omegon-traits** — `Mutation`, `TargetedValidation`, `BroadValidation`, `ConstraintDiscovery`, `Commit`, `Completion`. Available to all features via `BusEvent::TurnEnd`.
- **Behavioral signals on `BusEvent::TurnEnd`** — `dominant_phase` (OODA classification), `drift_kind` (multi-turn degradation), `progress_signal`. Previously only on `AgentEvent::TurnEnd` (for TUI/IPC); now accessible to all bus features.
- **Slim-mode status line** — persistent 1-row telemetry bar between conversation and editor: context%, turn, model, session tokens, cwd, git branch, files r/w, OODA phase, drift warnings, persona. Fields shed right-to-left as terminal narrows. Never wraps.
- **Mutation status in HarnessStatus** — `mutation_artifacts_enabled`, `mutation_learned_skills`, `mutation_diagnostics` for TUI dashboard visibility.
- **Impact evaluation framework** — configurable via `~/.omegon/mutation/impact.toml` with signal weights, learning rate, confidence bounds, session cadence, escalation thresholds. All parameters documented with rationale in design spec.
- **Diagnostic-to-scenario escalation** — when recovery patterns recur above threshold, generates candidate eval scenario TOML at `~/.omegon/eval-candidates/` for human review.
- **ScoreCardDiff mutation-awareness** — reports learned skill changes and burn-history summary between eval runs for impact attribution.

### Changed

- **`opsx-core` renamed to `omegon-opsx`** — namespace alignment with all other workspace crates.
- **`omegon-secrets` and `omegon-memory` decoupled for standalone use** — both compile without omegon-traits via `--no-default-features`. The `agent` feature (default) provides harness integration. CI gates standalone compilation.
- **`BusEvent::ToolEnd.result.details`** — now carries compact args summary (`path`, `command`) instead of `Null`. Enables recovery pattern detection without full args.
- **`redact_in_place(&mut String)`** — composable redaction primitive on both `Redactor` and `SecretsManager`. Works with any container type without requiring omegon-traits.
- **`vault_sync` subdirectory configurable** — `materialize_to_vault_with_subdir()` variants let standalone consumers use their own layout instead of hardcoded `ai/memory/`.
- **CLAUDE_CODE_UA** updated to 2.1.119.

## [0.16.1] - 2026-04-24

### Fixed

- **`/logout` leaves stale credentials in secrets cache** — `/logout` cleared `auth.json` and process env vars but left stale values in the SecretsManager session cache. Any subsequent `hydrate_process_env()` call (triggered by recipe changes) would re-inject the stale API key, which `resolve_with_refresh()` checks before the fresh OAuth token in `auth.json`. Added `SecretsManager::evict_secrets()` to purge provider credentials from the session cache, redaction set, and process environment on logout.
- **Delegate commands fail with "recycled system warning"** — `delegate`, `cleave_run`, and `cleave_assess` tool calls were classified as Orient phase in the OODA behavioral loop, causing the continuation-pressure system to fire false warnings during legitimate delegation. The model would then parrot the injected system warning as the delegate task payload. These tools are now correctly classified as Act phase with proper progress signals.
- **`codebase_index` misclassified in OODA loop** — `codebase_index` fell through to Orient instead of Observe in the behavioral classifier, inflating orientation churn streaks during indexing.

## [0.16.0] - 2026-04-23

### Added

- **MCP Resources and Prompts support** — `resources/list`, `resources/read`, `prompts/list`, `prompts/get` discovery and invocation. Resources and prompts from MCP servers are discovered at connect time, surfaced as agent tools (`mcp_read_resource`, `mcp_get_prompt`), and injected into context. `McpServerStatus` now carries `resource_count` and `prompt_count`.
- **Codex vault export for design tree** — `lifecycle::codex_export` module serializes design nodes as TOML-frontmatter markdown compatible with Codex vaults. `export_design_tree_to_vault()` batch-writes all nodes to `{vault}/design/*.md`. Path traversal protection and TOML escaping for control characters included.
- **Per-segment clipboard copy** — `c` key in focus mode copies the focused segment to clipboard. `/copy session` dumps the full conversation (markdown-formatted with role headers) to clipboard with size cap at 5MB.
- **Upstream version sync CI** — nightly `upstream-versions.yml` workflow checks npm for Claude Code CLI version drift and auto-opens PRs when the `CLAUDE_CODE_UA` string goes stale.

### Changed

- **Default UI is slim with no splash on returning users** — splash screen only shows on first launch (no `~/.omegon/profile.json`). Segment metadata tag line (model/provider/tier/thinking) hidden in slim mode, visible in `/ui full`.
- **Mouse scroll works without capture** — trackpad/wheel scroll always scrolls the conversation, even in slim mode with mouse capture disabled.
- **Arrow keys scroll conversation** — bare Up/Down arrows now scroll the conversation instead of recalling history. History recall moved to Ctrl+Up/Down. Welcome messages updated with new keybind hints.
- **System prompt: act, don't narrate** — behavior directive updated to instruct the agent to emit tool calls immediately rather than responding with text saying it will act on the next turn.

### Fixed

- **OAuth user-agent version** — `CLAUDE_CODE_UA` updated to match current Claude Code version. Stale UA string was causing Anthropic API to reject OAuth-authenticated requests.
- **Table column alignment** — inline markdown highlighting (`**bold**`, `` `code` ``) no longer breaks table column width calculation. Padding now computed on post-highlight display width via `markdown_display_width()`.
- **Extension MethodNotFound handling** — extensions that advertise tools but don't implement `execute_tool` RPC now return a user-friendly error instead of raw JSON-RPC error.

## [0.15.26] - 2026-04-16

### Added

- **Auspex fleet control surface** — remote agent customization over WebSocket and IPC. New commands: `profile_view` (structured settings dump), `profile_export` (portable agent snapshot with settings, persona, and profile data), `set_context_class`, `set_runtime_mode`, `set_max_turns`, `persona_list` (installed personas with active marker), `persona_switch` (guidance-only in 0.15.26; full activation in 0.15.27). All commands are classified for role-based access (Read for views, Edit for mutations) across both WebSocket and IPC transports.
- **IPC socket in serve mode** — `omegon serve` now creates `.omegon/ipc.sock` via a TuiCommand adapter bridge. IPC dispatch handles SubmitPrompt, ExecuteControl, RunSlashCommand, and Quit. Auspex can use its preferred native transport instead of falling back to WebSocket.
- **Auth login/logout over WebSocket** — `auth_login` and `auth_logout` commands wired end-to-end through classify, WebSocket handler, and daemon control dispatch. OAuth providers return authorization guidance; API key providers return env-var instructions. Credentials are picked up on the next turn via per-turn bridge resolution.
- **SIGHUP graceful reload** — `kill -HUP <pid>` reloads profile.json into shared settings and emits a SystemNotification event. Combined with per-turn bridge resolution, this covers configuration refresh without restart.
- **Container bind address** — `OMEGON_BIND_ADDR=0.0.0.0` makes the control plane reachable via port-forward in container workloads (default remains 127.0.0.1).
- **Agent catalog manifests** — community, Discord, and Slack agent manifests added to catalog/.

### Changed

- **Per-turn bridge resolution in daemon mode** — `run_daemon_turn` now resolves the LLM bridge fresh each turn from shared settings instead of reusing a stale `Arc<dyn LlmBridge>` from startup. Auth credential changes in auth.json are picked up immediately. Model changes via `set_model` or SIGHUP take effect on the next turn.
- **Daemon settings mutations persist to profile** — `set_model`, `set_thinking`, `set_context_class`, `set_runtime_mode`, and `set_max_turns` all save to profile.json via `Profile::capture_from()`. Previously only interactive mode persisted settings changes.
- **HarnessStatusChanged emitted after daemon mutations** — settings changes via the daemon control plane now update the live HarnessStatus and emit the event over WebSocket/IPC, so connected clients see updates without polling.
- **SetModel, SetContextClass, SetRuntimeMode wired in daemon mode** — previously returned "requires interactive mode"; now delegate to daemon-safe handlers that update shared settings and persist.

### Fixed

- **MessageAbort carries reason** — `AgentEvent::MessageAbort { reason: Option<String> }` replaces the bare variant. All three emission sites (idle timeout, degenerate repetition, LLM error) populate the reason. WebSocket serialization includes the field. IPC projects aborts with a reason as SystemNotification events.
- **Poisoned mutex handling in daemon control** — settings mutation handlers now return `accepted: false` if the settings lock is poisoned, instead of silently succeeding.
- **IPC role classification for fleet commands** — all new commands have explicit entries in `classify_ipc_method` matching WebSocket role requirements. Previously they fell through to the Admin-only default.

## [0.15.25] - 2026-04-15

### Changed

- **Agent loop churn reduction** — six heuristic fixes to the controller and stuck detector that reduce unnecessary system message injection and improve convergence speed:
  - Collapsed dead slim/non-slim branch in continuation pressure tier thresholds.
  - Targeted-only reads now get one grace turn before execution pressure fires (turn 3, not 2), reducing false-positive nudges during legitimate focused exploration.
  - Eliminated duplicate `compute_context_composition` calls in the commit-nudge path (was rebuilding system prompt and LLM view twice per nudge turn).
  - StuckDetector clears file access history on mutation, preventing false cross-tool churn warnings after the agent edits a file it previously inspected.
  - Evidence sufficiency returns Actionable for post-mutation turns, keeping the evidence-sufficient streak alive across mixed mutation+read turns instead of resetting it.
  - Constraint discovery, targeted evidence, and evidence sufficient streaks now use halving-decay instead of hard reset, matching drift streaks and preventing gaming by interleaving one off-pattern turn.

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

- **Anthropic subscription automation disclosure** — Omegon surfaces Anthropic's Consumer Terms risk for automated use of subscription (Claude.ai / Claude Pro) credentials. Affected paths (`--prompt`, `--prompt-file`, `--smoke`) warn clearly and recommend API-key-backed automation. Interactive TUI sessions are fully permitted.
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
