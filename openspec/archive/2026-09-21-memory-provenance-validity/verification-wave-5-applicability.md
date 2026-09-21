# Wave 5 applicability

## Scope

Branch: `feat/memory-wave5-applicability`.
Implementation and tests: `869d1d8c`.
Schema v13 records explicit applicability rules with their recorded time. Current
retrieval applies known constraints before candidate limits and discloses unknown
applicability. Historical valid-time queries preserve the recorded history and its
current lifecycle label; they do not reconstruct old database knowledge versions.

Workspace identity follows the existing runtime path-based ID. Revision is exact
Git HEAD identity, independent of dirty working-tree contents. Missing descriptors
remain unknown. These identities are selection context, not authorization grants.

## Tests and red–green evidence

The first platform regression returned `bad-00` rather than the eligible fact. It
now passes with an excluded population larger than the previous over-fetch budget.

`tests/applicability.rs` covers:

- Platform filtering before lexical/vector limits.
- Graph scope filtering before edge limits.
- Inclusive/exclusive validity boundaries, nanosecond comparisons, and timezone offsets.
- Historical retrieval retaining recorded time without reinforcement.
- Missing target context versus known workspace/revision/component mismatch.
- Scope updates, optimistic conflicts, replay, legacy omission, and transport.
- Exact-scope deduplication across both backends.
- Schema-v12 migration, failed-write rollback, reopen, and vault scope projection.

Host tests cover fresh HEAD reads, unchanged HEAD under dirty working-tree edits,
same-turn context retirement after a HEAD change, and expiry through a versioned
scope update without reinforcement. A failed eligibility read also retires the
ephemeral injection while preserving stored facts. The standalone provider has a
matching scoped-store/update/context-retirement test.

## Review boundaries

Same-executor review; no independent review is claimed. Scope matching does not
establish truth or validate unrecorded conditions. Inventory and explicit inspection
can show inapplicable records as stored data. Current search/context channels filter
them. Record scope does not automatically transfer to a distinct replacement claim.

Scoped JSONL uses a distinct tag for compatibility. Old readers do not support these
records; use a scope-aware binary rather than downgrading over a v13 store or export.
Vault section pages retain readable scope metadata and remain projections, not a
second lossless fact codec.

## Gates

All final gates passed:

- `just test-crate omegon-memory` and all-features tests: 93 unit and 60 integration tests; the schema generator remains ignored.
- No-default-features memory tests: 89 unit and 53 integration tests.
- Three focused hosted applicability tests.
- `RUST_TEST_THREADS=1 just test-commit`: 5,313 main-crate unit tests, default integration suites, and memory tests.
- All five portable memory campaigns.
- `just clippy-changed` for both affected crates/all targets.
- Named OpenSpec validation and whitespace checks.

The older confirmation migration fixture was updated to expect the current schema
and to remove the new applicability column when constructing its v11 source.
This preserved the historical fixture rather than weakening migration coverage.

The applicability slice is accepted. No blocking finding remains in its
same-executor review. Wave 5's token-aware selection/cache, checkpoint scheduling,
and comparative evaluation work remain open; the parent corpus is not archived.

Final gate transcript:
`~/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/sh_08f27c2c20018h5FIILc35WuIe.out`.
