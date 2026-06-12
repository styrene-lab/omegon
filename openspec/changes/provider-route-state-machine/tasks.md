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
- [ ] 3.3 `/auth status` renders the current RouteSnapshot including LoginPending elapsed time and last terminal outcome with reason
- [ ] 3.4 Tests: timeout reverts to prior Fallback; stale-state-only deadline produces Failed{stale_state_only} with close-old-tabs guidance; success from Fallback clears the warning

## 4. Consumers become route projections
<!-- specs: provider-route -->

- [ ] 4.1 loop.rs: StreamOptions.model and TurnEnd model attribution read the RouteSnapshot; delete `config.bridge_model` seeding (loop.rs:340-380) and the bridge_model/settings fallback chain at loop.rs:707
- [ ] 4.2 tui/mod.rs + footer.rs: footer sync projects RouteSnapshot; delete `settings.runtime_bridge_model` and `footer_data.fallback_from` (interim 77bf6227 mechanism); Fallback/Disconnected render persistent warnings, LoginPending renders provider + elapsed
- [ ] 4.3 Command handlers (/model, set_model_tier, switch_to_offline_driver) call `controller.switch_model`; remove direct `settings.set_model` mutation from handlers; refused switch keeps route and reports why
- [ ] 4.4 Tests: footer fallback render against RouteSnapshot (port model_card_shows_fallback_marker_when_bridge_diverges); refused /model switch leaves settings.model and route unchanged

## 5. End-to-end verification
<!-- specs: provider-route, provider-route/login -->

- [ ] 5.1 Scenario test: profile selects codex, no codex creds, empty fallback → Disconnected, actionable message, no Anthropic substitution
- [ ] 5.2 Scenario test: same but fallback_providers=["anthropic"] → Fallback state, footer warning, StreamOptions carries anthropic model
- [ ] 5.3 Scenario test: login success from Fallback → Serving, warning cleared, bridge and route swapped in one transition
- [ ] 5.4 Full suite `cargo test -p omegon` green (known flake: extensions::sdk_compat_spawn_tests, pre-existing)

## 6. Migration and release memory
<!-- specs: provider-route -->

- [ ] 6.1 CHANGELOG `[Unreleased]`: breaking-change entry — silent fallback removed; include exact `fallback_providers = [...]` profile snippet for operators who relied on it
- [ ] 6.2 Startup one-time notice when Disconnected would have silently fallen back under the old behavior (detected: fallback_providers empty AND a provider with valid creds exists) pointing at the new config key
- [ ] 6.3 Update docs/provider-route-state-machine.md design node impl notes with final file scope deltas
