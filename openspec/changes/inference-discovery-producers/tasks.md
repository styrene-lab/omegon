# Tasks — inference-discovery-producers

Dependencies: group 1 before 2 (scheduler drives fetchers); groups 1–2 before 3
(catalog projects what discovery populates); group 4 last.

## 1. Discovery fetchers
<!-- specs: inference/discovery -->

- [x] 1.1 Create `inference_discovery.rs` with `ModelDiscovery` trait, `DiscoveredModels`, `DiscoveryError`, and offering-patch construction (unknown ids → ungraded conservative defaults; registry ids absent from live enumeration → unavailable-on-endpoint patch)
- [x] 1.2 Implement `OpenAiCompatibleDiscovery` (`GET {baseUrl}/models`, bearer from `resolve_api_key_sync`/auth), driven by registry endpoint entries for openai, groq, mistral, xai, huggingface-router, ollama-cloud
- [x] 1.3 Implement `OpenRouterDiscovery` parsing context length, modalities, pricing into offering metadata
- [x] 1.4 Implement `AnthropicDiscovery` and `GoogleDiscovery` (token limits from `/v1beta/models`)
- [x] 1.5 Re-home Copilot token-exchange + `/models` transport from `github_copilot.rs` probe into a shared fn; implement `CopilotDiscovery` parsing per-model capabilities/limits from the response body; keep `auth copilot-probe` working on the shared transport
- [x] 1.6 Implement `OllamaLocalDiscovery` wrapping the existing `ollama list` query
- [x] 1.7 Add `discovery: none` handling driven by registry endpoint entries (perplexity now; openai-codex pending 4.2 probe) — skipped endpoints produce no fetch and no diagnostic noise
- [x] 1.8 Unit tests per fetcher against canned response fixtures (success, malformed body, non-2xx, empty list) asserting layer contents and evidence provenance

## 2. Refresh scheduler, TTL cache, persistence
<!-- specs: inference/discovery -->

- [x] 2.1 (partial: TTL state + explicit-refresh trigger done in DiscoveryCache/refresh_discovery; startup post-auth trigger not yet spawned) Implement `DiscoveryScheduler` with per-endpoint TTL state (Copilot TTL from `refresh_in`/`expires_at`; default 1h configurable), refresh triggers: startup post-auth, explicit refresh (TTL bypass), TTL expiry
- [x] 2.2 Wire scheduler output into `InferenceRuntime`/`InventoryHandle` refresh so discovery layers merge under existing last-known-good semantics and `InferenceRefreshReport` diagnostics
- [x] 2.3 Persist discovery cache (per-endpoint layer + fetched_at + ttl) to state dir; load on process start as cached-evidence discovery layer before any network activity
- [x] 2.4 Failure handling: retain previous per-endpoint layer on fetch error, emit redacted diagnostic with endpoint id
- [x] 2.5 Tests: TTL expiry refetch vs unexpired skip, explicit-refresh bypass, cold-start from cache file, fetch failure retains last-known-good and snapshot still validates

## 3. Catalog unification and selection surface
<!-- specs: inference/catalog-unification -->

- [x] 3.1 Migrate `ModelCatalog::cloud_only()`/`discover()` to project from the active `InventorySnapshot` (auth gating preserved; embedded registry serves only as bootstrap layer through the inventory)
- [x] 3.2 Apply chat-modality compatibility filtering to the selection projection (exclude embedding/internal ids, e.g. `text-embedding-3-small-inference`, `trajectory-compaction`)
- [x] 3.3 (partial: freshness lines + TTL-bypass via /runtime refresh shipped in model-list output; dedicated selector-widget display pending) Surface per-provider freshness (fresh/cached/stale + last-confirmed timestamp) and an explicit TTL-bypassing refresh action in the model selection surface (`tui/mod.rs` model list + `control_runtime.rs` consumers)
- [x] 3.4 Tests: catalog reflects discovered offerings (29-model Copilot fixture beats 4-entry registry), catalog build is network-free, ungraded discovered offering selectable but excluded from autonomous routing, Ollama section matches installed set

## 4. Live verification, docs, release
<!-- specs: inference/discovery -->

- [ ] 4.1 (partial: github-copilot + anthropic verified; remaining providers lack credentials on this machine) Live-verify each previously unverified enumeration endpoint (openai, groq, mistral, xai, openrouter, anthropic, google) and record `verifiedAt` + discovery contract in `data/model-registry.json` endpoints block
- [ ] 4.2 (blocked: no synchronously resolvable openai-codex token in this environment; stays discovery:none) Probe openai-codex ChatGPT-backend token against `/v1/models`; set `discovery: none` or enable the generic fetcher per result; record outcome in design node
- [x] 4.3 Inspect live Copilot `/models` body shape; confirm capabilities/limits mapping in `CopilotDiscovery` matches reality
- [ ] 4.4 Update CHANGELOG `[Unreleased]` → 0.28.2 section; full `just test-rust` + `just lint` green; design node → implemented
