+++
id = "407c3c50-9350-4476-8084-fdd8f3f639da"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omegon Project Directives

## What this is

Omegon is a Rust-native agent loop and lifecycle engine. You are working on the tool itself — the codebase you're editing is the same tool that's running you. Be precise.

## Architecture

- **Workspace root**: Cargo workspace at repo root. Crates at `core/crates/`: `omegon` (main binary), `omegon-memory`, `omegon-extension`, `omegon-traits`, `omegon-git`, `omegon-secrets`, `omegon-codescan`, `omegon-opsx`
- **Build + install**: `just build && just link` — builds release binary, writes dev aliases for `omegon` + `om`, installs bundled skills
- **Test**: `just test-rust` — 1800+ tests, must all pass before committing
- **Lint**: `just lint` — type check + clippy
- **Single crate**: `just test-crate omegon-memory`
- **Filter**: `just test-filter "vault_sync"`
- **Config schemas**: `pkl/` directory — 10 Pkl schemas validating config surfaces
- **Skills**: `skills/*/SKILL.md` — TOML frontmatter with `name` and `description` required

## Key conventions

- **Conventional commits** — `feat:`, `fix:`, `refactor:`, `chore:`, `docs:`. See `skills/git/SKILL.md`.
- **Direct commits to `main`** for focused changes. Feature branches for multi-session work. Release hardening happens on `release/X.Y` branches once the line is cut; the branch name must match the workspace release line.
- **Read before editing** — `Edit` requires exact text matches. Always read the file first.
- **Run tests after changes** — `cargo test -p omegon` from the repo root. Don't commit with failures.
- **Build and install after changes** — `just link` from the repo root to install over itself.
- **CHANGELOG.md is mandatory release memory** — every commit that changes behavior, docs/site output, tooling, packaging, public API, or operator workflow must update `[Unreleased]` in the same change. Every release/tag must have a complete section for that exact version and must not skip intermediate released versions.

## Provider system

- Providers are in `providers.rs`. Each has a client struct implementing `LlmBridge`.
- Tool schemas are normalized per-provider via `tool_schema.rs` (Full/OpenAI/Gemini dialects).
- OAuth credentials: Anthropic and OpenAI client IDs are public (shipped by their CLIs). Google Gemini CLI credentials are public per Google's installed-app policy.
- The `CLAUDE_CODE_UA` version string must stay current — nightly CI checks via `scripts/check_upstream_versions.py`.

## TUI

- `tui/mod.rs` is the main event loop (~8000 lines). Segments, widgets, footer, instruments are separate modules.
- Default mode is `Slim` — dashboard, instruments, and segment metadata hidden. `/ui full` reveals them.
- Table rendering uses `markdown_display_width` for column measurement (strips bold/code markers before padding).

## Codex integration

- Auto-detected when `.codex/config.toml` exists at project root. Config: `.codex/omegon-integration.toml` or `.omegon/codex.toml`.
- Memory facts materialize to `{vault}/ai/memory/` on session end. Design nodes export to `{vault}/design/`.
- Facts referenced by vault notes get reinforced (decay timer reset) on sync.

## MCP

- MCP servers configured via `.omegon/mcp.toml` or plugin manifests. Resources and prompts discovered at connect time.
- Context injection capped at 10 items per category with TTL=50.

## k8s / containers

- `omegon run task.toml` — bounded headless tasks with structured JSON output. Exit codes: 0=done, 1=error, 2=exhausted, 3=timeout.
- `omegon serve` — long-lived daemon with WebSocket/IPC control plane, health probes at `/api/healthz` and `/api/readyz`.
- Workload matrix: `docs/design/k8s-workload-matrix.md` — tracks implementation status.

## Things to be careful with

- **Never fabricate URLs, client IDs, or API endpoints.** Research real values from provider documentation or source code. The Antigravity provider had fabricated credentials that wasted significant time.
- **`Settings::provider()` returns `String`** (not `&str`). It uses `infer_provider_id` — no hardcoded catch-all.
- **Skill frontmatter is TOML** (`+++` delimiters), not YAML. `extract_description` handles both.
- **Extension `execute_tool` RPC** — extensions must implement this handler or the call returns a graceful error.
- **Memory/lifecycle features** have optional `codex_vault_path` — set via `with_codex_vault()` in `setup.rs`.
