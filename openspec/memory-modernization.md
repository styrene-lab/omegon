# Memory modernization: TDD corpus map

## Placement and status

Active change corpora live in this repository's `openspec/changes/` tree. The six
accepted foundation corpora are archived under `openspec/archive/2026-09-21-*`;
their requirements are merged into nine memory baseline files.
Memory behavior spans the domain crate, managed runtime, session evidence, context
assembly, and skills. Do not create a second OpenSpec root inside a crate.
Executable tests and synthetic fixtures live beside their owning code.

This document is a navigation and dependency map, not a second specification.
Each change owns its proposal, delta specifications, design, and TDD tasks.
Specification validation does not establish a failing test or a passing
implementation. The initial Waves 0–2 shipping slice is accepted at `f6846622`;
its verification records identify red/green evidence, same-executor adversarial
review, and passing landing gates. Wave 5 completed the remaining foundation
requirements and is **accepted and archived** as of 2026-09-21. Implementation
commit: `c922504b`; acceptance-docs commit: `9b382467`. See the
[joint acceptance and archive record](memory-wave-5-verification.md) for final
gates, scenario evidence, and the bounded synthetic live comparison. Waves 6–8
remain **planned**, with their implementation tasks unchecked.

Wave 3's independent extraction and evidence-backed formation slice is accepted
at `f90e793e`;
see its [verification record](archive/2026-09-21-memory-evidence-capture/verification-wave-3.md).

Wave 4 is accepted at `fa8b0959`: identified vectors, explicit score/relationship
signals, schema-v10 migration, and the verified repair CLI. See its
[verification record](archive/2026-09-21-memory-retrieval-contract/verification-wave-4.md).

The [phased execution plan](memory-modernization-execution.md) defines wave entry
criteria, TDD verification, adversarial review, and acceptance/handoff gates.

## Changes and full-corpus dependencies

| Change | Behavioral scope | Prerequisites |
|---|---|---|
| [memory-evaluation-corpus](archive/2026-09-21-memory-evaluation-corpus/proposal.md) — archived | Deterministic fixtures, task replay, attribution, cost and quality reports | None; establish a minimal fixture set before policy changes |
| [memory-retrieval-contract](archive/2026-09-21-memory-retrieval-contract/proposal.md) — archived | Historical search, filters, embedding identity, score semantics, graph eligibility | Evaluation fixture conventions |
| [memory-provenance-validity](archive/2026-09-21-memory-provenance-validity/proposal.md) — archived | Evidence, authority, applicability, migration and transport | Evaluation fixture conventions |
| [memory-capability-independence](archive/2026-09-21-memory-capability-independence/proposal.md) — archived | Independent extraction, embedding and storage readiness | None for readiness; retrieval contract for compatible indexing repair |
| [memory-context-selection](archive/2026-09-21-memory-context-selection/proposal.md) — archived | Shared task-aware selection, packing, pin handling, inspection | Retrieval contract and provenance |
| [memory-evidence-capture](archive/2026-09-21-memory-evidence-capture/proposal.md) — archived | Structured episodes, checkpoints, recoverable extraction | Provenance and capability independence |
| [memory-candidate-reconciliation](changes/memory-candidate-reconciliation/proposal.md) | Candidate admission, equivalence, corrections, unresolved conflicts | Provenance and evidence capture |
| [memory-maintenance-revalidation](changes/memory-maintenance-revalidation/proposal.md) | Separate evidence confidence, freshness and salience; source revalidation | Provenance and reconciliation |
| [memory-procedural-learning](changes/memory-procedural-learning/proposal.md) | Evidence-backed workflows and gotchas through existing skills ownership | Evidence capture, reconciliation and context selection |

Retrieval repairs, provenance, and capability independence may be developed as
separate slices. Land context-selection correctness before measuring its quality
against the old injection policy. Introduce maintenance and procedural learning
after the evidence and reconciliation contracts are available.

## Original effort and return prioritization

The estimates and queue below record the original planning rationale. Foundation
work through queue item 6 is accepted; later learning work remains planned. These
estimates are not measurements of implementation cost or demonstrated model quality.

Prioritize independently testable behavior slices rather than finishing each corpus
before starting the next. Full-corpus dependencies above do not block repairs that
can use existing fields and contracts. A partial slice does not complete or archive
its parent change.

The estimates below are planning judgments from source inspection, not measured
delivery times or demonstrated quality gains. Effort is focused engineer-days,
including TDD, integration, and applicable validation, excluding prerequisite work.
Migration and runtime-lifecycle discoveries can increase these ranges. Return means
expected agent usefulness or avoided failures relative to implementation effort.

| ROI rank | Corpus | Full incremental effort | Expected return | Reason |
|---|---|---|---|---|
| 1 | memory-capability-independence | 2–4 days | High, configuration-dependent | Small readiness fix prevents one optional outage from disabling another capability; complete indexing recovery costs more |
| 2 | memory-retrieval-contract | 4–7 days | High, high confidence in correctness benefit | Several directly reproducible contract defects; model-identity repair adds persistence work |
| 3 | memory-context-selection | 5–9 days | Very high absolute impact | Every affected turn benefits from less irrelevant context and better budget allocation |
| 4 | memory-evidence-capture | 7–12 days | Very high absolute impact | Removes the severe evidence bottleneck; durable checkpoint recovery is the expensive portion |
| 5 | memory-evaluation-corpus | 3–6 days | High enabling return | Makes later policy choices measurable; a small initial fixture slice is sufficient for first repairs |
| 6 | memory-provenance-validity | 7–12 days | High enabling return | Supports trustworthy updates and scope, but requires migrations and transport agreement |
| 7 | memory-candidate-reconciliation | 8–15 days | High after capture improves | Reduces duplicates/conflicts; semantic decisions require evidence and quality evaluation |
| 8 | memory-maintenance-revalidation | 6–10 days | Medium initially, increasing with age | Most valuable once evidence-linked knowledge exists; source scans and migration add cost |
| 9 | memory-procedural-learning | 10–18 days | Potentially high, least certain initially | Depends on reliable episodes, outcome attribution, applicability, and skills integration |

These are not the chronological execution order. Evaluation starts as a small
supporting slice; prerequisites land before their dependent policy changes. Do not
interpret enabling extraction as a quality improvement by itself: the current
truncated input and blanket Architecture classification also need repair.

### Biggest-impact/easiest-first execution queue

| Order | Slice | Estimated effort | Required evidence |
|---|---|---|---|
| 0 | Minimal offline fixtures for archive, section filtering, irrelevant floods, packing, and extractor availability | 0.5–1 day | Failing behavior tests with controlled data; no full benchmark platform required |
| 1 | Archive status selection and section-filter enforcement | 1–2 days | Cross-backend populations plus public tool tests; filter before truncation |
| 2 | Immediate selection repairs: deduplicate pins, skip oversized candidates, enforce existing eligibility, and prioritize task matches over section order | 2–4 days | Relevant constraint survives irrelevant Architecture flood; budget and pin regressions |
| 3 | Decouple extraction configuration from embedding readiness | 0.5–1 day | Fake-provider availability matrix; resolve or explicitly amend the legacy model-default conflict |
| 4 | Formation quality slice: structured categories and substantive episode input from committed session evidence | 2–4 days | Mid-session correction and verification retained; no fabricated evidence; land minimal provenance needed for references |
| 5 | Finish retrieval correctness: embedding-space identity, score semantics, and graph eligibility | 2–4 days | Equal-dimension/different-model rejection, labeled scores, conflict/status-aware neighbors |
| 6 | Build minimal durable provenance, then finish shared selection/token accounting and evidence checkpoint recovery | Use remaining full-corpus scope | Reopen/transport, adapter parity, cache invalidation, and interruption/replay tests |
| 7 | Expand evaluations and implement reconciliation, revalidation, and procedural learning in dependency order | Use remaining full-corpus scope | Measured downstream benefit and regression controls before each broader policy change |

Slice estimates are portions of the full-corpus estimates, not additional effort.
The first practical milestone is orders 0–2: restore trustworthy search behavior
and reduce obvious context waste. Orders 3–4 then improve formation availability
and input quality together. Measure these changes before committing to richer
temporal modeling, semantic consolidation, or procedural promotion.

## Test and fixture ownership

- `core/crates/omegon-memory/src/`: domain unit and existing backend tests.
- `core/crates/omegon-memory/tests/`: proposed cross-backend public-contract tests.
- `core/crates/omegon-memory/tests/fixtures/`: proposed synthetic facts, evidence,
  vectors, migrations, and expected eligibility data.
- `core/crates/omegon/src/features/memory.rs`, `memory_service.rs`, and
  `features/context.rs`: runtime adapter and managed-service integration tests.
- `core/crates/omegon/tests/`: proposed session replay and public runtime tests.
- `core/crates/omegon/tests/fixtures/memory/`: proposed synthetic multi-session
  trajectories, fake model responses, and held-out task expectations.
- `core/crates/omegon-skills/`: procedural artifact validation and activation tests,
  subject to that crate's directives when implementation starts.

Fixture files are test evidence, not OpenSpec baselines. Use one authoritative
fixture per case with scenario IDs in test names or fixture metadata. Store
expected answers outside data exposed to the memory writer and retriever.

## TDD protocol

1. For each scenario, write the smallest public-behavior test and record its red
   result. A compiler error or unrelated infrastructure failure is not the red
   evidence for the behavior.
2. Implement the owning policy or adapter until the scenario passes.
3. Refactor duplication while preserving the passing assertions.
4. Run both backends for shared storage/search semantics. Use fake clocks, fake
   extractors, and fixed vectors for deterministic tests.
5. For persistence changes, verify reopen, legacy deserialization, export/import,
   migration failure atomicity, and retry identity. For vault changes, verify
   idempotency and path boundaries.
6. Record scenario-to-test mapping, exact commands, red/green outcomes, and any
   unresolved scenarios in each change's implementation verification notes.

Characterization tests record the old system's output for comparison. They must
not assert known defects as desired behavior. Model-based experiments are a
separate evaluation tier and do not make credential-dependent tests a unit gate.

## Landing gates

For memory-domain implementation changes:

```bash
just test-crate omegon-memory
cargo test -p omegon-memory --all-features --locked
just clippy-changed
```

Add `cargo test -p omegon-memory --no-default-features --locked` when changing
public domain contracts or feature boundaries. For runtime/shared-contract work,
run focused main-crate tests and `just test-commit`; apply root broad gates when
setup, events, or multiple frontends materially change. Record actual commands
and outcomes rather than assuming a crate gate covers runtime behavior.

For these planning artifacts, validate each named change with the OpenSpec tool
and run `git diff --check`. Archive only after implementation tasks and behavioral
verification are complete. Keep existing baselines unchanged until archival.

## Baseline reconciliation and decisions

- `baseline/memory/injection-budget.md` owns the existing injection-budget and
  audit requirements. The selection change modifies those titles in place.
- `baseline/memory/lifecycle.md` already requires confirmation of inferred
  lifecycle summaries. Provenance preserves that policy and makes it durable.
- `baseline/memory/models.md` now records independently configurable host routing,
  explicit Rust defaults, profile overrides/disable, and the existing child policy.
  Capability-independence archival replaced the historical cloud-default and
  subprocess-routing requirements explicitly.
- `baseline/memory.md` still contains legacy extension and `.pi/memory` vocabulary.
  Its transport and metrics obligations must be checked against any affected
  runtime surface. This map does not authorize silently dropping those contracts.
- The archived [selection design](archive/2026-09-21-memory-context-selection/design.md)
  records the routine cap and allocation policy. Hard budget compliance remains
  independent of policy choice.
- The archived [evaluation evidence](archive/2026-09-21-memory-evaluation-corpus/verification-wave-5.md)
  records frozen synthetic smoke criteria and held-out comparisons. These results
  do not establish production or general model-quality superiority.
- The archived [provenance design](archive/2026-09-21-memory-provenance-validity/design.md)
  defines workspace/revision applicability and evidence retention.

## Research basis

The assessment informing these corpora uses the following primary sources:

- [Anthropic: effective context engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)
- [LangGraph: memory overview](https://docs.langchain.com/oss/python/concepts/memory)
- [Letta: agent memory](https://www.letta.com/blog/agent-memory)
- [Mem0: extraction and consolidation](https://arxiv.org/abs/2504.19413)
- [Zep: temporal memory](https://arxiv.org/abs/2501.13956)
- [Hindsight: evidence and belief separation](https://aclanthology.org/2026.acl-demo.27/)
- [ACE: evolving procedural context](https://arxiv.org/abs/2510.04618)
- [LongMemEval](https://github.com/xiaowu0162/LongMemEval)
- [LongMemEval-V2](https://github.com/xiaowu0162/LongMemEval-V2)

These sources motivate experiments and behavioral contracts. Their reported
scores are not acceptance thresholds for Omegon.
