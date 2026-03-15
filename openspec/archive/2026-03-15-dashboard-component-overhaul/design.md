# Raised dashboard component overhaul — model topology clarity and horizontal layout — Design

## Architecture Decisions

### Decision: Raised mode should use component cards rather than stacked prose-like status rows

**Status:** decided
**Rationale:** The lower dashboard now carries stable, semantically distinct data: context pressure, model topology, memory state, runtime/system state, and recovery/fallback state. Presenting these as explicit cards/components makes better use of horizontal space, improves scanability, and gives the footer a coherent visual grammar instead of looking like appended terminal text.

### Decision: Model topology must distinguish driver, embeddings, extraction, and fallback state

**Status:** decided
**Rationale:** Users need to understand which model is actually driving the session versus which models power semantic retrieval and background memory work. A single `OFFLINE: ...` label is insufficient and misleading. Raised mode should surface role-based model information with clear labels and source/status semantics rather than implying there is only one active model.

### Decision: Raised footer and focused overlay should share a common information architecture

**Status:** decided
**Rationale:** The persistent raised footer should feel like a condensed dashboard, not a separate parallel UI. Sharing the same semantic sections and labels across raised/footer and focused overlay reduces operator confusion, makes transitions between summary and drill-down predictable, and avoids inconsistent presentations of model/routing state.

### Decision: Responsive layout should use explicit width tiers with graceful card reflow

**Status:** decided
**Rationale:** Terminal size varies too much to rely on one layout plus truncation. Raised mode should define width tiers—narrow, medium, and wide—with intentional card composition rules for each tier. Narrow terminals should stack compact summaries; medium terminals should use a two-zone layout with prioritized cards; wide terminals should expose multiple horizontal cards. This preserves clarity without forcing every terminal into the same visual shape.

### Decision: Raised mode should use a fixed card inventory with priority-based degradation

**Status:** decided
**Rationale:** The redesign should not invent a different footer per terminal width. It should have a stable inventory of cards—lifecycle/work summary, context, model topology, memory, runtime/system, and conditional recovery—and then degrade by collapsing lower-priority cards or summaries as width shrinks. This keeps the dashboard recognizable while still adapting intelligently.

### Decision: Model topology card should summarize role, source, and state rather than raw model strings

**Status:** decided
**Rationale:** To avoid repeating the current ambiguity, each model-role row should encode three things: what role it serves (driver, embeddings, extraction), where it comes from (cloud/local), and what state it is in (active, fallback, offline, alias/legacy). The footer should favor canonical labels and compact badges over raw provider/model strings unless width allows secondary metadata.

### Decision: Responsive tiers should preserve a consistent vertical reading order even when cards reflow horizontally

**Status:** decided
**Rationale:** Operators should not have to relearn the dashboard at each terminal width. The same semantic order should remain legible across tiers: lifecycle/work first, then context, model topology, memory, and runtime/system, with recovery inserted only when relevant. Horizontal reflow should change grouping, not the conceptual reading order.

### Decision: Narrow tiers should compress card content before dropping card categories

**Status:** decided
**Rationale:** At small widths the dashboard should first collapse cards to compact summaries, abbreviate secondary metadata, and hide optional annotations before removing entire card categories. This preserves semantic continuity and avoids a narrow terminal silently losing the operator's view of critical subsystems like model topology.

## Research Context

### Current raised footer is still a text report with stacked HUD sections

`extensions/dashboard/footer.ts` builds the raised footer as a boxed text layout with a bottom HUD zone composed of three sequential sections: context, memory, and system. Even in wide layouts the lower HUD remains vertically stacked and text-oriented, which underuses horizontal space and makes stable status data feel like log output instead of intentional interface components.

### Model information is split across incompatible surfaces

The raised footer currently shows only the active chat model and thinking level in the context HUD, while the system overlay separately exposes effort fields like `driver` and `extract` in `extensions/dashboard/overlay-data.ts`. Offline status is injected through extension status text (`OFFLINE: ...`) rather than a first-class model-role surface. This makes the visible raised dashboard ambiguous about whether it is showing the driver model, embeddings, extraction, or merely fallback state.

### The focused overlay already has richer system data than the raised footer

The `/dashboard` overlay exposes a dedicated System tab and already models expandable sections for routing policy, effort tier, memory injection, and recovery state. The raised footer should not duplicate every detail, but it should align with the same conceptual structure so that the compact persistent dashboard reads like a summary of the richer inspectable surfaces rather than a separate ad hoc text summary.

### Responsive behavior should be explicit rather than emergent from truncation

The current raised footer mainly adapts by truncating lines and switching between stacked vs two-column regions at a coarse width threshold. A component overhaul should define explicit breakpoint behaviors for narrow, medium, and wide terminals so the dashboard intentionally reflows cards instead of merely clipping text. This is especially important for model-topology cards, which can otherwise become ambiguous again when labels collapse under width pressure.

### Proposed responsive layout contract

Use explicit width tiers for raised mode. Narrow (<100 cols): stacked cards only, each card limited to 1–2 summary rows; model topology compresses to role labels with canonical names and source badges. Medium (100–139 cols): two-zone layout with lifecycle/work summary in one zone and system/model cards in the other; context and model topology remain mandatory, memory/runtime may collapse to single-row summaries. Wide (140+ cols): multi-card horizontal footer zone with separate context, model topology, memory, and runtime/system cards, plus conditional recovery card when actionable state exists.

### Card priority should be stable across width tiers

Not every card has equal priority. Context pressure and model topology are mandatory because they explain current operating conditions. Memory and runtime/system are secondary but still expected in persistent raised mode. Recovery/fallback is conditional and should only claim space when actionable or fresh. This priority ordering gives the renderer a principled way to drop detail as width shrinks without making the dashboard semantically incoherent.

### Tier-by-tier layout blueprint

Define a concrete raised-layout blueprint by width tier. Narrow (<100): render lifecycle/work summary first, then stacked cards in this order: context, model topology, memory, runtime/system, with recovery inserted above runtime/system only when actionable or fresh; each card is capped to compact 1–2 row summaries. Medium (100–139): keep lifecycle/work summary above, then split the lower zone into two columns—left: context + memory; right: model topology + runtime/system; recovery occupies the right column ahead of runtime/system when present. Wide (140+): keep lifecycle/work summary above, then render a multi-card footer row with context, model topology, memory, and runtime/system as distinct cards; actionable/fresh recovery becomes an additional card or replaces the runtime/system slot when space pressure exists.

## File Changes

- `extensions/dashboard/footer.ts` (modified) — Refactor raised-mode rendering into composable card/layout builders for context, model topology, memory, runtime/system, and lifecycle summary sections.
- `extensions/dashboard/types.ts` (modified) — Add any dashboard state/types needed to represent role-based model topology and card summaries without overloading extension status text.
- `extensions/dashboard/index.ts` (modified) — Keep raised/panel/focused mode behavior coherent if the footer component architecture changes or additional shared state is required.
- `extensions/dashboard/overlay-data.ts` (modified) — Align system-tab labels and model-role terminology with the raised footer so both surfaces describe the same driver/extract/embedding concepts.
- `extensions/dashboard/footer-raised.test.ts` (modified) — Update layout expectations and add coverage for card composition, model-role clarity, and wide-screen horizontal usage.

## Constraints

- Raised mode must remain readable in narrow terminals and degrade gracefully to stacked sections when width is limited.
- The persistent footer must summarize role-based model topology without dumping every system-tab detail.
- Canonical model labels should be shown where possible; legacy aliases or fallback state may appear only as secondary metadata.
- The redesign should preserve compatibility with existing dashboard modes (`compact`, `raised`, `panel`, `focused`) and their keyboard/command flows.
- Responsive behavior should be implemented with explicit width-tier layout rules rather than relying only on truncation side effects.
- Context and model topology are mandatory cards across all width tiers; recovery is conditional and only expands when actionable or fresh.
