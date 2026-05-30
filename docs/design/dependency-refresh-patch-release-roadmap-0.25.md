+++
title = "Dependency Refresh Patch Release Roadmap for 0.25.x"
tags = ["dependencies","release","roadmap"]
+++

# Dependency Refresh Patch Release Roadmap for 0.25.x

+++
id = "dependency-refresh-patch-release-roadmap-0-25"
kind = "design_node"

[data]
title = "Dependency Refresh Patch Release Roadmap for 0.25.x"
status = "decided"
issue_type = "design"
priority = 2
dependencies = []
open_questions = []
+++

## Overview

Plan the remaining dependency modernization work as incremental 0.25.x patch releases instead of one broad upgrade branch. The compatible lockfile refresh and Ratatui stack bump provide a bounded 0.25.5 slice; remaining major-version cliffs need separate validation surfaces and rollback boundaries.

## Current baseline

The `chore/dependency-refresh-0.25.5` branch establishes the near-term baseline:

- `cargo update` refreshed all compatible lockfile entries.
- `ratatui` moved to `0.31.0`.
- `ratatui-image` moved to `11.0.2`.
- `ratatui-textarea` moved to `0.9.1` and required `DataCursor` API adaptation.
- `omegon-extension` moved to `0.25.1` and required explicit `TerminalPlacement::BackgroundSession` handling.
- Main local validation passed for `omegon`, integration tests, `omegon-memory`, and `omegon-secrets`.

The graph is now compatible-fresh, but not major-fresh. `cargo update --dry-run --verbose` still reports major/minor cliffs including ACP, keyring, rusqlite, reqwest, sysinfo, tree-sitter, toml, scraper, git2, jj-lib, and crypto/network transitive families.

## Patch release milestones

### 0.25.5 — compatible refresh and TUI stack

Scope:

- Ship the compatible lockfile refresh.
- Ship Ratatui 0.31-facing updates.
- Ship `omegon-extension` 0.25.1 compatibility.
- Do not include ACP or storage/secrets major upgrades.

Files likely touched:

- `Cargo.lock`
- `core/crates/omegon/Cargo.toml`
- `core/crates/omegon/src/tui/editor.rs`
- `core/crates/omegon/src/extensions/host_actions.rs`
- `CHANGELOG.md`

Validation gate:

- `cargo test -p omegon --bin omegon -- --nocapture`
- `cargo test -p omegon --tests -- --nocapture`
- `cargo test -p omegon-memory --no-default-features`
- `cargo test -p omegon-secrets --no-default-features`
- CI split jobs green.

Exit criteria:

- Lockfile has no compatible updates pending.
- TUI/editor cursor behavior remains covered by tests.
- Extension host-action terminal placement remains backward compatible.

### 0.25.6 — ACP 0.12 compatibility upgrade

Scope:

- Upgrade `agent-client-protocol` from `0.10.4` to `0.12.x`.
- Upgrade `agent-client-protocol-schema` within the matching supported range.
- Preserve existing ACP stdio/WebSocket behavior unless a protocol change forces adaptation.
- Avoid adopting optional new ACP features unless needed for compatibility.

Expected risk:

- High. ACP is an external protocol boundary with clients such as Zed/Flynt and schema compatibility implications.

Likely work areas:

- `core/crates/omegon/src/acp.rs`
- `core/crates/omegon/src/acp_worker.rs`
- `core/crates/omegon/src/web/acp_ws.rs`
- provider/session metadata paths
- ACP tests and client smoke fixtures

Validation gate:

- Existing ACP unit tests.
- WebSocket ACP tests.
- Manual or scripted initialize/session smoke against the supported host client if available.
- Full `cargo test -p omegon --bin omegon` and `cargo test -p omegon --tests`.

Exit criteria:

- ACP compile/API breakages are resolved.
- Existing clients can initialize and exchange session/tool messages.
- Any newly exposed ACP capabilities are explicitly documented as adopted or deferred.

### 0.25.7 — secrets and storage substrate upgrade

Scope:

- Upgrade `keyring` from `3.6.x` to `4.x`.
- Upgrade `rusqlite` from `0.32.x` toward current `0.40.x` if compatible with `libsqlite3-sys` and existing migrations.
- Treat these as one release only if they remain isolated to `omegon-secrets` and `omegon-memory`; split if either expands.

Expected risk:

- Medium-high. Both touch persisted operator data: secrets and memory/storage.

Likely work areas:

- `core/crates/omegon-secrets/`
- `core/crates/omegon-memory/`
- any callers depending on error types or connection behavior
- schema/migration tests

Validation gate:

- `cargo test -p omegon-secrets --no-default-features`
- `cargo test -p omegon-memory --no-default-features`
- full workspace compile/test for callers
- migration/schema contract tests
- manual secret read/write/delete smoke on macOS keychain if possible

Exit criteria:

- Existing secret retrieval semantics are preserved.
- Memory schema tests pass without unexpected migration churn.
- No accidental key namespace or service-name drift.

### 0.25.8 — HTTP/TLS/network convergence

Scope:

- Investigate and, if feasible, converge duplicate `reqwest` versions (`0.12.x` and `0.13.x`).
- Review duplicate `rustls`, `rustls-webpki`, `socket2`, and related network stack versions.
- Upgrade `sigstore` if it helps convergence without broad API churn.

Expected risk:

- Medium. This touches providers, OCI/signature verification, downloads, OAuth, and MCP/network paths.

Likely work areas:

- provider clients
- update/download code
- OCI/sigstore integration
- OAuth/client auth code
- MCP HTTP/SSE transports

Validation gate:

- provider endpoint compatibility tests
- update/download tests
- OCI/sigstore tests, if available
- live upstream smoke remains opt-in, but local protocol tests must pass

Exit criteria:

- Fewer duplicated HTTP/TLS stack versions, or a documented blocker naming the upstream crate preventing convergence.
- No regression in provider error classification or auth refresh behavior.

### 0.25.9 — parser and document processing stack

Scope:

- Upgrade `tree-sitter` from `0.23.x` toward `0.26.x`.
- Upgrade `pulldown-cmark`, `scraper`, and related document parsing crates where feasible.
- Keep UI rendering changes out unless forced by parser APIs.

Expected risk:

- Medium. Code scanning, markdown/document extraction, and web fetch/readability paths can regress subtly.

Likely work areas:

- `omegon-codescan`
- markdown/document rendering/extraction helpers
- web fetch/readability code
- syntax highlighting tests

Validation gate:

- code scan tests
- markdown/rendering tests
- web/document extraction tests
- sample project scan on representative Rust/TypeScript/Python files

Exit criteria:

- Parser upgrades compile and pass existing tests.
- Any changed syntax tree behavior is documented with test fixture updates.

### 0.25.10 — system/git/runtime utility upgrades

Scope:

- Upgrade `sysinfo` from `0.33.x` toward `0.39.x`.
- Upgrade `git2` from `0.20.x` to `0.21.x`.
- Upgrade `jj-lib` if practical.
- Review small utility cliffs such as `toml`, `shlex`, `matchit`, and crypto helper crates.

Expected risk:

- Medium. These touch workspace/process telemetry, git operations, config parsing, and routing.

Likely work areas:

- workspace leases/process status
- git helpers
- config parsing
- web route matching if `matchit` moves through axum constraints

Validation gate:

- git helper tests
- workspace runtime/admission tests
- config parse tests
- daemon smoke

Exit criteria:

- Runtime/system introspection remains stable.
- Git workflows and config parsing preserve existing semantics.

## Dependency hygiene policy

For each patch release:

1. Start from current `main` on a named branch: `chore/deps-0.25.N-<domain>`.
2. Run `cargo update --dry-run --verbose` before changes and record the targeted stale packages in the PR body.
3. Upgrade one domain boundary per branch.
4. Run the narrow crate tests first, then the split CI-equivalent local gates.
5. Update `[Unreleased]` in `CHANGELOG.md` in the same commit.
6. If duplicate families remain, document whether they are accepted debt or blocked by an upstream crate.
7. Do not combine protocol, persistence, and network major upgrades in one PR.

## Open Questions

No open questions for the roadmap itself. Each milestone should spawn its own design or implementation node if API breakage expands beyond the expected file scope.

## Decisions

### Decision: use patch releases as rollback boundaries

Dependency modernization will proceed through separate 0.25.x patch releases rather than one broad branch. Each release owns one risk domain and can be reverted or patched independently.

### Decision: ACP gets its own 0.25.6 release

ACP protocol upgrades are isolated from storage, network, and parser upgrades because ACP is a client-facing protocol boundary.

### Decision: secrets and storage can share 0.25.7 only if isolated

`keyring` and `rusqlite` may share 0.25.7 because both are persistence substrate work, but they must split if either causes broad API or behavior churn.
