# Raised dashboard component overhaul — model topology clarity and horizontal layout — Tasks

## 1. Shared model-topology surface and terminology

<!-- specs: dashboard/raised-layout -->

- [x] 1.1 Add dashboard state/types for role-based model topology summaries in `extensions/dashboard/types.ts`
- [x] 1.2 Define a compact summary contract that represents, at minimum, driver, embeddings, extraction, source (cloud/local), and state (active/fallback/offline/legacy alias)
- [x] 1.3 Update `extensions/dashboard/overlay-data.ts` so the System tab uses the same role names and ordering as the raised footer rather than a separate ad hoc vocabulary

## 2. Raised footer card architecture

<!-- specs: dashboard/raised-layout -->

- [x] 2.1 Refactor `extensions/dashboard/footer.ts` to replace the stacked HUD footer with composable card builders for lifecycle/work, context, model topology, memory, runtime/system, and conditional recovery
- [x] 2.2 Preserve a stable semantic reading order across all widths: lifecycle/work first, then context, model topology, memory, and runtime/system
- [x] 2.3 Implement the model-topology card so it summarizes role, source, and state with canonical labels first and legacy alias metadata second
- [x] 2.4 Keep the raised footer as a summary surface — do not mirror every expandable detail from the focused overlay

## 3. Responsive layout tiers

<!-- specs: dashboard/raised-layout -->

- [x] 3.1 Implement explicit narrow-tier (`<100`) behavior in `extensions/dashboard/footer.ts`: stacked cards, compact 1–2 row summaries, compress content before dropping categories
- [x] 3.2 Implement explicit medium-tier (`100–139`) behavior: two-zone lower layout with left = context + memory and right = model topology + runtime/system
- [x] 3.3 Implement explicit wide-tier (`140+`) behavior: multi-card horizontal lower zone with distinct context, model topology, memory, and runtime/system cards
- [x] 3.4 Make recovery conditional across tiers: it should claim space only when actionable or fresh, and degrade cleanly under width pressure

## 4. Dashboard mode coherence

- [x] 4.1 Update `extensions/dashboard/index.ts` as needed so raised, panel, and focused modes remain behaviorally coherent after the footer architecture changes
- [x] 4.2 Ensure the focused overlay still reads like the drill-down form of the same information architecture rather than a different dashboard product

## 5. Verification coverage

<!-- specs: dashboard/raised-layout -->

- [x] 5.1 Expand `extensions/dashboard/footer-raised.test.ts` to cover narrow, medium, and wide tier layouts explicitly
- [x] 5.2 Add assertions that raised mode always preserves context + model topology visibility across width tiers
- [x] 5.3 Add assertions that model-role labels are unambiguous and consistent with overlay terminology
- [x] 5.4 Add assertions that wide layouts actually use horizontal grouping rather than degenerating back into vertically stacked HUD sections

## 6. Final validation

- [x] 6.1 Run targeted dashboard tests for raised/footer behavior
- [x] 6.2 Run `npm run typecheck`
- [x] 6.3 Reconcile artifacts and run `/assess spec dashboard-component-overhaul`
