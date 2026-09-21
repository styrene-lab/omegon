# Wave 5 selection evaluation

The shared evaluator and raw reports are documented in
[Wave 5D verification](../2026-09-21-memory-evaluation-corpus/verification-wave-5.md).

## Policy ablation

`omegon-memory/tests/evaluation_corpus.rs` compares the shipped selector at a
1,024-byte-accounted cap with a candidate 256-byte cap. Both use the same query,
source evidence, candidate parser, lexical retrieval, and whole-block packer.
Each arm reports its cap and complete emitted byte count. This is an alternative
budget policy, not a claim that production policy changed to 256.

In `budget_ablation_preserves_small_evidence_after_oversized_lexical_noise`, the
larger cap includes a long lexical distractor. The smaller cap skips that complete
block and retains the small required source. The test verifies less injected
content with the same required evidence on SQLite and in-memory backends.

The held-out synthetic report contains four tasks per arm. File search and both
memory arms achieve four supported scripted outcomes. No memory succeeds on the
abstention case only. The two memory caps inject 341 accounted bytes each across
the four tasks; those small cases alone show no size advantage. The explicit
noise ablation supplies the budget-pressure case.

The final live reader/extractor comparison used the existing Codex subscription
with `openai-codex:gpt-5.6-luna` for both roles. All 16 held-out arm/case executions
completed. File search and both memory arms achieved 4/4 successes and full required
evidence recall; no memory achieved 1/4. Both memory caps injected 342 accounted
bytes across the live held-out cases. No stale-memory, repeated-error, or uncertain
judgments occurred. The fixed smoke thresholds passed without a candidate regression.

The full development and held-out experiment used 7,541 tokens and 149.616 seconds,
including prior authentication attempts. Original unavailable reports remain
preserved. See [Codex continuation](../2026-09-21-memory-evaluation-corpus/codex-continuation.md)
for exact accounting, frozen configuration, independent-review corrections, and
credential cleanup. The small extractive corpus does not establish general model
superiority or justify changing the routine cap.

## Runtime and inspection mapping

- `token_selection_standalone_and_hosted_ambient_agree` covers runtime-context
  parity across standalone and hosted adapters.
- `token_selection_intersects_profile_cap_and_host_allocation` covers the
  operator-configured cap and host allocation boundary.
- `inspection_reports_unavailable_declared_source_without_mutation`,
  `inspection_tracks_artifact_availability_without_reactivating_history`, and
  `inspection_does_not_validate_declared_references_or_follow_unsupported_paths`
  cover non-interactive inspection behavior.
- `verification-wave-5-selection.md` records previously accepted full selection
  gates. `verification-wave-5-cache.md` records accepted cache gates.

After parent integration, the `token_selection` filter passed all three tests.
The `features::memory::tests::inspection` filter also passed all three tests.
This closes the remaining scenario/evaluation task. The parent owns combined
shared-tree gates and joint archival. No archive was performed by this agent.
