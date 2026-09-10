# Wave 5 provenance inspection

## Scope

Branch: `feat/memory-wave5-provenance-inspection`.
Implementation and tests: `147db2fb`.
The shared `FactInspection` projection serves standalone and hosted `memory_inspect`
tools. Exact lookup is status-neutral and mind-scoped. The host checks supported
artifact references through the existing descriptor-relative reader. Schema remains
v12; inspection introduces no persistence mutation or migration.

This slice completes the read-only source-availability path. Applicability policy
and the remaining Wave 5 foundations remain open.

## Red–green and scenario mapping

The live missing-source test initially failed because `memory_inspect` was absent.
It now reports the pending inference and unavailable declared source without
changing the stored record.

| Guarantee | Test |
|---|---|
| All five statuses, mind isolation, unchanged reinforcement | `inspection_is_status_neutral_mind_scoped_and_does_not_reinforce` |
| Unicode preview bounds, full-content digest, invalid source metadata | `inspection_bounds_unicode_previews_and_reports_invalid_attribution` |
| Confirmed inference retains its evidence class | `confirmed_inference_is_not_reclassified_as_explicit_artifact_evidence` |
| Shared standalone provider projection | `standalone_inspection_uses_shared_projection` |
| Unavailable declared source does not mutate memory | `inspection_reports_unavailable_declared_source_without_mutation` |
| Matching/changed/missing snapshots and symlink rejection retain history | `inspection_tracks_artifact_availability_without_reactivating_history` |
| Readable declarations remain unverified; unsafe paths are unsupported | `inspection_does_not_validate_declared_references_or_follow_unsupported_paths` |

## Same-executor review

No independent review is claimed. Reviewed source-label authority spoofing,
inspection of inactive records, reference availability versus claim validity,
symlink/path boundaries, Unicode excerpt truncation, malformed source envelopes,
case-insensitive SHA256 representation, and accidental reinforcement through reads.
Inspection reports versions and stored timestamps; it does not invent missing
evidence or local observation times. File checks describe current readability or
snapshot equality, not continuous source stability or execution evidence.

## Gates

All gates passed:

- `just test-crate omegon-memory` and all-features tests: 93 unit and 51 integration tests; the schema generator remains ignored.
- No-default-features memory tests: 89 unit and 45 integration tests.
- `RUST_TEST_THREADS=1 just test-commit`: 5,310 main-crate unit tests, default integration suites, and memory tests.
- All five opt-in portable memory campaigns.
- `just clippy-changed` for both affected crates and all targets.
- Named OpenSpec validation and staged whitespace checks.

The inspection slice is accepted. No blocking finding remains in its same-executor
review. The parent corpus remains implementing; applicability is still open.

Local transcripts under
`~/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/`:

- `sh_08d1d0342001psX7dkkSE7hJd0.out`: live missing-source behavioral red.
- `sh_08d28aa3100106Y8gOd71d9Bu2.out`: focused domain/provider and host green.
- `sh_08d2b2c53001veJNYrL2uuTVks.out`: final feature matrix, affected-crate gate, campaigns, and Clippy. The same process continued through the conversation interruption.
