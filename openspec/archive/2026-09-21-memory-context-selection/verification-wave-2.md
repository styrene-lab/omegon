# Wave 2 verification — immediate context selection repairs

See [Wave 1](../2026-09-21-memory-retrieval-contract/verification-wave-1.md) for source identity,
fixture ownership, toolchain execution, and shared landing checks.

## Scoped behavior

- Shared early task-matched fact selection for standalone and managed ambient paths.
- Current eligibility and mind-scoped pins.
- Preserve selected order rather than prioritizing a presentation section.
- Deduplicate fact identities and continue packing after an oversized candidate.
- Explicitly clear a previous managed injection when task selection becomes empty.

Full token accounting, rich applicability, complete adapter parity, pin replacement
explanations, and selection caching remain pending. Character accounting preserves
the current host conversion boundary; four characters per token is not asserted to
be a universally safe token bound.

## Scenario evidence

| Behavior | Test | Red | Green |
|---|---|---|---|
| Duplicate pins and recalled fact | packing_deduplicates_pins_and_recalled_facts | Three entries instead of one | Passed |
| Oversized first fact | packing_skips_oversized_first_candidate | Useful smaller fact omitted | Passed |
| Task-matched constraint under flood | tools::task_context_excludes_unrelated_architecture_flood | Unrelated Architecture content remained | Passed |
| Task change with no matches | features::memory::tests::initial_wave_empty_task_selection_replaces_live_ttl | None retained prior live injection | Passed |

The flood test also supplies an active foreign-mind pin and an archived pin; neither
may appear in the selected block. Existing managed tests cover pin order, dirty
refresh, TTL refresh, priority, and render-budget bounds.

## Adversarial review

Same-executor review; no independent review claimed.

Blocking finding: the managed provider returned None for an empty new selection,
which means retain the old TTL, not remove old content. Fixed with an explicit empty
source replacement. Inspected `ContextManager::inject_external` to confirm it
replaces prior content by source. The new red/green test checks the provider result.

The standalone legacy context provider's broader host TTL/replacement convergence
remains Wave 5 scope. This wave does not claim that every external host consumes
legacy ContextInjection objects identically.

The corrected existing tiny-budget test now requires an empty replacement on the
first too-small budget and permits None only after the prior injection is cleared.
The resumed runtime memory suite passes 104 tests. The source/fixture candidate is
`f6846622`; shared landing results are recorded in the Wave 1 document.

G0–G4 passed for this scoped slice at `f6846622`, including the serial affected-crate
gate and Clippy. G3 was same-executor review. The parent context-selection corpus
remains implementing for its remaining Wave 5 requirements; it is not archived.
Recovery is a code-policy revert; this slice does not mutate persistence schema.
