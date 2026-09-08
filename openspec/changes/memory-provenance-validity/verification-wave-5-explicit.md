# Wave 5 explicit artifact admission

## Scope

Branch: `fix/memory-wave5-explicit-admission`.
Implementation and tests: `ef2a0e57`.
Explicit lifecycle claims require a matching structured statement in a bounded
repository artifact. Existing opsx parsers own Markdown interpretation. Memory
admission checks conclusion kind and lifecycle eligibility without changing the
lifecycle engine or its artifacts.

Typed attribution is encoded in the existing portable source string with the
`lifecycle-conclusion:v1:` prefix. Schema remains v11. Source validation records a
snapshot, not execution evidence or ongoing applicability. Candidate confirmation
and complete applicability remain open.

## Behavioral evidence

The live `lifecycle_explicit_claim_requires_artifact_evidence` test initially failed:
an unsupported assertion with only `authority: explicit` was accepted.

Host tests cover:

- Decisions with rationale, explicit constraints, and exclusion of open questions.
- Decided-node eligibility and rejection of unsupported/missing references.
- Baseline and dated archive specs, excluding active proposals and prose headings.
- Descriptor-relative symlink rejection and the 1 MiB artifact bound.
- Live correction, returned source identity/version, and stable-snapshot replay.

`tests/lifecycle_conclusions.rs` covers atomic correction, version conflict,
cross-mind rejection, exact-source reuse, preservation of different attribution,
receipt-failure rollback, reopen, and JSONL transport. Recall exposes versions for
correction preconditions.

The first design fixture had inconsistent frontmatter/section open questions and
was rejected by the parser diagnostics check. The fixture was corrected rather
than weakening artifact validation.

## Same-executor adversarial review

No independent reviewer is claimed. Reviewed fabricated explicit authority,
claim/artifact mismatches, proposal-stage chatter, path traversal, symlinked files
and directories, ambiguous requirement headings, oversized input, stale target
versions, cross-mind corrections, and dropped attribution through deduplication.

Artifact references are snapshot attribution, not executable instructions. The
caller supplies an expected correction version; the backend checks it inside the
same transaction as supersession and the operation receipt. Changed or unavailable
artifacts on tool retry are explicit errors; they do not delete prior evidence.

## Validation

Focused domain tests and 94 lifecycle-related host tests passed during iteration.
Final focused rechecks passed, including the three artifact-boundary tests and
both explicit live-tool tests. The final memory feature matrix and Clippy also
passed: default/all-features ran 93 unit and 43 integration tests; no-default-features
ran 89 unit and 38 integration tests. The existing schema generator remains ignored.
`RUST_TEST_THREADS=1 just test-commit` passed with 5,301 main-crate unit tests,
default integration suites, and memory tests. All five portable memory campaigns
passed. The final focused and feature-matrix rechecks cover the requirement-heading
restriction, exposed fact versions, and deterministic exact-source reuse added
during review. Named OpenSpec validation and whitespace checks passed.

This explicit-admission slice is accepted. No blocking finding remains in its
same-executor review. Candidate confirmation and complete applicability remain
open; the parent corpus is not archived.

Affected-crate and campaign transcript:
`~/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/sh_0835bdfce001nC0zq1eIJ4gYmn.out`.

Final matrix/Clippy transcript:
`~/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/sh_08362effb001u1tIMql1WiaAD8.out`.
