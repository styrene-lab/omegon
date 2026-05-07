+++
id = "077fa2ec-95a3-4b72-b08e-416ed90abbf3"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Orchestratable Provider Model — Tasks

## 1. ProviderInventory + BridgeFactory (routing.rs — new)
<!-- specs: routing -->

- [x] 1.1 Define `CapabilityTier` enum: `Leaf`, `Mid`, `Frontier`, `Max` with Display impl
- [x] 1.2 Define `ProviderEntry` struct
- [x] 1.3 Define `OllamaModelInfo` struct
- [x] 1.4 Define `ProviderInventory` struct
- [x] 1.5 Implement `ProviderInventory::probe()`
- [x] 1.6 Implement `ProviderInventory::refresh()`
- [x] 1.7 Implement `ProviderInventory::providers_with_credentials()`
- [x] 1.8 Define `CapabilityRequest` struct
- [x] 1.9 Define `ProviderCandidate` struct
- [x] 1.10 Implement `route()` — score providers by tier match, cost, preference
- [x] 1.11 Define `BridgeFactory` struct
- [x] 1.12 Implement `BridgeFactory::get_or_create()`
- [x] 1.13 Unit tests (8 tests)

## 2. OllamaManager (ollama.rs — new)
<!-- specs: ollama -->

- [x] 2.1 Define `OllamaManager` struct
- [x] 2.2 Implement `new()` — reads OLLAMA_HOST
- [x] 2.3 Implement `is_reachable()`
- [x] 2.4 Implement `list_models()`
- [x] 2.5 Implement `list_running()`
- [x] 2.6 Implement `hardware_profile()`
- [x] 2.7 Register `mod ollama` in main.rs
- [x] 2.8 Unit tests (5 tests)

## 3. Provider bridge integration (providers.rs — modified)
<!-- specs: routing -->

- [x] 3.1 Add `pub mod routing;` and `pub mod ollama;` to main.rs
- [x] 3.2 Refactor `auto_detect_bridge()` to use `route()` for fallback
- [x] 3.3 Preserve backward compat for provider-prefixed model specs
- [x] 3.4 Export `ProviderInventory` and `route`

## 4. Cleave per-child routing (orchestrator.rs + state.rs — modified)
<!-- specs: cleave -->

- [x] 4.1 Add `provider_id: Option<String>` to `ChildState`
- [x] 4.2 Add `inventory` to `CleaveConfig`
- [x] 4.3 Implement `infer_capability_tier()` — scope-based heuristic
- [x] 4.4 Route per-child model in dispatch
- [x] 4.5 Populate ChildState from routed result
- [x] 4.6 Fallback to config.model if route() empty
- [x] 4.7 Explicit execute_model bypasses routing

## 5. Startup integration (main.rs + tui — modified)

- [x] 5.1 Create `ProviderInventory` at startup after splash probes
- [x] 5.2 Store as `Arc<RwLock<ProviderInventory>>` on App
- [x] 5.3 CleaveFeature probes on demand if no inventory injected
- [x] 5.4 Inventory auto-refreshes at dispatch time (lazy probing)
- [x] 5.5 auto_detect_bridge fallback also uses route()

## Cross-cutting constraints

- [x] C.1 auto_detect_bridge backward compat — existing callers unchanged
- [x] C.2 Interactive bridge Arc<RwLock<Box<dyn LlmBridge>>> preserved
- [x] C.3 Cleave children are processes — --model flag is the interface
- [x] C.4 OllamaManager async-safe — no blocking in tokio context
- [x] C.5 ProviderInventory::probe() fast (env var checks only)
- [x] C.6 All existing 857 tests continue to pass
