+++
id = "e25d09d6-e95c-42ae-b71d-63ea77ef4d74"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Use cheap GPT models for memory extraction and embeddings

## Disposition — 2026-05-23

**Status: partially superseded / stale implementation scope.** The durable decision that background memory work should prefer cheap reliable cloud paths over heavyweight defaults remains relevant. The implementation notes are stale: the referenced `extensions/project-memory/*` TypeScript files are absent, while current memory and embedding behavior lives in Rust under `core/crates/omegon-memory/` and `core/crates/omegon/src/features/memory.rs`, `embedding.rs`, and optional `local_embedding.rs`.

Use this document for policy direction only. Reconcile extraction/embedding defaults against the Rust memory implementation before treating any file scope or cloud-first embedding detail as current behavior.

## Overview

Track the change that moves project-memory to cheap GPT-class cloud defaults for background extraction and semantic embeddings while preserving graceful degradation.

## Decisions

### Decision: Default project-memory extraction should use a cheap GPT cloud model

**Status:** decided

**Rationale:** Background extraction quality should remain Magos-tier while materially reducing cost relative to Sonnet and removing dependence on a local Ollama chat model.

### Decision: Semantic embeddings should be cloud-first

**Status:** decided

**Rationale:** Semantic recall should not depend on local Ollama availability; a cheap cloud embedding model provides predictable startup behavior and lower operator friction.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/project-memory/types.ts` (modified) — Default extraction and embedding model configuration
- `extensions/project-memory/embeddings.ts` (modified) — Cloud-first embedding backend with OpenAI default and optional Ollama fallback path
- `extensions/project-memory/index.ts` (modified) — Use configured embedding provider/model and env overrides during initialization
- `extensions/project-memory/embeddings.test.ts` (modified) — Coverage for cloud defaults and graceful degradation
- `extensions/project-memory/README.md` (modified) — Document cheap GPT extraction and cloud embedding defaults

### Constraints

- Preserve degraded memory behavior when cloud embeddings are unavailable by falling back to FTS5 search instead of failing startup.
- Keep extraction model overrideable by effort-tier routing and environment configuration.
- Keep Ollama embedding support available when explicitly configured, but not as the default path.
