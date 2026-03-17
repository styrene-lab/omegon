# Changelog

All notable changes to Omegon are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/). Versioning: [Semantic Versioning](https://semver.org/).

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
