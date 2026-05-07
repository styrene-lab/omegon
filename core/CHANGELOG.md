+++
id = "a9ebd511-f093-420d-a3fc-de2fc314c1a0"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Changelog

All notable changes to Omegon are documented here.

## [0.14.1-rc.15] — 2026-03-22

### Added

- **web**: styled conversation view with role-specific cards
### Fixed

- **build**: stop rerunning build.rs on every git index change
- **lifecycle**: scan docs/ from project root, not cwd
- **tui**: unknown slash commands show error instead of going to agent
- **tui**: global background fill, darker palette, brighter borders
- **web**: CORS origin mismatch blocking WebSocket upgrade
- **web**: WebSocket diagnostic logging and connection status clarity
- replace all raw byte-index string truncation with unicode-truncate
### Miscellaneous

- cleanup pass — dead code, consistency, test coverage
### Performance

- **build**: add dev-release profile for fast iteration builds## [0.14.1-rc.15] — 2026-03-22

### Fixed

- **switch**: x86_64 arch mapping, raw mode guard, dedup cleanup## [0.14.1-rc.14] — 2026-03-21

### Fixed

- address all 16 issues from adversarial assessment## [0.14.1-rc.13] — 2026-03-21

### Added

- **install**: implement versioned installation layout
- **switch**: implement omegon version switcher
- **switch**: version switcher — tfswitch-style binary management
### Cleave

- merge cleave/0-switch-module
- merge cleave/2-install-layout## [0.14.1-rc.12] — 2026-03-21

### Fixed

- **providers**: remove context-1m beta flag, probe /v1/models at startup## [0.14.1-rc.10] — 2026-03-21

### Performance

- **prompt**: cut per-request token overhead by ~38%## [0.14.1-rc.9] — 2026-03-21

### Fixed

- **auth**: re-resolve credentials per request, not once at startup## [0.14.1-rc.8] — 2026-03-21

### Added

- **tools**: default tool profiles — disable 18 rarely-used tools## [0.14.1-rc.7] — 2026-03-21

### Fixed

- **errors**: stop truncating diagnostic detail in error messages## [0.14.1-rc.6] — 2026-03-21

### Fixed

- **loop**: show LLM errors in conversation, not transient toasts## [0.14.1-rc.5] — 2026-03-21

### Fixed

- **providers**: better error diagnostics for OAuth API failures## [0.14.1-rc.4] — 2026-03-21

### Fixed

- **tui**: TUI-safe OAuth login, /login command, conversation background## [0.14.1-rc.2] — 2026-03-21

### Fixed

- **tui**: route tracing to file in default interactive mode
- **tui**: route tracing to file in default interactive mode## [0.14.1-rc.1] — 2026-03-21

### Added

- **auth**: implement unified auth surface with probe infrastructure and AuthFeature
- **cleave**: jj workspaces replace git worktrees
- **git**: add jj-lib integration module
- **git**: wire jj into RepoModel — working copy delegation
- **memory**: schema contract + full TS alignment
- **memory**: Port memory tools to MemoryFeature
- **secrets**: implement vault recipe type for LLM API key resolution
- **tools**: wire all unregistered Rust tools
- monorepo — absorb omegon-core, eliminate submodule (**BREAKING**)
- wire context class taxonomy into Rust settings and Profile
- polish install experience and fix stale version on site
- implement omegon.styrene.dev/docs sub-site
- persona system implementation — Lex Imperialis, ArmoryManifest, plugin spec
- Rust plugin registry + functional plugins (script/HTTP/WASM-backed tools)
- OCI container runner for functional plugins
- MCP transport — first-class Model Context Protocol support via rmcp
- MCP discovery pipeline + OCI container MCP servers + Docker MCP Gateway
- MCP over Styrene mesh — PQC-encrypted remote tool execution via RNS/Yggdrasil
- encrypted secret store — AES-256-GCM + Argon2id passphrase KDF
- memory schema v6 — persona_id, layer, tags columns for persona mind system
- HarnessStatus contract — unified UI surface for TUI, dashboard, and bootstrap
- HarnessStatus wiring — BusEvent, bootstrap panel, assemble() probe
- wire HarnessStatus into footer, WebSocket, and startup
- TUI event handler for HarnessStatusChanged — live footer updates
- /status slash command — re-display bootstrap panel mid-session
- ArmoryFeature — script and OCI tool execution engine
- dynamic context injection for armory plugins
- omegon plugin install/list/remove/update CLI
- /persona and /tone commands + persona loader wiring
- PersonaFeature — expose persona/tone as agent-callable tools
- harness_settings tool — unified agent access to harness configuration
- implement unified auth surface CLI and TUI commands
- unified auth surface — all backends, all entry points
- add harness status section to dashboard
- TUI surface pass — dashboard harness section, context selector, toasts, compaction indicator
- TUI integration tests — 22 scenario tests for commands, selectors, events
- T1 insta snapshot tests — 10 visual regression tests for TUI widgets
- fractal status surface + clean up 4 warnings + close 4 decided nodes
- implement DelegateFeature and TUI integration
- delegate subagent system — on-demand specialist invocation
- knowledge quadrant lifecycle — readiness score, assumption tracking, prompt guidance
- supply chain security — code signing, SBOM, provenance attestation
### Documentation

- fix noted issues from adversarial review round 2
### Fixed

- **memory**: align Rust schema with TS factstore.ts v5
- **tools**: deduplicate tool registrations and add build fingerprint
- address adversarial review of jj integration
- nginx trailing slash 404s + landing page docs links + interactive install
- adversarial review — all critical/warning/security findings resolved
- adversarial review round 2 — all HarnessStatus findings resolved
- adversarial review findings — eliminate unsafe env vars, add missing tests
- remaining adversarial review findings — all 8 issues addressed
- 6 architectural synergy gaps — fractal wired, status propagation, delegate personas
### Miscellaneous

- **cleave**: auto-commit work from child 'mcp-http-transport'
- **cleave**: auto-commit work from child 'context-selector-and-toasts'
- **cleave**: auto-commit work from child 'footer-compaction-indicator'
- **cleave**: checkpoint before cleave
- **cleave**: auto-commit work from child 'fractal-and-status-wiring'
- lifecycle cleanup + jj-lib made optional
- add Justfile — consolidated build, test, and inspect commands
### Cleave

- merge cleave/0-auth-probe-and-feature
- merge cleave/1-vault-recipe-and-secrets
- merge cleave/2-mcp-http-transport
- merge cleave/3-cli-and-tui-commands
- merge cleave/0-dashboard-harness-section
- merge cleave/1-context-selector-and-toasts
- merge cleave/2-footer-compaction-indicator
- merge cleave/0-wire-existing-tools
- merge cleave/1-memory-tools
- merge cleave/1-delegate-feature-and-tui
- merge cleave/0-fractal-and-status-wiring
