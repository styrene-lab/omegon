# LSP workspace root strategy for monorepos — Design Spec (extracted)

> Auto-extracted from docs/lsp-monorepo-workspace-handling.md at decide-time.

## Decisions

### Walk up from repo_path to find highest workspace manifest per language; one server instance per root (decided)

Spawning from the git repo root (repo_path) and walking up to the highest Cargo.toml [workspace] or tsconfig.json gives rust-analyzer and tsserver the full project context. One server instance per resolved workspace root per language, cached for the session lifetime. This handles the Omegon monorepo pattern correctly and matches how human IDE users configure their language servers. Edge cases (multiple unrelated projects in one repo) are deferred to .omegon/lsp.toml explicit configuration.

## Research Summary

### Monorepo workspace root — pragmatic answer

This repo (`omegon`) is itself a Cargo workspace with a `core/Cargo.toml` workspace root. The patterns from Omegon's own structure inform the decision.

**The rust-analyzer workspace model:**
rust-analyzer discovers the workspace root by walking up from the cwd looking for `Cargo.toml` with `[workspace]`. The Omegon case: `core/Cargo.toml` is the workspace root, `core/crates/omegon/Cargo.toml` is a member. rust-analyzer spawned from `core/` understands all crates. Spawned from `core/crates/omego…
