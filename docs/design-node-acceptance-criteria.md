+++
id = "3d698594-f583-4d3d-bfa5-83d5a486b1c6"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Design Node Acceptance Criteria — format, storage, and evaluation

## Overview

> Parent: [Dual-Lifecycle OpenSpec — Design Layer + Implementation Layer](dual-lifecycle-openspec.md)
> Spawned from: "Where do acceptance criteria live on a design node — new '## Acceptance Criteria' body section (consistent with OpenSpec spec files, human-readable), a YAML frontmatter array, or derived implicitly from open_questions.length === 0 + decisions.length > 0?"

*To be explored.*

## Research

### Format options and analysis



### Composite format — three artifact types within one section

The section needs to capture three distinct kinds of verification, each with a different evaluation mechanism:

| Artifact type | Purpose | Evaluated by |
|---|---|---|
| **Scenarios** | Consequence reasoning — "if we chose this, when X happens, then Y" | LLM reads node doc, checks each Then clause is satisfiable |
| **Falsifiability** | Intellectual honesty — "what would prove this decision wrong" | LLM checks each condition is either ruled out or accepted as known risk |
| **Constraints** | Structural checklist — explicit things that must be true before decided | Structural where possible (open_questions.length, decisions.length), LLM for prose |

Proposed `## Acceptance Criteria` format:

```markdown

## Decisions

### Decision: ## Acceptance Criteria body section with three subsections: Scenarios, Falsifiability, Constraints

**Status:** decided
**Rationale:** Explicit over implicit (no derivation), human-readable over YAML (no frontmatter array), and composite over single-format. Three subsections capture distinct verification kinds: Scenarios for consequence reasoning (LLM-evaluated), Falsifiability for intellectual honesty (LLM-evaluated), Constraints for structural checklist (structural + LLM). Given/When/Then in Scenarios forces articulation of consequences, not just presence of decisions. Falsifiability forces acknowledgment of what would invalidate the choice. Constraints catch structural gaps that scenarios and falsifiability don't cover.

### Decision: Acceptance criteria written at exploring time, before research begins

**Status:** decided
**Rationale:** The falsifiability conditions and consequence scenarios must be written before the research, not after. Writing them after is confirmation bias by construction — you already know the answer and write scenarios that your answer satisfies. Writing them before forces honest articulation of what would change your mind and what consequences you're committing to examine. This is the core anti-hallucination mechanism. The constraint checklist can be partially filled in during exploration as structural facts are confirmed.

### Decision: DocumentSections.acceptanceCriteria as a new top-level parsed field

**Status:** decided
**Rationale:** Acceptance criteria are a peer of openQuestions, decisions, and research in the document model — not an extraSection or appendix. tree.ts section parser extended with AcceptanceCriteriaScenario[], string[] (falsifiability), and AcceptanceCriteriaConstraint[] ({text, checked}) types. Surfaced in design_tree(action="node") response alongside other sections. Constraint checked-state is structural truth the agent can read without LLM evaluation.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/design-tree/types.ts` (modified) — Add AcceptanceCriteriaScenario, AcceptanceCriteriaConstraint interfaces. Add acceptanceCriteria to DocumentSections.
- `extensions/design-tree/tree.ts` (modified) — Parse ## Acceptance Criteria section with ### Scenarios, ### Falsifiability, ### Constraints subsections.
- `extensions/design-tree/index.ts` (modified) — Surface acceptanceCriteria in node action response. Scaffold ## Acceptance Criteria in set_status(exploring) template.

### Constraints

- Acceptance criteria section is optional on seed nodes — required only before set_status(decided) via /assess design gate
- Falsifiability items use prefix 'This decision is wrong if: ' for consistent parsing
- Constraint checkboxes follow GFM task list syntax: '- [ ]' unchecked, '- [x]' checked
- Scenarios use bold Given/When/Then labels within freeform prose — not rigid line-by-line parsing

## Acceptance Criteria

### Scenarios

**Given** this storage architecture is adopted,
**When** a new contributor joins without prior context,
**Then** they can articulate the chosen approach, why alternatives were rejected,
and what would need to change if write volume grew 10x.

**Given** we commit to SQLite-backed storage,
**When** cross-region replication becomes a requirement in 12 months,
**Then** the blast radius and migration path are documented in Implementation Notes.

### Falsifiability

- This decision is wrong if: write volume under production load exceeds SQLite's WAL throughput ceiling and no migration path is specified
- This decision is wrong if: the embedding model is replaced and semantic retrieval degrades without a documented fallback strategy

### Constraints

- [ ] All evaluated alternatives documented with explicit tradeoff comparison (not just the winner)
- [ ] At least one rejected alternative documented with rationale for rejection
- [ ] Implementation Notes include file scope (or explicit "no file changes" justification)
- [ ] No open questions remain at decision time
- [ ] Decision rationale addresses at least one second-order consequence
```

### Parsing implications

`tree.ts` section parser needs three new subsection types under `## Acceptance Criteria`:
- `### Scenarios` → parsed as `AcceptanceCriteriaScenario[]` (Given/When/Then blocks)
- `### Falsifiability` → parsed as `string[]` (bullet list, "This decision is wrong if: ...")
- `### Constraints` → parsed as `AcceptanceCriteriaConstraint[]` ({text, checked: boolean})

`DocumentSections.acceptanceCriteria` becomes a new top-level field alongside `openQuestions`, `decisions`, `research`.

### Assessment evaluation model

`/assess design` receives the full node document + the three artifact types and evaluates:
1. **Constraints**: structural checks run first (open_questions.length === 0, decisions.length > 0). Prose constraints evaluated by LLM against the document body.
2. **Scenarios**: LLM checks each "Then" clause is satisfiable from the Research + Decisions content. Returns pass/fail per scenario with a brief rationale.
3. **Falsifiability**: LLM checks each "This decision is wrong if" condition is addressed — either explicitly ruled out in Research, accepted as known risk in Decisions, or flagged as a gap.

Assessment fails if any constraint is unchecked, any scenario's Then clause is unsatisfied, or any falsifiability condition is unaddressed. Each failure produces a specific, actionable finding — not a generic "assessment failed".

## Option A — Implicit derivation

`decided` is valid when `open_questions.length === 0 && decisions.length > 0`.

Rejected immediately. This is exactly the vibes-based problem we're solving — it lets a node be "decided" with no stated rationale for what "done" means. A node can have one throwaway decision and zero questions and pass. Nothing was formally specified before exploration began.

## Option B — YAML frontmatter array

```yaml
acceptance_criteria:
  - All storage alternatives evaluated with latency/durability tradeoffs documented
  - Chosen approach compared against at least one rejected alternative
  - Implementation notes include file scope
```

Hidden from casual reading. Not consistent with how OpenSpec spec files look. Doesn't support multi-line scenario prose. Hard to write Given/When/Then in inline YAML. Works mechanically but fights the format.

## Option C — `## Acceptance Criteria` body section

Same treatment as `## Open Questions`, `## Research`, `## Decisions` — a first-class structured section in the document body. Written at `exploring` time. Read by the section parser in `tree.ts`. Evaluated by `/assess design`.

Format mirrors implementation spec scenarios — Given/When/Then — because design assessments need the same explicit falsifiability conditions:

```markdown

## Acceptance Criteria

**Given** this storage architecture decision,
**When** a new contributor reads the node document without prior context,
**Then** they can articulate the chosen approach, why the alternatives were rejected,
and what would need to change if write volume grew 10x.

**Given** we commit to approach A,
**When** the requirements change to require cross-region replication in 12 months,
**Then** the blast radius of that change is documented in Implementation Notes.
```

The "Then" clauses are LLM-evaluated against the document body — not mechanically verifiable, but consistently structured so `/assess design` can apply them uniformly.

**Why Given/When/Then for design, not just a checklist:**
A checklist ("decision documented: ✓") is still vibes-based — it's just a more explicit checklist of vibes. Given/When/Then forces articulation of *consequences*: "if we made this choice, when X happens, the system/team/codebase behaves like Y." That's the second/third/fourth-order effects forcing function. Checklists don't require thinking about consequences; scenarios do.
