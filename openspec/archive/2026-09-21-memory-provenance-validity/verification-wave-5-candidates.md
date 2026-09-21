# Wave 5 inferred lifecycle candidates

## Scope

Branch: `fix/memory-wave5-lifecycle-candidates`.
Implementation and tests: `1096d870`.
The inferred lifecycle route now stores a pending fact rather than established
knowledge. Schema v11 adds typed declared-reference metadata, with v5–v10 migration.
Recorded time is the fact creation timestamp. References are attribution supplied
by the caller, not validated source observations.

Validated explicit admission, confirmation, applicability, and correction mutations
remain open. This verification slice does not establish completion of Wave 5.

## Behavioral evidence

The live `lifecycle_inference_is_pending_and_not_recalled_as_knowledge` test failed
against the previous route and passed after pending admission was implemented.

`tests/lifecycle_candidates.rs` verifies:

- Candidate replay and declared artifact/proposed-supersession retention.
- Identical active facts are not reinforced by inferred candidate admission.
- Current/historical recall, pinned rendering, and embedding writes exclude candidates.
- JSONL preserves pending metadata and cannot promote an existing candidate by
  relabeling it or stripping that metadata.
- Schema-v10 migration, legacy unknown preservation, reopen, and receipt replay.
- Missing candidate attribution is a corruption error.
- Receipt-write failure rolls back the candidate; repeated vault publication does
  not include pending content.

## Adversarial review

Same-executor review; no independent reviewer is claimed. Reviewed candidate
promotion through import, deduplication against active facts, legacy embedding APIs,
missing persisted attribution, and unconfirmed supersession. Proposed supersession
remains metadata and never changes the target fact. References are not read as paths.

## Gates

All final gates passed:

- `just test-crate omegon-memory` and `cargo test -p omegon-memory --all-features --locked`: 93 unit and 40 integration tests; the schema generator remains ignored.
- `cargo test -p omegon-memory --no-default-features --locked`: passed.
- `RUST_TEST_THREADS=1 just test-commit`: 5,296 main-crate unit tests, default integration suites, and memory tests passed.
- All five opt-in `memory_campaign` tests passed.
- `just clippy-changed`: both affected crates, all targets, passed.
- Named OpenSpec validation and whitespace checks passed.

The first vault test incorrectly expected an index file for an otherwise empty
vault. Its assertion was corrected to verify zero facts published, no Decisions
file, and zero changed files on repeat. This was a fixture correction, not a
production defect.

The inferred-candidate slice is accepted. No blocking finding remains in its
same-executor review. The parent corpus remains implementing with its other
Wave 5 tasks open.

Local execution transcripts under
`~/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/`:

- `sh_0831710ab001mDqLWPDa6v7Qvd.out`: live-tool behavioral red.
- `sh_0831c2791001164cAftMHaPZDr.out`: focused domain and live-tool green.
- `sh_0831fc570001ad5favk96PWSJO.out`: final feature matrix, affected-crate tests, portable campaigns, and Clippy.
