# Procedural learning design — planned contracts

## Ownership and baseline

Evidence boundary: `7b2da402`, with findings in [assessment.md](assessment.md).
Root directives and actual owner contracts apply. There is no local
`omegon-skills/AGENTS.md` at this baseline.

- `omegon-memory`: procedure/application vocabulary, persistence, reconciliation,
  applicability, selection, budget accounting, and cache invalidation. Extend
  `types.rs`, `backend.rs`, SQLite, and in-memory implementations consistently.
- `omegon-skills`: YAML/TOML parsing, strict promotion diagnostics, provenance,
  activation metadata, and disclosure. Preserve tolerant inventory loading.
- `omegon`: canonical runtime evidence, operator approval, guarded filesystem
  composition, command registration, and semantic surfaces consumed by frontends.
- `omegon-traits`: shared frontend contracts when required by integration.

Reuse `features/skills.rs`, `skills.rs`, and `contribution_loading.rs`. Existing
`force` writes and optional manifest `version` are not compare-and-swap semantics.
The `/skills` registry entry does not establish create/import remote parity:
`control_runtime.rs` returns `None` for those TUI prompt routes.

## Procedure and application lifecycle

A procedure has a stable ID and immutable versions. Each version records task
class, prerequisites, environment/tool constraints, steps or source references,
expected postconditions, verification method, known failures, and supporting and
counterexample evidence. Content, scope, and evidence changes produce a new
version. Retirement and supersession preserve historical versions and feedback.

Keep `hypothesis`, `evidenced guidance`, and `retired` states distinct. A verified
run supports only its evidenced scope. Failed attempts can support cautions and
repair hypotheses, but cannot establish successful guidance. Automatic admission
and correction wait for accepted Wave 6 reconciliation. Unknown or truncated
sources retain explicit reasons and cannot silently become complete evidence.

Each actual application has its own ID, separate from procedure/version and
selection IDs. Record selected, attempted, completed, and verified-task-success
events separately. No state implies the next. A denied or undispatched tool call
does not prove an attempt executed. A successful tool disposition does not prove
workflow postconditions. Verification has its own run ID, method, expected
postconditions, result, and source references; uncertainty is an observable result.

Bind application feedback to the selected procedure version, observed environment
and tool versions, canonical session/stream/event identities and sequences, and
verification run. Later verification may occur in another stream, with an explicit
application link. Version feedback records and retain corrections instead of
rewriting earlier judgments. Importing the same canonical execution twice does
not create independent applications or increase support. Conflicting claims for
one execution remain a reconciliation conflict.

Retention can remove source payload only under the frozen policy contract. Keep
source identity and an unavailable/expired reason; invalidate affected claims and
selection caches as required. Usage counts never replace evidentiary support.

## Selection and disclosure

Use existing applicability and shared selection. Required platform/tool-version
constraints fail closed when observations are missing or unknown. A scoped
hypothesis may be inspected as uncertain material, but is not directly applicable
execution guidance. Retirement excludes active guidance while preserving history.

Charge the entire rendered unit: identity/version, provenance, applicability,
prerequisites, steps, failure conditions, and validation. If it does not fit, omit
it with a reason rather than stripping prerequisites or validation. Cache keys
include procedure/evidence revisions, retirement/validity state, policy version,
task and environment observations, budget/rendering version, and admitted skill
snapshot. Deduplicate a procedure and its promoted skill using promotion lineage
and artifact identity. A human-edited artifact invalidates equivalence and requires
fresh inspection; a stale lineage link cannot suppress or bless changed content.
Composition uses existing memory and skills disclosure owners, not another injector.

## Explicit promotion and shared surfaces

First slice is a prompt-only artifact at an absent project-local destination under
`.omegon/skills/`. Reject replacement, user/global scope, bundled-skill mutation,
and executable assets as unsupported. Never map a conflict to `force=true`.

Freeze these semantics and their versioned DTOs before parallel host integration:

1. **Plan:** create an immutable, inspectable proposal without destination writes.
   Include operation ID, canonical input hash, exact artifact hash/bytes,
   procedure ID/version, evidence revisions/eligibility, validation-policy version,
   target workspace/scope identity, guarded relative destination, and expected
   absence. The precondition vocabulary also defines expected content hash for a
   future replacement slice; this slice rejects that mode.
2. **Inspect:** show exact content, provenance, prerequisites, diagnostics, scope,
   excluded assets, preconditions, and operation state through one semantic result.
3. **Approve/apply:** only an explicit operator action can approve this operation
   ID and plan digest. Agent suggestion, selection, extraction, or a tool call
   alone is not approval. Apply verifies the approval binding, input/artifact
   hashes, procedure/evidence preconditions, scope identity, and target absence
   again at the guarded mutation boundary. Changed inputs require a new plan and
   approval. Non-interactive callers must supply the same operator authorization.
4. **Result:** return stable operation identity, state, artifact identity, validation
   diagnostics, conflict/unavailable reason, and replay/recovery receipt. Equal
   operation ID and inputs replay the receipt; unequal inputs conflict.

Select the concrete registered command spelling and approval transport at the
interface-freeze gate. TUI, CLI remote, and ACP consume the same plan/apply/inspect
and result contracts. Unsupported routes return explicit unavailable results,
not a successful no-op or an unreviewed prompt. Test actual routing, not registry
metadata alone.

Strict diagnostics belong to the existing skills owner. Parse success is not
validation: malformed YAML/TOML can currently recover to defaults. Promotion
requires well-formed frontmatter, required fields, supported activation metadata,
consistent provenance, and complete prerequisites/verification content. Preserve
source references without inventing source URLs. Failed diagnostics block writes.

Promotion approval permits only the reviewed artifact write. It grants no trusted
paths, executable trust, global installation, or ambient instruction authority.
Successful creation does not itself grant admission or force activation. Normal
reload and activation use the existing admitted-snapshot and disclosure policy.
Setup already requires operator
admission for requested external paths, despite older auto-trust comments.

## Filesystem/database recovery

The filesystem and memory database cannot commit in one SQL transaction. Persist
an operation journal before publication and make each database state transition
atomic. Planned states distinguish prepared, published, recorded, and conflict.
Use guarded no-clobber publication with operation-owned staging and a durable
publication identity. Equal content alone is not proof that this operation wrote it.

- Before publication: recover the stored intent and recheck all preconditions.
  If still valid, resume without new approval only for the exact approved plan.
- After publication, before the database receipt: verify durable publication
  identity and artifact hash, then forward-repair the receipt and lineage.
- If a human created or changed the destination: record a conflict and leave all
  human bytes unchanged. Do not delete or restore the old artifact.
- If publication ownership cannot be proven: report unresolved recovery. Do not
  infer ownership from matching bytes or repeat an unsafe write.
- After completion: replay returns the same receipt without writing. Inspection
  separately reports whether the current artifact has changed or disappeared.

Freeze publication-marker ordering, durability, and guarded target identity with
the owner before coding. Exercise crash boundaries around journal, publication,
and receipt writes, including reopen and human edits between those stages.

## Dependencies, parallel work, and quality

Wave 5 is accepted. Wave 6 acceptance gates automatic admission/correction. Obtain
the explicit Wave 7 contract for evidence versus usage, verification, validity,
retention, unknown-source preservation, and versioned feedback. Wave 8 does not
depend on the complete Wave 7 scheduler.

After interface freeze, procedure/storage work, strict diagnostics, recovery
design, and fixtures can proceed in parallel within their owners. Parallel host
adapters start only after operator-promotion and shared result contracts freeze.

Reuse Wave 5 evaluation infrastructure with new actual-application tasks and
nonzero repeated-failure opportunities. Include negative transfer, unfamiliar tool
versions, withheld environments, unused retrievals, partial executions, and false
tool-success cases. Attribute outcomes to formation, reconciliation, selection,
application, verification, and promotion. Report denominators, independent
executions, task success, repeated errors, unknown judgments, and resource use.
Wave 5's zero repeated-error smoke matches establish no improvement denominator.

Freeze development-derived acceptance thresholds before held-out evaluation.
Record a fresh operator-authorized model/token/time/cost budget before live runs;
do not reuse Wave 5 authorization or invent numeric thresholds. Deterministic
fixtures can establish contracts without claiming model-quality improvement.
