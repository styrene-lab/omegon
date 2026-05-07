+++
id = "d58d2e25-d069-46bd-8ba9-b228954ac706"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Context Class Taxonomy and Routing Policy — named context classes, route envelopes, downgrade safeguards — Design Spec (extracted)

> Auto-extracted from docs/context-class-taxonomy-and-routing-policy.md at decide-time.

## Decisions

### Four stable context classes: Compact (128k), Standard (272k), Extended (400k), Ultra (1m+) (decided)

These classes match the real breakpoints currently observed across Anthropic, OpenAI, Copilot, Codex, and local Ollama models. They are easy for operators to reason about and stable enough to survive upstream drift. Exact token counts remain internal metadata; the classes are the operator-facing and policy-facing abstraction.

### Routing state tracks both active context capacity and required minimum context floor (decided)

Safe routing requires distinguishing the capacity of the currently selected model from the minimum capacity the current session can safely tolerate. The state model tracks: activeContextWindow/activeContextClass, requiredMinContextWindow/requiredMinContextClass, optional pinned floor, observed usage/headroom, and downgrade safety arm/override status.

### Final operator taxonomy: context classes are Squad / Maniple / Clan / Legion; thinking levels are Servitor / Functionary / Adept / Magos / Archmagos / Omnissiah (decided)

Context names express formation scale and memory span (Iron Hands / Mechanicum blend). Thinking levels express cognitive sophistication (Mechanicum cognition ladder). This keeps context, thinking, and capability tier as three clearly distinct semantic axes.

### Downgrades classified as compatible, compatible-with-compaction, degrading, or ineligible (decided)

The harness compares the current session's required minimum context floor against concrete provider/model envelopes. Each candidate route is classified into one of four categories. This route-envelope classification becomes the basis for all automatic and manual downgrade behavior.

### Downgrade policy: auto-reroute when compatible, auto-compact in safe bounds, otherwise require operator confirmation (decided)

Prefer compatible reroute, then safe compaction, then operator-confirmed degradation. Unsafe context downshifts must never happen silently. Large multi-class drops (e.g. Legion to Squad) always require explicit operator confirmation.

### Routing selection starts from authenticated providers with opinionated default preference and operator override (decided)

Filter to providers the operator is logged into. Within that set, prefer Anthropic by default where all constraints are satisfied. Provider preference is user-configurable routing policy layered on hard feasibility checks, not a hidden override of safety constraints.

### Dangerous switches use explicit confirmation with durable 'don't ask again' overrides (decided)

Confirmation dialog shows current route, target route, context class delta, compaction consequences. Operator may approve once, cancel, or persist override. Persisted overrides remain visible and reversible, never bypass hard feasibility checks.

### Argo control plane refreshes provider metadata on schedule, emits reviewed route-matrix snapshots (decided)

Scheduled automation probes upstream catalogs, normalizes into a candidate route matrix, compares against current reviewed snapshot. Drift opens issues/PRs. Runtime consumes only the last reviewed local snapshot, never raw live upstream data.

### Refresh pipeline: collect → assess drift → promote reviewed snapshot (decided)

Stage 1 collects from authoritative sources. Stage 2 classifies deltas (additive, limit increase/decrease, route removal, breakpoint change, ambiguity). Stage 3 auto-promotes safe changes via PR, blocks risky changes (context decreases, removals) for human review.

## Research Summary

### Providers expose fixed route ceilings, not operator-selectable context sizes

Upstream providers and local runtimes generally do not let operators choose an arbitrary context size per request. Instead, each concrete route has a fixed offered envelope. Current observed examples: Anthropic API Claude Opus 4.6 = 1,000,000 and Claude Sonnet 4.6 = 1,000,000; OpenAI API GPT-5.4 = 272,000, GPT-5.4 Pro = 1,050,000, GPT-5.4 mini = 400,000; GitHub Copilot Claude 4.6 routes = 128,000, GitHub Copilot GPT-5.4 = 400,000; OpenAI Codex GPT-5.4 = 272,000; local Ollama models span 262,144 …

### Some providers have breakpoint zones inside a larger ceiling

Even where a route supports a large ceiling, provider docs expose important internal breakpoints. Anthropic docs indicate Claude 4.6 requests over 200k input tokens now work automatically without beta headers, making 200k a meaningful operational boundary even within a 1M route. OpenAI docs indicate GPT-5.4 long-context routes have a 272k breakpoint above which full-session pricing changes materially. These are not separate selectable context modes, but they are policy-relevant stability/cost zo…

### Control-plane requirement: track upstream drift without routing against live claims

Provider context metadata changes over time and sometimes regresses or diverges by transport. Pi/Omegon therefore should not fetch provider claims at request time and trust them blindly for routing. Instead, upstream state should be monitored on a schedule, compared against the checked-in local route matrix, and promoted only through a reviewed snapshot. This keeps operator behavior stable while still tracking a fast-moving provider ecosystem.
