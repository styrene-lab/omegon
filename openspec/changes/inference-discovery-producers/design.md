# Design — Discovery-layer producers and model catalog unification

Target release: **0.28.2** (release/0.28 branch).
Design node: `docs/inference-discovery-producers.md` (decided).

## Problem

`inference_inventory.rs` implements the layered merge with `InventorySource::Discovery`
(precedence 50), but no producer ever constructs that layer — discovery is a slot with
zero writers. `tui/model_catalog.rs` bypasses the inventory entirely and projects the
static embedded `data/model-registry.json`. Verified consequence: GitHub Copilot shows
4 registry models while the live `/models` endpoint returns 29 (probe, 2026-07-14).
Curation cost currently grows with provider×model count.

## Architecture

### New module: `core/crates/omegon/src/inference_discovery.rs`

```
pub trait ModelDiscovery: Send + Sync {
    fn endpoint_id(&self) -> &str;
    async fn fetch(&self) -> Result<DiscoveredModels, DiscoveryError>;
}
```

Fetchers are **protocol-keyed** (accepted decision):

| Fetcher | Covers | Contract |
|---|---|---|
| `OpenAiCompatibleDiscovery` | openai, groq, mistral, xai, hf-router, ollama-cloud | `GET {baseUrl}/models`, bearer |
| `OpenRouterDiscovery` | openrouter | `GET /api/v1/models`, parses context/pricing/modalities |
| `AnthropicDiscovery` | anthropic | `GET /v1/models` |
| `GoogleDiscovery` | google | `GET /v1beta/models`, parses token limits |
| `CopilotDiscovery` | github-copilot | token exchange + `/models`; **re-homes** the transport from `github_copilot.rs` probe (shared fn, no duplication); parses per-model capabilities/limits when present |
| `OllamaLocalDiscovery` | ollama | wraps existing `ollama list` query from `model_catalog.rs` |

Endpoints without a contract (perplexity; openai-codex unless the implementation-time
probe proves otherwise) declare `discovery: none` in the registry endpoints block and
are skipped — first-class, not an error.

Each successful fetch produces an `InventoryLayer::new(InventorySource::Discovery, …)`
of `OfferingPatch`es. Unknown ids get conservative defaults (128k/16k, `coding`),
no grade — the existing ungraded-offering semantics apply unchanged. Registry-listed
ids missing from a live enumeration are patched unavailable-on-endpoint.

### Refresh pipeline

- `DiscoveryScheduler` owns per-endpoint TTL state. Copilot TTL from token-exchange
  `refresh_in`/`expires_at`; default 1h elsewhere (configurable via existing inference
  runtime config surface).
- Runs: startup (post credential resolution), explicit operator refresh (TTL bypass),
  TTL expiry. Always async; catalog reads never touch the network.
- On completion, builds discovery layers and drives `InferenceRuntime::refresh()` /
  `InventoryHandle` so last-known-good snapshot semantics and `InferenceRefreshReport`
  diagnostics are reused as-is.
- Persistence: discovery cache file (JSON, per-endpoint layer + fetched_at + ttl) under
  the project/user state dir; loaded as the initial discovery layer on process start,
  evidenced as cached.
- Fetch failure: retain previous layer for that endpoint, record redacted diagnostic.

### Catalog migration

`ModelCatalog::cloud_only()` stops reading `ModelRegistry::global()` directly and
projects the active `InventorySnapshot` (auth gating preserved; chat-modality
compatibility filter excludes embedding/internal ids such as
`text-embedding-3-small-inference`, `trajectory-compaction`). The embedded registry
remains the bootstrap layer inside the inventory — role change, no data deletion.
Selection surface shows per-provider freshness (fresh/cached/stale + timestamp) and
an explicit refresh action.

## Verification obligations carried from design node

1. Live-verify each unverified enumeration endpoint during implementation; record
   `verifiedAt` per endpoint.
2. Probe openai-codex ChatGPT-backend token against `/v1/models`; on failure set
   `discovery: none`.
3. Inspect full Copilot `/models` body; map capabilities/limits into patches where
   trustworthy.

## Non-goals

- No grade synthesis from discovery.
- No removal of `data/model-registry.json`.
- No new provider bridges — discovery lists offerings; routing/bridge support is
  unchanged (`route_supported_by_compiled_bridge` still gates autonomous routing).
