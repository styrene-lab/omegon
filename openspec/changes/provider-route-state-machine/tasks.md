# provider-route-state-machine — Tasks

Dependencies: Group 1 has no dependencies. Groups 2–4 depend on Group 1.
Group 5 depends on Groups 2–4. Group 6 (cleanup) depends on all prior groups.
Out of scope: serve/daemon routing (omegon-quartus auspex workstream owns it);
delegate/cleave child routing.

## 1. RouteController core and CredentialLedger
<!-- specs: provider-route -->

- [x] 1.1 Create `core/crates/omegon/src/route.rs`: `ProviderRoute` enum (Serving/Fallback/LoginPending/Disconnected), `FallbackReason`, `RouteSnapshot`, `RouteController` owning `Arc<RwLock<Box<dyn LlmBridge>>>` plus route state behind one lock
- [x] 1.2 Controller transition methods: `resolve_startup(selected, fallback_providers, ledger)`, `begin_login(provider)`, `complete_login(outcome)`, `switch_model(to)`, `logout(provider)` — each returns the new snapshot and emits `AgentEvent::RouteChanged`
- [x] 1.3 `CredentialState` enum {Valid{source}, Expired{refreshable}, Missing{probed_sources}} and `CredentialLedger::probe(provider)` wrapping `resolve_api_key_sync` (providers.rs:178) with structured return; re-probe on every call, no caching
- [x] 1.4 No TUI types in route.rs API surface (quartus constraint) — consumers get `RouteSnapshot` + events only
- [x] 1.5 Unit tests: every transition method from every state; property test enumerating startup matrix {valid,expired,missing} × {empty,with-creds,without-creds} asserts exactly one state, no substitution without explicit fallback config

## 2. Startup decision table and fallback config
<!-- specs: provider-route -->

- [x] 2.1 Add `fallback_providers: Vec<String>` to Settings (persisted, default empty) and to the Pkl profile schema in `pkl/`
- [x] 2.2 Replace the ad-hoc fallback block in main.rs:3843-3885 with `RouteController::resolve_startup`; retire `automation_safe_model()` from the interactive path
- [x] 2.3 Disconnected message: name the selected provider, list probed credential sources from the ledger, give the exact remediation (`/login <provider>` or env var name)
- [x] 2.4 Fallback-exhausted message lists every provider tried and the per-provider ledger reason
- [x] 2.5 Integration tests: empty-fallback startup with missing creds → Disconnected + NullBridge; configured fallback with valid creds → Fallback + RouteChanged emitted

## 3. Login lifecycle on the controller
<!-- specs: provider-route/login -->

- [x] 3.1 Rework the login task (main.rs ~5000): `begin_login` before spawn, `complete_login(Succeeded)` or `complete_login(Failed{timeout|stale_state_only|refused})` at terminal outcome — map from accept_oauth_callback errors and token-exchange failures
- [x] 3.2 Failed login reverts route to `prior` and sets a persistent footer warning (cleared on next attempt), not only a SystemNotification
- [x] 3.3 `/auth status` renders the current RouteSnapshot including LoginPending elapsed time and last terminal outcome with reason
- [x] 3.4 Tests: timeout reverts to prior Fallback; stale-state-only deadline produces Failed{stale_state_only} with close-old-tabs guidance; success from Fallback clears the warning

## 4. Consumers become route projections
<!-- specs: provider-route -->

- [x] 4.1 loop.rs: StreamOptions.model and TurnEnd model attribution read the RouteSnapshot; delete `config.bridge_model` seeding (loop.rs:340-380) and the bridge_model/settings fallback chain at loop.rs:707
- [x] 4.2 tui/mod.rs + footer.rs: footer sync projects RouteSnapshot; delete `settings.runtime_bridge_model` and `footer_data.fallback_from` (interim 77bf6227 mechanism); Fallback/Disconnected render persistent warnings, LoginPending renders provider + elapsed
- [x] 4.3 Command handlers (/model, set_model_tier, switch_to_offline_driver) call `controller.switch_model`; remove direct `settings.set_model` mutation from handlers; refused switch keeps route and reports why
- [x] 4.4 Tests: footer fallback render against RouteSnapshot (port model_card_shows_fallback_marker_when_bridge_diverges); refused /model switch leaves settings.model and route unchanged

## 5. End-to-end verification
<!-- specs: provider-route, provider-route/login -->

- [x] 5.1 Scenario test: profile selects codex, no codex creds, empty fallback → Disconnected, actionable message, no Anthropic substitution
- [x] 5.2 Scenario test: same but fallback_providers=["anthropic"] → Fallback state, footer warning, StreamOptions carries anthropic model
- [x] 5.3 Scenario test: login success from Fallback → Serving, warning cleared, bridge and route swapped in one transition
- [x] 5.4 Full suite `cargo test -p omegon` green (known flake: extensions::sdk_compat_spawn_tests, pre-existing)

## 6. Migration and release memory
<!-- specs: provider-route -->

- [x] 6.1 CHANGELOG `[Unreleased]`: breaking-change entry — silent fallback removed; include exact `fallback_providers = [...]` profile snippet for operators who relied on it
- [x] 6.2 Startup one-time notice when Disconnected would have silently fallen back under the old behavior (detected: fallback_providers empty AND a provider with valid creds exists) pointing at the new config key
- [x] 6.3 Update docs/provider-route-state-machine.md design node impl notes with final file scope deltas


## 7. 0.27.0 model intent and endpoint matrix follow-up
<!-- specs: provider-route/model-intent -->

- [x] 7.1 Replace legacy `ModelTier` vocabulary with provider-neutral `ModelGrade` F/D/C/B/A/S; remove `Local` as a capability value.
- [x] 7.2 Remove legacy slash commands `/gloriana`, `/victory`, `/retribution`, `/opus`, `/sonnet`, and `/haiku` entirely; they should be unknown commands, not hidden aliases.
- [x] 7.3 Replace legacy `set_model_tier` semantics with model-intent tooling (`set_model_intent` preferred, or explicit grade/provider/policy tools during migration).
- [x] 7.4 Extend the model registry from provider-tier maps to endpoint/model capability rows with grade, grade source, context window, tool/streaming/json/vision support, and cost/latency bands.
- [x] 7.5 Add endpoint definitions carrying endpoint id, display name, endpoint class (`LocalDev`/`Upstream`), protocol kind, base URL, credential reference, and enabled state.
- [x] 7.6 Implement OpenAI-compatible endpoint profiles for OpenRouter, Groq, Mistral, xAI, Hugging Face router, Gemini compatibility, and private OpenAI-compatible endpoints; keep Anthropic as a custom adapter.
- [x] 7.7 Add request sanitization/profile shaping for OpenAI-compatible endpoints, including unsupported fields and required/optional headers.
- [x] 7.8 Add `/model grade`, `/model provider`, `/model policy`, `/model route`, and `/model providers` to the canonical command registry and parser path.
- [x] 7.9 Preserve operator intent separately from active route so route failover can change the serving endpoint without erasing requested grade/provider/policy. (Implemented: RouteSnapshot/RouteState carry ModelIntent; exact switches pin intent; /model grade/provider/policy update durable intent; /model unpin clears exact overrides; Profile.modelIntent persists and hydrates startup intent; ModelIntent-to-CapabilityRequest bridge, candidate selection, RouteController intent-candidate resolution, and live runtime route resolution after intent changes are wired while preserving intent.)
- [x] 7.10 Add tests proving local is not accepted as a grade, legacy commands/tools are absent, and grade+provider intent resolves through endpoint capability rows.

- [x] 7.11 Replace stale TypeScript implementation scope in provider-neutral model-control docs with Rust-native files before coding from the design.
- [x] 7.12 Define default grade, provider selection, grade policy, failover policy, and degradation policy for interactive sessions and daemon agents.
- [x] 7.13 Add exact model override clearing (`/model unpin` or equivalent) and make pinned state visible in route/status projections.
- [x] 7.14 Enforce reserved provider selector tokens (`auto`, `local`, `upstream`) in registry/profile validation.
- [x] 7.15 Extend OpenAI-compatible endpoint profiles to normalize responses and provider-specific errors, not only request fields. (Implemented: request shaping via `shape_openai_request`; error normalization via 7.21; active streaming text/tool-call normalization via 7.22. No separate non-streaming OpenAI-compatible response path currently requires additional normalization.)
- [x] 7.16 Add data-driven endpoint auth schemes and route credential probing through endpoint metadata.
- [x] 7.17 Update or remove baseline routing/effort specs that still require `/local`, `/haiku`, `/sonnet`, `/opus`, or `set_model_tier`.

- [x] 7.18 Replace remaining internal `ModelTier` bridge and `data/model-registry.json` `tiers`/`tier` fields with grade/capability-row data.
- [x] 7.19 Update web/status/IPC projections that still expose `capability_tier` and legacy options (`retribution`, `victory`, `gloriana`).
- [x] 7.20 Rewrite stale long-lived docs that still describe public tiers as stable (`docs/model-routing.md` was patched; baseline specs remain).
- [x] 7.21 Add OpenAI-compatible error normalization profiles that map provider-specific error envelopes and rate-limit responses into common route/provider error categories.
- [x] 7.22 Add OpenAI-compatible response/stream normalization profiles for endpoint-specific tool-call delta quirks.
