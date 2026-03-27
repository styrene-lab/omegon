---
id: lsp-monorepo-workspace-handling
title: LSP workspace root strategy for monorepos
status: decided
parent: lsp-integration
open_questions: []
jj_change_id: ykqqlwnsttquosykvkzllqlnqkwqookt
---

# LSP workspace root strategy for monorepos

## Overview

> Parent: [LSP integration — language server protocol for code-aware navigation and diagnostics](lsp-integration.md)
> Spawned from: "How should LSP tools handle multi-root workspaces (monorepos with multiple Cargo.toml files at different depths)?"

*To be explored.*

## Research

### Monorepo workspace root — pragmatic answer

This repo (`omegon`) is itself a Cargo workspace with a `core/Cargo.toml` workspace root. The patterns from Omegon's own structure inform the decision.

**The rust-analyzer workspace model:**
rust-analyzer discovers the workspace root by walking up from the cwd looking for `Cargo.toml` with `[workspace]`. The Omegon case: `core/Cargo.toml` is the workspace root, `core/crates/omegon/Cargo.toml` is a member. rust-analyzer spawned from `core/` understands all crates. Spawned from `core/crates/omegon/` it sees only one crate.

**Practical answer:** Spawn LSP servers rooted at the git root (same as the agent's `repo_path`). This mirrors how cleave children use `repo_path` as cwd. For Cargo workspaces, walk up from the file being queried to find the highest `Cargo.toml` with `[workspace]`, then spawn rust-analyzer there.

**For typescript monorepos:** tsserver uses `tsconfig.json` as root. A repo with multiple packages under `packages/*/tsconfig.json` would ideally use the project root, and tsserver will handle sub-project references via TypeScript project references.

**Decision:** One LSP server instance per workspace root per language, resolved by walking up to the highest project manifest. Store the resolved root path in the server registry. Do not spawn a new server per file or per tool call.

This is a solvable, bounded problem. The edge cases (multiple unrelated projects in one git repo) are rare enough to punt on for MVP; document them in lsp.toml override docs.

## Decisions

### Decision: Walk up from repo_path to find highest workspace manifest per language; one server instance per root

**Status:** decided
**Rationale:** Spawning from the git repo root (repo_path) and walking up to the highest Cargo.toml [workspace] or tsconfig.json gives rust-analyzer and tsserver the full project context. One server instance per resolved workspace root per language, cached for the session lifetime. This handles the Omegon monorepo pattern correctly and matches how human IDE users configure their language servers. Edge cases (multiple unrelated projects in one repo) are deferred to .omegon/lsp.toml explicit configuration.

## Open Questions

*No open questions.*
