# One common agent policy

## Intent

Consolidate Full, Slim, and Constrained into one compact, host-owned base policy.
Preserve useful obligations and scoped contracts instead of deleting instruction
bundles by size. The operator endorsed this direction and requested a specification
and landmine review, then said **begin** after reviewing the preservation-led spec.
The recommended policy and compatibility decisions are accepted for implementation.

Observed source revision: `4733fb4b2c36ee7078911e4e6c5a540dbfb63c69`.
The [design node](../../../docs/unified-system-prompt.md) remains `exploring`.
Implementation and bounded acceptance are complete on `feat/unified-system-prompt`.
The node's native lifecycle transition remains pending; no native tools are available.

## Scope

- Replace model/posture-selected prompt families with one canonical core, targeting
  approximately 150–200 words. This is a readability target, not a truncation limit.
- Account for every instruction category in the [preservation ledger](preservation-matrix.md).
  Remove duplicate core injection and unrequested universal workflow/style mandates.
- Recommend complete, attributed global and project instructions on every common
  agent route. Existing-but-unreadable global instructions become a preparation
  error, rather than silently disappearing.
- Replace default commit/publication timing with scope established by the operator
  or an applicable authorized workflow. Preserve child harvesting contracts.
- Preserve request evidence, route/resource policy, permissions, tool admission,
  content-pack boundaries, and purpose-specific auxiliary prompts.

The existing [request-preparation contract](../unified-model-request-contract/specs/inference/preparation.md)
and [resolved route policy](../resolved-inference-policy/specs/inference/route-policy.md)
remain requirements. This work introduces no new request envelope or persistence
schema by default. Git consent, index ownership, hooks, signing, and validation
receipts remain in the separate local Git concern, `docs/git-execution-boundaries.md`.
That uncommitted review artifact is outside this change's publication scope.

## Success criteria

- All common agent routes emit the same core identity and bytes for the same
  explicit inputs, independent of model grade, posture, or presentation.
- The core occurs once by source identity, including when persona injection is
  admitted. No hidden Lex bundle or renamed prompt family restores the old policy.
- Required instructions remain complete and attributed. A route that cannot fit
  mandatory input fails before dispatch under the existing accounting policy.
- Every retained or relocated rule has an owner and verification target. Tool
  procedures remain usable through actual tool contracts, not a replacement
  universal behavior appendix.
- Historical requests remain readable and immutable. New requests identify the
  current core without misrepresenting earlier captures.

## Review package

[Design and proposed core text](design.md) ·
[Preservation ledger](preservation-matrix.md) ·
[Common-policy delta](specs/prompt/common-policy.md) ·
[Instruction-loading delta](specs/harness-parity/opencode2.md) ·
[Host-authority/content-pack delta](specs/runtime-contributions/content-packs.md) ·
[Verified implementation tasks](tasks.md).

The three recommended decisions are accepted: exact compact policy and semantic
retirements, complete global loading with existing source order, and the narrow
host-core/content-pack exception with deliberate augmentation migration.
See [review decisions](design.md#review-decisions) and the
[implementation record](implementation.md).

Artifact validation is separate from runtime acceptance. The implementation record
reports checks actually run. Existing native lifecycle closure gaps remain open.
