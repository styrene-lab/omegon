# Changelog

All notable changes to Omegon are documented here.

## [0.13.0] — 2026-03-19

### Added

- **release**: versioning system — cargo-release, git-cliff, --version flag
- **site**: add BSL 1.1 license link and date range to footer
- **site**: replace text logo with hydra icon SVG
- **site**: Alpharius colors (blue omega, green hydras), scanlines, Tomorrow font
- **site**: larger icon, saturated green, pithier tagline
- TUI interaction test suite + global logging flags + CI/container tests
- Alpharius theme system + tracing fix for interactive mode
- footer summary cards — context gauge, models, memory, system
- command palette, interrupt (Escape/Ctrl+C), double Ctrl+C to quit
- settings system + functional slash commands with subcommands
- interactive selector popup for /model and /think
- OpenAI Codex (ChatGPT Plus/Pro) OAuth login + auth-aware model selector
- project profile — settings persist in .omegon/profile.json
- handle missing SSE events + tool result display in conversation
- /migrate — import settings from 8 CLI agent tools
- rename binary to 'omegon', bare invocation launches interactive
- harden IntentDocument, context decay, ambient capture, and ContextManager
- change + speculate tools — atomic edits and git checkpointing
- enriched prompt, args_summary in decay, proactive memory injection
- auto-batch edit calls — secret atomic rollback for multi-file turns
- surgical context injection — signal-driven guidelines replace prompt dumping
- evidence-grounding directives + lifecycle-aware prompt
- TUI visual system — shared widget primitives + conversation restructure
- pre-populate TUI from startup snapshot — first frame has real data
- TUI overhaul — remove sidebar, fix scrolling, dynamic footer, terminal-style editor
- context window mode — toggle between 200k and 1M for Anthropic
- visual polish — contextual welcome, hint bar, turn separators
- vertical dividers between footer cards
- /detail — toggle compact vs detailed tool cards
- spinner verbs — flavorful action messages during tool execution
- visual weight for tool cards — colored left bar, contrasting backgrounds
- filled backgrounds — footer, editor, hint bar no longer float in void
- glitch-convergence splash screen — CRT phosphor animation
- TUI polish — bordered cards, mouse scroll, cursor fix, tachyonfx effects
- Feature trait + EventBus — foundation for extension migration
- wire EventBus through setup → loop → context pipeline
- Phase 0 complete — bus is the sole tool dispatch + command routing
- first extension migrations — chronos, terminal-title, version-check
- persistent editor history across sessions
- proper bordered tool cards in conversation view
- segment-based conversation widget — architectural rewrite
- syntax highlighting + scroll clipping fix
- markdown table rendering + Alpharius color contrast overhaul
- parameterized theme system — loads from alpharius.json
- expandable tool cards + text selection hint + correct line counts
- lifecycle Feature — design-tree + openspec as native bus Feature with dashboard panel
- cleave Feature — assessment + orchestrator dispatch + dashboard progress
- live dashboard — shared Arc handles for real-time lifecycle + cleave state
- inline image rendering via ratatui-image
- session-log + model-budget Features — Tier 2 extension migration
- embedded web dashboard — axum server + WebSocket agent protocol + /dash open
- type-ahead editing — editor always active during agent work
- enriched TUI dashboard + web graph view
- omegon.styrene.dev install site + hardened install.sh
- plugin system — TOML manifest discovery + HTTP-backed tools
- add omegon-secrets crate — output redaction, tool guards, recipes, audit
- add whoami tool — multi-provider auth status (git/gh/glab/aws/k8s/oci/vault)
- add memory_episodes, memory_compact, memory_search_archive tools
- add manage_tools, switch_to_offline_driver, memory_ingest_lifecycle
### Changed

- **secrets**: upgrade to keyring + secrecy + aho-corasick
- rename LegacyToolFeature → ToolAdapter
### Fixed

- **site**: dark-bg icon SVG, remove baked text, drop CSS filter hack
- **site**: inline SVG icon, remove redundant title text
- **site**: actually inline the recolored SVG into index.html
- **stream**: add idle timeouts to prevent indefinite hangs on stalled LLM streams
- model selector only shows authenticated providers
- address all critical + warning findings from adversarial review
- SQLite contention during cleave — busy_timeout + child read-only mode
- /model and /think no longer send command as user prompt to LLM
- update all model IDs to current versions — purge stale references
- tool cards — strip cwd, compact display, visual grouping
- Ctrl+C/Escape interrupt no longer locks up the TUI
- adversarial review — all 15 issues resolved
- broken indentation from sed replacement in loop.rs
- assessment findings — deduplicate budget, emit ToolStart, clean response path
- terminal-title guards against non-TTY stderr
- scroll inversion, layout reorder, effects timing, paste handling
- tool cards default to Detailed view, not Compact
- panic on Ctrl+R with empty history — index out of bounds
- scroll direction, height calculation, card visual contrast
- proper scroll clipping + syntax highlighting for .mjs
- assessment cleanup — perf, correctness, tests, API hygiene
- web dashboard assessment — all 16 findings resolved
- second assessment — all 15 findings resolved
- polish graph — physics, labels, visual hierarchy
- polish overview cards — typography, alignment, visual consistency
- Ctrl+C and Cmd+Backspace clear web prompt input
- Ctrl+C clears editor when text present (TUI)
- remove emoji from install site — use Unicode symbols from Alpharius palette
- visual polish — table rendering, dashboard hierarchy, palette contrast
- bash tool cards — contextual names, no line numbers on output
- wire thinking to LLM + improve thinking/table rendering
- memory_query output formatting + branding updates## [0.12.0] — 2026-03-18

### Added

- **cleave**: add Rust cleave orchestrator
- **cleave**: support resuming from existing state.json and task files
- **cleave**: NDJSON progress events on stdout for dashboard observability
- **cleave**: guardrail discovery, enriched task files, post-merge guardrails
- **loop**: nudge agent to commit when stopping with uncommitted mutations
- **memory**: define MemoryBackend trait + types + decay math
- **memory**: InMemoryBackend + MemoryProvider with TDD test suite
- **memory**: SqliteBackend — production MemoryBackend with FTS5, WAL, full schema
- **memory**: wire SqliteBackend + MemoryProvider into omegon-agent binary
- **memory**: JSONL import on startup + export on shutdown, fix mind to 'default'
- **prompt**: add testing directive to Rust agent system prompt
- scaffold Rust agent loop and lifecycle engine
- implement LLM bridge + 4 primitive tools
- wire headless agent loop — Phase 0 MVA complete
- agent loop resilience — turn limits, retry, stuck detection
- session HUD, auto-validation, parallel dispatch, model passthrough
- end-to-end validated against real LLM
- system prompt enrichment (AGENTS.md) + session save/resume
- compaction system — token estimation, LLM-driven summarization, IntentDocument injection
- lifecycle crates Phase 1a — read-only design-tree + openspec parsers
- wire lifecycle ContextProvider into agent loop
- session persistence — save/load/list/resume with CLI integration
- Phase 2 MVP — ratatui interactive TUI with editor, conversation view, agent loop integration
- TUI polish — footer, input history, model display, turn/tool counters
- web_search tool — first feature crate migration (Brave/Tavily/Serper)
- local_inference tool — Ollama management (ask/list/manage)
- view tool — file rendering (images, PDFs, documents, code)
- render tools — D2 diagrams + FLUX.1 image generation
- TUI dashboard panel + slash commands + welcome message
- native Anthropic + OpenAI providers — Node.js no longer required
- OAuth login + token refresh — subscription users need zero npm
- CI workflows + install script + version alignment
### Changed

- extract AgentSetup, clippy sweep, structural cleanup
### Fixed

- **cleave**: pass prompt via file, add --prompt-file flag
- **cleave**: use -f flag for git worktree add to handle stale registrations
- **cleave**: make plan rationale optional, allow single-child plans
- **cleave**: detect empty merges and fix remove_worktree branch deletion bug
- **cleave**: auto-commit uncommitted worktree changes after child exits
- **cleave**: exclude .cleave-prompt.md from auto-commit to avoid false positives
- **cleave**: scope-filtered auto-commit, non-empty merge errors
- **memory**: address all review findings (C1-C2, W1-W5, omissions)
- **memory**: address all review findings (C1-C2, W1-W5, N1-N2, omissions)
- **memory**: remove auto-export on shutdown — JSONL is explicit transport only
- Omegon owns the wire format — bridge translates, not passes through
- six issues found during fresh-eyes inspection
- harden all deferred issues — no more 'noted for later'
- three warnings from adversarial assessment
- address all adversarial review findings (C1-C3, W1-W4, N1-N3)
- address all review findings (C1-C2, W1-W3, N1, omissions)
- per-prompt cancellation token in interactive mode
- zero clippy warnings — ChildDispatchConfig struct, collapsed ifs, unused vars
- Anthropic OAuth compat + configurable file logging + e2e verified
### Miscellaneous

- add dispatch logging to orchestrator
- remove unused ChildProgressStatus::Running variant
### Build

- add release profile (LTO, strip, codegen-units=1, panic=abort)
