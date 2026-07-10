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

- Added GPT-5.6 to the OpenAI and OpenAI Codex model registry defaults, S-grade routes, and model selector entries; Codex GPT-5.6 now records upstream rollout metadata, enriches unsupported-model errors with rollout diagnostics, and expands the operator-facing `gpt-5.6` alias to the concrete native `gpt-5.6-sol` route without making the Codex CLI a dispatch dependency.
- Fixed `/skill` so the bare alias opens the structured skills menu instead of writing usage text into the conversation transcript; `/skills` and `/skills list` remain structured-menu-only inventory views.
- Fixed structured menu width handling so `/init` and other popup menus truncate header/footer text to the actual popup width, compact overlong rows, and wrap selected-row descriptions instead of spilling past the border.
- Added the first profile-registry slice: startup profile loading now discovers scoped project/user registry files, supports dynamic `/profile use <id> [--scope=project|user]` switching through TUI/daemon control flows, honors explicit active-profile pointer files before legacy singleton profiles, keeps existing `.omegon/profile.json` compatibility as a fallback, and upgrades `/init` into a harness initialization menu with pending substrate actions, detected compatibility prompts, and project-signal skill recommendations.
- Hardened `/secrets` menu UX so inventory rows make resolution the primary action: Enter now runs `/secrets get <NAME>` for redacted existence/status knowledge, set/replace remains an explicit secondary action, the TUI warms every discovered secret before building the menu snapshot so rows open as known `resolved`, `configured`, `deferred`, or `missing` instead of `not checked`, and bundled catalog agents no longer mark `ANTHROPIC_API_KEY` as globally required.
- Hardened `/variables` menu UX to match the management-first `/secrets` posture: the inventory tab is now `Manage`, Enter updates the selected variable, empty state primes `/variables set`, and read-only value display moved to a secondary action.
- 0.27.8 was not published as a usable artifact release; 0.28.0 supersedes it as the next installable release line.

## [0.28.0] - 2026-07-09

0.28.0 completes the permissions intent architecture: filesystem mediation now carries structured intent, path dialect, environment/mount, sensitive-infrastructure, privilege-escalation, and trusted-grant context through bash and terminal preflight.

### Added
- Make permissions intent architecture the primary 0.28.0 release target: filesystem access is moving toward structured intent, provenance-rich path diagnostics, and confidence-aware mediation for suspicious shell-derived paths such as `/Ig` and mistaken root-dot paths such as `/.omegon`.
- Added the first permissions intent slice: bash and terminal preflight now extract structured filesystem intents, block low-confidence `/Ig`-class shell artifacts diagnostically, and annotate `/.omegon`-class host-absolute paths with workspace-relative correction guidance while preserving strict boundary behavior.
- Added the second permissions intent hardening slice: path classification now recognizes POSIX, Windows drive/root/UNC/verbatim/device forms, WSL `/mnt/<drive>` mounts, MSYS `/c/...`, and Cygwin `/cygdrive/c/...` before `PathBuf` resolution so foreign absolute paths cannot silently fall back to workspace-relative handling.
- Expanded the permissions intent design target with path dialect, VM/container mount context, sensitive infrastructure path, and trust-grant mount-identity affordances so 0.28.0 hardening covers Windows/WSL, containers, VM shared folders, Kubernetes projected secrets, runtime sockets, and sandbox portals.
- Added the third permissions intent hardening slice: shell dialect detection now uses WSL/MSYS/Cygwin environment markers, drive-absolute Windows paths get distinct diagnostics from verbatim paths, and Windows root-relative paths are explicitly classified instead of falling through POSIX handling.
- Added the fourth permissions intent hardening slice: dynamic shell-expanded path targets (`$OUT`, `$(mktemp)`, process substitution) are blocked diagnostically before execution, and shell path token normalization now uses the existing `shlex` dependency for quoted/escaped redirect paths.
- Completed the permissions intent architecture for 0.28.0: resolved filesystem targets now carry sensitive infrastructure risks, best-effort environment and mount context, trusted-external relations, and persistent trusted-directory mount/environment grant identity where available.

### Fixed
- Locked in the privilege-escalation mediation model: non-interactive `bash` blocks interactive sudo/doas/su/pkexec paths and directs elevation through the operator-visible terminal surface, while terminal allows operator-mediated sudo/doas by default and supports an emergency `OMEGON_DENY_PRIVILEGE_ESCALATION` blanket deny.
- Sanitized assistant reasoning segments so provider-specific `<think>` wrappers, HTML comment artifacts, and standalone bold heading markers do not leak into the TUI transcript.
- Fix 0.27.7 release smoke regressions in capability inventory tests, remote `/auth logout` routing, and settings menu drift rendering.
- Reduce active-turn worker notifications to a single queued-prompt warning instead of periodic no-action status toasts while long turns are still running.
- Bound post-loop auto-memory ingestion so late SessionEnd memory writes cannot keep the interactive worker active after a terminal TurnEnd.
- Disable post-loop bus request draining for interactive turns so terminal TurnEnd/AgentEnd immediately return control to the supervisor instead of waiting on side-effect cleanup.
- Make `just link` install bundled catalog agents offline by default so local build/link loops do not repeatedly hit unauthenticated GitHub raw catalog endpoints.
- Ensure structured menu rendering keeps the selected row visible on long menus by windowing against rendered line height and showing descriptions only for the selected row.

## [0.27.7] - 2026-07-08

0.27.7 hardens turn-completion diagnostics, auth command routing, and the secrets readiness surface. It adds first-party secret capability inventory for LLM, search, and forge integrations while preserving value-redaction guarantees.

### Added
- Add audit-log tool duration/progress cadence summaries plus structured runtime queue, prompt-start, context, stream-idle, and provider retry/failure projections.
- Add runtime queue wait/elapsed timings and queue-blocked prompt log fields for active-turn diagnostics.
- Add interactive-turn telemetry for post-turn cleanup phases and slow active-turn workers so logs and operator notifications can identify where queued prompts are blocked after visible completion.
- Add runtime turn lifecycle events wired through the supervisor tick/event loop, audit log, WebSocket serialization, and daemon SSE stream.
- Extend runtime turn lifecycle telemetry into worker-side phases so diagnostics can distinguish inner loop execution from post-loop cleanup stalls.
- Surface first-party harness secret catalog entries in `/secrets` readiness, including LLM provider, web-search, and forge credentials, and include configured/warmed undeclared secrets without resolving values.
- Add grouped first-party harness capability readiness over the secret catalog, so `/secrets` can show LLM provider, web-search, and forge readiness beyond per-secret consumers.

### Fixed
- Treat terminal `TurnEnd` events as authoritative for releasing the TUI active-turn gate, preventing the activity row from showing `active turn` while the footer says `turn done`.
- Fix TUI interrupt input suppression so an interrupted turn that never emits `AgentEnd` no longer leaves the composer permanently unable to accept typing or `/exit`.
- Propagate cancellation and timeout handling through extension tool retry paths after transport reconnects.
- Clarify slow active-turn notification wording so it does not imply the worker is in cleanup unless lifecycle telemetry proves that separately.
- Align `/auth` command registry and autocomplete with the executable grammar: provider IDs complete under `/auth login` and `/auth logout`, not as invalid direct `/auth <provider>` commands.
- Make `/auth` parser semantics match TUI intent by treating bare `/auth` as the auth menu and wiring advertised `/auth unlock` to the unlock command.
- Align `/secrets` menu and text status copy with the first-party secret catalog, including search-provider keys and the correct `OPENROUTER_API_KEY` spelling.

## [0.27.5] - 2026-07-08

0.27.5 hardens the 0.27.4 guidance loop from field evidence: the stuck detector no longer fires false repetition warnings on healthy exploration, continuation pressure recognizes interleaved analysis as output, Guarded mode returns trailing decision questions to the operator, and environment-flaky test/tool paths (keyring, read paging, spawn cwd) are made deterministic.

### Fixed
- Excluded path-normalized read/inspection, mutation, validation, and progress entries from the StuckDetector "same arguments" repeat pattern, so paging through one file in distinct ranges, distinct edits to one file, and repeated validation runs no longer fire false stuck-loop warnings.
- Fingerprinted search observations by query and roots instead of a constant marker, so healthy exploration with distinct grep/search queries is no longer misclassified as argument repetition.
- Made bash observation segmentation quote- and escape-aware, so quoted regex alternations like `grep -E "a|b"` produce one search event instead of shredding into bogus segments.
- Softened the stuck-loop escalation injection from an accusatory verdict to neutral repetition-pressure framing that names the exit paths (produce the deliverable, take one different action, or state the blocker).
- Held the continuation-pressure counter when the assistant emits substantive interleaved prose alongside tool calls, so "exploring without producing output" pressure only accrues on genuinely silent tool grinding.
- Stopped Guarded-mode dead-mouse continuation from auto-answering a trailing "want me to proceed?" when the operator prompt never authorized changes (e.g. assessment/review asks), returning that decision to the operator.
- Accepted provider numeric-encoding variants (`4237`, `4237.0`, `"4237"`) for the `read` tool's offset/limit arguments in both local and host-delegated paths, so paging arguments are no longer silently dropped and whole large files no longer dump from line 1.
- Suppressed platform keyring access deterministically in test executables (detected via the `target/**/deps/` path) so parallel test runs cannot race the env-var latch into live macOS keychain reads that fail in dark wake or prompt the operator.
- Gave TDD runner tests their own temp working directory instead of the process CWD, removing an ENOENT flake when parallel tests delete the directory they chdir'd into.
- Fixed the path-traversal boundary test to assert escaping into a non-temp sibling, not `/tmp`; temp roots are intentionally allowlisted for logs/test artifacts, which made the CI Linux assertion invalid.
- Kept the auth default-path test's environment mutation under the shared auth env lock, removing a parallel-test race that could clear `OMEGON_AUTH_JSON_PATH` while another test was writing its override auth file.
- `scripts/release_branch.py merge-forward` no longer regresses main's workspace version when main is behind the just-released version: version state is only preserved from main when main's version is >= the release branch's version; otherwise the release branch's version state is taken and the decision is logged.

## [0.27.4] - 2026-07-08

0.27.4 refines the harness guidance loop: an evidence ledger drives convergence pressure from discovery novelty instead of raw file counts, a task-mode channel relaxes pressure for research sessions, and a unified observation normalizer feeds structured-tool and bash evidence through one path.

### Added
- Added an A3 evidence ledger for harness guidance so discovery novelty and low-novelty revisits, not scalar file counts alone, drive evidence convergence pressure.
- Added a guidance task-mode channel (implementation vs research) inferred from the operator prompt, with explicit `/mode ...` / `[mode: ...]` markers, so research/Q&A sessions relax execution and orientation-churn pressure while failure-driven pressure stays active.
- Added a unified observation normalizer for harness guidance so capability-catalog tools and conservative bash read/search/validation/commit/minimal-mutation commands feed intent evidence consistently.
- Made git guidance and validation recommendations document-aware so Markdown-only human documents/knowledge notes are not framed as code changes.

### Fixed
- Replaced the `files_read <= 2` actionability shortcut with novelty-decay-based local evidence sufficiency, preventing first targeted reads from forcing premature convergence.
- Suppressed anti-orientation drift and read-only evidence convergence in research mode unless validation or mutation evidence makes the target actionable.
- Required matching successful tool results before normalized observations create positive guidance evidence, so orphaned/missing tool results no longer look like successful reads or mutations.
- Routed StuckDetector file/churn tracking through normalized observation events so bash/view/search evidence shares the same validation and mutation reset semantics as structured tool calls.
- Gated live OpenAI-compatible provider endpoint probes behind `OMEGON_LIVE_ENDPOINT_PROBES=1` so default test runs do not fail or stall on external network availability.
- Serialized current-directory-mutating tests on a shared CWD test lock so the default parallel `cargo test -p omegon` run no longer observes deleted temporary working directories.
- Coalesced duplicate tool results after provider-compatible ID sanitization so wrapper-tool rollback/no-op paths cannot poison Anthropic history with multiple `tool_result` blocks.

## [0.27.3] - 2026-07-07

0.27.3 makes GitHub Copilot a first-class provider path, including Copilot OAuth, semantic route selection, model catalog metadata, tool-call support, and the `gpt-5.5` Responses API transport required by current Copilot models.

### Added
- Added first-class operator copy blocks for auth/device-code flows, including best-effort host clipboard copy for GitHub Copilot device codes and exact copy/export payloads in conversation surfaces.
- Added a GitHub Copilot provider bridge with OpenAI-compatible tool-call support, Copilot OAuth token exchange, and `/chat/completions` requests using VS Code-compatible integration headers.
- Added a redacted GitHub Copilot tools contract probe that verifies OpenAI-style `tools`, returned `tool_calls`, and tool-result continuation against the live Copilot endpoint.
- Added GitHub Copilot device OAuth login plus a redacted inventory probe that verifies Copilot token exchange and `/models` access using VS Code-compatible integration headers.
- Added a non-UI redacted GitHub Copilot contract probe helper for token exchange and model-list endpoint verification without exposing token values.
- Grouped `/model list` output by conceptual model identity and included provider, producer, execution class, and availability per route.
- Added conceptual/producer/execution metadata to the TUI model catalog and grouped catalog routes by conceptual model identity.
- Added semantic route resolution so conceptual model intents can resolve to policy-compliant provider routes such as Copilot-only Claude Sonnet/Opus/GPT routes.
- Documented that the local Copilot CLI is a cleanroom oracle only, not a shipping dependency, and captured direct GitHub Copilot transport requirements before provider bridge work.
- Added producer and execution-class metadata validation for model registry entries while keeping producer optional for local/offline/dynamic routes.
- Added additive conceptual model IDs to the model registry so multiple provider routes can map to the same semantic model class ahead of first-class GitHub Copilot routing.

### Fixed
- Routed GitHub Copilot `gpt-5.5` through Copilot's `/responses` endpoint instead of the unsupported `/chat/completions` path and made Copilot runtime errors name the failing endpoint.
- Prevented GitHub Copilot routes with an empty effective model from sending blank `model` values to the Copilot API and reject explicit empty model switches such as `github-copilot:`.
- Added `github-copilot` to `/auth` command suggestions so Copilot login is discoverable from the TUI command picker.
- Unified `/auth` status probing with the canonical operator auth provider list so GitHub Copilot appears on every auth surface and obsolete Ollama Cloud rows no longer leak through the old hardcoded status path.
- Added typed `ModelRouteSpec` storage for provider route state and documented the route-state invariant so nested provider specs are canonicalized at construction rather than tolerated by every downstream consumer, including direct enum construction in tests and internal code.
- Canonicalized route-controller startup and model-switch inputs before storing route state, making nested provider strings unrepresentable at the route boundary.
- Updated capability grade inference to use shared Copilot-aware provider/model normalization so nested Copilot routes do not get down-ranked as Anthropic producer routes during orchestration.
- Aligned strict provider inference and auth-status provider/model parsing with the Copilot nested-provider rules so connected/authenticated indicators do not consult expired producer credentials.
- Forced malformed nested Copilot route strings to resolve and probe as `github-copilot`, preventing producer prefixes from winning provider inference in route switches, setup checks, footer state, and other shared provider callers.
- Routed `/auth list` to the shared auth status/list surface and hardened nested provider-qualified model IDs so Copilot routes like `anthropic:github-copilot:gpt-5.5` do not probe Anthropic credentials or preserve producer prefixes as execution providers.
- Added GitHub Copilot to the shared TUI `/auth` and `/model providers` provider rows, routed TUI/remote login paths through GitHub Copilot OAuth, refreshed CLI/null-provider auth guidance, and added coverage so operator auth providers cannot silently fall out of the menu again.
- Replaced unstable `if let` match guards in slash-command parsing so `just run` compiles on the supported Rust toolchain.

## [0.27.2] - 2026-07-05

### Fixed
- Fixed managed self-updates so `/update install` refreshes the real installer receipt at `~/.config/omegon/install-receipt.json`, writes the replaced binary into `~/.omegon/versions/<latest>/omegon`, repoints the `omegon`/`om` launcher symlinks, and rejects Cargo-managed installs with the correct `cargo install --force` remediation instead of mutating Cargo's bin directory.
- Set the trunk workspace target to `0.27.2` so release builds and `just run` no longer present as stale release candidates.

## [0.27.1] - 2026-07-04

- Added `/smoke` as a unified operator-facing smoke-test namespace and `/smoke cleave` to drive a value-set of shared live cleave/delegate progress and Workbench plan projections used by TUI, remote slash, ACP, and served WebUIs instead of writing progress into conversation transcript segments.

### Added

- Added Auspex local-authority proxy strict mode for the web daemon: `serve`/`embedded` can load a trusted proxy identity JSON, require matching Auspex subject/fingerprint headers before accepting proxy principals, reject query-token-only surface streams in strict mode, and report web authority status in startup metadata.

- Added a manual cleave approval gate so `cleave_assess` advertises menu-based operator approval metadata, `cleave_run` records pending approval state and refuses to launch clove workstreams without a matching approved plan digest, high-cost cleaves require second confirmation, and `/cleave` exposes approve/modify/deny/phased/save/evidence/reassess action backends.
- Extended `cleave_assess` with a backward-compatible decomposition strategy envelope, including strategy mode, confidence breakdown, warnings, assumptions, evidence, draft perforation lines, and parent obligations so future cleave runs can distinguish direct, phased, delegate, sequential, and parallel workstream plans.
- Added a first-pass cleave approval gate so `cleave_assess` advertises menu-based operator approval metadata, `cleave_run` records pending approval state and refuses to launch clove workstreams unless the harness supplies an approved plan marker, and `/cleave` exposes shared action backends for approve/modify/deny/phased/save/evidence/reassess.
- Documented the unified workstream-event direction for delegate and cleave continuations, including runtime event schemas, trust boundaries, turn injection, and Workbench synthesis behavior.
- Added semantic branch labels for delegates so Workbench rows can display human-readable labels such as `verify/tests` while preserving `delegate_N` as the stable result handle, and refreshed shared delegate progress immediately on background completion/failure so rows converge to terminal state without waiting for turn-end polling.

### Fixed

- Added `/smoke` as a unified operator-facing smoke-test namespace and `/smoke cleave` to drive a value-set of shared live cleave/delegate progress and Workbench plan projections used by TUI, remote slash, ACP, and served WebUIs instead of writing progress into conversation transcript segments.
- Fixed smoke surface terminal reconciliation so completed cleave and delegate smoke runs terminalize shared operation snapshots before publishing completed workstreams, clear stale live operation handles once the completed plan reaches the operator-ready state, and queued cleave children no longer inflate the Workbench `running` count.
- Made `manage_tools` report enabled tools from the active callable schema inventory so registered-but-filtered tools show as unavailable instead of falsely enabled.
- Hardened the cleave approval gate so high-cost confirmations are enforced at the final `cleave_run` boundary, non-approved approval states cannot be replayed, payload drift is rejected, generated approval IDs include a monotonic suffix to avoid rapid-call collisions, pending cleave approvals persist across reloads as review-required Workbench rows, active approvals are conservatively downgraded after restart, active plans are not clobbered by cleave approval workstreams, and approval menus only advertise implemented release-safe actions.
- Restored operator recovery when interrupting wedged turns by abandoning the active turn immediately after cancellation instead of waiting inside the potentially stuck turn path.
- Hardened delegate result handling so tool-name/agent-name confusion gets corrective guidance, background delegates return machine-readable result tool calls, and empty clove output is treated as degraded instead of successful.
- Aligned delegate Workbench rows with plan-style transition language, prioritizing active and pending-result delegates when the live log area is constrained.
- Removed slash-command delegate retrieval hints from result surfaces in favor of structured `delegate_result` tool-call guidance.
- Covered delegate Workbench transition labels and constrained-row priority ordering with focused tests.

## [0.27.0] - 2026-07-01

0.27.0 is a hardening and surface-coherence line. It makes provider/auth routing explicit instead of implicit, gives operators truthful TUI status when credentials or fallbacks are involved, expands the backend capability substrate for future console clients, and continues extracting lifecycle/TUI surfaces behind shared semantic boundaries.

### Added


- Added guided CRUD affordances to `/variables` and `/secrets` structured menus, including variable update actions, common variable templates, recipe-specific secret rows, and per-secret clear/replace actions that never display secret values.
- Added `/variables` and `/vars` as the MVP non-secret session configuration surface, with list/status, set, get, and delete/remove/rm command plumbing plus plain-value control readouts; secret-like names are flagged with `/secrets` guidance without blocking operator-defined variables.
- Added design notes for the `/variables` non-secret runtime configuration surface, including second- and third-order effects around OCI deployment, process injection, trust boundaries, and project portability.
- Documented the validator capability UX plan, including `/validate` Overview/Active/Available/Overrides/Runs/Settings tabs, Armory/community validator trust lifecycle, explain output, backend DTOs, and phased implementation plan.
- Updated model-budget grade resolution tests for the Anthropic B-grade default moving to Claude Sonnet 5.
- Added a self-contained `core.skill-manifest` domain validator for `skills/**/SKILL.md`, covering YAML/TOML frontmatter parsing, required `name`/`description`, and optional skill metadata field shapes.
- Added a self-contained `core.openapi-contract` domain validator for `*.openapi.yaml`/`*.openapi.yml` specs, covering required metadata, paths, operation responses/ids, path parameters, and local `$ref` resolution.
- Added a self-contained `core.model-registry` domain validator for `data/model-registry.json` that checks required model fields, duplicate provider/model ids, and default/grade references.
- Structured `validate` plan details as DTO-style entries with validator id, kind, source, mode, status, paths, runner summary, and replacement metadata so UIs no longer need to parse prose.
- Coalesced `validate` toolchain adapter runs by language so multiple Rust, TypeScript, or Python paths trigger one project-level validator run per language instead of duplicate command executions.
- Made `.omegon/validators.toml` parse/schema problems fail `validate` loudly as validation configuration errors instead of silently ignoring broken operator validator policy.
- Added self-contained embedded `validate` checks for JSON, TOML, YAML, and Markdown hygiene so common artifact validation no longer depends on external tools, and wired operator `mode = "replace"` to suppress named built-ins such as `core.markdown-basic`.
- Hardened `validate`'s built-in process execution to run validator programs directly with fixed argument vectors instead of shell interpolation.
- Split the built-in `validate` tool checks into bounded Rust, TypeScript, and Python validator implementations behind a shared validator trait, preparing the surface for extension-backed validators.
- Added Anthropic Claude Sonnet 5 to the model registry and promoted it as the Anthropic B-grade balanced route while keeping Claude Fable 5 as the S-grade/default route.
- Added `scripts/setup-dwarfstar.sh` and `scripts/start-dwarfstar.sh`, opt-in happy-path helpers for cloning/building/downloading and launching a local DwarfStar/ds4 OpenAI-compatible server with RAM guard defaults and Omegon consumption hints.

- Added an Actions tab to the shared `/memory` menu with recall/list/focus/release/compact rows; argument-taking actions prime the editor and memory compaction requires confirmation.
- Added refreshable menu action close policies and menu-level action hotkeys so `/ui` toggles and lean/full shortcuts update the active menu in place.
- Added typed menu action dispositions for run-command versus focus-row actions, and routed TUI menu dispatch through the typed action substrate while retaining compatibility helpers.
- Added second-activation confirmation gating for high-impact menu actions such as profile apply, extension update, and runtime substrate refresh.
- Routed bare `/sessions` to a shared saved-session menu with resume actions while preserving `/sessions list` and `/sessions all` as text readouts.
- Added human-friendly Docker-style session display names, kept canonical timestamp ids as the filename/order key, and made friendly-name resume lookup require uniqueness.
- Cleaned up slash-command metadata drift so menu-default commands advertise menus and bare `/context`, `/model route`, `/profile`, and `/sessions` no longer canonicalize as legacy text readouts.
- Hardened extension/runtime menu mutations so runtime refresh and extension update require confirmation and refresh the shared menu after execution.
- Routed bare `/memory` to a read-only shared memory overview menu while preserving `/memory status` and `/memory overview` as text readouts.
- Routed bare `/extension`, `/ext`, and `/runtime` to a shared Extensions & Runtime menu, preserved extension inventory readouts, and made runtime refresh/reload aliases canonical.
- Routed bare `/secrets` to a safe shared secrets menu while preserving status/list readouts, hidden input, recipe-backed set commands, and explicit get/delete commands.
- Routed bare `/ui` and `/ui surfaces` to the shared UI menu while preserving `/ui status` as the text readout and keeping direct preset/surface toggle commands.
- Made `/skills` a top-level active skills readout: skill inventory rows now come first with inspect/install actions, `i` installs the selected skill from the menu, session/project operations are grouped separately, and `/skills --help` owns command syntax help.
- Added a renderer-neutral menu projection and TUI menu surface, then routed `/skills` to a structured keyboard-first menu with tabs, filtering, badges, grouped rows, and selected-command execution through the existing slash-command path.
- Added structured stream-idle telemetry (`AgentEvent::StreamIdle`, Web `stream.idle`, ACP `_stream/idle`) so provider idle/stall diagnostics can flow to UIs without being rendered as assistant-authored content.
- Added dedicated agent-callable skills tools (`skills_list`, `skills_get`, `skills_create`, `skills_import`, `skills_install`, `skills_delete`, `skills_reload`) backed by a first-class skills feature and shared augment registry reload path.
- Added `/extension refresh`, `/extension reload`, and `/extension restart` as safe aliases for a partial live runtime substrate refresh: skill augments reload immediately, extension/process/widget changes remain candidate inventory until explicit promotion support lands.
- Added a guarded `/runtime restart` substrate-refresh candidate surface with runtime generation, live startup substrate inventory, and non-mutating extension/skill candidate inventory so the extension/runtime reload design has an operator-visible command seam before generation promotion lands.
- Aligned `/skill`/`/skills` command discovery, TUI usage, ACP help, and the skills palette with current skill internals: scoped create/new/import, Armory installs, reload/refresh aliases, provenance/shadow/conflict inspection, and current-session reload semantics.
- Added `docs/web-ui-backend-rbac-identity-requirements.md` to capture the Web UI backend identity, RBAC, trusted-proxy principal, stream-auth, contract, and validation requirements for future work.
- Added `docs/web-ws-contract.md` as the authoritative contract for the legacy `/ws` WebSocket control surface, documenting auth, per-message role checks, inbound command payloads, slash-command remote-safety, server response envelopes, and security notes for Web UI compatibility clients.
- Omegon Web now supports prompt attachments end to end: the SPA composer has a file picker that base64-stages files via `POST /api/web/attachments`, and `submit_prompt` resolves the staged ids to on-disk paths (materialized under their original filename so downstream extension-based image detection works) and forwards them as the prompt's attachment paths. Image-only prompts (attachments, no text) are accepted; unknown/expired/malformed attachment ids are rejected with `400`. Staging path-sanitizes filenames. The TUI-hosted web path gets full image support; the headless `serve` daemon logs and ignores attachments for now.
- Omegon Web reloads are now lossless: a single-writer accumulator task subscribes to the agent event bus and folds `TurnStart`/`MessageChunk`/`TurnEnd` into a bounded (400-segment) conversation transcript, with user prompts recorded at submission so ordering is preserved. `GET /api/web/surfaces` serves this transcript, and the SPA rebuilds it idempotently on load/reconnect — replaying prior turns and resuming an in-flight assistant turn instead of starting blank or duplicating bubbles. Text-free (tool-only) assistant turns are dropped from replay.
- Wired interactive operator-wait resolution through the Omegon Web SPA: broadcast `OperatorWaitRequest` events are `acknowledge`-d synchronously on capture (beating the producer's ~2s deadline) and their responder is captured by id, so `POST /api/web/actions` with `respond_operator_wait` delivers the operator's Completed/Cancelled decision — `202` on success, `404` when unknown/already-answered. The SPA renders inline Mark-complete/Cancel buttons. Contract updated.
- Wired interactive tool-permission approval through the Omegon Web SPA: broadcast `PermissionRequest` events now have their responder captured by the surface stream (keyed by a coordination-free `Arc`-identity id emitted to the browser as `request_id`), and `POST /api/web/actions` with `respond_permission` resolves the prompt — returning `202` on success or `404` when the request is unknown/already-answered. The SPA renders inline Approve/Deny buttons. Updated `docs/web-api.openapi.yaml` accordingly.
- Added the Omegon Web single-agent SPA at `GET /web` — a self-contained, no-CDN HTML/JS client that consumes the `/api/web/*` contract: loads the `/api/web/surfaces` snapshot, streams live deltas over the `/api/web/surfaces/stream` WebSocket (assistant output, tool lifecycle, permission/operator-wait banners), and submits prompts/cancel/slash commands via `POST /api/web/actions` with optimistic transcript rendering and auto-reconnect. Startup logging now advertises the SPA URL alongside the dashboard.
- Added `docs/web-api.openapi.yaml` — an OpenAPI 3.1 contract for the omegon-web `/api/web/*` surface (capabilities, launch-context, surface snapshot, surface stream WebSocket, action ingress, sessions, attachments), derived from the axum handlers and DTOs in `core/crates/omegon/src/web/`.
- Added a Rust-native OpenAPI contract linter (`cargo test -p omegon --test openapi_contract_lint`, also `just lint-openapi`) that structurally validates every `docs/**/*.openapi.{yaml,yml}` spec — version, info, non-empty paths, per-operation responses, unique operationIds, path-template/parameter agreement, and local `$ref` resolution — so contracts are gated by CI without a Node/Python toolchain.
- Added initial Omegon Web discovery endpoints for browser/Auspex capability metadata, launch context, native surface snapshots, browser action ingress, browser-native surface event streaming, session discovery, and staged browser attachments.
- Added a registry-native `/loop` MVP for durable recurring prompt job definitions, binding jobs to prompt path and content hash ahead of daemon scheduler execution.
- Added `/loop menu` schedule recipes plus agent-facing `loop_list`, `loop_create`, `loop_status`, and `loop_stop` tools for durable loop jobs.
- Wired daemon loop-job scheduling for enabled interval jobs with run-history records, prompt-hash drift pausing, and max-run stop enforcement.
- Improved `/loop list` observability with run counts, last outcome, next due time, and disabled-job next-state markers.
- Added machine-readable loop scheduler summary to daemon startup status.
- Added focused daemon loop-scheduler tests for due dispatch, prompt-hash drift pausing, and max-run stop handling.
- Added `/ui activity` controls for showing, hiding, and toggling the slim live activity stream independently from dashboard, instruments, and footer surfaces.
- Made `/init` the explicit boundary for creating project-local `ai/memory/` durable memory scaffolding.
- Ignored local per-project `.omegon/profile.json` files so live profile changes do not dirty source checkouts.
- Added `omegon skills import <path> [--project] [--force]` for importing Claude/Omegon skill bundles while preserving scripts and resources.
- Added read-only `omegon skills doctor` onboarding diagnostics for Claude-compatible skill roots, script references, and registry conflicts.
- Locked in the ecosystem-superset skill registry design for Claude-compatible skills, upstream Git skill repositories, source lock/provenance, and merge-first conflict resolution.
- Added extension manifest-declared skill bundles plus conflict-resolution metadata for non-1:1 activation/trigger overlap.
- Added `/api/acp` as the documented external-client ACP WebSocket path while preserving the legacy `/acp` alias, and clarified that Auspex should use native HTTP/SSE/WebSocket APIs as its first-party control plane.
- Added native first-party session resource endpoints under `/api/sessions` for Auspex session create/metadata/surfaces/actions access, including assistant profile readiness validation during session creation, explicit singleton-live allocation metadata, consistent session response links, and session-scoped stream id validation.
- Added `GET /api/assistant-profiles/{id}` to return the detailed assistant profile substrate plus compact readiness metadata for Auspex profile selectors.
- Added a local `omegon-rbac` crate for Omegon/Auspex capability vocabulary layered on `styrene-rbac`, runtime RBAC policy descriptors, and endpoint-level native-session action enforcement, plus upstream issue notes for generic Styrene RBAC extension points.
- Hardened Web UI backend RBAC across native session creation/read, surface snapshot/stream access, daemon event ingress, and legacy WebSocket prompt/cancel ingress so configured read-only roles cannot reach write-class paths by omitting caller role metadata.
- Redacted tool arguments, output tails, and summaries before storing or streaming Web UI instrument state so browser-visible tool telemetry does not retain known secret values.
- Added read-only lifecycle query endpoints for ready, blocked, and frontier design nodes.
- Added read-only lifecycle endpoints for web/console clients: `GET /api/lifecycle/snapshot`, `GET /api/lifecycle/design`, and `GET /api/lifecycle/design/{id}`.
- Added `GET /api/workspaces/leases` to expose active checkout workspace leases for the web/console backend.
- Added `GET /api/providers/status` and `GET /api/extensions` read-only web backend endpoints for provider readiness and installed extension status inventory.
- Added `GET /api/runtime/status` and `GET /api/runtime/capabilities` as stable read-only runtime foundation endpoints for the Omegon web/console backend.
- Added prompt skill conflict resolution so overlapping activation/trigger skills inject only one provider by default and user-facing output recommends project-local merge resolution.
- Added semantic composer draft UI actions for replacing, clearing, and attaching paths to the TUI editor through the shared `UiAction` seam.
- Added semantic composer cursor/edit UI actions for character/word movement and bounded edit operations without exposing frontend key events.
- Added semantic composer text insertion for typed characters and pasted text, including existing large-paste collapse behavior through the shared action seam.
- Added parent-lane system notifications when background delegates complete or fail, including actionable `/delegate result <id>` hints.
- Injected the live delegate queue into model context when background delegates are active or have unviewed terminal results, making reconciliation front-and-center for the agent.
- Strengthened active plan context injection so the Workbench plan remains front-and-center until active/todo items are reconciled.
- Improved TUI composer history recall to preserve the pre-recall draft and clear history session state when the operator edits recalled input.
- Added a visibly button-like `Copy` conversation affordance with a wider click target for segment copy actions.
- Added a first-class `/secrets` action selector with list/set/delete choices, shared set-secret picker behavior, and aliases for status/remove/rm so the secrets menu matches the newer command-selector flows.
- Added semantic UI actions for copying a selected conversation segment or the latest assistant response so frontend adapters can route copy through the shared action seam.
- Added a confirm-first empty-prompt history preload: pressing Enter on an empty prompt previews the last prompt as ghost text, and pressing Enter again materializes it for explicit resend.
- Added source-aware profile load/save primitives and `/profile save|capture --project|--user|--active` parsing so profile capture preserves project/user persistence boundaries.
- Added requested-context-class persistence to profiles so saved defaults can restore the operator-requested working-set policy separately from the model-derived context window.
- Added a pure bridge from durable model intent to provider routing requests, including provider-only filters for endpoint selection.
- Added endpoint-profile-driven error normalization for OpenAI-compatible non-success responses.
- Added profile schema support for durable model intent persistence.
- Added `/model route` and `/model providers` aliases for inspecting active model routing and provider options.
- Added `/model policy <exact|minimum|nearest>` to update durable grade-policy intent without switching the active route.
- Added `/model unpin` to clear exact model overrides while preserving the active route and grade/provider intent.
- Added a renderer-neutral slash command menu projection that merges built-in and feature command definitions with source, availability, safety, and badge metadata for shared autocomplete/help surfaces.
- Added per-command safety metadata to built-in slash command specs so autocomplete and `/help` can distinguish read-only, queue-mutating, state-changing, external, and destructive commands.
- Moved built-in slash command metadata into a shared command registry module so non-TUI surfaces can consume the same command definitions.
- Routed ACP available-command advertisement and `/help` through shared slash command registry metadata while preserving the existing ACP `/thinking` and `/login` command names.
- Let selector popups opened from `/settings` handle their own keyboard input while preserving the persistent settings overlay behind them, so Enter confirms row edits instead of being recaptured by settings navigation.
- Added Workspace role/kind rows to the shared settings projection so the `/settings` workspace tab exposes the existing workspace selectors instead of hiding dispatched editors behind unreachable row IDs.
- Wired Enter on TUI `/settings` rows to the existing model, thinking, context, tool-density, update-channel, workspace-role, and workspace-kind selectors so the settings screen now edits already-supported choice settings.
- Expanded the shared settings projection with Runtime, UI, Workspace, and Updates tabs plus editor, choice, status, route, and persistence metadata for full-screen settings rendering.
- Added initial TUI settings-screen state backed by the renderer-neutral settings surface projection so `/settings` can evolve into a persistent surface rather than a selector chain.
- Added a renderer-neutral settings surface projection under `surfaces/settings.rs` and routed the harness settings command/tool overview through it so TUI, ACP, CLI, web, and agent-facing paths can share one semantic settings model.
- Exposed `/settings` through the shared command registry, including ACP availability, so ACP-hosted clients can read the same harness settings overview as the in-agent settings surface.
- Added a pure permission policy evaluator for per-tool allow/prompt/deny rules with simple wildcard subject matching as the first slice of the broader permission-policy engine.
- Wired live settings into delegate and cleave features so `/autonomy` changes resolve the same subagent policy for prompt/status and tool gate enforcement while preserving conservative fallback.
- Made delegate and cleave autonomy gates policy-injectable so tests can prove higher-autonomy policies allow operations that conservative defaults gate, reducing prompt/status/tool policy drift.
- Wired base prompt assembly to accept the operator-selected subagent policy so future `/autonomy autonomous` sessions can render orchestrator guidance instead of the conservative default.
- Exposed the mapped subagent autonomy policy in `/automation status` so the operator can see delegate/cleave posture, limits, and the loop/scheduled-job non-escalation note.
- Mapped `/autonomy flow` to the conservative subagent policy so flow continuation does not implicitly make `cleave_run` a normal operation; `/autonomy autonomous` remains the power-user orchestration posture.
- Added a mapping from the existing `/autonomy` automation levels to subagent autonomy policy levels so future prompt/gate wiring can use one operator-facing knob.
- Added an internal autonomy envelope model for session, loop, scheduled-job, and explicit-approval policy precedence without changing loop execution behavior yet.
- Added execution substrate projection to IPC/web runtime snapshots so attached clients can see the detected host/container substrate alongside harness status.
- Added a conservative subagent autonomy policy layer that now drives prompt guidance for delegate/cleave operations, keeping `cleave_run` approval-oriented by default while allowing bounded scout/verify delegates.
- Gated mutating `delegate` patch workers behind structured approval details under the conservative subagent autonomy policy before allocating delegate tasks or spawning clove workers.
- Gated over-limit `cleave_run` requests behind structured approval details under the conservative subagent autonomy policy before creating cleave workspaces or spawning cloves.
- Added CLI parser coverage for the `--oci` alias, OCI image/runtime overrides, and conflict handling with `--dangerously-bypass-permissions`.
- Marked host-shim OCI launches with `OMEGON_RUNTIME_CONTEXT=host-shim-oci`/`OMEGON_OCI_LAUNCHER=omegon` and made recursive `--oci` requests inside an OCI container fail closed.
- Added an orchestrated OCI runtime design for Kubernetes/CRI deployments where Omegon is launched by the orchestrator rather than by the native `--oci` host shim.
- Added `--oci` as an alias for the existing OCI-backed `--sandboxed` launcher path, plus CLI/env image and runtime overrides that default the containerized substrate to `ghcr.io/styrene-lab/omegon-full:<version>`.
- Added an OCI CLI execution-boundary design for a host-shim `omegon --oci` mode, covering image/runtime resolution, raw argv forwarding, mount/auth policy, daemon port mapping, extension isolation, and phased integration.
- Documented the validated containerized daemon/control-port probe for the local `omegon-full` OCI image, including current provider-auth, host-extension, and IPC limitations.
- Documented the validated Nix/Lima/nix2container/Podman smoke path for the local `omegon-full` OCI substrate and added local export/load recipes for pre-publication testing.
- Added architecture-aware local OCI build guidance/target and made `oci-smoke` platform selection explicit via `OCI_PLATFORM` instead of forcing amd64 on Apple Silicon.
- Added an initial Nex-facing `substrates/omegon-full` package/profile seed for the full-first subagent OCI substrate.
- Expanded the subagent OCI substrate design around a full-first trim-down strategy, Armory/extension layer composition, mount policy, and Ratatui dogfooding constraints.
- Documented the inherited-default subagent execution boundary and added a Podman-first OCI smoke target for the explicit isolated subagent substrate.
- Added first-class delegate cancellation status so cancelled delegate cloves project as terminal non-failure operation rows instead of failed tasks.
- Added `delegate_cancel` so running delegate tasks can be marked cancelled explicitly, preserving terminal non-failure state in delegate status and Workbench operation rows.
- Fixed subagent/delegate review findings by keeping background delegate startup responses machine-readable, marking delegate/subagent status commands read-only, narrowing the plural `/subagents` alias, and avoiding cleave tool names in base prompts when only `delegate` is exposed.
- Clarified subagent/delegate/cleave design guidance so requests like “use subagents to merge these branches into main” still route through `cleave_assess`/`cleave_run` when the task shape requires coordinated merge work.
- Clarified subagent prompt guidance so operator requests to "use subagents" are classified by task shape and may still route to `cleave_assess`/`cleave_run` for coordinated multi-branch or multi-scope work.
- Added ACP command-registry coverage for the `subagent` alias and clearer background subagent start messages with status/result retrieval guidance.
- Added `/subagent` and `/subagents` as operator-facing aliases for delegate status, and updated delegate metadata to describe same-provider subagents as the default path.
- Added `just link-doctor` to inspect launcher PATH resolution, selected target, channel files, and fallback binary state.
- Added fixture coverage for the stable launcher resolution policy, including env overrides, nearest checkout selection, channels, fallback binaries, paths with spaces, dev-release fallback, and self-recursion rejection.
- Added a stable `omegon`/`om` launcher installed by `just link`, with deterministic multi-checkout resolution via explicit env overrides, nearest checkout builds, named `~/.omegon/channels`, and a fallback installed binary.
- Added backend surface contracts for ACP/RPC skill and prompt definition management, covering list/get/create/update/delete plus skill install and prompt preview endpoints in the capability registry, wired ACP/IPC/WebSocket dispatch for existing skill and prompt read/preview handlers, added reusable prompt definition storage and safety verdicts for bundled/user/project-local prompts, registered `/prompt` as the command-palette-native prompt routing surface, deprecated `_prompts/submit` as a read-only preview alias, and documented prompt/user-command authoring.
- Added advisory activation metadata (`activation`, `profile`, `project_signals`) to bundled skill manifests and the skill builder schema so profile-aware skill loading can be implemented from declared intent instead of prose inference.
- Added typed activation/profile diagnostics for skill manifests so future consumers can normalize advisory metadata and report unknown or unsafe user-skill values without crashing.
- Added advisory project-signal matching helpers for skill activation metadata, including root-relative literal matching, root-only globs, recursive `**/` globs, ignored vendor/build directories, and invalid-pattern rejection.
- Added a source-agnostic skill suggestion helper that evaluates parsed bundled, user-installed, Armory-installed, or project-local skills against profile, intent, and project-signal evidence without performing runtime injection, preserving external metadata diagnostics instead of silently dropping malformed activation hints.
- Added the Modern Command Palettes design node for palette-style `/skills` and `/prompt` operator surfaces.
- Add Java, Kotlin, and C# discovery/chunking to `codebase_search`; add a safe repo-relative `within` result scope, cancellation-aware index/search execution, and scope diagnostics in tool details.
- Trace auth.json provider key-set deltas on credential writes, refreshes, and logout so future OpenAI/Codex credential disappearance incidents identify the mutating operation and whether `openai-codex` was dropped. Auth updates now also refuse to replace an unparsable existing auth store with a partial credential file.
- Add redacted provider-auth and route-state diagnostic tracing for auth.json path selection, credential source/probe decisions, OAuth refresh/write-back, login outcomes, and fallback/disconnected route causes so relaunch login regressions leave an attributable trail.
- Rebuild the release binary inside `just link` before selecting the binary to alias so local `omegon --version` cannot point at a stale artifact from an older HEAD.
- Add peer-agent conversation representation/projection support so delegate/cleave/A2A output can carry producer identity independently from assistant/tool rendering.
- Add an explicit assess-time assumption question to the design-tree prompt injection: reviewing a design node now directs the agent to ask what unstated assumptions the design makes and record them as `[assumption]`-tagged questions.
- Add `just source-clean` and source-vs-agent-state dirty classification so live `.omegon/` telemetry no longer blocks source-plane cleanliness checks.
- Add a provider route controller path for startup, login, model-tier switches, offline model switches, and runtime turn configuration so route state is explicit instead of inferred from whichever bridge happens to be installed.
- Publish provider route changes as structured events and persist provider route warnings in the TUI footer so unavailable credentials, fallback routing, and disconnected states remain visible after the triggering operation.
- Include provider route state in `/auth status`, including the selected route, served bridge, credential diagnostics, and remediation guidance.
- Add explicit startup-route, route-login terminal outcome, and login-lifecycle regression coverage for connected, fallback, disconnected, relogin, and failed-login paths.
- Add a read-only capability inventory substrate and ACP `_capabilities/inventory` surface for installed extensions, Armory assets, catalog agents, assistant profiles, capability graphs, trust summaries, secret readiness, assistant launch readiness, assistant-list projections, and assistant run read models.
- Add assistant run read surfaces and terminal-state compatibility so the non-interactive run contract uses `completed` while still accepting legacy `succeeded`.
- Add metadata-only backend endpoint registry entries for ACP/runtime/lifecycle/provider/extension/secret/package/plan/task surfaces, including provider retry/failure and turn-cancelled telemetry contracts.
- Add richer cleave/delegate progress visibility, injected-clove execution tests, timeout/cancellation coverage, and Workbench integration for active lifecycle workstreams.
- Add headless ACP lifecycle read surfaces for lifecycle snapshots and design-node list/get/ready/blocked/frontier projections.
- Add `/copy answer` plus focused `/help copy` and `/help mouse` guidance for normal-mode answer copy, transcript scroll, and mouse passthrough.
- Add affected-crate validation tooling (`just affected`, `just test-changed`, `just check-changed`, `just clippy-changed`, `just test-commit`) plus `just source-clean` and source-vs-agent-state dirty classification for safer local commits.
- Add provider-drift tooling with `just upstream-provider-check`, Anthropic public-model drift checks, Claude Fable 5 / limited-access Mythos 5 registry entries, and adaptive-thinking metadata derived from the model registry.
- Add lifecycle/design documentation and an assess-time assumption prompt so design reviews explicitly ask what unstated assumptions should be recorded as `[assumption]` questions.
- Add Codebase Mind metadata and design documentation for durable structural repository memory.

### Changed

- OpenAI/Codex stream-stall exhaustion now uses longer, reasoning-aware budgets and operator wording that distinguishes ambiguous silent reasoning windows from definite wedged streams.
- `/skills reload` now advances and reports the runtime generation, and `/skills refresh` is accepted as an alias so iterative skill authoring has the same operator vocabulary as substrate refresh flows.
- Skill activation/resolution events now have a first-class conversation segment and ACP projection so startup and reload skill provenance can render as single-line TUI timeline affordances instead of generic system text.
- The skill creator prompt now supports upstream-assisted authoring: named public skill sources are searched/fetched/reviewed first, adapted with provenance and safety notes, and only then written to external skill directories.
- Upstream skill adaptation now treats bundled executable assets as a binary trust decision rather than a static-analysis safety problem, defaults Node/npm/pnpm/yarn assets to clean-room rewrite or omission, and requires clean-room adaptations to document omitted upstream assets.
- `skills doctor` now surfaces adapted-skill provenance warnings for missing safety/adaptation sections and executable/tooling mentions without an explicit trust posture.
- Skill manifest parsing, activation metadata validation, project-signal matching, and adapted-skill linting now live in the new `omegon-skills` crate and are re-exported by the CLI skills module.
- Resolved skill-entry metadata, provider ranking, shadow/conflict detection, and entry finalization now live in `omegon-skills`, leaving the CLI skills module focused on filesystem discovery and command rendering.
- Skill bundle discovery, script-reference diagnostics, import-summary metadata, and bundle conflict detection now live in `omegon-skills`; the CLI skills module now wraps those helpers for filesystem roots and report formatting.
- Runtime skill metadata extraction for trusted paths and phase/step tracking now lives in `omegon-skills`, with the CLI skills module retaining compatibility re-exports for setup and loop callers.
- Skill manifests now support structured upstream provenance metadata, and `skills doctor` uses it to audit adaptation posture and executable-asset trust decisions before targeted imports.
- Skill suggestion activation logic and source-aware recommendation tests now live in `omegon-skills`, leaving the CLI skills module focused on inventory and command surfaces.
- Bootstrap status now accounts for the active runtime settings before rendering, showing the selected route/profile alongside context and thinking instead of preserving the stale default `Tier: B` orientation. Bootstrap expectations are now computed as structured status data with subject-kind classification for orchestration, inference, memory, skill inventory, and extension inventory deltas, backed by startup skill and extension inventory discovery.
- Disabled broad editor-area breathing/glow effects in the TUI so post-render color shifts no longer mutate engine-ribbon separator styling.
- Expanded the omegon-web browser API capability descriptor with explicit surface stream, actions API, and legacy WebSocket flags, and typed the surface-stream WebSocket envelope for a stable UI contract.
- Added `omegon skills install` cleanup and `skills doctor` detection for stale installed copies of the old bundled `vault` markdown skill after its rename to `flynt`.
- Renamed the bundled interlinked-markdown skill from `vault` to `flynt` so markdown workspace guidance no longer overloads HashiCorp Vault/security terminology.
- Polished bundled skill guidance ahead of 0.27.0 by removing overly project-specific language, softening generic Python environment-manager defaults, and aligning the OCI CI example with immutable-tag guidance.
- Surfaced startup skill activation events in the runtime event stream and mirrored them into audit logs so operators can see and review skill activation/provenance.
- Added `omegon skills import <path> [--project] [--force]` for copying Claude/Omegon skill bundles or direct `SKILL.md` files into user or project skill roots, and updated `skills doctor` to recommend those import commands.
- Renamed the abstract runtime guidance registry from `PluginRegistry` to `AugmentRegistry` to distinguish prompt/persona/skill augmentation from extension/plugin package concepts.
- Included skill source/editability/reload/shadow metadata in `/skills get` and CLI `omegon skills get` output.
- Rendered skill source/editability/reload/shadow metadata in the `/skills` palette so override provenance is visible without opening each skill.
- Added source/editability/reload/shadow metadata to structured skill listings so project overrides expose the lower-precedence skills they shadow.
- Locked `SKILL.md` generation to YAML frontmatter as the canonical portable skill metadata format while preserving TOML frontmatter parsing for existing Omegon skills.
- Renamed the internal plan surface aggregate to `PlanSurfaceInputs` and removed the local lane projection mirror so active plan lanes use the shared `omegon_traits` projection DTO end-to-end.
- Promoted internal `PlanUpdated` events to typed plan surface projections while keeping web/IPC/MQTT legacy snapshot adapters for external compatibility.
- Added a shared tool visual identity resolver for hierarchical tool realm/family/transport labels and routed transcript/workbench tool chrome through it, with unknown shell-mediated commands honestly retained as shell-family labels instead of expanding an exhaustive command taxonomy.
- Routed TUI workbench plan projection tests through typed `omegon_traits` plan DTOs and tightened active-plan row copy around the next actionable item.
- Added double-click expansion for the compact workbench plan row so operators can open full plan details from the workbench surface.
- Added structured child vital-sign tool activity so instruments and websocket projections can preserve shell args summaries while keeping legacy `last_tool` compatibility.
- Routed TUI tool category glyph selection through structured tool identity/category mappings instead of raw display-name matching where identity is available.
- Updated tool inspection detail headers to use tool args summaries when resolving category glyphs for shell-mediated commands.
- Refined Unicode TUI tool category glyphs to reduce box/diamond collisions across read, design, memory, and generic roles.
- Moved the Nerd Font brain glyph to the memory tool category and made the generic fallback visually quiet.
- Updated the TUI glyph matrix workspace, tool, and tool-state domains with the selected ASCII, Noto Symbols 2 Unicode, and Nerd Font symbols.
- Restyled the lean editor engine block as an angular Nerd Font/Powerline ribbon with an omega mark, provider glyphs, grade emblem, thinking icon, and context database glyph.
- Added TUI glyph profiles with confidence-scored Nerd Font detection, Kitty config/font-install probes, and portable fallback icons for terminals without compatible fonts.
- Routed ACP plan update handling through typed plan projections and replaced JSON snapshot plan-entry tests with semantic projection coverage.
- Documented IPC/MQTT `plan.updated` as a legacy compatibility envelope over the typed internal plan projection contract.
- Removed legacy JSON workbench adapters from TUI plan state tests and routed workbench state through typed plan projections instead.
- Polished the compact TUI workbench row with symbolic workspace labels for repo, directory, and git branch, deriving the repo label from git remote metadata instead of the local checkout directory and omitting duplicate directory labels when the repo and current directory share the same name.
- Removed the Anthropic subscription ToS startup toast from normal interactive TUI sessions; the disclosure now stays scoped to headless automation and explicit `/cleave` background-worker launches.
- Removed inline `^O details` affordances from slim tool rows now that single-click selects and double-click expands or copies segments.
- Renamed the compact TUI rows internally around the engine/workbench/session contract and removed duplicated model/context fields from the bottom session row.
- Moved command panel, prompt, toast, modal, and severity DTOs into renderer-neutral surface projections so TUI command modals share backend-decoupled state with future clients.
- Unified TUI command/modal geometry across the persistent settings surface and slash-command selectors so tabs and nested command editors keep a cohesive footprint.
- Added settings-screen search/filter mode so operators can narrow visible rows before editing.
- Wired settings-screen max-turn, sandbox, and auto-update rows to existing control/persistence paths instead of showing placeholder toasts.
- Moved settings-screen row selection and tab switching into reusable screen helpers and routed choice rows through their semantic projection metadata instead of hard-coded per-row selector builders.
- Replaced the hard-to-discover inline conversation `Copy` button with a double-click copy flow: double-click a collapsed tool card to expand it, then double-click the expanded/copyable segment to copy plaintext.
- Added selected conversation segment hints for double-click copy/expand actions and a visible centered action toast with conversation/footer pulses on successful copy.
- Added a scrollable copy-text modal as an intermediate selected-segment detail surface before replacing selected-segment copy with inline transcript copy buttons.
- Removed the old TUI-local `/context status` dump and routed context status through the shared control-runtime palette projection.
- Rendered `/think list` and `/think status` through the shared palette projection so thinking-level discovery now uses action rows while bare `/think` continues to open the existing TUI selector and `/think <level>` remains the direct setter.
- Rendered `/prompt list` through the shared palette projection so prompt inventory now uses the same action/object row semantics as `/skills`.
- Extracted the shared palette row/group projection used by modern command-menu surfaces and moved `/skills` rendering onto it as the first consumer.
- Started the modern menu/settings consolidation tracks and changed `/skills` default output into a compact action/object palette with detail-on-demand guidance instead of a report-style inventory dump.
- Moved simple TUI settings selector apply parsing for tool density, update channel, and workspace role/kind into the settings menu migration seam so the main TUI loop only dispatches validated outcomes.
- Moved more TUI settings selector projections — preferences, tool-density, update-channel, and workspace role/kind choices — into the settings menu module as the next step in extracting settings behavior from the main TUI loop.
- Removed the TUI Focus mode path; Tab on an empty composer now toggles the latest tool detail row directly, Shift+Tab collapses it, and `/focus` reports the replacement controls.
- Wrapped slim TUI reasoning preview rows to the terminal width so long thoughts stay legible instead of running off-screen.
- Made `just run` rebuild this workspace's dev-release binary from current source before execing it, so TUI smoke checks cannot reuse a stale dev-release executable.
- Unified Slim TUI tool inspection around concrete transcript tool cards so live tools and Ctrl+O-pinned details share the same inspection renderer, with success glyphs aligned before tool identity/category glyphs and delegate/cleave rows classified as subagent tool calls.
- Routed TUI, web dashboard, startup, tutorial, skill-builder, persona-builder, voice, and active-turn prompt/cancel paths through actor-tagged runtime supervisor ingress instead of TUI/web-local queue or cancellation side channels.
- Improved slim TUI summaries for shell-based git pushes so remote boilerplate no longer leaves command rows looking empty when a branch is pushed or tracking is set up.
- Propagated `--dangerously-bypass-permissions` into delegate and cleave clove Omegon processes so higher-order workers inherit parent permission bypass authority.
- Updated the Claude Code OAuth user-agent to match upstream `@anthropic-ai/claude-code` 2.1.179.
- Expanded upstream provider failure classification for Anthropic, OpenAI/Codex, Gemini, Groq, Mistral, OpenRouter, xAI, and Cerebras degradation signals, with explicit false-positive guards for generic quota, capacity, and not-found prose.
- Clarified that policy prompts are allow-once until durable policy grants exist, and documented default-open unknown-tool behavior plus lexical permission-pattern matching.
- Strengthened Lex Imperialis operator-agency guidance to require interactive background terminal/session handling for OAuth, browser, device-code, approval, and other human-blocking workflows when that tooling is available.
- Improved slim TUI memory tool rows so recall, query, episode, archive-search, and memory mutation operations expose richer structured result summaries.
- Moved delegate child prompt files under `.omegon/delegate-prompts/` and cleave clove prompt files under `.omegon/cleave-prompts/`; child prompt writing now creates nested state directories and rejects absolute or parent-directory escape paths.
- Shared delegate/cleave clove-agent substrate for prompt path routing, runtime profile deserialization, child activity/task parsing, and sandboxed Nex profile resolution.
- Shortened common compound tool names in slim TUI rows, including search, memory, context, wait, and browser tools.
- Routed engine status and operation workbench worker rows through inline row projections instead of ad hoc separator/truncation composition.
- Documented that memory sync and multi-checkout federation workflows are context-gated so one-off and non-Git tasks remain task-local.
- Scoped the first federation/memory-sync implementation pass to a read-only context projection over Git-tracked JSONL fact files before any synchronization or artifact writes.
- Added the renderer-neutral memory/federation status projection that classifies one-off, Git, lifecycle, and federation contexts while treating Git-tracked JSONL facts as the cross-checkout memory authority.
- Routed the startup memory probe through the shared memory/federation projection so splash/bootstrap memory status follows the same Git JSONL authority model.
- Made `delegate` background startup register and detach without awaiting an async boundary before returning the task id.
- Made `cleave_run` default to background execution so child-agent orchestration returns an operation id immediately and leaves the main agent/operator control path free; pass `background: false` for the previous attached wait behavior.
- Routed `/cleave status` display rows through the shared operation projection so cleave command status uses the same canonical child status and failure-kind semantics as Workbench.
- Routed `delegate_status` structured details through the shared operation projection so delegate command/API status output now carries operation kind/id, canonical clove statuses, and projected failure payloads.
- Updated the `omegon-full` substrate profile seed to separate container home from the mounted workspace and align optional auth/config mounts with `/data/home` for orchestrated OCI deployments.
- Mapped typed cleave clove failure causes into the shared operation/workbench projection, including upstream exhaustion, merge conflicts, scope violations, timeouts, validation failures, and legacy upstream-exhausted compatibility.
- Routed delegate/cleave transcript lifecycle milestones through the shared operation projection so Workbench and transcript rendering share operation semantics.
- Replaced emoji-style status icons in Rust runtime/TUI strings with text-oriented Unicode glyphs for consistent terminal alignment.
- Added explicit delegate clove execution-boundary prompt sections so scoped workers see their readable/writable scope, enabled/unavailable tools, sandbox profile, and blocker-reporting guidance before acting.
- Hardened the shared operation failure taxonomy used by delegate/cleave Workbench projections, including distinct wall-clock timeout vs idle-timeout classification and sandbox-policy classification before generic permission failures.
- Routed delegate and cleave Workbench rows through a shared renderer-neutral operation projection with canonical clove statuses and failure summaries.
- Decomposition lifecycle events now carry operation provenance so delegate-originated child work no longer renders or publishes as cleave activity.
- Made bundled prompt templates and dynamic prompt guidance capability-aware so hidden or mode-gated memory/lifecycle tools do not get presented as unconditional agent instructions.
- Tightened the bundled `code-act` skill so batch scripting no longer overrides the canonical read → `edit` → `validate` loop for small source changes.
- Updated bundled `typescript` and `openspec` skills to remove legacy `pi-*` SDK example drift and make lifecycle tool usage capability-aware.
- Reworked the bundled OpenSpec skill so slash commands are documented as optional operator-surface conveniences rather than mandatory agent workflow steps.
- Clarified bundled prompt, Lex, and OpenSpec guidance so `delegate` is the default one-shot subagent path while `cleave_run` is reserved for coordinated multi-child work.
- Make `om` an exact synonym for `omegon`; launcher name no longer implicitly enables slim/explorator mode. Use explicit posture flags or profile defaults for runtime mode selection.
- Update harness prompt/project directives for Workbench plan-state reconciliation, semantic surface discipline, registry-backed command surfaces, and prompt/loop provenance safety.
- Interactive startup no longer silently falls back to `automation_safe_model()` when the selected provider is unavailable. Fallback is now opt-in via profile `fallbackProviders = ["provider-id"]`; otherwise startup enters an explicit disconnected state with remediation guidance. Operators who previously would have received a silent fallback now also get a one-time startup notice with the exact `fallbackProviders = [...]` snippet needed to opt back in.
- Add explicit command registry availability/safety metadata, including CLI/ACP confirmation gates with `--dangerously-bypass-permissions` bypass support.
- Preserve queued prompt dispatch mode at queue time so delayed prompts keep their original queue semantics.
- Rename the TUI Plan Dock into a Workbench surface and surface active cleave/delegate progress there instead of only in optional instruments.
- Render plan status glyphs (`●`/`◐`/`⊘`/`○`) in Workbench plan rows alongside the text labels so progress state is visible at a glance in Slim mode.
- Make delegate runner script fixtures flush to disk before execution so CI timeout tests fail only on runner behavior, not file visibility races.
- Add a local `just upstream-provider-check` gate for cheap provider drift checks without waiting for CI.
- Add a lightweight Anthropic model drift checker that compares public Claude API model IDs against the registry.
- Add Claude Fable 5 and limited-access Claude Mythos 5 to the Anthropic registry, make Fable the highest-tier Anthropic default, and update Claude Code OAuth UA to 2.1.173.
- Derive Anthropic adaptive-thinking support from model registry metadata with a family fallback for unreleased Fable/Mythos/Sonnet/Opus model IDs.
- Recommend Anthropic OAuth relogin on repeated stalled-stream exhaustion when the active credential source is OAuth-only.
- Slim Workbench plan rows now keep active and todo items visible before completed items when compact height forces overflow.
- Disconnected TUI engine footer rows now name the selected provider and exact `/login <provider>` remediation instead of a generic provider warning.
- Slim tool summary rows now render detail affordances as right-aligned inline controls using compact key glyphs such as `⌃O details`.
- TUI engine footer rows now use flex-style spacing so row values align against the right edge while preserving label/value styling.
- TUI conversation rendering now marks the explicitly selected segment, shows queued prompt info below the operator editor, and shows an `Enter: details` hint only for selected segments with detail affordances.
- Keep extension JSON-RPC request IDs monotonic after optional `initialize` timeouts, and update extension test fixtures to echo dynamic request IDs.
- Start decoupling TUI conversation segments by moving role/emphasis/tool visual projection types into a dedicated conversation projection module.
- Add parameterized conversation segment projection structs for user, assistant, tool, system, lifecycle, image, and separator segment types.
- Add Ratatui-facing conversation render projection traits so widget measurement/rendering can target render metadata instead of matching segment internals.
- Add a borrowed semantic projection trait for concrete conversation segments, giving TUI and future ACP adapters a shared client-facing interface boundary.
- Extract high-level TUI surface preset state into a shared surface projection module to seed the same projection-boundary pattern for dashboard, instruments, and footer surfaces.
- Add a dashboard semantic projection boundary for lifecycle, OpenSpec, session, and context dashboard state before Ratatui rendering.
- Add footer/status semantic projection types so slim status and future clients consume provider, context, memory, session, and workspace telemetry through a shared boundary.
- Add an instrument panel semantic projection boundary for inference, tool activity, and worker activity telemetry before Ratatui rendering.
- Add an editor/input semantic projection boundary for prompt text, mode, cursor, visual line count, and inline token state.
- Move tool visual color resolution out of semantic conversation projection into the Ratatui render adapter boundary.
- Add an ACP conversation surface DTO adapter with explicit identity, revision, and redaction policy derived from semantic conversation projections.
- Add an ACP conversation surface stream adapter that assigns stable segment identities and revisions from worker-style conversation events before protocol emission.
- Shadow the live ACP worker event stream through the conversation surface adapter while preserving existing SessionUpdate client behavior.
- Factor ACP shadow surface ingestion helpers so worker-event-to-surface mapping remains isolated from live SessionUpdate emission.
- Add trace-only observability for ACP shadow conversation surface updates without changing client-visible protocol output.
- Add a default-off ACP extension-notification hook for shadow conversation surface updates.
- Advertise ACP conversation surface metadata during initialize and enable surface updates by default for Flynt clients while preserving Zed defaults.
- Add ACP initialize regression coverage for Flynt-enabled and Zed-disabled conversation surface metadata.
- Centralize ACP conversation surface extension metadata constants to keep initialize advertisements and notifications aligned.
- Move conversation semantic projection out of the TUI module into shared surfaces so ACP and TUI consume a sibling contract.
- Clarify shared conversation surface semantics by renaming tool visual classification to semantic tool categories and adding projection boundary tests.
- Move footer/status semantic projection types into shared surfaces while keeping the TUI FooterData adapter in the TUI layer.
- Move dashboard, editor, instrument, and layout semantic projection types into shared surfaces while keeping TUI adapters/rendering in the TUI layer.
- Replace footer context class Debug-string projection with explicit context-class mapping.
- Rename the TUI conversation render adapter color hook to `tool_category_color` to align Ratatui rendering with shared semantic tool categories.
- Centralize focus-mode conversation segment chrome resolution in the Ratatui conversation render adapter.
- Centralize tool-card display name and status chrome resolution in the Ratatui conversation render adapter.
- Extract slim/full TUI surface area allocation into a dedicated layout projection seam.
- Extract active tool stream rendering into a dedicated TUI sub-surface module.
- Extract permission lane rendering and key mapping into a dedicated TUI sub-surface module.
- Extract slim plan snapshot rendering and hint policy into a dedicated TUI sub-surface module.
- Extract extension modal and action prompt overlay rendering into a dedicated TUI sub-surface module.
- Extract focus-mode conversation line projection and rendering into a dedicated TUI sub-surface module.
- Extract conversation tab bar rendering into a dedicated TUI sub-surface module.
- Add a tool-card segment component boundary for conversation segment rendering.
- Move slim tool-card row rendering helpers into the tool-card segment component.
- Move tool-card right-title metadata span construction into the tool-card segment component.
- Move tool-card args and lean-summary section construction into the tool-card segment component.
- Move tool-card live progress section construction into the tool-card segment component.
- Move tool-card edit diff section construction into the tool-card segment component.
- Move tool-card generic result section construction into the tool-card segment component.
- Move full tool-card segment rendering into the tool-card segment component.
- Add an assistant segment component boundary for conversation segment rendering.
- Add an operator prompt segment component boundary for conversation segment rendering.
- Add system and lifecycle segment component boundaries for conversation segment rendering.
- Add image and separator segment component boundaries for conversation segment rendering.
- Move separator segment rendering into its dedicated component module.
- Move lifecycle segment rendering into its dedicated component module.
- Move image placeholder segment rendering into its dedicated component module.
- Move assistant segment rendering into its dedicated component module.
- Add an internal UI runtime action contract and route initial Ratatui prompt, slash command, permission, and operator-wait actions through the semantic action seam.
- Route active-turn Escape/Ctrl+C cancellation through the semantic UI action seam.
- Add internal versioned UI runtime envelopes for semantic surface/action replay boundaries.
- Add UI action outcome replay helpers for deterministic semantic action tests.
- Add a monotonic UI revision counter for runtime/replay fixtures without introducing clock semantics.
- Add a pure Rust UI replay fixture builder that records action outcomes and advances revisions only for accepted actions.
- Route UI preset and individual surface visibility changes through semantic UI actions.
- Add semantic conversation segment selection and detail-open actions for portable conversation affordances.
- Add a first-pass selected segment detail pane using existing Ratatui primitives.
- Add conversation segment capability flags and route selection/detail-open eligibility through them.
- Remove the automatic launch/resume welcome block from the conversation transcript; `/status` and live surfaces now carry startup/status information on demand.
- Keep queued prompts and skill/persona builder starts out of the conversation transcript; they now update runtime state without adding chrome-only system cards.
- Move selector confirmations, mouse-mode toggles, empty catalog notices, and queued voice prompts from conversation system cards to transient UI toasts/logs.
- Add reusable TUI command surface componentry for panel, toast, and modal-style outputs, and route slash-command display responses into a command panel instead of the conversation transcript.
- Route focus-mode and mouse conversation segment selection/detail paths through semantic UI action helpers.
- Move system notification segment rendering into its dedicated component module.
- Move user prompt segment rendering into its dedicated component module.
- Document the TUI surface architecture boundaries across shared semantic surfaces, ACP adapters, layout projection, sub-surfaces, and segment components.
- Remove Google and Bing from zero-key web search: automatic free search now uses only DuckDuckGo, while reliable search remains available through configured Brave, Tavily, Serper, or Firecrawl API keys.
- Interactive startup no longer silently falls back to `automation_safe_model()` when the selected provider is unavailable. Fallback is now explicit and opt-in through profile `fallbackProviders = ["provider-id"]`; otherwise Omegon enters a disconnected state with remediation guidance.
- Import discovered external provider credentials into Omegon auth storage during startup hydration, adopt valid external credentials when internal auth storage is missing or expired, and warn visibly when profile-model credentials are unavailable.
- Keep the selected provider/model distinct from the served bridge/model throughout fallback routing, turn configuration, active-model tracking, footer projection, and session-log emission.
- Rename the TUI Plan Dock to the Workbench and surface active cleave/delegate/lifecycle work there with plan status glyphs (`●`/`◐`/`⊘`/`○`) in Slim mode.
- Slim the TUI engine footer into a compact model/status sidecar and keep lifecycle, engine, inference, tools, and Workbench telemetry in separate rows/surfaces instead of competing for one footer block.
- Replace the old full-dashboard tree renderer with a thin read-only project lifecycle strip above the footer/tooling area.
- Extract TUI conversation, footer, dashboard, editor, instrument, layout, tool-card, segment, permission, plan, extension-overlay, focus-view, and tab-bar rendering behind semantic projection/component boundaries shared with future ACP surfaces.
- Add command registry availability/safety metadata, including CLI/ACP confirmation gates and `--dangerously-bypass-permissions` bypass support.
- Rename context classes from Squad/Maniple/Clan/Legion to Compact/Standard/Extended/Massive while retaining legacy aliases.
- Route design-tree and OpenSpec lifecycle mutations through named lifecycle services for node creation, status, questions, research, decisions, links, implementation notes, branch, metadata, archive, implement scaffolding, query policy, FSM sync, and archive recovery.
- Split `omegon-codescan` language-specific scanner logic into bounded modules, add Java/Kotlin/C# discovery, and attach extraction language/strategy/confidence metadata to code chunks.
- Clarify trunk/release policy: `main` owns nightly/trunk work, `release/X.Y` branches are internal stabilization branches, and hardening fixes merge forward.

### Fixed

- Fixed Linux CI failures in the Tests workflow: pin TUI snapshot runs to the Nerd Font glyph profile, make git-branch tests independent of host default branch names, avoid `/tmp` auto-allow false positives in trusted-directory path tests, serialize ACP `OMEGON_HOME` mutation tests, keep the model selector usable without provider credentials, and make settings-menu assertions robust to temp-path wrapping.
- Fixed the release recipe failing at `git add .omegon/milestones.json`: a blanket `.omegon/` gitignore entry (introduced with the Flynt local-state block) shadowed tracked release state; replaced with granular ignores for runtime/history/audit/test-generated state.
- Fixed `/model list` in the TUI to open the interactive model selector instead of dumping the available-models text readout into the terminal, and closed parent structured menus when launching selector rows so arrow keys move the selector instead of the underlying menu.
- Hardened shared `/extension` and `/runtime` menu actions: refresh/update require confirmed menu activation, and extension search now primes `/extension search ` for an explicit query instead of executing a query-less menu command.
- Kept structured menus open underneath menu-launched command output and rendered those raw slash responses in a returnable, scrollable command panel instead of dumping them into the conversation transcript.
- Routed `/settings` through the shared structured menu substrate and removed the legacy in-app settings screen render/input state from the TUI app.
- Routed `/help` to the shared command inventory menu by default while preserving `/help all` as the full text readout, establishing the first general-purpose slash-command menu affordance beyond skills/settings.
- Routed bare `/model` to a shared model-routing menu with current-model selector entry, grade/provider/policy rows, and row-target action affordances for menu-local navigation.
- Routed bare `/auth` and `/login` to the shared provider credential menu, reusing the provider status rows from `/model providers` while preserving direct login/logout commands.
- Added route-aware selected/serving badges to provider menu rows and included route state, serving model, and route warnings in the `/model` menu summary.
- Hardened provider route menus with selected-provider fallback from configured settings, explicit fallback badges, route warnings in `/auth`, and clearer `/model` configured-model wording.
- Routed bare `/context` to a shared context-control menu with policy selection, status, compact, and fresh-context actions while preserving direct context subcommands.
- Hardened `/secrets set` so direct secret values entered on the slash line are redirected into hidden input instead of being queued as plaintext control requests; recipe forms (`env:`, `cmd:`, `vault:`) still queue normally.
- Tightened canonical `/secrets set` parsing so plaintext values are rejected before they can become generic control requests; only recipe forms canonicalize outside hidden input.
- Added a `/model providers` tab in the shared model menu with credential-probe status and login remediation actions for common model providers.
- Reclassified the consumer ambiguous-reasoning-phase idle bail as a transient stalled stream instead of an unknown (non-transient) error. Previously a wedged reasoning stream that timed out in the ambiguous phase hard-failed the turn with no retry; it now retries like every other stall.
- Gave the producer-side SSE watchdog (`process_sse`, shared by Anthropic/OpenAI/codex) the same single re-arm as the consumer: an active-phase silence downgrades once to the reasoning budget before being treated as a stall, so legitimate inter-item reasoning gaps no longer abort live turns. Producer stalls now classify as stalled streams consistently with the consumer.
- Added completion guards to the Ollama (NDJSON `done`) and Google Antigravity (Gemini `finishReason`) stream parsers so a mid-response connection drop surfaces a transient error instead of replaying truncated content as a completed turn.
- Made the Antigravity/Gemini stream watchdog phase-aware (re-arm to the reasoning budget once) instead of a flat 90s idle, and stopped it from emitting a `Done` after it had already surfaced a stream error.
- Closed a partial-content poisoning gap on the Anthropic and OpenAI streaming paths: when the SSE byte stream ends without a `message_stop`/`finish_reason` terminal event (a mid-response connection drop), the producer now emits a `BridgeDropped` error so the retry loop handles it, instead of silently returning truncated text/tool-calls as a completed turn. Mirrors the existing Codex completion guard.
- Stopped the consumer stream watchdog from raising spurious `Upstream stalled stream — retrying` failures when reasoning providers (notably `openai-codex`) pause between output items without first closing the active text/tool block. The first active-output silence now re-arms once to the generous reasoning budget; a genuinely dead stream still surfaces inside the retry budget.
- Added `/resume <session-id>` and `/sessions resume <session-id>` to switch the live TUI to a saved session by id/prefix.
- Added human-readable session descriptions to saved session metadata and session list/web summaries so resume IDs have operator context.
- Printed both CLI (`omegon --resume <id>`) and in-app (`/resume <id>`) resume commands when sessions are saved on exit.
- Printed the saved session id and exact `omegon --resume <id>` command when an interactive or standalone headless session exits.
- Prevented native and OCI extension stderr logs from leaking into the interactive terminal by piping extension stderr into Omegon tracing instead of inheriting it.
- Surfaced the active profile source in the engine line so project/user/default profile state is visible at a glance; named profiles now render by short name instead of source-only labels.
- Prevented explicit provider-qualified model specs from silently falling back to unrelated automation-safe providers.
- Refreshed the TUI Workbench workspace context from live git state so the displayed branch reflects the current checkout instead of a stale harness snapshot after workspace/branch changes.
- Added compact git state to the TUI Workbench branch field: ahead/behind counts, tracked dirty marker, detached HEAD label, and merge/rebase/cherry-pick state when present.
- Compressed slim activity rows to their rendered content height so completed one-line tool activity no longer leaves blank Workbench rows.
- Made `omegon skills --cwd <path>` honor the requested project directory for project-local skill create/get/list/import/delete operations, so iterative skill work can target another checkout without changing the shell cwd.
- Serialized skills and Claude migration tests that mutate process-global `OMEGON_HOME` or the current working directory, making plain parallel `cargo test -p omegon` pass without relying on `--test-threads=1`.
- Made the loop stream idle watchdog phase-aware for post-text, post-thinking, and post-tool-call inter-item gaps so normal provider decision silence does not trip the tighter active-output stall timeout.
- Improved the slim TUI engine/footer band: active turns no longer replace provider/model, tier, thinking, and context details with the spinner verb; thinking mode now renders as its own shaded engine-ribbon segment, and the spinner/status verb moves to a subtle always-on activity row above the engine line. Updated the stale memory-tool label assertion to match the shared `mem read` conversation-surface identity.
- Fixed the stale-Workbench-plan problem where the agent could repeatedly claim completion while the visible plan still showed active/todo rows. The plan-reconciliation nudge was gated on a one-shot `plan_reconciliation_nudged` latch that fired once per session and never re-armed — so after a single early nudge, every later completion-claim-with-stale-plan went unguarded for the rest of the session. The nudge is now keyed on a fingerprint of the open (Pending/Active) plan items: a changed fingerprint (genuine progress, a replaced plan, or a newly orphaned one) re-arms it, while an identical stale state is bounded to `MAX_PLAN_RECONCILIATION_NUDGES` (3) to avoid livelock with a model that refuses to reconcile.
- Made the **consumer-side** LLM stream watchdog reasoning-aware, fixing the remaining `stream stall exhaustion` failures on `openai-codex:gpt-5.5` and other reasoning models. `consume_llm_stream` previously treated `ThinkingStart` as "content received," dropping its idle budget to the 90s content window — so a silent reasoning gap (reasoning models can stream nothing, not even reasoning-summary deltas, for minutes) tripped the watchdog and, after cumulative retries, exhausted. A new reasoning phase (ThinkingStart..first content/tool) now carries a generous budget (default 300s, override `OMEGON_LLM_REASONING_IDLE_TIMEOUT_SECS`, min 60), cleared the moment real text or a tool call begins. This complements the earlier provider-side `process_sse` fix: both the network reader and the event consumer are now phase-aware.
- Made the SSE stream watchdog phase-aware so extended reasoning is no longer misclassified as a stall: idle budget is now 90s while content/tool tokens actively stream and 300s while the model is reasoning or before the first token. Fixes repeated `stream stall exhaustion` failures on reasoning models (OpenAI gpt-5.x / o-series, Anthropic interleaved thinking) where the flat 90s idle timeout tripped during legitimate silent reasoning. Reasoning budget is overridable via `OMEGON_SSE_REASONING_IDLE_TIMEOUT_SECS` (default 300, min 60); active budget retains `OMEGON_SSE_IDLE_TIMEOUT_SECS` (default 90, min 30).
- Auto-expanded TUI tool cards for direct bang shell commands like `!ls` so operator-requested command output is visible by default.
- Made `memory_query` inventory-only for large stores so mature projects show section counts instead of noisy sample facts.
- Rejected invalid native memory sections, including lifecycle ingestion paths, instead of silently defaulting them to Architecture, and made empty `memory_recall` queries return an explicit error result.
- Clarified extension tool descriptions as extension-owned rather than Omegon core semantics, reducing confusion between Flynt document-memory tools and native `memory_*` runtime memory.
- Tightened native memory tool guidance to discourage over-storing, prefer targeted recall, and reserve broad `memory_query` for inventory/debugging.
- Routed all in-tree bash execution paths, including code-act proxy execution, through workspace-boundary mediation so path permission prompts remain harness-level instead of leaking back as ordinary tool output.
- Kept completed live tool activity visible briefly before clearing the slim activity stream so successful and failed operations do not disappear abruptly.
- Routed TUI prompt visibility through the runtime-start event so submitted and queued operator prompts do not render before the runtime actually starts sending them.
- Cleared stale Workbench active-plan state when typed plan projections report no active lane, while preserving workspace context across plan updates.
- Stopped ordinary startup from creating project-local `ai/` or `.omegon/` memory directories; uninitialized projects now stay clean until `/init` is run.
- Retained bounded multi-tool activity rows with display-ready projection metadata so slim activity can show recent running/completed/cancelled tools without TUI-only conversation lookups.
- Allowed workspace-boundary-safe temp directory paths by default so tools can use `/tmp`-style scratch files without unnecessary permission prompts.
- Clarified permission prompts with typed request metadata so the TUI distinguishes policy-only approvals from persisted directory grants and shows the actual grant path.
- Hardened extension-declared skill paths against absolute/path-traversal escapes and made skill conflict matching case-insensitive.
- Resolved prompt assembly to load one directive per skill name so project-local skills override same-named user skills instead of injecting both.
- Updated the in-code skill schema documentation to show YAML as canonical while documenting TOML as compatibility.
- Matched `/skills get` provenance metadata to the resolved skill path so project-local overrides do not accidentally inherit bundled metadata.
- Kept the Workbench context line reserved above active plan rows so expanded plans render below the Workbench instead of covering it.
- Fixed direct plan item completion so completing the active item advances/reconciles the visible Workbench pointer even when later items were already marked done.
- Made turn cancellation phase-aware: early interrupted submissions are forgotten from canonical replay, while interrupts after assistant/tool output keep the prompt for the next turn; the session bar now exposes interrupt/keep/forget disposition.
- Fixed upstream LLM error recovery so a provider rejection is surfaced in the transcript without leaving the rejected active prompt in canonical replay and poisoning subsequent sends.
- Fixed Codex Responses replay so assistant history uses request-safe `output_text` content blocks instead of invalid `input_text`, while preserving queued operator prompt visibility in the TUI conversation.
- Display queued prompts as normal operator segments when the runtime actually starts them, instead of inserting them into the transcript at queue time.
- Clarified memory tool segment labels and recall summaries so read/write/pin/compact operations are visually distinct and recall counts render as hits instead of total stored facts.
- Split stable Workbench plan/context rendering from dynamic activity rendering so live tools plus cleave/delegate progress use tiered activity height instead of resizing the Workbench.
- Replayed assistant text for the ChatGPT/Codex Responses route with request-safe `input_text` content blocks instead of output-only `output_text` blocks, avoiding upstream `400 Unsupported content type` failures on continued turns.
- Converted the slim turn-state TUI test away from brittle glyph/text assertions to semantic `SlimTurnState` and tool-card state checks.
- Preserve structured Codex SSE error details in operator-facing failures instead of manufacturing bare `unknown error`, including upstream status/code/type context and tracing logs for raw error events.
- Added semantic Workbench projection tests for typed operation worker rows so delegate/cleave coverage can assert structured status/tool/detail mapping before terminal rendering.
- Added semantic Workbench projection tests for typed plan lanes so TUI coverage can assert DTO mapping without brittle terminal text matching.
- Routed lifecycle-aware workbench plan snapshots through the same shared plan surface projection used by `plan list`/`plan show`, removing duplicate registry assembly in main loop surfaces.
- Routed `plan list` and `plan show` text through shared plan renderers so direct command output and tool-result enrichment cannot drift across harness surfaces.
- Always inject active plan context into the model when a session or repo-bound visible plan is live, even before any tool call/current-task/compaction signal exists.
- Added typed PlanUpdated edge coverage for IPC and MQTT compatibility payloads so external legacy snapshots are proven to derive from `omegon_traits::PlanSurfaceProjection`.
- Kept `manage_tools list` synchronized with the EventBus's finalized model-visible tool inventory so it no longer reports `0 total` tools while directly callable tools are available.
- Render git tool rows with a dedicated git category glyph instead of reusing the file-write/edit document glyph.
- Render lean editor engine ribbon tail fields as distinct gradient-style badge segments so grade, thinking, and context keep visible backgrounds and Powerline divider bridges instead of falling back to plain metadata text.
- Routed slim tool segment category glyphs through the locked structured tool identity matrix, including shell-mediated read/search/write classifications, so compact tool rows no longer show stale generic/status symbols for context, codebase, search, and read tools.
- Filled lean editor engine ribbon divider backgrounds so Powerline separators blend with the preceding route/card segments instead of rendering as floating triangles.
- Preserve intentional theme badge backgrounds during final TUI background cleanup so the lean editor provider/model route remains visible as dark-on-accent text instead of being flattened to black-on-black.
- Render the lean editor ribbon in route-first order (`provider/model`, grade, thinking, context) so `openai-codex/gpt-5.5` appears directly before the S-tier badge instead of after optional context fields.
- Keep the lean editor ribbon provider/model route anchored at the right edge after optional context/grade/thinking segments so narrow terminals still show the active provider and model.
- Collapsed the lean editor ribbon context segment to text-only summary so the context meter no longer consumes the remaining title width and hides the active model on narrow terminals.
- Render the lean editor engine ribbon's pinned route as a single provider/model label with an explicit missing-provider fallback, so provider identity cannot disappear silently behind separator math or empty provider state.
- Scoped OAuth expiry toasts to the active provider route so stale non-active provider credentials remain visible in auth status without interrupting healthy conversations.
- Removed duplicate workspace dir/git metadata from the compact TUI session row now that the workbench row owns workspace context.
- Populate the compact TUI workbench row with known workspace context (repo, dir, and git branch) even before a tool, plan, delegate, or cleave run is active.
- Routed command-panel rendering through the shared command modal geometry so slash-command output panels do not keep a separate ad hoc modal size.
- Kept overflowing command selectors scroll-aligned around the cursor after standardizing modal height, so long menus remain navigable instead of rendering only their first page.
- Clarified TUI settings profile-source chrome by rendering `profile: project|user · file: <full path>` instead of formatting the profile file as the project label; noted a future affordance for opening the active profile file.
- Restored the inline conversation-segment copy affordance to the high-contrast `⧉` glyph and aligned its click target with the one-cell label.
- Made inline conversation-segment `⧉` copy affordances use the same selected-segment copy path as keyboard shortcuts, including visible success/failure toasts.
- Added a shared `Segment::human_plaintext_detail()` accessor for full operator/client-readable plaintext segment detail, and routed the selected-segment modal through it instead of copy-policy export.
- Clarified the Rust skill's Cargo test filtering guidance so agents use one positional test filter per invocation or a single broader shared filter.
- Routed verbose informational slash-command output into conversation system segments while keeping usage/error responses in command panels and unknown commands in compact warning toasts.
- Projected semantic segment surface policy through ACP conversation DTOs so external clients can distinguish transcript, card, panel, copy, selection, and detail affordances without TUI-specific heuristics.
- Routed selected/latest conversation copy through semantic segment copy policy so assistant copies exclude reasoning chrome, tool copies prefer result detail, and non-copyable media/chrome segments return no clipboard text.
- Made slim TUI transcript rendering copy-friendlier by letting assistant, operator, and system message bodies avoid full-width trailing surface paint while keeping tool cards structured.
- Reused the profile-aware settings projection for TUI settings modal navigation so tab/row bounds stay aligned with profile-drift rendering.
- Wired TUI settings-modal `s` and `a` shortcuts to the existing `/profile save` and `/profile apply` control paths.
- Added command-palette persistence hints for `/think`, `/context`, `/profile`, and `/settings`, and rendered command metadata in the TUI slash popup.
- Rendered profile source, runtime drift, and `/profile save`/`/profile apply` affordances directly in the TUI settings modal so defaults count as the active profile source.
- Routed the TUI settings screen through the profile-aware settings projection and added `/profile save` variants to command-palette completions so defaults/runtime drift and explicit save actions share the existing profile command path.
- Expanded the profile-defaults/runtime-drift design with concrete settings menu renderer and slash popup/chrome staging guidance, including cached projection refresh requirements and profile drift chrome contracts.
- Added profile-drift metadata to the shared settings surface projection and routed the agent-facing settings overview through profile-aware settings rows.
- Made thinking and requested-context-class commands runtime-only by default; `/profile view` now shows the resulting drift and `/profile save|capture` is the explicit persistence path.
- Added a renderer-neutral profile drift projection and routed `/profile view` through a human-readable drift summary with source metadata, save/apply actions, live runtime details, and saved profile JSON.
- Expanded the profile-defaults/runtime-drift design with user/project source-target semantics, future signed Armory-published portable profile goals, plus a cutover-oriented TDD plan covering profile schema, drift projection, `/profile` view/save/revert, runtime-only `/think`/`/context`, settings rows, slash popup metadata, and chrome cues.
- Documented the profile-defaults/runtime-drift design as a shared projection concern for settings menu rows, slash popup command semantics, chrome drift cues, and `/profile` save/apply affordances.
- Simplified the TUI title context signal to show only the resolved upstream context envelope plus pressure, removing requested-to-actual class deltas from persistent chrome.
- Removed model pricing ownership from routing, model registry metadata, model catalog UI, footer session summaries, and Sentry budgets so Omegon routes by capability/context/token limits rather than hardcoded dollar heuristics.
- Completed provider-route-state-machine endpoint normalization lifecycle reconciliation.
- Wired live route resolution after `/model grade`, `/model provider`, and `/model policy` intent changes while preserving durable model intent.
- Corrected provider-route OpenSpec task state to keep OpenAI-compatible response/error normalization open until implemented.
- Added the first durable model-intent state slice so route snapshots carry requested grade/provider intent separately from the active serving route.
- Added registry validation coverage requiring every OpenAI-compatible endpoint to declare verified profile metadata.
- Wired endpoint-declared auth schemes into provider API-key resolution for registry-only endpoints.
- Migrated the model registry and routing internals from legacy tier fields to provider-neutral grade mappings while retaining external DTO compatibility where required.
- Added endpoint definitions and OpenAI-compatible request shaping hooks to the model registry/provider path.
- Added OpenAI-compatible endpoint profile metadata to the bundled model registry for known upstream endpoints, capturing supported capabilities, unsupported fields, optional headers, and provider quirks.
- Added endpoint definitions to the bundled model registry, including endpoint class/protocol/auth metadata and validation for reserved selector tokens and duplicate endpoint IDs.
- Migrated routing internals and the bundled model registry from legacy tier lookups to grade-based registry helpers while retaining one-release loader tolerance for old registry files.
- Migrated the bundled model registry from legacy tier maps/route fields to grade-based registry data and removed registry tier compatibility shims.
- Updated the routing baseline delta spec from legacy tier aliases to provider-neutral grade and provider-selector scenarios.
- Updated baseline context-class routing scenarios from legacy tier names to grade-policy terminology.
- Updated baseline routing and effort specs from legacy tier commands/tooling to model grades, endpoint selectors, and `set_model_intent`.
- Migrated internal harness status and dispatcher projections from legacy model-tier values to provider-neutral grade values while preserving external snapshot field compatibility.
- Adversarially corrected stale model-routing documentation that still claimed legacy public tiers were stable, and reopened implementation tasks for internal tier/data-model and baseline-spec cleanup.
- Replaced the advertised model-tier control surface with provider-neutral model intent grades, added canonical `/model grade <F|D|C|B|A|S>` parsing, and removed legacy tier slash command dispatch.
- Tightened the model-control redesign plan with adversarial follow-up requirements for Rust-native implementation scope, explicit grade/failover/degradation policies, atomic model-intent tooling, endpoint auth schemes, response/error normalization, and baseline spec cleanup.
- Documented the 0.27.0 model-control redesign plan: provider-neutral F/D/C/B/A/S grades, separate endpoint/provider selection, protocol-profile-driven upstream provider matrix, and removal of legacy model-tier commands/tool semantics.
- Strengthened model-facing Workbench/plan guidance so visible plans are treated as the operator's primary awareness surface and must be updated, completed, skipped, or cleared before final replies.
- Added a runtime final-answer nudge when an assistant turn would end while the visible Workbench plan still has active/todo items.
- Moved TUI prompt-history navigation from `Ctrl+Up`/`Ctrl+Down` to macOS-safe `Alt+Up`/`Alt+Down`, avoiding Mission Control and Stage Manager shortcut collisions.
- Made bare Up editor-navigation-only as well, so macOS/Kitty wheel gestures translated to Up cannot recall prompt history; explicit `Alt+Up`/`Alt+Down` remain the prompt history controls.
- Made bare Down an editor-navigation-only key so Kitty wheel gestures translated to Down cannot advance or clear prompt history; explicit `Alt+Up`/`Alt+Down` remain the prompt history controls.
- Split TUI mouse-wheel scrolling from keyboard history handling so crossterm mouse events cannot mutate the editor buffer or history index.
- Hardened terminal session spawning to prefer an absolute bash path so PTY-backed terminal tests and sessions still start when `PATH` is empty.
- Made the TUI model-grade badge reflect the actual displayed model capability from the registry instead of the current route-intent default, so `openai-codex:gpt-5.5` renders as `‹S›`, and replaced the duplicate editor context rail with a single `ctx:<class>@<capacity> <pct>% <fill> <used>` signal.
- Documented the registry-backed slash-command matrix and added drift coverage for canonical parser commands versus intentional hidden compatibility aliases.
- Gated built-in remote slash execution with command-registry CLI availability metadata so interactive-only commands are rejected consistently.
- Added explicit built-in remote slash policy for confirmation-required commands while preserving remote logout compatibility.
- Kept agent/tool `context_status` calls local to their tool card so they no longer enqueue the full `/context` palette into the transcript after assistant turns.
- Persisted interactive `/think <level>` changes to the project profile and restored explicit thinking preferences after slim/full posture startup defaults.
- Restored the full idle editor top-line engine block so it shows provider, tier, and thinking level beside the short model label.
- Right-aligned slim tool-row detail affordances through the shared semantic inline renderer, including `read`/tool rows with crowded detail text.
- Summarized `/context status` tool rows from the palette summary instead of repeating the `Context` block title at the end of the row.
- Accounted for the skills command matrix by parsing `/skills create --project|--user` and `/skills import [--project|--user] <path>`, then routing both through runtime prompts instead of falling into usage text; import prompts now escape backticks in rendered path text.
- Mapped bare `/context` to the status command so the matrix behavior matches the `/model`, `/skills`, and other view-command families.
- Routed palette-shaped slash-command notifications for context, thinking, skills, and prompt list into the command panel instead of treating them as transcript system messages.
- Routed `/context status` slash output to the command panel instead of appending the palette markdown as a plaintext system block at the end of the transcript.
- Kept expanded `context_status` tool cards compact by suppressing the full palette markdown result body after the summary row.
- Preserved slim compact-row child indentation across wrapped reasoning/tool continuation lines so multi-line details stay visually nested.
- Made `/version` render multiline build details so it opens in the command-output panel instead of disappearing as a short toast, and documented `/q` and `/quit` alongside `/exit` in the slash palette.
- Clamped slim reasoning and memory/tool segment rows to the visible terminal width so long summaries cannot bleed past the right edge.
- Made `/settings` open the persistent settings page directly without also rendering a command-output panel, and let Ctrl+C close that page like Esc.
- Removed mouse copyability from the preferences/settings menu and profile persistence while preserving `/mouse` and Ctrl+Shift+T as transient app-mouse versus terminal-selection controls.
- Removed the obsolete hardcoded auth-based model selector scaffold in favor of the existing catalog-backed model selector path.
- Cleared Clippy warnings that blocked `just lint`, including derivable defaults, single-character string pushes, manual contains checks, collapsible conditionals, bool assert comparisons, and explicit allowances for intentionally broad dispatcher signatures.
- Documented that bash permission mediation is static/advisory for common shell forms and that hard filesystem containment belongs to the sandbox layer.
- Guarded auth.json writes and logout mutations in test builds so credential mutation paths fail fast unless `OMEGON_AUTH_JSON_PATH` points at an explicit fixture, preventing test-suite runs from mutating the operator's real `~/.config/omegon/auth.json`.
- Routed bash workspace-boundary hits through the same typed permission mediation as read/write/edit so approved outside-workspace shell writes retry instead of remaining bash-local blocks.
- Made outside-workspace permission prompts wait for an explicit operator allow/deny decision instead of timing out as a denial after 120 seconds; explicit run cancellation still unblocks the wait as a denial/cancelled decision, and `--dangerously-bypass-permissions` still bypasses the prompt before it is raised.
- Unified provider credential reads through canonical auth.json keys so OpenAI Codex OAuth aliases resolve the persisted `openai-codex` grant instead of forcing reauthentication after rebuilds.
- Isolated the voice bridge extension fixture from ambient Kubernetes substrate environment so full-suite runs do not reject the native test extension.
- Avoid caching Pkl binary availability so env-mutating tests or runtime PATH changes cannot poison agent manifest loading for the rest of the process.
- `omegon --which` now reports checkout `HEAD` freshness for launcher-selected checkout/channel binaries and suggests `cd <root> && just link` when a local build is stale.
- Replaced active ACP binary-linking guidance that recommended direct `~/.local/bin` symlinks with the stable launcher/channel workflow.
- **Startup provider credential race** — interactive launch now performs a refresh/adoption pass before emitting a missing or expired credential warning for the selected provider. This prevents a just-saved `openai-codex` OAuth entry from being reported as absent on the next rebuilt launch.
- **Slim tool row affordance alignment** — completed and live slim tool rows now reuse the shared inline row renderer so `⌃O details` stays right-aligned consistently instead of drifting in the legacy compact path.
- **Selected segment focus rail** — selected conversation segments now use a plain vertical focus rail instead of repeated diamond glyphs that could collide with narrow terminal content.
- **Auth store credential preservation** — provider credential writes now fail before touching disk if an update would remove unrelated provider entries, including existing `openai-codex` OAuth credentials.
- Cleaned `/context` and `/think` command-palette metadata so context actions are not duplicated and thinking suggestions match supported levels.
- Selected conversation segment chrome now renders through an explicit frame component with selection-aware measurement, reserving a gutter instead of overwriting or clipping content.
- Leaving focus mode now clears focus-owned segment detail panes so tool detail panels do not remain pinned in the normal Slim layout.
- Keep capability inventory endpoints available when one installed extension or catalog agent has invalid metadata by skipping the bad entry with a warning instead of failing the entire snapshot.
- Normalize remaining Slim tool-card detail/expand affordances to the inline-row glyph contract (`⌃O details` / `⌃O expand`) instead of legacy left-flow `Ctrl+O` prose.
- Scope ACP plan-task durable binding capability metadata to repo-backed tasks with explicit stable IDs so clients do not over-trust guarded `_tasks/bind` support.
- Prefer fresh persisted/external OAuth credentials over stale OAuth environment variables during synchronous provider resolution so `/login openai-codex` immediately unblocks the selected Codex route.
- Clear the release lint gate by grouping active-tool tail rendering style parameters and keeping the persisted OAuth precedence regression test's process-wide auth environment lock out of async execution.
- Accept-loop the OAuth callback listener for Anthropic, OpenAI/Codex, and Antigravity logins: browser speculative preconnections, favicon requests, and redirects from stale login tabs no longer abort the login after the operator completed authentication in the browser.
- Route interactive fallback LLM calls with the fallback bridge model while preserving the selected profile model, preventing OpenAI/Codex startup fallback from sending `gpt-5.5` to Anthropic.
- Initialize the loop's active-model tracking from the bridge runtime model so TurnEnd events and session-log entries emitted before the first per-turn refresh report the real fallback model instead of the profile model.
- Hide successful `/plan` tool-call cards from the main TUI transcript before they render so Workbench progress appears as one live plan snapshot instead of separate approve/execute/complete audit blocks.
- Keep incomplete Workbench plans visible after an assistant turn completes so operators can inspect and continue active plans between turns.
- Pin the slim plan panel below the composer and above the engine status bar, with a compact height cap so large plans do not crowd typed input.
- Reject staged agent-state telemetry in source-clean checks so live audit logs cannot accidentally leak into source commits.
- Make interactive startup import discovered provider credentials before bridge selection, visibly warn on expired profile-model credentials, and fall back to an authenticated provider before installing the login-only null bridge.
- Include lifecycle-derived workstreams in Plan Dock update payloads from both `/plan` commands and tool-result plan mutations so active plans can be shown alongside paused/waiting/blocked lifecycle work.
- Add `just test-commit` as the focused changed-crate validation gate for local commits while keeping `just test-rust` as the full CI/release workspace gate.
- Keep slim TUI instrument footers to inference and tools only; engine telemetry now stays solely in the slim status sidecar row instead of duplicating as a footer block.
- Render the slim TUI engine sidecar as its own status row below the lifecycle row so lifecycle and engine telemetry no longer compete for one line.
- Import discovered external provider credentials into Omegon auth storage once at startup so OpenAI/Codex OAuth survives rebuilt binary relinks and subsequent session hydration uses internal auth.json.
- Adopt valid external provider credentials during startup env hydration when Omegon auth storage is missing or expired, preventing rebuilt sessions from falling back to login-only mode.
- Prefer refreshable persisted OAuth credentials over hydrated OAuth env vars so stale `CHATGPT_OAUTH_TOKEN` session values cannot shadow `auth.json` refresh for OpenAI/Codex.
- The TUI footer/model card now renders the model actually served by the runtime bridge when startup installed a fallback bridge (selected provider had no credentials), with a warning line naming the unavailable operator-selected model. Previously the footer showed the profile-selected model (e.g. `gpt-5.5`) even while every request was being served by the fallback provider, making it look like the wrong provider was active.
- Treat malformed provider entries in `auth.json` as unreadable credentials instead of missing credentials during startup route probing, so OpenAI/Codex relaunch failures no longer misreport a valid-but-unparseable auth store as needing `/login`.
- Accept-loop OAuth callback listeners for Anthropic, OpenAI/Codex, and Antigravity logins so speculative browser preconnections, favicon requests, and stale login-tab redirects no longer consume the one request the login flow was waiting for.
- Report route credential diagnostics and route interactive fallback calls with the fallback bridge model while preserving the operator-selected profile model.
- Render the actual served runtime bridge model in the TUI footer/model card when fallback routing is active, with a warning naming the unavailable selected model.
- Seed loop active-model tracking from the runtime bridge model so TurnEnd events and session logs report the real served model before the first per-turn refresh.
- Repair provider secret resolution, skip expired OAuth session hydration, reduce startup credential preflight prompts, and prevent rebuilt sessions from falling into login-only mode when valid external credentials exist.
- Recommend Anthropic OAuth relogin after repeated stalled-stream exhaustion when the active credential source is OAuth-only.
- Keep incomplete Workbench plans visible between turns, hide successful `/plan` tool-call audit cards from the main transcript, refresh worker progress per frame, and pin the slim plan panel below the composer with a compact height cap.
- Render the slim active-tool lane through the semantic conversation presentation hierarchy, preserving readable live-log gutters, tabular spacing, ANSI-sanitized output, and structured content labels.
- Reject staged agent-state telemetry in source-clean checks so live `.omegon/` audit logs cannot leak into source commits.
- Make delegate runner script fixtures flush before execution so timeout tests fail only on runner behavior, not file visibility races.
- Create cleave git worktree branches explicitly before adding libgit2 worktrees, and report failed cloves with no salvaged changes as skipped instead of successful no-op merges.
- Keep extension JSON-RPC request IDs monotonic after optional `initialize` timeouts, recover auto-disabled extensions, keep stdio logs out of transcripts, and retain extension setup events before prompts.
- Preserve long assistant response tails, clear stale slim plan lanes, prioritize blocking prompt input, and keep interrupt/tutorial feedback out of the transcript.
- Guard codescan's HEAD fast path against relevant dirty working-tree changes so local edits do not return stale cached chunks.

## [0.26.16] - 2026-06-07

### Added

- Add ACP lifecycle task binding surfaces for Flynt and future ACP clients, including stable task identities, task revisions, source metadata, repo/session binding durability, task identity findings, filtered task listings, and session external-task imports.
- Add design nodes for the ACP plan/task durability, stable identity, binding store, mutation, revision/event, status/error/pagination, external work surface, external task promotion, and ecosystem capability contracts needed before clients can treat task links as authoritative.

## [0.26.15] - 2026-06-07

### Fixed

- Clear stale Slim pinned plan lanes when assistant turns complete or cancel without a final plan-complete update, preventing old plan progress from lingering under a `turn done` status.

## [0.26.14] - 2026-06-06

### Fixed

- Preserve the tail of long assistant responses in the TUI by removing the legacy 400-row assistant measurement cap and hardening conversation scroll height arithmetic against overflow.

## [0.26.13] - 2026-06-06

### Fixed

- Remove the remaining race in ACP extension setup capture by assigning the original pre-spawn worker event receiver to the persistent lifecycle subscriber instead of using `resubscribe()` after the worker may have emitted setup events.

## [0.26.12] - 2026-06-06

### Fixed

- Preserve worker-emitted extension metadata and live RPC handles before the first prompt by starting a persistent ACP worker-event subscriber during worker creation, allowing `_extensions/list` and `_extensions/call` to report/call loaded extensions immediately after `new_session`.

## [0.26.11] - 2026-06-06

### Fixed

- Keep ACP stdio runtime logs file-only by default so startup/runtime tracing and ANSI escapes do not leak into ACP client transcripts or destabilize client request handling.

## [0.26.10] - 2026-06-06

### Fixed

- Make `_extensions/enable` fully recover auto-disabled extensions by clearing stale crash, health-check, last-error, and auto-disable state; expose full extension stability diagnostics from `_extensions/list` so ACP clients can render recovery UI without direct state-file edits.

## [0.26.9] - 2026-06-06

### Added

- Complete ACP issue #132 P0 runtime observability with `_runtime/status`, `_provider/status`, truthful `_runtime/capabilities`, extension load/callability diagnostics, and generic `_extensions/call` for live extension RPC control-plane access.
## [0.26.8] - 2026-06-06

### Added

- Add ACP-facing secret capability discovery and non-resolving recipe descriptors so settings panels can guide operators through keyring/Vault/env/cmd/file-backed secret setup without exposing resolved values or executing recipes during list/status rendering; setting recipes now records metadata without resolving side-effectful `cmd:`/`file:`/Vault recipes.

### Fixed

- Make harness secret storage idempotent across recipes, keyring values, session caches, redaction, and process-env projection so named secret repairs can recover orphaned keychain entries without scanning the whole keychain.
- Strip TOML frontmatter from design lifecycle documents and generated site legal/changelog pages so publication metadata does not leak into rendered content or duplicate design-node titles.

## [0.26.7] - 2026-06-05

### Added

- Add structured context compaction diagnostics across manual, forced-loop, auto, bus-requested, and context-overflow compaction paths, including WebSocket `context.compaction` events and tests for manual no-payload/success outcomes.

## [0.26.6] - 2026-06-05

### Fixed

- Make `_packages/install` idempotent for already-installed plugin-backed packages by returning structured `already_installed` package metadata and explicit `version_check` information instead of a bare error envelope.

## [0.26.5] - 2026-06-05

### Fixed

- Add structured ACP turn-control telemetry for Flynt: validate standard `session/cancel` by active session, return `StopReason::Cancelled` for cancelled prompts, emit `_turn/cancelled`, and move provider retry/failure telemetry to `_provider/retry` and `_provider/failure` extension notifications instead of assistant-authored text.

## [0.26.4] - 2026-06-04

### Added

- Add an ACP package substrate (`packages/plan`, `packages/install`, `packages/search`, `packages/list`, `packages/remove`, and `packages/update`) that models installs as packages materializing typed contributions, with Armory, legacy plugin, skill, extension, catalog, and local package stores becoming adapters behind the package facade; package removal/update IDs are validated against path traversal.

### Changed

- Enforce extension SDK contract compatibility during extension startup, accepting the current `0.25` contract, warning for legacy/missing or `0.24` compatibility-window metadata, rejecting malformed/unsupported/newer contracts, and surfacing SDK compatibility diagnostics through extension metadata.
- Add `resource.open@1` HostAction support to the extension host pipeline, including SDK-backed manifest policy consumption, secure `${workspace}` file-root enforcement, backend registry scaffolding, deterministic unavailable-backend fallback, and fake-backend routing coverage for Flynt, Zed, terminal, and fallback resource classes.
- Document the #125 `resource.open@1` real-backend implementation decisions for Flynt, Zed, terminal/Bookokrat ownership, availability diagnostics, file URI parsing, and workspace-root handling.
- Report selected/preferred `resource.open@1` backend diagnostics when a backend is unavailable or fails after policy validation.
- Route ebook/pdf `resource.open@1` requests through the terminal/Bookokrat backend when the real terminal executor is configured, with Flynt/Zed retaining explicit unavailable diagnostics.
- Parse `resource.open@1` `file://` URIs with `url::Url` before workspace-root checks, including encoded path handling and non-local host rejection.
- Scope #125 to terminal/Bookokrat real backend completion and split real Flynt/Zed backends into follow-up design nodes.

## [0.26.3] - 2026-06-04

### Fixed

- Route raw ACP extension-method requests with leading `_` through `ext_method` so Flynt can call Armory, extension, skills, personas, catalog, and future custom methods over the ACP wire format.

## [0.26.2] - 2026-06-04

### Fixed

- Shape ACP `armory/search` results and `armory/install` responses for Flynt's Armory UI while preserving the existing Armory service-layer delegation.

## [0.26.1] - 2026-06-04

### Fixed

- Expose Flynt deployment metadata through ACP initialize and session metadata using both the canonical `omegon/extensions.flynt` namespace and the Flynt compatibility alias so Flynt preflight can verify deployment status.

## [0.26.0] - 2026-06-01

### Added

- Add the `nex_substrate` read-only tool boundary for consuming Nex project substrate reports and deriving Omegon policy findings without duplicating devenv or SecretSpec parsing while preserving Omegon's single-binary default operation when Nex is absent.
- Extract `nex_substrate` into a dedicated provider, catalog `omegon-nex` read-only delegation metadata for `devenv.inspect`, `devenv.explain`, and `machine-profile.inspect`, validate the advertised schema/provider/tool/output/argv contract before accepting it, prefer the narrow provider-routed `devenv.inspect` executor when available, enforce workspace-boundary path checks before inspection, and test that missing Nex remains an advisory degraded report rather than a startup/default-operation dependency while keeping direct Nex binary inspection as an explicit fallback.
- Add a native `omegon-tdd-savepoint` extension scaffold exposing TDD savepoint presets, plan/run operations, diff hashing, readiness status, evidence classification tools, and `.omegon/evidence/` normalized record projection while preserving the existing core CLI kernel during extraction.
- Document the next TDD savepoint extraction stage: argv-only extension execution, deferred watch mode, plan/run ownership, and the core CLI compatibility-wrapper migration.
- Define `.omegon/evidence/` manifest, evidence, surface, edge, and artifact schemas as the project-wide generated evidence-map substrate for Flynt and other consumers, including the core/OpenSpec boundary for provider-neutral claim consumption.
- Add and harden a dogfood Rust surface evidence generator that uses nightly rustdoc JSON to populate `.omegon/evidence/surfaces.jsonl`, resolve source paths/hashes, filter derive noise, render basic signatures, append `code-evidence` records, emit source/artifact edges, report Rust doc coverage warnings, build a derived SQLite/FTS query index, connect documentation coverage evidence to an explicit claim record, generate a markdown coverage summary artifact, support bounded provider-record replacement, and provide a lightweight evidence query helper.
- Define the Omegon Agentic Development Kit extension suite and choose `omegon-code-evidence` as the less ambiguous home for code/API/tool/config surface evidence generation.
- Add rustdoc comments to the public `omegon-tdd-savepoint` evidence kernel surfaces so the dogfood `public-api-documented` claim is supported by regenerated code evidence.
- Add a provider-neutral core evidence read model that loads `.omegon/evidence` claims, records, and edges, summarizes claim support/refutation, annotates OpenSpec scenarios with `evidence-claim` support metadata, and reports refuted/mixed scenario evidence claims for future project-rules enforcement without hard-blocking core archive operations.
- Harden the Project Rules policy boundary and add a warn-mode `omegon project-rules check` dogfood command/config so OpenSpec remains descriptive, evidence findings stay advisory by default, and per-project/CI contexts own hard-deny behavior.
- Document the experimental evidence map, OpenSpec evidence-claim annotations, dogfood code-evidence generator, query helper, and deterministic project-rules checks.
- Add a local sandbox evidence-substrate smoke suite that copies evidence/project-rule inputs into temporary directories, verifies query helpers, checks unsupported/refuted claim behavior, proves auth-free/Nex-degraded operation, and dry-runs the Rust evidence generator in a detached worktree.
- Route `omegon tdd evidence` through the native savepoint extension first with a legacy core fallback during the extraction window.
- Add scenario projection design and implementation plan covering external project tracking, test-condition modeling, provider-neutral evidence, readiness aggregation, and opt-in archive gates.
- Add an initial deterministic language-agnostic `omegon tdd watch` savepoint kernel that hashes command identity, captures failing-to-passing transitions, records git/worktree evidence, supports command timeouts and explicit baseline event emission, projects stable attributed summaries into OpenSpec change evidence directories, reads raw/projected JSONL evidence, classifies TDD evidence status, annotates parsed OpenSpec scenarios with TDD evidence, supports explicit failing-run persistence, explicit scenario IDs with stable derived fallback, automatic current diff hashing for evidence queries, and appends raw JSONL lifecycle events under `.omegon/lifecycle/savepoints/`.

### Fixed

- Ensure the Omegon build script watches the workspace manifest and the correct workspace `.git` paths so release binaries refresh embedded version and git metadata when the workspace version or HEAD changes.

## [0.25.6] - 2026-05-31

### Changed

- Upgrade ACP dependencies to the 0.12 SDK and matching schema updates while preserving existing stdio/WebSocket behavior.
- Preserve Omegon's local single-thread ACP/runtime ownership through a local JSON-RPC adapter rather than forcing ACP handlers into shared `Send` runtime state.

### Fixed

- Harden ACP host request handling so local adapter requests send JSON-RPC error responses instead of terminating the receive loop before replying.
- Prevent ACP host proxy requests from holding the shared client connection borrow across awaited host RPCs.
- Document the `self_cell` Apache-2.0 license selection so stable releases pass third-party license audit.

### Added

- Add a release-gap check that fails preflight when an upstream stable tag exists without a matching GitHub Release.

## [0.25.5] - 2026-05-30

### Changed

- Refresh compatible Rust dependency lockfile entries, including `omegon-extension` 0.25.1 and the Ratatui 0.31-compatible stack.
- Update first-party GitHub Actions workflow dependencies to Node 24-backed major releases (`checkout@v6`, `setup-node@v6`, `upload-artifact@v6`, `download-artifact@v6`) and keep the daemon smoke job timeout aligned with cold build time.
- Split Rust CI into focused build, unit, integration, standalone-crate, extension-install, daemon-smoke, and benchmark-validation jobs so hangs identify the failing bucket instead of cancelling the entire test sequence.

### Fixed

- Accept explicit `background_session` terminal placement from `omegon-extension` 0.25.1 host action requests.
- Ensure spawned extension subprocesses are killed when host-side process handles are dropped, preventing extension RPC tests from keeping `cargo test -p omegon` alive indefinitely.

## [0.25.4] - 2026-05-29

### Added

- Add `package.install@1` HostAction support for trusted `omegon-nex` helper-tool installs through host-managed terminal execution.
- Surface extension initialize metadata through ACP initialize and session info updates for clients such as Flynt.

## [0.25.3] - 2026-05-29

### Added

- Add `nex_capability` as a read-only agent tool for checking host availability and resolving Nex overlay/extension recommendations.

### Changed

- Canonicalize fresh conversation starts under `/context reset`, with `/context clear` and `/new` routed as aliases to the same context reset path.
- Documented the canonical domain-scoped slash command map for consolidating top-level aliases such as `/new`, `/login`, and `/bench` under domain parents like `/context`, `/auth`, and `/stats`.

## [0.25.2] - 2026-05-29

### Fixed

- Route `/auth login <provider>` and `/auth logout <provider>` through the interactive auth command handlers so documented 401 recovery hints work from the canonical `/auth` surface.

## [0.25.0] - 2026-05-28

### Added

- Extracted the Rust extension SDK into the standalone `omegon-extension` crate and updated the host to consume the published SDK instead of the internal workspace crate.
- Added 0.25 extension SDK planning docs covering the contract artifact, host compatibility policy, cross-language lockstep, conformance suite, and standalone repository extraction.

### Changed

- Removed stale host CI/release-plz references to the former internal `omegon-extension` workspace crate; SDK protocol smoke tests and SDK publishing now belong to the standalone repository.
- Updated first-party extension integration docs and the browser extension consumer path for the published SDK crate.
- Keep default or omitted `terminal.create@1` placement mapped to the host background-session backend while the public 0.25 SDK request contract remains `default`, `side_pane`, `bottom_pane`, and `new_tab`.

## [0.25.1] - 2026-05-28

### Changed

- Remove the redundant `standard` UI preset and obsolete `/ui` aliases, leaving `lean` and `full` as the named layout presets.
- Consolidate visible slash commands under canonical parents: `/auth login|logout`, `/stats bench`, `/ui detail`, `/help tutorial`, and transitional `/notes`.
- Add an internal slash/CLI command map documenting canonical surfaces, hidden deletion debt, and the planned scratchpad extension extraction path.

## [0.24.11] - 2026-05-28

### Changed

- Add Claude Opus 4.8 to the model registry, mark Claude Opus 4.7 as not recommended, and refresh the Claude Code OAuth user-agent to 2.1.154.

## [0.24.10] - 2026-05-28

### Fixed

- Keep the Claude Code OAuth user-agent version in sync with upstream `@anthropic-ai/claude-code` 2.1.153, and let the scheduled upstream-version workflow create update PRs instead of failing as soon as drift is detected.
- Keep Slim footer waiting-state labels width-stable so provider/stream transitions do not churn the rest of the footer.
- Keep the command palette and editor cursor visible during active turns unless editor input is explicitly suppressed, so queued steering commands retain autocomplete feedback.
- Strip terminal control sequences from tool summaries and unparsable live tails so noisy command output cannot leak ANSI controls into the TUI.

## [0.24.9] - 2026-05-28

### Changed

- Complete lifecycle closeout for extension push notification routing, terminal backend registry, and voice MVP integration test OpenSpec tracks before opening the 0.25.x line.

## [0.24.8] - 2026-05-28

### Fixed

- Show elapsed wall-clock time in the Slim active-tool stream header while tools are running.
- Replace unclear Slim plan footer actions with plan pin/tracking/OpenSpec/design-lifecycle status labels.
- Add explicit Slim footer labels for directory and git branch fields, and render OODA phase as a dim acronym with the active letter highlighted.
- Dispatch filesystem read/view tools serially so multiple outside-workspace permission prompts cannot overwrite each other and fail before the operator responds.
- Disambiguate Slim turn status while waiting on provider request setup, stream opening, answer/thinking streaming, tools, and upstream retry backoff.
- Keep read-only OpenSpec status and lifecycle snapshot calls from materializing discovered file-backed changes into `ai/lifecycle/state.json`.

## [0.24.7] - 2026-05-27

### Changed

- Preserve voice transcription control metadata (`radio_cue`, `end_of_turn`, `close_session_requested`, and utterance duration) through host prompt routing so radio cues can drive lifecycle behavior without leaking into prompt text.
- Represent spoken-output lifecycle via status-only `voice/tts_state` notifications, independent from microphone capture state, and deterministically handle `over_and_out` close intent after prompt acceptance; close-session control requires both `radio_cue=over_and_out` and `close_session_requested=true`.

### Fixed

- Stop lifecycle read-model snapshots from rewriting `ai/lifecycle/state.json`, including idempotent OpenSpec progress sync timestamp churn, so dashboard/status reads do not dirty downstream agents' worktrees.

## [0.24.6] - 2026-05-26

### Fixed

- Keep reading voice extension stdout while the TUI is idle so post-turn `voice/transcription` notifications become `🎙` prompts and can start the next agent turn.

## [0.24.5] - 2026-05-26

### Added

- Route process-local voice extension transcriptions into the interactive TUI prompt flow, queueing them while turns are active and ignoring malformed or non-transcription voice notifications.

### Fixed

- Surface declarative native-extension HostAction candidates to ACP host approval before execution, including auto-eligible `terminal.create@1` actions, so visual hosts can review or reject them before Omegon falls back to local execution.
- Keep the interactive TUI command loop responsive during active turns so operator input, including queued voice prompts, remains live.

## [0.24.4] - 2026-05-26

### Added

- Added explicit MCP HostAction policy parsing and manual `needs_approval` outcomes so configured MCP servers can advertise approved action requests without gaining auto-execution.

### Fixed

- Shorten pre-content provider/SSE idle detection from five minutes to 90 seconds, with environment overrides, so stale provider sessions surface as failures instead of apparent hangs.
- Read Codex CLI JWT `exp` claims and refresh OpenAI OAuth tokens five minutes early so adopted CLI credentials do not wait for stale `last_refresh` timestamps before re-authentication.
- Stop promoting persisted OAuth credentials into the parent process environment, so one Omegon session no longer shadows shared auth.json refreshes with a stale per-process token.
- Skip expired OAuth entries when hydrating provider auth for child/delegate sessions, preventing stale Codex `CHATGPT_OAUTH_TOKEN` inheritance from bypassing auth.json refresh/re-login state.
- Split well-known secrets into static env credentials and refreshable OAuth session tokens, and only auto-hydrate static credentials into the parent process environment.
- Slim plan pinning now trusts only live `PlanUpdated` projections so legacy transcript plan text remains history instead of resurrecting stale active plan lanes.
- Refine the `plan-refinement` OpenSpec plan around plan/task ACP projections, Flynt task-board linkage, and explicit stale/resume semantics.
- Add plan registry core data shapes, stable plan id constructors, external task refs, and repo-bound clear-as-detach semantics.

## [0.24.2] - 2026-05-25

### Added

- Added OpenSpec-owned task checkbox status updates via `openspec_manage`, with strict numeric task-id matching and ambiguity refusal.

## [0.24.1] - 2026-05-25

### Added

- Started the plan-refinement lifecycle: small work plans now have a compatibility wrapper, central mutation action API, session-scoped visible plan projection metadata, read-only `/plan list` surfaces for operators and agents, initial registry projection types, and OpenSpec-owned task checkbox status updates while preserving existing `/plan` snapshot fields.
- Surface voice-capable extension `voice/state` notifications in harness status/footer summaries using extension-reported `state` and `mic_open` only.
- Route `terminal.create@1` execution through a terminal backend registry so visual hosts can satisfy placement requests while portable PTY remains the background fallback.

### Fixed

- Preserve voice transcription `utterance_id` metadata when routing voice-capable extension notifications into daemon prompt events.

## [0.24.0] - 2026-05-25

### Added

- Added `omegon-extension` HostAction SDK types, host action capabilities, typed `ToolResult` actions, `HostProxy::execute_action()`, and `terminal.create@1` protocol structs for extension-side host side-effect declarations.
- Added manifest policy parsing and host-side validation for declarative HostActions, including terminal create permission checks and structured policy outcomes.
- Added native extension HostAction execution through the canonical executor registry, including the `terminal.create@1` backend adapter.
- Preserve HostActions across MCP metadata using `_meta["omegon/hostActions"]` for native extension MCP exposure and MCP-origin tool results.
- Added deny-by-default MCP HostAction metadata handling so MCP-origin actions are preserved, validated, surfaced as outcomes, and never auto-executed without a future explicit policy layer.
- Added an extension `voice` capability flag as the first substrate for push-based local voice notification routing.
- Route voice-capable extension `voice/transcription` notifications into operator-trusted daemon prompt events.
- Added host-side voice MVP integration coverage proving fake voice extensions route through the existing daemon event ingress rather than a parallel prompt stream.

### Fixed

- Keep dynamically registered native extension tools visible in the lazy model tool surface after turn 1 so installed extension tools such as `reader_doctor` and `reader_open` can be discovered during an active session.
- Normalize native extension SDK `get_tools` schemas that use `inputSchema` into Omegon's internal tool definitions so installed extensions advertise their tools instead of silently registering zero tools.
- Harden extension tool-result envelope parsing and HostAction policy outcomes.
- Avoid blocking-runtime panics when native HostAction terminal execution starts a local terminal backend from an interactive turn runtime.
- **Completed plans surface in Slim** — completed plan updates now leave the active pinned plan lane clear while keeping a `plan done · view` affordance visible so the last completed plan can be recalled.
- **Completed plans remain recoverable** — completed work plans are now recorded as bounded session state, survive save/resume, and `/plan view` can show the last completed plan even after the active plan has been cleared.

## [0.23.9] - 2026-05-25

### Fixed

- **Session persistence is atomic and versioned** — session snapshots and metadata now use locked atomic writes, snapshots are saved before listing metadata, and new snapshots include schema/version metadata for safer resume across upgrades.
- **Interactive startup preserves model preferences** — launching the TUI no longer rewrites the selected/profile model to an automation-safe fallback such as Sonnet when the preferred provider is temporarily unavailable; unavailable providers now surface as disconnected instead of resetting the model.
- **Operator-facing URLs render as explicit links** — base prompt behavior now instructs agents to format localhost/server/viewer URLs as Markdown links so validation endpoints are clickable in the TUI.

## [0.23.8] - 2026-05-25

### Added

- **Lifecycle runtime state split plan** — documented the Option C design for separating tracked lifecycle baseline artifacts from ignored mutable runtime state so release branches stop picking up lifecycle session churn.

### Fixed

- **Focus mode opens at the live tail** — entering focus mode now selects the latest readable conversation segment instead of anchoring to an older visible tool card when viewport height caches lag behind streaming output.
- **Completed Slim plans detach into history** — plan completion now clears the pinned plan lane and reattaches the conversation viewport so completed plans scroll with the transcript instead of leaving stale `view detached` / `more below` hints above the composer.

## [0.23.7] - 2026-05-24

### Fixed

- **Terminal PTY tests tolerate CI scheduling latency** — terminal send/read, TTY detection, and name-reuse tests now use deadline-based polling so GitHub-hosted PTY startup and transcript writes do not fail on a one-second race.

## [0.23.6] - 2026-05-24

### Added

- **Repo dirty-tree hygiene report** — `just dirty-report` now classifies working-tree changes into lifecycle state, source, release memory, tooling, and other buckets so runtime lifecycle churn and unrelated formatter drift are easier to split from scoped commits.

### Fixed

- **Slim tool expansion has a recent-card fallback** — Ctrl+O/Tab/a no longer depend solely on a fresh render-height cache; if the viewport cache is stale or no card intersects it, Slim falls back to the most recent tool cards so recent compressed calls can still be expanded on demand.
- **Slim plan and tool pinning stay visually verifiable** — completed plan snapshots now remain in the pinned Slim plan lane until explicitly cleared, pinned tool cards render a visible `pinned` marker, and expanded Slim tool cards render their result details so Ctrl+O has an observable effect.
- **Slim bottom UX starts separating live state from history** — the slim footer now avoids duplicate `plan: next` text when the pinned plan already shows the next item, running tools maintain a display-only active stream above the pinned plan without entering the conversation focus ring, permission prompts use a pinned decision lane with Shift+A required for persistent grants, and constrained layouts compact live tool/plan panels before crowding out the conversation.
- **ACP plan status lint gate** — redundant plan-status formatting branch removed so the release Clippy gate passes without changing user-visible ACP status text.

## [0.23.5] - 2026-05-23

### Fixed

- **OpenSpec archive crash recovery** — archive operations now write a repo-local transaction journal, recover interrupted moves before lifecycle doctor/archive runs, complete content-moved archives by marking lifecycle state archived, clean journals after successful rollback, and report ambiguous archive conflicts without deleting content.

## [0.23.4] - 2026-05-23

### Added

- **Side-process substrate design docs** — documented extension-facing side-process pane APIs, backend capability negotiation, manifest policy, terminal compatibility matrices, and macOS/Linux backend posture for Zellij, Cockpit/par-term, Kitty, and fallback reader workflows.
- **Managed reader workspace research** — captured reader workspace design nodes, Zellij/Cockpit/par-term spike plans, Bookokrat side-pane contracts, and par-term graphics prototype evidence for embedded reader-pane evaluation.
- **Scratch probe cleanup** — moved useful `.tmp` par-term/Cockpit evidence into design nodes and removed stale local probe/build artifacts.
- **Design-node stale-content audit** — started the `docs/design/` cleanup pass with an audit node and disposition headers for the first batch of historical/stale implementation-scope design docs.

### Fixed

- **Zed ACP plan presentation** — plan status updates now render as concise plain text while native ACP plan updates own the checklist, avoiding raw plan receipts and markdown underscore artifacts in Zed.

## [0.23.3] - 2026-05-23

### Fixed

- **Slim completed responses stay at the live tail** — completed assistant turns no longer rewind compact sessions to the start of long responses, and the conversation renderer reserves a one-column edge gutter to avoid right-edge rendering pressure.

## [0.23.2] - 2026-05-22

### Fixed

- **ACP plan updates use Zed native plan UI** — ordinary `plan` tool snapshots now flow through ACP `SessionUpdate::Plan`, not only cleave/decomposition progress, so Zed can track agent work-plan state directly.
- **Slim tool rows no longer fake-link Markdown paths** — bare `.md` paths in expanded tool summaries now render as plain text instead of terminal hyperlinks that show a hand cursor without opening.
- **Image previews use a crisp high-contrast edge** — inline image placeholders now use a slim accent border, clear background fill, and explicit `file://` caption link so rendered images do not blend into surrounding chrome.

## [0.23.1] - 2026-05-20

### Changed

- **Slim status hints are meaning-first** — the footer now distinguishes active, completed, and absent plan state with concise operator hints, and file activity uses semantic `files: … touched/changed/read` labels instead of opaque `r/w` shorthand.
- **Recursive tasking design is unified** — documented Slim plans, IntentDocument work plans, design/OpenSpec tasking, cleave decomposition, and memory-backed supersession as projections of one recursive tasking system with suspend/block/resume/supersede lifecycle semantics.

### Fixed

- **Slim-mode focus navigation targets visible tools** — empty-editor `Tab` enters/cycles visible tool focus, `Shift+Tab` cycles backward, `a` expands visible tool cards, and `Ctrl+O` prefers current running/visible tool cards over stale selections.
- **Session plans are coupled to execution state** — the agent loop now broadcasts structured plan snapshots whenever work-plan state changes, while execution-mode intent injection explicitly instructs agents to call the `plan` tool when active items complete so the pinned operator checklist stays synchronized with real progress.
- **Completed session plans remain visible** — automatic completion now marks the plan `complete` without erasing its item snapshot, while explicit plan clears and newly set plans still replace the displayed checklist.
- **Slim-mode Ctrl+O targets the visible tool card** — detail expansion now uses the actual conversation viewport height so Ctrl+O expands the bottom/visible segment instead of repeatedly expanding the top cached segment.
- **Slim-mode tool errors use compact rows** — failed tool calls now follow the same compact Slim rendering path as successful tools, preserving the red error status without expanding into full bordered cards.
- **ACP Zed compatibility is release-ready** — ACP now treats prompt resources as the canonical external integration surface, including Zed `@file`, `@selection`, and `@directory` mentions, embedded text resources, ecosystem text files, line slicing, bounded directory listings, binary-resource suppression, root containment, and symlink escape rejection.
- **ACP model controls respect configured providers** — the ACP model dropdown now filters registry models by exact provider availability, distinguishes OpenAI API credentials from Codex OAuth, labels stale unavailable current models, and persists model/thinking/posture changes across ACP sessions.
- **ACP host writes are permission-gated and recoverable** — delegated host writes now request ACP permission before mutation, include failed paths in diagnostics, and fall back to local writes only after permission is granted.
- **TUI tool overflow hints are scoped to real expansion targets** — Slim-mode tool overflow rows now advertise `Ctrl+O details` only when the hidden cells include expandable detail content, avoiding stale hints on non-addressable summary overflow.
- **TUI tool interaction stays live while tools run** — global conversation keys such as `Ctrl+O`, `PageUp`, `PageDown`, `Home`, and `End` are handled before input suppression, and `Ctrl+O` targets the latest running tool card when no explicit selection exists.
- **TUI assistant replies no longer clip markdown tails** — assistant response height measurement now over-allocates the temporary render buffer before trimming, preventing narrow fenced-code responses from appearing truncated above the composer.

## [0.23.0] - 2026-05-20

### Added

- **Interactive background terminal tool** — added a first-class PTY-backed `terminal` core tool with `start`, `send`, `read`, `stop`, and `list` actions for session-scoped interactive processes, including transcript files, stdin/exit audit markers, output tails, TUI shutdown cleanup, and the same workspace-boundary permission scan used by `bash`.
- **Slim operator contract** — documented the `om` UX contract for rendering existing harness state through compact tool evidence, pinned plan state, consequence-complete permission prompts, contextual footer hints, and shared ACP/TUI persistence paths without introducing shadow control planes.
- **OCI-safe terminal profile control** — added profile/env controls for the PTY-backed `terminal` tool so hardened k8s/OCI agents can disable it with `terminalTool: false` or `OMEGON_TERMINAL_TOOL=0`, while bootstrap auto-hides the tool when `/dev/pts` or transcript storage is unavailable.

### Fixed

- **Slim-mode long responses are easier to read** — provider stop reasons from OpenAI-compatible and Anthropic streams are now surfaced when output may be incomplete, and Slim mode pins very long completed assistant replies at their beginning instead of leaving operators at the tail.
- **Slim-mode transcript chrome is lower noise** — assistant prose now renders without response headers, completed successful tool cards collapse to one-line timeline markers that still show command/path/output summaries, and active plan progress is pinned above the composer instead of reappearing as scrollback cards.
- **Slim-mode operator contract is visible in the UI** — pinned plan rows now render from structured session plan snapshots with `done`/`active`/`skipped`/`todo` labels and `+N more` overflow, the status line shows contextual plan/copy/transcript/automation hints, and permission prompts use the consequence-complete tool/target/reason/persist/key-map shape.
- **Slim-mode tool rows carry operational evidence** — compact completed tool rows now extract targets from JSON arguments, show shell commands instead of opaque wrapper names, summarize validation scope, and report output line counts plus the first useful result line.
- **Slim-mode dense tool rows split cleanly** — long compact tool rows now break into bounded indented evidence lines with subtle row background separation instead of clipping command/result summaries off the right edge.
- **Slim-mode live tools expand only while active** — running tool rows now show a compact indented live-evidence view under the tool header, then collapse back to a single row once complete so old tool history does not stay visually expanded.
- **Slim-mode reasoning noise is consolidated** — reasoning-only turns now render as a single subtle status row in Slim mode instead of dumping full intermediate thought blocks between every tool row.
- **Slim-mode terminal rows identify their target** — PTY terminal actions now summarize start/send/read/stop/list targets, session ids, bounded read sizes, useful output tails, and transcript paths instead of collapsing to opaque action names.
- **Slim-mode tool expansion is discoverable** — compact tool rows that have captured arguments, results, or live output now advertise `Ctrl+O details`, reusing the existing selected/nearest tool expansion path without adding another operator surface.
- **Slim-mode running tools show live evidence** — in-flight tool cards now collapse to a one-line Slim row with the target command/path, live phase, progress units, elapsed time, idle heartbeat marker, and latest output tail when available.
- **Slim-mode turn completion is explicit** — the status line now carries a turn-state field (`ready`, `thinking`, `responding`, `running <tool>`, `turn done`, `turn continuing`, `turn cancelled`) so operators do not have to infer whether a turn is still active or finished from scrollback shape.
- **Slim-mode footer hints prioritize blocking action** — permission prompts, manual waits, terminal-copy mode, plan controls, and default copy/transcript affordances now share one ordered status-line hint path so the operator sees the most urgent available action first.
- **Permissions cleanup prefers the canonical operator surface** — denial recovery text, preferences output, trait docs, and Slim contract examples now point at `/permissions` and `profile.permissions.trustedDirectories`, with `/trust` presented only as a compatibility alias.
- **Stuck-loop recovery no longer ends the turn before recovery** — repeated-tool escalation now injects corrective guidance and clears the detector window so the model can take the next concrete action, instead of force-breaking into a summary while valid work remains.
- **Codex login and model selection persist across restarts** — successful OpenAI/Codex login now stores the provider default model in the project profile, external Codex CLI auth adoption persists into Omegon auth storage with account identity, and project-root discovery prefers the repo root over nested build manifests so model defaults are not written into split profile files.
- **Nested Omegon state no longer shadows global model defaults** — project-root discovery now treats nested `.omegon/` directories as state rather than hard workspace boundaries inside an existing Git checkout, preventing stale subdirectory profiles from forcing Anthropic/Sonnet over a global OpenAI Codex/GPT selection.
- **Profile capture no longer leaves stale non-default toggles** — provider/login persistence now saves through the active workspace root and clears defaulted profile fields such as update channel, mouse mode, sandbox, and terminal-tool enablement when settings return to defaults.
- **Session plan updates are structured across surfaces** — plan changes now emit a `plan.updated` event for TUI, IPC, MQTT, and WebSocket consumers so operator surfaces no longer need to parse human-readable plan notifications for live state.
- **Slim-mode detached scroll state is visible** — the status line now shows when the conversation viewport is detached from the live tail, making auto-pinned long responses distinguishable from truncated turns.
- **Slim-mode detached pages no longer look truncated** — detached conversation viewports now render an inline `more below · End to tail` marker at the bottom of the transcript pane, so fenced blocks and long answers do not appear to end mid-response without explanation.
- **Slim-mode completed replies no longer reuse stale streaming height** — long completed assistant responses are remeasured before auto-pinning, and detached completed tails refresh their cached height so a finished turn cannot appear clipped mid-answer.
- **Incomplete structured replies continue under automation** — Flow/Autonomous turns now recover from text-only responses that end on open code fences or dangling phase/list structures instead of surfacing them as cleanly done.
- **Validate skips are actionable** — `validate` now returns a structured skipped result with recommended project-specific checks and Armory validator-plugin guidance for unsupported file types instead of failing with only the built-in source-type list.
- **Armory validator metadata is supported** — Armory manifests can declare `[[validators]]` entries that point to plugin tools by file extension, mark those tools as validation-capable, and surface installed validator recommendations from the built-in `validate` tool.
- **Completed plans clear the operator surface** — finishing the final plan item now clears the active plan state and emits a clear snapshot so Slim mode does not leave a stale pinned checklist after plan completion.
- **Clean transcript copy paths are available** — `/copy latest` copies the latest assistant response from semantic segment text, and `/transcript` writes a deduplicated Markdown transcript with a clickable `.md` file link; `/transcript scrollback` keeps the native scrollback export available explicitly.
- **Spinner tests no longer race shared state** — global spinner counter tests now serialize access and assert against the active verb list, preventing parallel test flakes after startup initializes shuffled verbs.
- **Background terminal security posture is tighter** — PTY sessions now reject credential-prompt commands, cap command/input/session/transcript growth, write transcripts with owner-only permissions on Unix, and strip terminal control sequences before output is returned or audited.

## [0.22.4] - 2026-05-18

### Fixed

- **TUI update checks recover after release asset delays** — `/update` and `/update install` now force fresh checks when needed, avoid caching incomplete GitHub release metadata, distinguish published-but-not-yet-downloadable releases, and keep periodic polling aligned with the active update channel.

## [0.22.3] - 2026-05-18

### Added

- **Manual operator wait tool** — added `wait_for_operator` so agents can pause for explicit physical/manual operator action with TUI confirmation, cancellation, live heartbeats, and a bounded safety timeout.

## [0.22.2] - 2026-05-18

### Added

- **ACP workspace mutations** — ACP clients can now call the same workspace lifecycle mutation surface as the TUI, including create, destroy, adopt, release, archive, prune, bind, role, and kind operations through `control/workspace_*` methods.
- **Clickable terminal links** — assistant, operator, system, and tool-card text now render bare `http://`, `https://`, and `file://` URLs as OSC 8 hyperlinks, and file tool summary rows normalize relative paths into clickable `file://` targets.
- **Profile-backed automation policy** — added `/automation` and `/autonomy` controls for choosing `ask`, `guarded`, `flow`, or `autonomous` continuation behavior, persisted through the project profile while keeping permission, security, plan, interrupt, and max-turn gates as hard boundaries.
- **Unified `/permissions` operator surface** — added `/permissions list|add|remove` as the canonical permission-grant control surface, with `/trust` retained as an alias so TUI and ACP/control callers share the same persisted profile permissions path.
- **Native `/plan` session gate** — added a first-class TUI/remote slash surface over the existing session intent work plan with `set`, `approve`, `execute`, `advance`, `skip`, `clear`, and status rendering so high-level plan mode can reuse the current conversation state instead of creating a shadow planning store.
- **Unified profile operator surface** — `/profile`, IPC/web control commands, and ACP control methods can now view, capture, apply, and edit profile defaults for MQTT, extension allow/deny policy, persona, and tone.
- **Unified Armory install surface** — `/armory install`, `armory/install`, extension installs, and named skill installs now route through one Armory installer that materializes extensions, plugins, and skills into the runtime paths Omegon actually loads.
- **Profile-scoped integration defaults** — project/global profiles can now opt into MQTT bridge startup and constrain native extension loading with allow/deny lists instead of letting every installed operator extension load everywhere by default.

### Changed

- **Permissions persistence now has a single profile surface** — path grants are written under `profile.permissions.trustedDirectories`, with legacy `trustedDirectories` still accepted as a read/write migration alias.
- **Release candidates retired again** — release branches now carry stable semver versions directly, `just release` cuts stable tags without opening a follow-on RC line, and install/Homebrew/docs surfaces only advertise stable and nightly channels.

### Fixed

- **Text-only continuation stalls now auto-recover** — when the operator has already said to proceed or requested a concrete action, assistant replies that only ask for confirmation or describe future work now trigger an internal continuation nudge instead of ending the turn and forcing the operator to type "continue" again.
- **Plan tool progress is now visible in the TUI** — model-driven `plan` tool calls now emit an operator-facing checklist snapshot after set/advance/skip/execute/status updates instead of only appearing as an opaque tool card.
- **Plan progress no longer floods the TUI timeline** — repeated plan snapshots now replace the latest plan progress card across intervening tool cards, keeping one live checklist while preserving the tool cards for audit detail.
- **Auspex projected provider auth is honored** — `OMEGON_AUTH_JSON_PATH` now overrides the provider `auth.json` location, provider readers and legacy resolvers share that path, projected credentials are registered for output redaction, and read-only refresh write-back failures report credential-rotation guidance without exposing secret material.
- **TUI operator surfaces are clearer** — permission prompts now show tool/path/key consequences explicitly, queued prompts explain when they will run, `/auth status` reports the active provider auth file source, and `/permissions`/`/automation` status output documents persistence and hard boundaries.
- **Publish links the local binary with an absolute target** — `just publish` no longer creates a broken `~/.local/bin/omegon -> target/release/omegon` symlink when run from the repository root.
- **ACP and TUI always-allow now persist through the same grant path** — `allow_always` decisions route through the internal permission grant tool, so host-panel approvals and terminal approvals update the same project profile permission store.
- **Standard device streams are no longer blocked as outside-workspace paths** — `/dev/null`, standard stdio aliases, and fd aliases for descriptors 0-2 are allowed by the shared workspace boundary instead of triggering permission prompts.
- **Workspace path discovery no longer escapes into ancestor home repos** — Omegon project/runtime state now respects explicit project markers and shell git commands run with a project-root discovery ceiling, preserving legitimate nested repo status while preventing clove workspaces from inheriting unrelated parent repositories.
- **MQTT bridge no longer starts implicitly** — interactive and daemon sessions now leave MQTT disabled unless the profile or environment explicitly enables it, and enabled bridges preflight the broker socket before handing control to the MQTT client event loop.
- **Startup persona and tone now honor profile defaults** — local, ACP, and embedded startup can load persona/tone defaults from the profile instead of requiring ad hoc child environment variables.
- **TUI provider status no longer probes credentials every frame** — OAuth footer state is cached on model changes instead of repeatedly reading external credential files during redraws.
- **Armory installation is reachable and discoverable from the TUI** — command suggestions, slash usage, ACP help, browse output, dispatcher routing, and post-install messages now point operators at `/armory install`, `/skills install <name>`, and `/extension install <name|url|path>` instead of leaving registry installs as a hidden CLI path.
- **Queued TUI prompts no longer interrupt by default** — submitting a follow-up while the agent is active now queues it until the current turn finishes instead of cancelling the active turn under the misleading "queued" banner. Explicit interrupt queue mode still cancels when selected.
- **Web search timeout path no longer burns one timeout per free engine** — automatic web search now tries DuckDuckGo, Bing, and Google through the shared concurrent failover path instead of spending a full sequential timeout on each free engine, and the tool schema exposes a real `timeout` parameter.
- **Validate failures identify rejected paths** — `validate` now reports each unsupported path and each supported source file missing a project validator instead of only saying the supported source types.
- **Non-UTF8 read and shell output errors are actionable** — `read` now reports the path and invalid byte offset for non-UTF8 text files or identifies binary files, and `bash` output capture no longer fails the whole command when stdout/stderr contains invalid UTF-8 bytes.
- **Image tool results survive the full surface stack** — `view`, `read`, render tools, and MCP image outputs now keep structured image payloads in the LLM-facing tool result path, expose local render metadata for the TUI, and report explicit terminal render-path failures instead of silently degrading to metadata-only success.
- **Stale native extensions recover after transport failure** — extension tool calls now drop broken stdin/stdout handles, respawn the extension, rerun the handshake, and retry once when the child process exits or the pipe closes.
- **OpenSpec docs no longer advertise removed `/opsx:*` slash commands** — the site now points operators at the current `openspec_manage` lifecycle tool actions.
- **Non-English TUI output has regression coverage** — added coverage for Cyrillic streaming output so future truncation/rendering changes cannot reintroduce byte-boundary panics.

## [0.22.1] - 2026-05-16

### Fixed

- **Interrupted interactive turns can no longer hold the TUI hostage indefinitely** — local TUI cancellation now gives the active agent loop a bounded grace period to drain and then recovers the operator surface with an explicit warning if a provider/tool future fails to stop.
- **Publish and smoke recipe summaries no longer fail after successful release work** — `just publish` now keeps the docs page count in the parent shell, and `just smoke` sums multi-binary test results before its safety-floor comparison so release verification output stays clean under `set -u`.

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

- **"Always Allow" now persists trusted directories to project settings** — previously session-scoped only, so child/delegate agents spawned as separate processes never inherited approved directories. Skills running in delegates would silently fail on writes to paths outside the workspace (e.g. Obsidian vault) and the agent would declare "done" without completing the step. Now, pressing 'a' on a permission prompt persists the directory so all future cloves inherit it.
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

- **Nex sandbox profiles** — deterministic OCI container isolation for delegate/cleave cloves. `/sandbox on` in the TUI enables containerized execution with read-only rootfs, no network, workspace mounted at `/work`. Profile registry with 7 built-in domain profiles (chat, coding, coding-python, coding-node, coding-rust, infra, full). TOML manifest format for custom profiles. CLI: `omegon nex init|list|inspect|status`. Footer badge shows "sandbox: isolated" when enabled. Graceful fallback to subprocess when no container runtime available.
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
- **Arrow keys scroll conversation** — bare Up/Down arrows now scroll the conversation instead of recalling history. History recall moved to Alt+Up/Down. Welcome messages updated with new keybind hints.
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

- **Cleave/provider routing hardening** — separated OpenAI API routing from Codex OAuth, repaired cross-provider model routing, passed warmed session secrets into cloves, reset internal workspaces more reliably, and simplified child finalization/cleanup.
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
- **Per-child cleave routing** — `CleaveConfig.inventory` and `CloveState.provider_id` enable scope-aware provider assignment. Children with ≤2 files get Leaf tier, 3–5 get Mid, 6+ get Frontier. Falls back to global model if no inventory or route() returns empty.
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
  - Large overlay during AutoPrompt steps covers conversation chaos while the agent works; instrument footer remains visible for telemetry.
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
- **Multi-layer testing directive**: AGENTS.md "Testing Standards" section, cleave clove contract, task file contract, and system prompt guideline all enforce test-writing as a mandatory part of code changes.
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

- **Orphaned subprocess elimination** — Clove processes spawned with `detached: true` now have three layers of cleanup defense: (1) `process.on('exit')` handler that SIGKILLs all tracked children synchronously when the parent exits for any reason, (2) PID file tracking in `$TMPDIR` with startup scan that kills orphans from dead parents, (3) SIGKILL escalation timer no longer `.unref()`'d so it actually fires during shutdown. Previously, if the parent process crashed or was killed, `session_shutdown` never fired and detached children survived indefinitely.
- **Nested cleave prevention** — Cleave extension now exits immediately when `PI_CHILD=1` is set, preventing child processes from registering cleave tools or spawning nested subprocesses. Previously, every cleave clove loaded the full cleave extension, creating a vector for exponential process growth.
- **Lifecycle batch ingest contention** — `ingestLifecycleCandidatesBatch` no longer wraps the full batch in a single transaction, reducing SQLite write-lock hold time and SQLITE_BUSY errors when concurrent processes share the database.

## [0.6.9] - 2026-03-15

### Fixed

- **Cleave subprocess lifecycle** — Clove dispatch and spec-assessment subprocesses now spawn with `detached: true`, are tracked in a shared process registry, and are killed by process group (`-pid`). A `session_shutdown` handler sweeps all tracked processes with SIGTERM→SIGKILL escalation, preventing orphaned `pi` processes from accumulating and causing runaway CPU/thermal issues.

## [0.6.7] - 2026-03-15

### Fixed

- **Memory injection budget discipline** — project-memory now uses a tighter routine-turn budget and only adds structural filler, episodes, and global facts on higher-signal turns, reducing repeated prompt overhead while keeping high-priority working memory first.
- **Node runtime guardrails** — Omegon now declares Node.js 20+ at the root package boundary and fails early during install on unsupported runtimes instead of crashing later on Unicode `/v` regex parsing in bundled pi-tui.
- **Design assessment stability** — `/assess design` no longer depends on a nested subprocess successfully loading a second extension graph to produce a result.
- **Cleave volatile runtime hygiene** — `.pi/runtime/operator-profile.json` is treated as volatile runtime state instead of blocking cleave dirty-tree preflight.

## [0.6.6] - 2026-03-15

### Fixed

- **Internal subprocess boundary hardening** — Clove dispatch, bridged assess subprocesses, and project-memory subprocess fallback now re-enter Omegon explicitly through the canonical Omegon-owned entrypoint instead of depending on PATH resolution of the legacy `pi` alias.
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
- **Clove progress emission** — `emitCleaveChildProgress()` in `extensions/cleave/dispatcher.ts` now updates shared state and emits `DASHBOARD_UPDATE_EVENT` so the terminal title and dashboard footer reflect clove progress in real time.

### Changed

- OpenSpec commands converted from plain `pi.registerCommand()` to bridge-registered with `structuredExecutor` and `interactiveHandler` separation.
- Cleave `/assess` now uses the shared bridge instance instead of creating a local one.
- Operator fallback logic extended with cooldown tracking and alternate candidate resolution for rate-limited providers.

### Fixed

- Terminal tab title now updates dynamically as cleave clove progress changes (was static after initial render).
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
  - Each tier controls: driver model + thinking level, extraction model, compaction routing, cleave clove floor/preferLocal, and review model
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
- Clove local model selection uses `PREFERRED_ORDER_CODE` preference list instead of `models[0]` (non-deterministic). Prefers `qwen2.5-coder:32b` → `qwen3-coder:30b` → `devstral:24b` → ... → `qwen3:4b`.
- `/effort` slash commands (`/opus`, `/sonnet`, `/haiku`) now enforce the effort cap — no silent bypass.
- `AbortSignal.any()` gracefully falls back on Node.js < 20.3 (was a hard crash).
- Duplicate cloud model string extracted to `EFFORT_EXTRACTION_MODELS` constant in project-memory.

### Fixed

- **Cap ceiling bug** — `checkEffortCap` now derives ceiling from `capLevel` via `tierConfig()`, not `effort.driver`. Cap survived tier switches incorrectly before this fix.
- **Tier matrix divergence** — Ruthless (4) and Lethal (5) corrected to `extraction: "local"` and `compaction: "local"` per design matrix (cleave clove implemented them with cloud extraction).
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

- **Scenario-first task generation** — cleave clove tasks are now matched to spec scenarios using 3-tier priority: spec-domain annotations (`<!-- specs: domain -->`) → file scope matching → word-overlap fallback. Prevents cross-cutting spec scenarios (e.g., RBAC enforcement) from falling between children when tasks are split by file layer.
- **Orphan scenario auto-injection** — any spec scenario matching zero children is automatically injected into the closest child with a `⚠️ CROSS-CUTTING` marker for observability.
- **`TaskGroup.specDomains`** — parsed from `<!-- specs: ... -->` HTML comments in tasks.md group headers for deterministic scenario-to-child mapping.
- **`matchScenariosToChildren`** — exported function for pre-computing scenario assignments across all cloves with orphan detection.

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
