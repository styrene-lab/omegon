# Provider Route State Machine — eliminate implicit auth/fallback states

## Intent

Three model-identity honesty bugs were fixed individually within 48h (42ce0bb6 session-log model chain, a9fc3215 TurnEnd active-model seeding, 77bf6227 footer fallback display) — whack-a-mole that proves the architecture, not the patches, is the problem. Audit of the provider/auth system (2026-06-12) found six structural fragilities:

F1 — Five+ sources of truth for "which model is serving": settings.model (operator intent, persisted), runtime_resources.bridge_model (Arc<Mutex<Option<String>>>, actual route), LoopConfig.bridge_model + config.model (frozen per loop run, loop.rs:707), loop-local active_model (per-turn), footer_data.model_id (display copy), startup_decision.* (startup only). Each consumer re-derives reality; each derivation is an opportunity to lie.

F2 — Implicit hardcoded fallback chain: providers::automation_safe_model() (providers.rs:70) silently walks anthropic → openai-codex → google → google-antigravity → openai → openrouter → ollama. Not configurable, not surfaced at startup beyond a log line and one auth warning string. No way to express "never fall back" or "fall back to local only."

F3 — Credential resolution is a per-call 3-source merge: resolve_api_key_sync (providers.rs:178) merges env vars + auth.json + external tool credentials (Codex CLI etc.) on every call. Expired OAuth is tagged in debug logs, not modeled as state. Explains observed confusion: CHATGPT_OAUTH_TOKEN "missing" at secrets preflight while Codex traffic flows via external credentials.

F4 — Login attempts are fire-and-forget: spawn_operator_task reduces the entire login outcome to one SystemNotification string (main.rs ~5000). No LoginAttempt state object; nothing to query for "pending/failed/why". Operator believed a login completed when it had timed out twice (2026-06-12, stale-tab 409 + timeout).

F5 — Divergent routing semantics per runtime: interactive TUI fixes the bridge at startup + hot-swaps on login; serve/daemon re-runs auto_detect_bridge per turn from settings.model (main.rs:1953-1973). Same config, different behavior.

F6 — NullBridge is an implicit fourth state signaled only by the provider_connected bool.

PROPOSAL — single authoritative ProviderRoute state machine:

enum ProviderRoute {
  Serving { model },
  Fallback { selected, serving, reason: FallbackReason },
  LoginPending { provider, since, prior: Box<ProviderRoute> },
  Disconnected { selected, reason },
}

Owned by one RouteController that also guards the bridge Arc — transitions only through it, every transition emits AgentEvent::RouteChanged plus a tracing event. All consumers (footer, loop StreamOptions.model, TurnEnd, session-log, /auth status, serve path) become read-only views of the route; settings.runtime_bridge_model / footer_data.fallback_from / loop active_model seeding are deleted, not maintained. Profile gains explicit `fallback_providers = [..]` ordered list; empty = fail hard to Disconnected with actionable message. Startup becomes a total decision table — (selected creds?, fallback creds?) maps to exactly one of the four states, enumerable by property test. Login attempts are tracked objects with terminal states: Succeeded → Serving (hot-swap inside the controller), Failed{timeout|stale_state|refused} → revert to prior route + persistent footer warning rather than a scrolling notification.

## Scope

_TBD_

## Constraints

_None identified yet._
