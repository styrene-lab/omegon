+++
id = "511ce7ab-3ea1-4170-a72b-2556eed649c5"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Codex Tier Routing — Provider-aware model selection for Cleave and tooling — Tasks

## 1. Shared provider-aware resolver
<!-- specs: routing/spec -->

- [ ] 1.1 Add a shared routing module under `extensions/lib/` that resolves canonical tiers (`local|haiku|sonnet|opus`) to concrete `{ provider, modelId }` results using the pi model registry
- [ ] 1.2 Define the session policy shape for provider order, avoid-provider list, cheap-cloud-over-local, large-run preflight, and operator notes
- [ ] 1.3 Implement provider matcher rules for Anthropic, OpenAI, and local models without duplicating lookup logic in individual extensions
- [ ] 1.4 Add unit tests covering provider preference, avoid-provider behavior, provider fallback, and explicit local resolution

## 2. Model-budget and effort integration
<!-- specs: routing/spec -->

- [ ] 2.1 Refactor `extensions/model-budget.ts` to use the shared resolver instead of Anthropic-only prefix matching
- [ ] 2.2 Update `set_model_tier` and related status/help text to display Servitor/Adept/Magos/Archmagos labels while keeping canonical tier keys in tool schemas
- [ ] 2.3 Refactor `extensions/effort/index.ts` driver switching to use the shared resolver for cloud tiers and to prefer cheap cloud over local where policy requires
- [ ] 2.4 Add or update tests for effort/model-budget behavior under mixed Anthropic/OpenAI availability and cap enforcement

## 3. Cleave explicit model dispatch
<!-- specs: routing/spec -->

- [ ] 3.1 Replace Cleave's fuzzy alias dispatch in `extensions/cleave/dispatcher.ts` with explicit model ID resolution for child execution
- [ ] 3.2 Resolve review models through the shared resolver and pass explicit model IDs to review subprocesses
- [ ] 3.3 Preserve canonical planning values in `ChildPlan.executeModel` and ensure serialization/tests continue using `local|haiku|sonnet|opus`
- [ ] 3.4 Add dispatcher tests verifying `--model <explicit-id>` is passed for sonnet-, opus-, and review-tier execution

## 4. Large-run provider preflight
<!-- specs: routing/spec -->

- [ ] 4.1 Define a heuristic for when a Cleave run counts as a large burn (for example child count, review enabled, or expected cloud fan-out)
- [ ] 4.2 Add a preflight step before large Cleave dispatches that asks the operator for current provider posture when session policy requires it
- [ ] 4.3 Persist the resulting provider preference update into shared state for the current session
- [ ] 4.4 Add tests covering large-run prompt behavior and the no-preflight path for small runs

## 5. Docs and UX polish
<!-- specs: routing/spec -->

- [ ] 5.1 Update README and relevant extension help text to describe provider-aware routing and the Servitor/Adept/Magos/Archmagos display labels
- [ ] 5.2 Document that local is a fallback/resilience path rather than the preferred cheap path when cheap cloud is available
- [ ] 5.3 Review existing OpenSpec baseline text impacted by this change and prepare any necessary follow-up baseline updates during archive
