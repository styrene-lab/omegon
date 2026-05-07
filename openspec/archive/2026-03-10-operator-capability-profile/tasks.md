+++
id = "ae2108e7-6283-4f42-9a90-918b286e83d3"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# operator-capability-profile — Tasks

Dependencies:
- Group 1 defines the schema and config/runtime persistence used by all later groups.
- Group 2 builds the resolver on top of Group 1.
- Group 3 integrates bootstrap/setup prompts after Groups 1-2 exist.
- Group 4 wires runtime failure handling and fallback behavior after Group 2.
- Group 5 updates copy/tests/docs after implementation details settle.

## 1. Profile schema + config/runtime persistence
<!-- specs: models/profile -->

- [x] 1.1 Add operator profile types and defaults in a new `extensions/lib/operator-profile.ts`
- [x] 1.2 Define the full public role ladder: `archmagos`, `magos`, `adept`, `servitor`, `servoskull`
- [x] 1.3 Define structured candidate objects with `id`, `provider`, `source`, `weight`, and `maxThinking`
- [x] 1.4 Define fallback policy enums using `allow | ask | deny`, with comments leaving room for future values like `allow_once` and `background_only`
- [x] 1.5 Load and save durable operator profile state through `.pi/config.json` without regressing existing `lastUsedModel` behavior
- [x] 1.6 Add a separate runtime-state helper for transient machine/provider availability and cooldown data
- [x] 1.7 Add tests for profile parsing, conservative default synthesis, and config round-tripping

## 2. Role resolver + selection policy
<!-- specs: models/profile -->

- [x] 2.1 Add a resolver that maps requested roles or task aliases onto ordered public role candidates
- [x] 2.2 Filter candidates by provider enablement, auth/capability availability, local presence, and cooldown state
- [x] 2.3 Enforce per-candidate `maxThinking` ceilings when selecting a candidate
- [x] 2.4 Model fallback boundaries explicitly: same-role, cross-provider, cross-source, heavy-local, unknown-local-performance
- [x] 2.5 Return structured outcomes for `allow`, `ask`, and `deny` instead of inventing candidates outside the profile/default profile
- [x] 2.6 Integrate resolver use with existing model selection helpers (`model-routing`, `model-budget`, or a new shared bridge as appropriate)
- [x] 2.7 Add tests covering overlapping tiers, servoskull thinking-off enforcement, and conservative cross-source behavior

## 3. Bootstrap + setup capture
<!-- specs: models/profile -->

- [x] 3.1 Extend bootstrap to detect missing operator profile state separately from dependency installation
- [x] 3.2 Add first-run/new-machine prompts that capture qualitative local policies, provider preferences, and setup completion state
- [x] 3.3 Make skipped setup synthesize and persist or expose a safe default profile without forcing local benchmarking
- [x] 3.4 Reuse existing auth/provider checks where possible so setup can reflect current upstream readiness without duplicating logic
- [x] 3.5 Add tests for bootstrap/profile setup flows and skipped-setup graceful degradation

## 4. Runtime failure handling + guarded fallback behavior
<!-- specs: models/profile -->

- [x] 4.1 Classify transient upstream failures such as Anthropic 429s and OpenAI session-limit exhaustion as temporary capability loss
- [x] 4.2 Record cooldown windows in runtime state for failed candidates/providers and skip them during subsequent resolution
- [x] 4.3 Update offline/fallback paths so upstream-to-local transitions consult operator policy before switching to heavy or uncertain local candidates
- [x] 4.4 Surface operator-facing explanations when resolution requires confirmation or is denied by policy
- [x] 4.5 Add tests for same-role cross-provider retry, cooldown expiry behavior, and blocked heavy-local fallback

## 5. Naming, copy, docs, and lifecycle cleanup
<!-- specs: models/profile -->

- [x] 5.1 Replace stale frontier/local role copy in affected design/runtime messages with the public ladder terminology
- [x] 5.2 Update any existing comments or docs that still imply hidden/private capability tiers
- [x] 5.3 Add or update documentation describing the operator profile schema, role semantics, and fallback policy model
- [x] 5.4 Reconcile design/OpenSpec artifacts so superseded assumptions (`frontier.*`, reduced public role set) no longer appear as current implementation guidance
