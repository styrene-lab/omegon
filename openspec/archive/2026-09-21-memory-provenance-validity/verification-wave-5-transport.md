# Wave 5 transport prerequisite

## Scope

Branch: `fix/memory-wave5-transport-history`.
Implementation and tests: `5bc5dfd6`.
This slice preserves existing historical and operational data before richer
provenance and validity fields are introduced. Schema remains v10.
Lifecycle admission, applicability, checkpoint recovery, shared context selection,
and comparative evaluation remain open in their parent corpora.

## Red–green evidence

`transport_preserves_history_and_operational_state_across_backends` initially failed:
an export of four status populations contained only the active fact.
Modern JSONL now carries an optional `operational` object and exports all statuses.
The test compares every source/destination backend combination and repeated imports.

| Guarantee | Test in `tests/transport_history.rs` |
|---|---|
| All status populations, metadata, unknown source, idempotency | `transport_preserves_history_and_operational_state_across_backends` |
| Legacy omission preserves state; newer modern input replaces it | `legacy_updates_preserve_known_operational_state_and_modern_updates_replace_it` |
| Invalid numeric/timestamp metadata rolls back records and receipts | `invalid_operational_state_rolls_back_batch_and_receipt` |
| Historical endpoints, correction link, reopen, corrupt-row export failure | `history_edges_and_operational_state_survive_reopen_and_failed_export_is_not_partial` |
| Corrupt lifecycle status is not promoted to active | `corrupt_history_status_is_not_reinterpreted_as_active_knowledge` |

## Adversarial review

Same-executor review; no independent review is claimed.

- Equal and older Lamport versions remain no-ops even when operational values differ.
- New legacy records retain existing initialization defaults. Missing operational
  metadata on updates cannot reset known confidence or freshness.
- Numeric and timestamp validation occurs within atomic import behavior. Failed
  imports do not occupy an operation receipt identity.
- SQLite's non-null source column uses an empty value for unknown imported source.
  Reads map this to `None`; import does not fabricate a `manual` source label.
- Fact export propagates read failures instead of omitting corrupt historical rows.
- The corrupt-status test reproduced SQLite's fallback to `Active`. Unknown stored
  statuses now produce a conversion error instead of relabeling historical facts.
- Historical fact inclusion repairs same-mind exported correction-edge endpoints.

## Validation

The final five transport tests passed. The final source also passed
`just test-crate omegon-memory`, all-features tests, and no-default-features tests.
Default/all-features ran 93 unit and 37 integration tests; no-default-features ran
89 unit and 32 integration tests. The existing schema generator remains ignored.

`RUST_TEST_THREADS=1 just test-commit` passed: 5,295 main-crate unit tests,
the default integration suites, and the memory crate tests. All five opt-in
`memory_campaign` tests passed after rebuilding against the final domain changes.
The final memory feature matrix covers the additional corrupt-status and empty-source
hardening. `just clippy-changed` passed for both affected crates and all targets.
OpenSpec validation and whitespace checks passed.

The foreground Clippy recheck exceeded its wrapper deadline without a compiler
diagnostic. The background recheck completed successfully; the timeout is not
reported as a compiler failure.

The transport slice is accepted. No blocking finding remains in this slice's
same-executor review. Wave 5 remains implementing, and the parent corpus is not archived.

Local execution transcripts under
`~/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/`:

- `sh_082ee222c001nbD78uefI1BBCW.out`: corrupt-status behavioral red.
- `sh_082ece983001NImi79xQX8vd4L.out`: affected-crate landing run, five campaigns, and Clippy.
- `sh_082f3c469001IMHcfAXlb8XxPF.out`: final Clippy recheck.
