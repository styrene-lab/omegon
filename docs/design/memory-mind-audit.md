+++
id = "e4f08270-e39e-4903-9e0d-02d09b1e7b65"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Memory / Mind System Audit — Context Injection, Accuracy, and Optimization

## Overview

Deep inspection of the current memory and mind system: how many facts are injected into context, how memory token estimation works, whether the dashboard context bar accurately reflects memory-vs-conversation usage, and what optimizations are available for retrieval, injection, and dashboard accounting.

## Research

### Current state — memory injection and dashboard accounting

**Memory injection entry point**
- `extensions/project-memory/index.ts` injects memory on `before_agent_start`.
- Injection happens on first turn and post-compaction (`if (!firstTurn && !postCompaction) return;`).
- This means project memory is not re-injected every turn; the dashboard's memory estimate reflects the last generated injection blob, not a per-turn recomputation.

**Fact selection rules**
- If active fact count <= 3: inject a welcome/usage message instead of full memory.
- If embeddings unavailable, vector coverage <50%, prompt too short, or fact count <=20: inject full dump via `store.renderForInjection(mind)`.
- Full dump path caps facts at 50 by default (`factstore.renderForInjection()` maxFacts default 50) and edges at 20.
- Semantic path activates only when embeddings are available, vector coverage >=50%, user prompt length >10, and fact count >20.
- Semantic path injects: all `Constraints` + all `Specs` facts, all pinned working-memory facts (cap 25 in session state), and top semantic hits until total injected facts reaches 30.
- Semantic path does **not** render edges; it uses `renderFactList()` instead of `renderForInjection()`.

**Other injected memory content beyond facts**
- Recent episodes: up to 3 episodes appended.
- Global knowledge: separate global mind dump appended, up to 15 facts, no edges.
- Memory usage/help text and optional context-pressure guidance are prepended/appended.
- Therefore the memory payload is larger than just the rendered fact bullets.

**Dashboard memory bar accounting**
- `sharedState.memoryTokenEstimate` is written in `project-memory/index.ts` as `Math.round(injectionContent.length / 4)`.
- The dashboard bar in `extensions/dashboard/footer.ts` computes memory share as `memoryTokenEstimate / contextWindow` and colors it with the secondary accent block.
- This is explicitly documented as an *approximate* token count in `extensions/shared-state.ts`.
- The bar is therefore approximate in at least three ways:
  1. char/4 heuristic, not tokenizer-accurate
  2. based on the last generated memory injection blob, not necessarily the currently retained live context
  3. includes more than facts (episodes, global knowledge, help text, pressure warning)

**Current visible limits and caps**
- Working memory cap: 25 fact IDs.
- Full dump cap: 50 facts, 20 edges.
- Semantic retrieval cap: top 20 semantic hits, overall injected-fact cap 30.
- Global mind injection cap: 15 facts.
- Recent episodes cap: 3.

**Initial audit questions that fall directly out of the current code**
1. Is `memoryTokenEstimate` systematically over/under-counting compared with actual provider token accounting?
2. Does using `contextWindow` as the denominator make the secondary bar segment visually truthful enough, given prompt/system overhead and non-memory system content?
3. Should the bar represent total injected memory payload, or only rendered facts?
4. Should memory be re-estimated after compaction and other context mutations, not just after `before_agent_start`?
5. Is the semantic-mode fact cap (30) and full-dump cap (50) still appropriate now that episodes/global knowledge are appended too?

### Instrumentation slice 1 — implemented baseline metrics

Implemented an initial audit instrumentation slice.

**Files added/changed**
- `extensions/project-memory/injection-metrics.ts`
- `extensions/project-memory/injection-metrics.test.ts`
- `extensions/project-memory/shared-state.test.ts`
- `extensions/project-memory/index.ts`
- `extensions/shared-state.ts`
- `openspec/changes/memory-mind-audit-instrumentation/*`

**What is now measured**
- injection mode: `tiny | full | semantic`
- injected project fact count
- injected edge count
- injected working-memory fact count
- injected semantic-hit count
- appended recent episode count
- appended global fact count
- payload character count
- estimated token count

**Runtime visibility added**
- `sharedState.lastMemoryInjection` now stores the most recent structured injection snapshot.
- `/memory stats` now prints the last recorded injection snapshot so the operator can inspect current behavior without reading code.
- The dashboard remains backward compatible: it still uses `sharedState.memoryTokenEstimate` for the memory segment.

**Current limitations of slice 1**
- The estimate is still char/4 heuristic.
- Stats output is covered via helper tests, but there is not yet a higher-level command-output test for `/memory stats`.
- The dashboard bar is still not showing the richer breakdown; it only has access to the snapshot for future audit/visual work.

**Verification**
- `npm run check` passed after the instrumentation slice (`1153/1153` tests passing at the time of run).

### Instrumentation slice 2 — dashboard-visible memory audit line

Added a dashboard-visible memory audit summary without changing existing bar semantics.

**Files added/changed**
- `extensions/dashboard/memory-audit.ts`
- `extensions/dashboard/memory-audit.test.ts`
- `extensions/dashboard/footer.ts`
- `extensions/dashboard/footer-dashboard.test.ts`
- `openspec/changes/memory-mind-audit-instrumentation/tasks.md`

**Behavior added**
- Raised dashboard footer now renders a dedicated memory audit line beneath the existing context gauge.
- The line reads from `sharedState.lastMemoryInjection` and summarizes the latest injection snapshot.
- Narrower widths show a compact summary (`mode`, facts, working-memory count, episodes, global facts, estimate).
- Wider widths show a fuller breakdown (`mode`, facts, edges, working-memory facts, semantic hits, episodes, global facts, chars, estimated tokens).
- The existing context gauge remains unchanged and still uses `sharedState.memoryTokenEstimate`, preserving current semantics for comparison.

**Why this matters for the audit**
- The operator can now compare the color-coded memory bar with the latest measured injection composition in the same dashboard surface.
- This makes it easier to spot likely mismatches between perceived context pressure and actual memory payload composition before changing accounting logic.

**Verification**
- `npm run check` passed after this slice (`1157/1157` tests passing at the time of run).

### Gauge truthfulness audit — harness semantics and fix

Traced pi harness context accounting and patched the dashboard accordingly.

**Harness findings**
- `ctx.getContextUsage()` comes from `agent-session.getContextUsage()` in pi core.
- It returns `{ tokens, contextWindow, percent }`.
- After compaction, if there has not yet been a valid post-compaction assistant response, pi intentionally returns `tokens: null` and `percent: null` because pre-compaction assistant usage is no longer trustworthy.
- When known, `percent` reflects overall estimated context pressure using last assistant usage plus trailing estimated message tokens; it is not just chat history.

**Dashboard audit conclusion**
- The dashboard's secondary segment does not represent pure conversation; it represents **other non-memory context pressure** inside total used context.
- The previous implementation incorrectly collapsed `null` percent to `0`, which could falsely render an empty bar immediately after compaction.

**Fix implemented**
- Added `extensions/dashboard/context-gauge.ts` with a pure `buildContextGaugeModel()` helper.
- Updated `extensions/dashboard/footer.ts` to:
  - preserve existing known-state behavior (estimated memory segment + other-context segment)
  - render explicit unknown state when `percent` is null
  - use internal naming aligned with semantics (`other` rather than `conv`)
- Added tests in `extensions/dashboard/context-gauge.test.ts`.

**Resulting semantics**
- Known state: gauge shows total used context pressure with an estimated memory-highlighted subsegment.
- Unknown state: gauge shows `?` instead of falsely implying `0%`.

**Verification**
- `npm run check` passed after the fix (`1160/1160` tests passing at the time of run).

### Instrumentation slice 3 — estimate calibration against observed provider input

Added a first-pass calibration path to compare the memory estimate with observed provider input usage on the next completed assistant turn.

**Files changed**
- `extensions/project-memory/injection-metrics.ts`
- `extensions/project-memory/injection-metrics.test.ts`
- `extensions/project-memory/index.ts`

**What is now recorded**
- `baselineContextTokens`: context usage before injection (or null if unknown)
- `userPromptTokensEstimate`: char/4 estimate for the user prompt text
- `observedInputTokens`: provider-reported input tokens from the next completed assistant message
- `inferredAdditionalPromptTokens`: `observedInputTokens - baselineContextTokens - userPromptTokensEstimate` when baseline is known
- `estimatedVsObservedDelta`: `estimatedTokens - inferredAdditionalPromptTokens` when inference is possible

**Interpretation and caveats**
- This is an audit calibration, not a tokenizer-perfect attribution model.
- `inferredAdditionalPromptTokens` approximates how many prompt tokens were added on the turn beyond the previous retained context and the new user prompt.
- That inferred value may still include other turn-scoped additions besides project-memory injection (for example other extension-provided custom messages).
- When baseline context is unknown (notably right after compaction), the inferred comparison remains unknown until a later turn with known baseline.

**Operational consequence**
- `/memory stats` can now surface not just the estimated memory payload but also the next observed input usage and estimate-vs-inferred delta once at least one assistant response has completed after the injection event.

**Verification**
- `npm run check` passed after this slice (`1160/1160` tests passing at the time of run).

### Instrumentation slice 4 — event-driven disk refresh for dashboard producers

Implemented producer-side filesystem watchers so arbitrary on-disk Design Tree and OpenSpec edits become dashboard events.

**Files added/changed**
- `extensions/dashboard/file-watch.ts`
- `extensions/dashboard/file-watch.test.ts`
- `extensions/design-tree/index.ts`
- `extensions/openspec/index.ts`
- `openspec/changes/memory-mind-audit-instrumentation/tasks.md`

**Behavior added**
- Design Tree extension now starts a watcher on `docs/` at session start.
- OpenSpec extension now starts a watcher on `openspec/` at session start.
- On matching markdown file changes, each producer debounces refresh work and re-emits its dashboard state via the existing `dashboard:update` event.
- The footer and `/dash` overlay already subscribe to that event, so they now update for direct on-disk edits as well as extension-managed mutations.

**Semantics**
- The event system remains the single UI invalidation mechanism.
- Filesystem changes are now promoted into that event system by the producers.
- Refreshes are coalesced with a short debounce (`75ms`) to avoid flooding the event bus during save bursts.

**Current caveat**
- Watchers use `fs.watch(..., { recursive: true })` on the local Node runtime. This is best-effort and works well for the current macOS harness environment; unsupported runtimes fall back to command/tool-driven emits without breaking the extension.

**Verification**
- `npm run check` passed after this slice (`1164/1164` tests passing at the time of run).

## Decisions

### Decision: Dashboard context gauge treats post-compaction null usage as unknown, not zero

**Status:** decided
**Rationale:** Pi's `getContextUsage()` returns `percent: null` after compaction until a new assistant response provides trustworthy usage. Rendering that as `0%` falsely suggests an empty context. The dashboard should show an explicit unknown state while preserving the existing estimated-memory overlay semantics for known usage.

## Open Questions

*No open questions.*
