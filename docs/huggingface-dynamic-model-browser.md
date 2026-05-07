+++
id = "691d29e2-e293-4d51-b80a-24a4b4905d94"
kind = "document"
title = "HuggingFace dynamic model browser with API caching"
status = "seed"
tags = ["models", "huggingface", "provider", "discovery", "0.16.0"]
aliases = ["huggingface-dynamic-model-browser"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
open_questions = ["Should we require HUGGING_FACE_TOKEN for browsing, or make public models accessible without auth?"]
parent = "model-selector-ux"
related = []
+++

# HuggingFace dynamic model browser with API caching

## Overview

Enable operators to browse and select from HuggingFace's 500k+ model catalog directly in the TUI. Fetch model metadata via HF API, cache results with TTL, support authenticated access via secrets, and provide browser fallback for full catalog experience.

Deferred to 0.16.0 to keep rc.1 scope focused on static catalog + core /model selector UX.

## Research

### HuggingFace API Integration

**HuggingFace API endpoints:**
- `GET https://huggingface.co/api/models` — list all public models (paginated, 1000 per request)
- Query params: `?filter=text-generation&sort=downloads&limit=100` — filter by task, sort, pagination
- Rate limits: Unauthenticated 30/min, authenticated 200/min
- Each model returns: id, name, downloads, likes, created_at, tags, private, gated

**Caching strategy:**
- In-memory LRU cache with TTL (default 1 hour)
- Store: provider name → (models vec, fetched_at, expires_at)
- Refresh on demand if expired or explicitly requested
- Fallback: if HF API down, show cached models or "unable to reach HF, open browser"

**Search/filter approach:**
- Fetch top 200 models (by popularity) on first request
- Client-side filter by task (text-generation, summarization, etc.)
- Search bar in selector widget for name/id match
- "Show more" → fetch next 1000 (paginated load)

**Browser fallback:**
- `/model huggingface` with no network → "Open huggingface.co/models in your browser"
- Maintains UX graceful degradation if API fails

### Implementation Strategy

**Rust implementation approach:**
- Create `HuggingFaceClient` struct with reqwest async HTTP client
- Implement trait `DynamicProviderBrowser` with `async fn fetch_models(filter, limit) -> Result<Vec<ModelInfo>>`
- Cache layer: `ModelCache` with TTL-aware LRU
- Extend `ModelCatalog::refresh_provider(provider: &str)` to handle async refresh
- Error handling: If fetch fails, return cached models or empty list (graceful degrade)

**Integration with /model selector:**
- Detect `/model huggingface` → call `refresh_provider("huggingface")` async
- Present first 50 by downloads in selector
- Add search filter in selector widget
- "Show more" paginated load (next 100 at a time)

**Dependencies:**
- `reqwest` (already in workspace for HTTP)
- `lru` crate for cache
- No new major deps needed

## Decisions

### Defer to 0.16.0 for rc.1 scope

**Status:** decided

**Rationale:** 

## Open Questions

- Should we require HUGGING_FACE_TOKEN for browsing, or make public models accessible without auth?
