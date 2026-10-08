# Resolved inference route policy — implementation tasks

The user accepted all three decisions in the
[canonical node](../../../docs/resolved-inference-policy.md#accepted-decisions).
Check a task only after its implementation and applicable validation complete.
Numeric task IDs are stable within this change.

Dependencies: group 1 precedes group 2; groups 1–2 precede group 3;
group 4 consumes group 3's captured policy and accounting. Tests use offline
fixtures and fake bridges. Existing preparation and authority requirements apply
to every group; no task authorizes changing their security or lifecycle owners.

## 1. Route facts and working capacity
<!-- specs: inference/route-policy -->

- [x] 1.1 Add route-keyed fact/provenance vocabulary through existing `model_registry.rs` and `inference_inventory.rs` owners; review legacy capacity meanings and retain inventory source ownership/precedence, with fixtures for route isolation, stale facts, newer discovery supersession, and unresolved same-authority conflicts.
- [x] 1.2 Resolve numeric route targets in request/session/project/user/default order with narrower host caps; cover the route-policy precedence fixtures (saved 600k versus request 300k, invalid winner above 872k, known/unknown maximum materialization), legacy unscoped non-consent, and stable-route token refresh without profile rewrites.
- [x] 1.3 Implement the route-policy `I/P/G_h/E/G/B_input` accounting equations and adapter coverage semantics; test the 90k-reject/80k-fit total-window boundary, input-only and unspecified bases, conservative heuristics, unknown enforcement, schema/cache deduplication, exhausted budgets, and named configured thresholds.

## 2. Route-specific reasoning and compatibility
<!-- specs: inference/route-policy -->

- [x] 2.1 Resolve Disabled/Enabled/Categorical/TokenBudget/Adaptive/ProviderDefault through model-route capabilities and provider adapters; cover Astra supported sets, Anthropic budget/adaptive semantics, local boolean enabled/disabled support, and unknown defaults with offline fixtures.
- [x] 2.2 Preserve requested/effective reasoning and explicit normalization reasons; assert Minimal-to-low wire/UI agreement while retaining labeled heuristic reserves and rejecting unsupported concrete efforts, including ultra and raw option strings.
- [x] 2.3 Add needs-resolution handling for saved unsupported Off and model-switch incompatibility; provide actionable supported choices, preserve stored profile values, and verify explicit operator resolution is required before dispatch.

## 3. Prepared policy capture across existing flows
<!-- specs: inference/route-policy -->

- [x] 3.1 Bind resolved policy and complete accounting to `PreparedModelRequest` from the captured route/model/metadata generation; test connection, login, model, and metadata changes plus stale dispatch rejection under existing execution-binding rules.
- [x] 3.2 Integrate normal/repair, captured and idle compaction, and bounded auxiliary adapters with their own policy inputs; use inspecting bridges and authority fixtures to verify retry/repair identities, no-tools restrictions, distinct models, sessionless attribution, and unchanged cancellation/completion ownership.
- [x] 3.3 Validate policy-owned fields after relevant protocol adaptation; reject contradictory late body/options overrides and test captured output enforcement, reasoning, native model, and context options before any provider stream opens.

## 4. Semantic projection and delivery evidence
<!-- specs: inference/policy-projection, inference/route-policy -->

- [x] 4.1 Project one captured policy through `src/surfaces/` to TUI and ACP; add regressions for the percentage/denominator mismatch, mandatory-input estimate, effective effort, startup unavailable usage, model switches, and late provider usage for an older request.
- [x] 4.2 Extend registered `/context status` through `command_registry.rs` and `features/context.rs` to explain provenance, age, overrides, defaults/ceilings, reserves, output enforcement, normalization, and named thresholds while retaining surface availability and compact headers.
- [x] 4.3 Run relevant scenario tests and owning-crate landing gates, then bounded private headless projection acceptance; document fixture revisions and migration compatibility, and reconcile design/OpenSpec evidence through supported lifecycle owners only when authorized. Preserve existing persisted facts and leave unsupported native closure explicit.
