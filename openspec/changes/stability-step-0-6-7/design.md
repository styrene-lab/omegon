# 0.6.7 stability step — memory injection budget discipline and Node runtime compatibility — Design

## Architecture Decisions

### Decision: Tighten memory injection by default and gate low-value additions

**Status:** decided
**Rationale:** The session-usage spike is most plausibly caused by an over-generous default per-turn memory payload rather than a missing retrieval feature. 0.6.7 should reduce the default injection budget and make structural fill, episodic memory, and global facts more conditional so routine turns carry less repeated context while still preserving high-priority working-memory and directly relevant facts.

### Decision: Enforce Node 20+ explicitly instead of attempting Node 18 compatibility

**Status:** decided
**Rationale:** The vendored pi-tui dependency already requires Node >=20 and uses /v Unicode regexes that are not parseable on Node 18. The stability fix is to fail early and clearly through package/startup guardrails rather than carrying a bespoke compatibility fork for unsupported runtimes in a patch release.

## Research Context

### Initial assessment

Project-memory currently injects a unified semantic payload on every agent turn from before_agent_start. The budget is derived as 15% of estimated total context, floored at 4K chars and capped at 16K chars, then populated via six tiers: pinned working memory, top Decisions, top Architecture, hybrid search hits, structural fill from multiple sections, and recency fill. It can additionally append one recent episode and up to 4-6 global facts. Injection metrics already record payload chars, estimated tokens, baseline context tokens, and inferred added prompt tokens, so the likely session-usage spike is from an overly generous default budget plus mandatory structural/episode/global additions rather than lack of observability. Separately, vendor/pi-mono/packages/tui/src/utils.ts uses Unicode regexes with the /v flag (zeroWidthRegex, leadingNonPrintingRegex, rgiEmojiRegex). vendor/pi-mono/package.json already declares engines.node >=20.0.0, but Omegon root package.json currently lacks a matching engines floor, so installs/runs on Node 18 fail late at runtime with SyntaxError: Invalid regular expression flags instead of an upfront compatibility gate.

### Assessment gate status

Attempted `/assess design stability-step-0-6-7`, but the design-assessment subprocess failed before producing JSON because the spawned Omegon process loaded duplicate extension roots and then project-memory errored with `no such column: only`. This is an assessment-tooling/runtime issue, not an unresolved design question in this node. Design intent remains clear enough to proceed with tracked implementation: tighten per-turn memory injection defaults and enforce Node 20+ explicitly.

## File Changes

- `extensions/project-memory/index.ts` (modified) — Reduce default memory injection budget and gate structural/global/episode additions based on need and observed cost.
- `extensions/project-memory/injection-metrics.ts` (modified) — Extend or clarify injection telemetry if needed to validate reduced prompt overhead.
- `package.json` (modified) — Declare Node engine floor matching vendored runtime requirements.
- `scripts/preinstall.sh` (modified) — Fail early with a clear unsupported-Node message before install/build proceeds.
- `README.md` (modified) — Document supported Node runtime expectations for operators.
- `vendor/pi-mono/packages/tui/src/utils.ts` (modified) — Only if a minimal upstream-compatible guard or comment is needed; avoid a compatibility fork unless forced.

## Constraints

- 0.6.7 must reduce routine prompt bloat without removing high-priority working-memory continuity.
- Unsupported Node versions must fail early with a clear message rather than crashing later on /v Unicode regex parsing.
- Prefer aligning Omegon's runtime contract with vendored pi-mono requirements over introducing a Node 18 compatibility fork in a patch release.
