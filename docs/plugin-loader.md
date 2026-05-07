+++
id = "e1bc293d-482a-4ed6-81fe-e12a6e3596a1"
kind = "document"
title = "Plugin loader — TOML manifest discovery, HTTP-backed tools and context"
status = "implemented"
tags = ["architecture", "extension", "plugin"]
aliases = ["plugin-loader"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "scribe-omegon-integration"
+++

# Plugin loader — TOML manifest discovery, HTTP-backed tools and context

## Overview

Implement the plugin loader that reads ~/.omegon/plugins/*/plugin.toml manifests, creates ToolAdapter instances backed by HTTP endpoints, injects context from declared endpoints, and forwards agent events. This is the extension API contract for all external integrations.

## Decisions

### Decision: Plugin manifest format: TOML with activation rules, tools, context, events

**Status:** decided
**Rationale:** TOML is human-readable, widely understood, and already a dep (toml crate via Cargo.toml parsing). The manifest declares what the plugin provides (tools, context) and what it consumes (events). Activation is conditional on marker files or env vars — plugins don't load unless relevant to the current project.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `crates/omegon/src/plugins/mod.rs` (new) — Plugin loader — manifest discovery, parsing, activation check
- `crates/omegon/src/plugins/manifest.rs` (new) — PluginManifest struct + TOML deserialization
- `crates/omegon/src/plugins/http_feature.rs` (new) — HttpPluginFeature — Feature impl backed by HTTP endpoints
- `crates/omegon/src/setup.rs` (modified) — Register discovered plugins into EventBus
- `crates/omegon/Cargo.toml` (modified) — Add toml dep

### Constraints

- Plugin activation must be fast — only check marker files and env vars, no HTTP at activation time
- HTTP tool calls must have timeouts (5s default) and graceful degradation
- Plugin context injection follows the same TTL/priority system as native context
- No WASM, no dynamic linking — plugins are declarative manifests, not code
