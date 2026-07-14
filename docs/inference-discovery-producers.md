---
id: inference-discovery-producers
title: "Discovery-layer producers + model catalog unification"
status: implementing
tags: []
open_questions:
  - "[assumption] The unverified enumeration endpoints (openai, groq, mistral, xai, openrouter, anthropic, google) behave as the training-knowledge matrix claims — each needs a one-time live verification during implementation, recorded per-endpoint like openAiCompatibleProfile.verifiedAt"
  - "Can the openai-codex ChatGPT-backend OAuth token enumerate models at all? Needs a live probe during implementation; if not, openai-codex declares discovery:none like perplexity"
  - "[assumption] Copilot's /models response body carries per-model capabilities/limits/policy metadata worth parsing (probe currently keeps IDs only) — verify shape against the live account and decide how much to map into discovery patches vs defer"
dependencies: []
related: []
---

# Discovery-layer producers + model catalog unification

## Overview

The dynamic-inference-inventory foundation (inference_inventory.rs) implements layered deterministic merge with a Discovery layer (precedence 50), but no producers exist: InventorySource::Discovery is never constructed outside tests, and inference_manifest.rs only loads embedded + TOML file layers. Meanwhile the operator-visible model catalog (tui/model_catalog.rs) bypasses the inventory entirely, reading the static embedded data/model-registry.json (lastReviewed 2026-07-01). Result: GitHub Copilot shows 4 stale models while the live /models endpoint returns 29 (verified by probe on 2026-07-14, including claude-opus-4.8, gemini-3.1-pro-preview, gpt-5.3-codex absent from the registry). This node designs the discovery producers (protocol-generic fetchers, not per-provider), a TTL-cached refresh pipeline feeding InventoryHandle::refresh(), and migration of the model catalog to consume inventory snapshots — eliminating the parallel static/dynamic systems. Curation cost then scales with protocol count (slow-growing) instead of provider×model count. Target: 0.28.2 on release/0.28.

## Research

### Live probe evidence (2026-07-14)

`omegon auth copilot-probe` against a live business-tenant account: token exchange HTTP 200 (expires_at + refresh_in=1500 present — natural TTL key), models endpoint HTTP 200 with **29 model IDs** vs 4 in the embedded registry. New IDs absent from registry: claude-haiku-4.5, claude-opus-4.8, gemini-3.1-pro-preview, gemini-3.5-flash, gpt-5.3-codex, gpt-5.4-mini, plus legacy gpt-4* families, embedding models (text-embedding-3-small, ada-002), and internal models (trajectory-compaction, gpt-41-copilot). Findings: (1) no claude-fable-5 / gpt-5.6 on this tenant — absence is policy/tenant gating, which discovery represents truthfully and a static registry cannot; (2) embedding + internal models in the same listing validate the spec requirement that modality/interface compatibility filtering precedes selection; (3) probe currently parses IDs only (github_copilot.rs probe_models) and discards per-model metadata in the response body.

Provider enumeration matrix (unverified rows are training-knowledge hypotheses): openai/groq/mistral/xai/hf-router/ollama-cloud share OpenAI-compatible GET /v1/models (poor metadata: ids + owned_by; groq adds context_window); openrouter GET /api/v1/models is richest (context, pricing, modalities, supported params); anthropic GET /v1/models (ids + display_name); google GET /v1beta/models (good: token limits, supported methods); github-copilot /models via token exchange (rich: capabilities, limits, policy — verified); ollama local /api/tags (already wired); openai-codex unknown (ChatGPT backend, providers.rs:2917 — needs probe); perplexity believed non-enumerable (docs-only).

## Decisions

### Fetchers are keyed by endpoint protocol, not provider

**Status:** accepted

**Rationale:** The registry endpoints block already classifies most providers as openAiCompatible. One generic /v1/models fetcher covers openai, groq, mistral, xai, hf-router, ollama-cloud; special fetchers for openrouter (rich metadata parser), anthropic, google, and github-copilot (token exchange, already written in github_copilot.rs — re-homed from diagnostics). ~5 implementations cover ~12 providers, so maintenance scales with protocol count (slow) instead of provider count (fast). Providers without enumeration (perplexity, possibly openai-codex) declare discovery: none and fall back to embedded/manifest layers — non-enumerable is a first-class case, not an error.

### Discovery asserts availability only; metadata comes from lower layers or conservative defaults

**Status:** accepted

**Rationale:** Discovery layer patches assert offering existence/absence and whatever metadata the provider actually returns (with EvidenceKind provenance). IDs unknown to the embedded registry become ungraded offerings with conservative defaults (128k in / 16k out, coding capability) — selectable explicitly, excluded from autonomous routing by default per the existing dynamic-inference-inventory spec. No quality grades are synthesized from discovery. This keeps curation cost bounded: availability is free, only metadata/grading remains human-reviewed.

### Background refresh with TTL cache; catalog reads are never network-blocking

**Status:** accepted

**Rationale:** ModelCatalog::discover() is sync and called from TUI paths; discovery fetchers are async network calls. Resolution: discovery runs as an async background refresh (on startup after auth resolution, on explicit /model refresh, and on TTL expiry) that builds a Discovery InventoryLayer and calls InventoryHandle::refresh(). Failed or slow fetches never degrade the catalog — last-known-good snapshot semantics already exist in inference_inventory.rs. Discovered results are persisted to a cache file so a fresh process shows the last-known live inventory immediately. Copilot token exchange refresh_in/expires_at drive that provider's TTL; other providers get a default TTL (proposed: 1h, configurable).

### model_catalog.rs becomes a projection of the inventory snapshot

**Status:** accepted

**Rationale:** The TUI catalog currently reads ModelRegistry::global() directly, creating a parallel static system that ignores the inventory. Migration: cloud provider sections project from the active InventorySnapshot (auth-gating preserved), Ollama keeps its existing local query as a discovery producer. The embedded registry remains the bootstrap layer inside the inventory — no data is deleted, its role changes from sole source to lowest-precedence layer. This kills the System A/B duality in one release rather than letting them drift.

## Open Questions

- [assumption] The unverified enumeration endpoints (openai, groq, mistral, xai, openrouter, anthropic, google) behave as the training-knowledge matrix claims — each needs a one-time live verification during implementation, recorded per-endpoint like openAiCompatibleProfile.verifiedAt
- Can the openai-codex ChatGPT-backend OAuth token enumerate models at all? Needs a live probe during implementation; if not, openai-codex declares discovery:none like perplexity
- [assumption] Copilot's /models response body carries per-model capabilities/limits/policy metadata worth parsing (probe currently keeps IDs only) — verify shape against the live account and decide how much to map into discovery patches vs defer
