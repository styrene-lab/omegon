# Memory modernization execution plan

## Objective and status

Ship the highest-impact, lowest-effort improvements first. Each wave must demonstrate
correct behavior, survive adversarial review, and preserve durable evidence.

The [corpus map](memory-modernization.md) owns effort estimates and dependencies.
The nine change corpora own requirements and implementation tasks. This plan orders
their implementation slices and defines verification gates; it is not a second backlog.

The initial Waves 0–2 shipping slice is **accepted** at `f6846622`. Scoped evidence is recorded in
[Wave 0](archive/2026-09-21-memory-evaluation-corpus/verification-wave-0.md),
[Wave 1](archive/2026-09-21-memory-retrieval-contract/verification-wave-1.md), and
[Wave 2](archive/2026-09-21-memory-context-selection/verification-wave-2.md).
The Wave 3 slice is **accepted**
at `f90e793e`, with
[formation evidence and gate results](archive/2026-09-21-memory-evidence-capture/verification-wave-3.md).
Wave 4 is **accepted** at `fa8b0959`; see its
[verification record](archive/2026-09-21-memory-retrieval-contract/verification-wave-4.md).
Wave 5 is **accepted and archived** on 2026-09-21. Implementation commit
`c922504b` completes incremental capture, bounded replay, source fencing,
finalization queues, readiness, and comparative evaluation. Acceptance-docs commit
`9b382467` records the final gates. All six foundation corpora were archived with
the official OpenSpec tool, updating nine memory baseline files. See
[joint acceptance and archival](memory-wave-5-verification.md) for archive links,
full workspace and feature-matrix results, and the frozen synthetic live comparison.
The comparison passes its smoke criteria without establishing general model-quality
superiority or superiority over file search.
Waves 6–8 remain **planned**.
The [post-Wave-3 adversarial review](archive/2026-09-21-memory-evidence-capture/adversarial-review-wave-3.md)
is accepted at `987272da`; six reproduced findings were fixed before Wave 4.
Consult those records for gate outcomes rather
than inferring completion from this plan. Preserve unrelated work during execution.

## Wave sequence

| Wave | Deliverable | Entry dependency | Effort guidance |
|---|---|---|---|
| 0 | Minimal offline regression fixtures | None | 0.5–1 day |
| 1 | Correct archive search and section filtering | Relevant Wave 0 fixtures | 1–2 days |
| 2 | Remove obvious context waste | Wave 1 retrieval semantics | 2–4 days |
| 3 | Independent extraction and better formation input | Extraction fixtures and minimal durable source references | 2.5–5 days plus provenance groundwork |
| 4 | Embedding identity, scores, and graph eligibility | Wave 1 and embedding-space migration design | 2–4 days |
| 5 | Complete provenance, shared selection, recovery, and evaluation | Waves 2–4 | Remaining foundation scope |
| 6 | Reconcile duplicates, corrections, and conflicts | Wave 5 evidence and evaluation | Reconciliation corpus estimate |
| 7 | Revalidate aging and changed knowledge | Wave 6 reconciliation | Maintenance corpus estimate |
| 8 | Learn evidenced procedures through existing skills ownership | Waves 5–6; reuse Wave 7 policy where applicable | Procedural corpus estimate |

Effort is estimated focused engineer-days including TDD and validation. Slice
estimates are portions of full-corpus estimates, not additional costs. Re-estimate
later work after early waves expose actual integration and migration effort.

**First shipping milestone: Waves 0–2.** Search behaves as advertised and relevant
constraints survive irrelevant memory floods. Later sophistication must earn its
complexity against this corrected baseline.

## Gates for every implementation wave

### G0 — Scope and entry

- Name the included delta requirements and scenario titles, owning crates, starting
  commit, fixture revision, and required validation.
- Resolve decisions affecting this slice. Future-wave decisions do not block an
  independent repair.
- Split broad parent tasks before checking off a partial implementation. Remaining
  requirements stay unchecked and the parent corpus stays open.
- Identify compatibility boundaries and the rollback or forward-repair procedure.

**Pass:** acceptance cases are observable, prerequisites are satisfied, and no
unresolved decision changes the intended behavior of this slice.

### G1 — Red evidence

- Write public-behavior tests before implementation and demonstrate the expected
  failure against the pre-fix revision.
- Use fake clocks, fixed vectors, controlled extractors, and deterministic scheduling.
- Add the wave's adversarial cases and preserve already-correct regression behavior.
- If a supposed defect is already fixed, record that evidence rather than inventing
  a failing test. Compilation errors and missing infrastructure are not behavioral red.

**Pass:** runnable tests demonstrate the intended failure mechanism, with no leaked
answers or assertions that merely mirror the proposed implementation.

### G2 — Green and integration

- Implement the smallest change in the owning layer, then refactor duplicated policy.
- Run targeted tests and shared semantics against SQLite and InMemoryBackend.
  Different lexical ranking formulas need not produce equal numeric scores.
- Verify main-crate tools, setup, session capture, and context delivery when touched.
- For persistence changes, test migration failure atomicity, reopen, replay, and
  applicable transport. Vault changes require idempotency and path-boundary tests.
- Complete applicable landing commands and record exact outcomes.

**Pass:** targeted behavior and required integration checks pass at the candidate
revision. There are no unexplained failures or unverified changed boundaries.

### G3 — Adversarial review

Prefer a reviewer other than the implementer. Supply the scoped diff, requirements,
fixtures, and executable evidence. If only one executor is available, conduct a
separate adversarial pass and label it as same-executor review. Do not claim an
independent review without an independent reviewer.

The reviewer attempts to disprove the acceptance claims:

- Can wrong output still satisfy the tests?
- Can labels or future evidence leak into ingestion, retrieval, or prompts?
- Does another adapter bypass the new rules?
- Do cancellation, replay, or concurrent changes produce partial mutations?
- Does fallback hide a storage failure as an empty result?
- Does a measured gain come from a different model or a larger token budget?

Each finding records severity, violated scenario/contract, reproduction or evidence,
expected behavior, and disposition. Questions without a demonstrated mechanism are
investigated rather than automatically classified as blockers.

Blocking findings include data loss, partial mutation, wrong-scope or current/history
mixing, authority elevation, incompatible vector comparison, fabricated evidence,
unbounded work contrary to lifecycle contracts, label leakage, and reproducible
violations of the wave's acceptance scenarios.

**Pass:** no unresolved blocking findings. Nonblocking findings have an owner and
follow-up location. Every fix gets affected tests rerun and its finding rechecked.
If a fix changes another boundary, repeat that boundary's integration checks.

### G4 — Acceptance and handoff

- Freeze the reviewed candidate revision. Identify and re-review changes made after
  that revision rather than reusing an obsolete verdict.
- Record scenario-to-test mappings, commands, features/target, results, and artifacts.
- Attach the adversarial verdict and finding resolutions.
- Include paired evaluation for quality-sensitive policies where required below.
- Reconcile task checkboxes, designs, delta specs, and the next-wave handoff.
- Update `[Unreleased]` for implemented behavior/API/workflow changes; validate
  touched OpenSpec corpora and run whitespace checks.

**Pass:** the reviewed revision satisfies the slice's contracts and all required
gates. Wave completion does not automatically close its parent corpus. Archive only
when every parent requirement and task is verified, the archive check passes, and
there is explicit intent to close the change.

## Wave 0 — Minimal offline regression corpus

Waves 0–4 below retain their original slice scope and exit criteria. References to
later work describe those historical boundaries; Wave 5 closes the foundations.

**Corpus:** [memory-evaluation-corpus](archive/2026-09-21-memory-evaluation-corpus/proposal.md).

**Scope:** Establish synthetic fixtures with stable evidence IDs, timestamps, and
minds. Separate ingestion data from expected answers and future corrections. Cover
historical/current populations, filters, irrelevant Architecture floods, duplicate
pins, oversized facts, and provider availability. Use existing test runners.

**Verification:** Offline runs require no credentials or network. Repeated runs
produce identical semantic results. Record characterization separately from desired
behavior. Demonstrate the red cases needed for Waves 1–2.

**Adversarial review:** Attempt answer-label and future-event leakage. Change
insertion order and irrelevant content. Remove answer-bearing evidence and verify
that the evidence assertion fails. Ensure IDs do not secretly encode ranking.

**Exit:** Deliver reusable fixture conventions and reproducible defect evidence.
Full model evaluation and quality/cost reporting remain open for Wave 5.

## Wave 1 — Restore truthful search contracts

**Corpus:** [memory-retrieval-contract](archive/2026-09-21-memory-retrieval-contract/proposal.md).

**Scope:** Make archive search select historical statuses with labels. Enforce
section filtering before candidate limits in every participating retrieval channel.
Keep current and historical eligibility distinct. Preserve query-syntax tolerance
and observable storage errors. Reads must not reactivate or reinforce records.

**Verification:** Run shared populations against both backends and public tool
integration. An archived fact is found historically and excluded from current
search. With a result limit of one, wrong-section matches cannot hide an eligible
match. Check that status and reinforcement remain unchanged.

**Adversarial review:** Mix active, dormant, archived, and superseded records across
two minds. Flood the wrong section. Add quoted identifiers and FTS-like punctuation.
Make graph neighbors violate the filter. Inject storage failure and inspect both
standalone and live adapters for silent empty results or forgotten filters.

**Exit:** Ship trustworthy status selection and filters. Keep the corpus open for
embedding identity, score semantics, and graph work in Wave 4.

## Wave 2 — Remove high-cost context mistakes

**Corpus:** [memory-context-selection](archive/2026-09-21-memory-context-selection/proposal.md).

**Scope:** Deduplicate pins and retrieved facts. Skip oversized candidates rather
than ending selection. Apply existing current-fact eligibility before formatting.
Prefer task matches over section-order filler. Put policy in the domain owner and
preserve host budget enforcement. Full temporal applicability, token accounting,
and adapter convergence remain Wave 5 work.

**Verification:** The relevant-constraint flood case passes. Duplicate facts consume
one slot. An oversized first fact does not suppress a smaller eligible fact. Cover
zero budgets and inactive pins. Compare selected IDs and final block size before
and after; reduced size is not success if required evidence disappears.

**Adversarial review:** Put the relevant constraint last in insertion order and make
it old but eligible. Add lexical distractors, repeated pins, superseded pins, and
below-floor graph neighbors. Use Unicode/code identifiers at budget boundaries.
Inspect the rendered block, not only the intermediate selected list.

**Exit:** Ship the first useful-memory milestone: designated relevant evidence is
retained, ineligible IDs are excluded, and the current host budget contract holds.
Do not claim general model-quality improvement from these deterministic cases alone.

## Wave 3 — Better formation input and independent readiness

**Corpora:** [memory-capability-independence](archive/2026-09-21-memory-capability-independence/proposal.md),
[memory-evidence-capture](archive/2026-09-21-memory-evidence-capture/proposal.md), and the minimum
[memory-provenance-validity](archive/2026-09-21-memory-provenance-validity/proposal.md) slice.

**Entry decisions:** Resolve the legacy model-default conflict before changing
setup behavior. Select a minimal durable evidence-reference format before emitting
source-linked candidates. If that requires additional migration work, land it as a
prerequisite slice and re-estimate rather than substituting an untracked string.

**Scope:** Configure extraction independently from embeddings. Use substantive
committed session evidence instead of first/last fragments. Preserve corrections,
verification, and unresolved work in episodes. Validate candidate categories and
source references. Reuse the canonical session log. Preserve inferred lifecycle
admission rules; unresolved generated candidates remain pending until admission
can establish their status.

**Verification:** Fake providers cover extraction-only and embeddings-only cases.
Storage and lexical recall remain usable. A middle-of-session correction absent
from the final response survives formation. Invalid candidates do not discard valid
siblings. Reopen source-linked records where persisted.

**Adversarial review:** Let the final assistant response claim success after a failed
command. Put the correction deep inside a long session. Return malformed extractor
output, incorrect categories, and nonexistent source handles. Remove access to
evidence during extraction. Check for fabricated verification and blanket
Architecture classification.

**Exit:** Ship independent readiness with meaningful, validated formation input.
Durable checkpoint recovery and complete evidence-retention policy remain open.

## Wave 4 — Trustworthy retrieval signals

**Corpora:** remaining [memory-retrieval-contract](archive/2026-09-21-memory-retrieval-contract/proposal.md)
and indexing recovery from [memory-capability-independence](archive/2026-09-21-memory-capability-independence/proposal.md).

**Scope:** Bind vectors to model/revision, preprocessing, and dimensions. Handle
legacy unknown spaces explicitly and provide bounded repair. Expose named score
components. Apply neighbor eligibility and relation semantics with traversal bounds.
Preserve lexical fallback with typed optional-index degradation.

**Verification:** Equal-dimensional incompatible models are not compared. Unknown
legacy vectors require verified mapping or repair. Repair survives reopen/retry
without duplicate facts or reinforcement. Contradictions are not corroboration;
filtered or superseded neighbors cannot become current support. Storage failure is
distinct from optional-vector degradation.

**Adversarial review:** Mix embedding models within one mind and change preprocessing
without changing dimensions. Cancel indexing after fact commit. Retry after vector
commit but before acknowledgment. Construct cyclic, high-degree, contradictory,
and cross-mind graphs. Inspect score descriptions in lexical-only and fused modes.

**Exit:** Retrieval may close only when every parent scenario passes. Indexing is
observable and recoverable. Preserve the corrected lexical baseline for comparison.

## Wave 5 — Complete durable foundations

**Status: accepted and archived.** All subwaves and six foundation corpora are
closed. The [joint verification record](memory-wave-5-verification.md) owns final
gate outcomes and archive locations; the following scope and gates describe the
completed acceptance contract.

**Corpora:** Remaining provenance, context selection, evidence capture, capability
independence, and evaluation requirements.

### Ordered subwaves

1. **5A — Provenance:** Finish authority, applicability, valid/recorded time, legacy
   interpretation, source retention, and transport behavior.
2. **5B — Shared selection:** Converge adapters, enforce full token accounting, expose
   selection reasons, and invalidate caches on semantic input changes.
3. **5C — Durable capture:** Add checkpoint watermarks, persisted extraction results,
   replay-safe batches, backpressure, and cancellation/restart recovery.
4. **5D — Evaluation:** Complete stage-level attribution and paired no-memory,
   file-search, corrected-memory, and candidate-policy comparisons.

Each subwave passes G0–G4 independently. Avoid bundling migrations and runtime
policy into one large diff. Freeze routine budget policy and model-experiment
acceptance criteria using development cases before running held-out comparisons.

**Verification:** Migration failure preserves an original store or verified recovery
backup. DB/JSONL/applicable vault round-trips preserve evidence and unknowns without
reinforcement. Equivalent adapters select equivalent IDs and reasons. Final rendered
blocks fit the declared token budget including metadata. Task, scope, lifecycle,
pin, policy, budget, and eligibility changes invalidate caches. Unchanged turns avoid
full rescans. Partial-batch restart loses no acknowledged evidence and admits no
candidate twice. Reports distinguish formation, retrieval, selection, and task errors.

**Adversarial review:** Attempt authority elevation through imported content and
vault escape through symlinks. Simulate missing mounts. Change scope while TTL is
alive and archive a selected fact between turns. Test metadata-heavy Unicode blocks.
Interrupt migration and extraction at commit boundaries. Saturate queues. Compare
unequal token budgets and ensure reports expose the mismatch rather than crediting
an unfair improvement.

**Exit:** Foundational corpora close individually after their complete contracts
pass. The system has source-linked knowledge, consistent bounded context, recoverable
formation, and an evaluation baseline for later learning policies.

## Wave 6 — Reconcile knowledge instead of accumulating claims

**Corpus:** [memory-candidate-reconciliation](changes/memory-candidate-reconciliation/proposal.md).

**Post-Wave-5 entry:** The [drift assessment](changes/memory-candidate-reconciliation/assessment.md)
uses integrated `main` at `7b2da402`. Completed extraction batches and pending lifecycle
facts are different representations. Existing extraction recovery is not admission
recovery. Freeze candidate/evidence/decision identities and confirmation composition
before matching, atomic admission, or host adapters are implemented in parallel.

**Delivery:** `feat/memory-wave6-reconciliation` is a planning seed directly from
that main revision. Its change-local tasks own the ordered implementation slices.
One owner controls shared types, backend contracts, and migrations. Fixtures and
read-side/host planning can proceed against frozen contracts; migration numbers and
live-evaluation limits must be selected when implementation starts. New callable or
schema surface requires measured composition-budget review.

**Scope:** Add bounded scoped equivalence. Distinguish reuse, refinement, correction,
and unresolved conflict. Preserve independent evidence identities and retired history.
Commit candidate state, facts, edges, and receipts atomically with version checks.
Provider-dependent decisions remain replayable and pending on failure.

**Verification:** Paraphrases do not multiply active claims. Applicable explicit
corrections preserve supersession history. Cross-platform differences coexist. Newer
unsupported inferences do not replace observations. Replay does not increase
corroboration. Version conflict rolls back the full admission plan.

**Adversarial review:** Use near-identical statements with opposite negation,
different versions, or different scopes. Repeat one source through paraphrases.
Process competing corrections in both arrival orders. Reimport a retired claim.
Make the classifier timeout or emit a confident wrong merge. Verify that deterministic
validation limits model authority and unresolved claims remain inspectable.

**Exit:** Deterministic scenarios pass. Paired evaluation meets frozen duplicate
reduction and knowledge-update criteria without exceeding false-merge or task-regression
tolerances. If it fails, retain unresolved candidates and revise the policy before
enabling broader semantic admission.

Wave 5's quotation-based smoke comparison does not establish those semantic admission
criteria. Build development and held-out equivalence/correction/conflict cases,
including correlated sources and deliberate nonmatches, and obtain a fresh live-run
budget before any model-backed acceptance experiment.

## Wave 7 — Maintain freshness without confusing age with truth

**Corpus:** [memory-maintenance-revalidation](changes/memory-maintenance-revalidation/proposal.md).

**Scope:** Separate confidence, freshness, salience, validity, and retention. Migrate
legacy metadata without inventing verification. Detect source changes, schedule
bounded revalidation, and apply versioned maintenance plans. Preserve history.

**Verification:** Old supported invariants remain eligible. Repeated reads/references
do not increase evidence confidence. Expired workarounds leave current guidance but
remain historically searchable. Source-read failure preserves state and verification
time. A newer correction defeats a stale maintenance plan.

**Adversarial review:** Advance fake clocks across policy boundaries. Repeatedly
access an incorrect fact. Change an unrelated source. Temporarily remove vault access.
Apply a plan after correction and repeat it after restart. Ensure missing sources
and high usage are not treated as proof of falsehood or truth.

**Exit:** Retention/revalidation contracts pass, including transport and reopen.
Evaluation meets frozen stale-guidance criteria without unacceptable loss of old
valid knowledge. Inspection explains each transition and its evidence.

## Wave 8 — Learn procedures from observed outcomes

**Corpus:** [memory-procedural-learning](changes/memory-procedural-learning/proposal.md).

**Scope:** Form scoped procedures and gotchas from episodes. Retain prerequisites,
validation, counterexamples, and application evidence. Use shared context budgeting.
Promote explicitly through existing skill validation and registered command ownership,
with attribution and destination-version checks.

**Verification:** Failed attempts do not become verified successful procedures.
Retrieval alone is not successful application. Inapplicable procedures are excluded
from direct guidance. Counterexamples preserve history. Promotion passes the existing
skill parser and replay checks; concurrent human edits remain intact.

**Adversarial review:** Present one accidental success as a universal recipe. Vary
platform and tool version. Retrieve without executing. Inject text claiming extra
tool authority. Edit a skill after planning promotion. Repeat promotion. Test a
procedure that improves one task class while harming another.

**Exit:** Domain, skills, and runtime integration pass. Paired evaluation meets frozen
repeated-error and task-success criteria within the resource budget. If benefits do
not justify complexity, retain inspectable candidates and explicit skill workflows
while revising automatic learning policy.

## Validation and evidence records

Run focused tests while iterating, then the [applicable landing gates](memory-modernization.md#landing-gates).
For domain changes, these include:

```bash
just test-crate omegon-memory
cargo test -p omegon-memory --all-features --locked
just clippy-changed
```

Run no-default-features tests for domain/public-contract changes. Add focused
main-crate tests and `just test-commit` for shared contracts or runtime integration.
Apply root broad gates for high-risk setup, event, or multiple-frontend changes.
Record actual toolchain, features, target, and outcomes.

When acceptance depends on the real harness, use bounded `just run` execution with
isolated synthetic state and recorded build identity. Exercise the relevant
non-interactive surface. Install only when installed assets are part of the evidence.
Model experiments require declared execution intent and a resource budget; routine
contract tests remain offline and use synthetic data.

Validate each touched change and check whitespace:

```bash
python3 ~/.agents/skills/openspec/scripts/openspec.py validate <change-name>
git diff --check
```

During execution, create `verification-wave-<number>.md` in each participating change.
One primary record may link shared results instead of duplicating output. Include:

```text
Wave and scoped scenario titles:
Starting commit / reviewed candidate revision:
Fixture revision and evidence cutoff:
Resolved decisions / remaining scope:
Scenario -> test -> red evidence -> green evidence:
Commands, features/target, outcomes, artifact locations:
Runtime build identity and evidence where applicable:
Paired evaluation configuration and frozen criteria where applicable:
Reviewer identity and independent/same-executor status:
Findings, reproductions, fixes, recheck outcomes, follow-ups:
Migration/recovery or rollback evidence:
G0 / G1 / G2 / G3 / G4 verdicts and blockers:
Parent task updates and next-wave handoff:
```

## Failure and progression rules

- A failed gate blocks acceptance and work depending on that contract. Independent
  verified slices can continue. Do not restart unrelated completed checks.
- A timed-out build without a compiler/test verdict is indeterminate. Continue the
  existing long-running gate rather than repeatedly restarting it.
- Diagnose model-quality regressions by formation, retrieval, selection, and reader
  behavior. Changing models or token budgets requires a newly declared comparison.
- Revert nonpersistent policy to its last verified behavior when necessary. For
  schema changes, use the tested recovery/forward-repair procedure. Do not downgrade
  against an incompatible store or overwrite newer writes with an old backup.
- Extraction/indexing failure must preserve committed facts and recoverable work.
  Logging a background failure is not evidence that the operation completed.
- Archive only after every parent scenario has evidence, all tasks are complete,
  and `archive <change-name> --check` passes, with explicit closure intent.
