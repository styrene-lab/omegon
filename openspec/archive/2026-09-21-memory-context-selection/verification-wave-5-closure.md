# Context-selection closure audit

**Accepted on 2026-09-21.** The evaluation owner completed ablations and the live
Codex smoke comparison. Final workspace gates and all remaining tasks passed;
see [joint acceptance](../../memory-wave-5-verification.md).

Audited on 2026-09-21 in `feat/memory-wave5-completion`. This audit owns scenario
mapping and inspection-surface verification. Comparative policy ablation belongs
to the evaluation executor; its task checkbox is deliberately left unchanged.

Paths below are relative to `core/crates/`. Accepted selection and cache records
retain the original behavioral RED results and host landing evidence.

| Scenario | Executable evidence |
|---|---|
| Relevant constraint survives Architecture flood | `omegon-memory/tests/initial_waves.rs::tools::task_context_excludes_unrelated_architecture_flood`; applicability before-limit test |
| Equivalent inputs across adapters | Host `features/memory.rs::token_selection_standalone_and_hosted_ambient_agree`; accepted explicit `request_context` pack parity test |
| Pinned fact also retrieved | `initial_waves.rs::packing_deduplicates_pins_and_recalled_facts`; `token_selection.rs::selection_reports_distinct_eligibility_reasons_and_deduplicates_pins` |
| Pinned fact superseded | `token_selection.rs::low_signal_is_pin_only_and_superseded_pins_resolve_without_revival` |
| Task changes before TTL expiry | `selection_cache.rs::cache_key_covers_task_target_pins_policy_and_budget_and_clock_reversal`; host `initial_wave_empty_task_selection_replaces_live_ttl` |
| Selected fact archived before TTL expiry | `selection_cache.rs::cache_skips_retrieval_and_expires_for_previously_excluded_future_facts`; external commit test |
| Unchanged turn reuses bounded selection | Same counted-closure cache test; host `selection_cache_reuses_an_unchanged_hosted_turn` |
| Previously excluded fact becomes eligible | Same future-boundary test |
| Selected fact reaches validity end | `selection_cache.rs::cached_guidance_retires_at_exclusive_valid_until` |
| Another SQLite connection changes memory | `selection_cache.rs::sqlite_external_commits_and_backend_instances_invalidate_cache` |
| Multilingual content respects allocation | `token_selection.rs::conservative_packing_counts_unicode_metadata_and_skips_oversized_claims`; `exact_counter_receives_complete_emitted_text`; host `token_selection_never_exceeds_conservative_host_budget` |
| Oversized candidate does not starve smaller fact | Same conservative packing test; `initial_waves.rs::packing_skips_oversized_first_candidate` |
| Low-signal turn has no relevant episode | `low_signal_is_pin_only_and_superseded_pins_resolve_without_revival`; `episodic_additions_require_signal_and_a_bounded_share` |
| Operator inspects constrained selection | `selection_reports_distinct_eligibility_reasons_and_deduplicates_pins`; host/standalone parity test invokes the public `memory_selection` tool and compares all semantic fields |

## Runtime and non-interactive inspection

The shared service returns a `MemorySelectionReport`; both adapters expose it
through `memory_selection`. The accepted host parity test invokes both public
tool surfaces and compares selected handles, exclusion counts/reasons, accounting,
budget exhaustion, pin resolution, and retrieval degradation. This is executable
non-interactive inspection evidence, not an assertion based only on renderer text.

`MemoryFeature::provide_context` stores the report from shared selection and uses
explicit empty replacement to retire stale injected content. `request_context`
uses the managed selection request. The accepted counted-selection and cache
records cover host allocation/profile cap intersection and runtime TTL retirement.
Reports describe selector inputs; they do not claim an exhaustive inventory of
facts filtered upstream. Conservative byte accounting is labeled conservative;
the exact-counter fixture does not claim a production tokenizer measurement.

## Validation and readiness

The current closure-domain run passed 61 tests, including all six token-selection
and nine initial-wave tests. See the sibling provenance closure record for the
exact command. Host parity and cache evidence additionally come from the accepted
selection/cache gates; the parent owns combined main-crate revalidation.
`cargo test -p omegon-memory --lib selection_cache --locked` also passed all seven
cache tests on the current worktree.
Named OpenSpec validation passed (`implementing`), and scoped whitespace checks
passed.

No uncovered selection scenario was found. The aggregate task remains open for
the evaluation executor's comparative policy ablation and the parent's combined
validation. No task owned by that executor was checked, and nothing was archived.
