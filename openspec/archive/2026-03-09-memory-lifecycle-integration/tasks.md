+++
id = "cf0910f6-e823-43fe-b2f0-bf26c6a54b81"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Memory integration with Cleave, Design Tree, and OpenSpec — Tasks

Dependencies / execution order:
- Group 1 defines the shared lifecycle-memory candidate model and persistence rules first.
- Groups 2–4 emit structured candidate payloads from design-tree, OpenSpec, and cleave into the shared ingestion path.
- Group 5 is optional glue only if cross-extension coordination cannot stay source-local.
- Group 6 adds end-to-end and regression coverage across the lifecycle checkpoints.

## 1. Project-memory lifecycle candidate ingestion core
<!-- specs: memory/lifecycle -->

- [x] 1.1 Identify the project-memory extension entry points for lifecycle-driven writes in `extensions/project-memory/index.ts`, and add a shared ingestion API for structured lifecycle candidates rather than routing through free-form extraction.
- [x] 1.2 Define a normalized lifecycle candidate shape in `extensions/project-memory/` that captures source kind, authority level, target memory section, content, artifact reference, and whether the candidate is explicit or inferred.
- [x] 1.3 Implement candidate classification rules so explicit structured lifecycle conclusions auto-store, while inferred summaries are flagged for operator confirmation instead of immediate persistence.
- [x] 1.4 Implement pointer-fact formatting rules so stored lifecycle facts reference authoritative docs/specs rather than copying long-form artifact text into memory.
- [x] 1.5 Wire deduplication handling to prefer reinforcement or supersede/archive flows over duplicate active facts when equivalent lifecycle truths already exist.
- [x] 1.6 Ensure proposal-stage intent, open questions, child-task chatter, failed investigative breadcrumbs, and other low-signal artifacts are rejected by default by the ingestion rules.
- [x] 1.7 Add focused unit tests for candidate normalization, explicit-vs-inferred routing, pointer-fact formatting, and duplicate/supersede behavior under lifecycle ingestion.

## 2. Design-tree decision and constraint emitters
<!-- specs: memory/lifecycle -->

- [x] 2.1 Update `extensions/design-tree/index.ts` so `add_decision` emits a structured lifecycle candidate payload when a decision is recorded with `decision_status: "decided"`.
- [x] 2.2 Update `extensions/design-tree/index.ts` so `add_impl_notes` emits structured constraint candidates only for `constraints`, and never emits candidates for open questions or other exploratory text.
- [x] 2.3 Include authoritative artifact references in emitted candidates (node id and file path from `docs/<node>.md`) so downstream memory facts can point back to the design source.
- [x] 2.4 Add or extend tests covering decided-decision candidate emission, constraint candidate emission, and explicit rejection of open-question persistence.

## 3. OpenSpec archive and post-assess lifecycle emitters
<!-- specs: memory/lifecycle -->

- [x] 3.1 Update `extensions/openspec/index.ts` and supporting helpers so `archive` emits structured lifecycle candidates for durable baseline truths after `archiveChange()` succeeds.
- [x] 3.2 Ensure archived spec candidates carry domain/baseline references (for example `openspec/baseline/...`) so memory facts point to the authoritative spec artifact.
- [x] 3.3 Update the `reconcile_after_assess` flow to emit structured candidates for new constraints and known issues discovered during post-assess reconciliation, without treating speculative review prose as durable fact automatically.
- [x] 3.4 Ensure proposal/planning-stage OpenSpec artifacts do not auto-produce durable memory candidates before archive.
- [x] 3.5 Add tests for archive-driven durable fact generation, post-assess constraint/known-issue candidate generation, and rejection of proposal-stage auto-storage.

## 4. Cleave durable-outcome emitter
<!-- specs: memory/lifecycle -->

- [x] 4.1 Identify the cleave completion/review checkpoints in `extensions/cleave/index.ts` and related review paths where final durable outcomes are known.
- [x] 4.2 Emit only structured durable findings from cleave/review outcomes, such as resolved known issues or stable fix conclusions, and explicitly exclude raw child chatter, intermediate plans, and transient execution logs.
- [x] 4.3 When a bug fix resolves a known issue after review or assessment, emit a final lifecycle candidate that stores one durable conclusion and archives or ignores the investigation breadcrumbs.
- [x] 4.4 Add tests covering ignored transient child output and accepted durable final-outcome candidates.

## 5. Shared coordination glue (only if needed)
<!-- specs: memory/lifecycle -->

- [x] 5.1 Evaluate whether lifecycle candidate routing can remain source-local; only extend `extensions/shared-state.ts` if a lightweight shared channel is necessary for cross-extension coordination.
- [x] 5.2 If shared state is required, add the smallest possible lifecycle candidate/event summary contract without duplicating source-of-truth ownership from design-tree, OpenSpec, cleave, or project-memory.
- [x] 5.3 Add tests for any new shared-state contract to keep the integration explicit and stable.

## 6. End-to-end lifecycle verification
<!-- specs: memory/lifecycle -->

- [x] 6.1 Add an integration-style test path that records a decided design-tree decision and verifies a Decisions memory candidate is generated with a source reference.
- [x] 6.2 Add an integration-style test path that archives an OpenSpec change and verifies durable spec candidates are created from baseline authority, not proposal intent.
- [x] 6.3 Add an integration-style test path that processes a post-assess reconciliation or resolved bug outcome and verifies the result is one durable conclusion rather than breadcrumb spam.
- [x] 6.4 Run `npm run check` and verify the lifecycle-memory integration remains type-clean and regression-safe before implementation is marked complete.
