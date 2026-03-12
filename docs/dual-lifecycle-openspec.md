---
id: dual-lifecycle-openspec
title: Dual-Lifecycle OpenSpec — Design Layer + Implementation Layer
status: implemented
tags: [architecture, design-tree, openspec, lifecycle, dx]
open_questions: []
branches: ["feature/dual-lifecycle-openspec"]
openspec_change: dual-lifecycle-openspec
---

# Dual-Lifecycle OpenSpec — Design Layer + Implementation Layer

## Overview

Apply OpenSpec principles at two distinct architectural levels rather than only at implementation. The Design Tree tracks the exploration/decision lifecycle; OpenSpec changes track the implementation lifecycle. Both are spec-driven, both produce archived artifacts, both have explicit Definition of Done. Key changes: (1) rename the OpenSpec surface to "Implementation" for conceptual clarity, (2) add formal acceptance criteria to design nodes so "decided" is verifiable rather than vibes-based, (3) enforce design-tree-first entry into the implementation pipeline — OpenSpec changes always originate from a decided design node, never standalone.

## Research

### Current architecture: where the split already exists

The design tree node already mirrors OpenSpec's shape exactly:

| Design node section | OpenSpec equivalent |
|---|---|
| `## Overview` | `proposal.md` — the intent |
| `## Open Questions` | `tasks.md` — discrete work items to resolve |
| `## Research` + `## Decisions` | Implementation artifacts — the work product |
| `set_status(decided)` | Archive — the lifecycle terminal event |
| `## Acceptance Criteria` (missing) | `specs/*.md` — the Definition of Done |

The design tree is already running an OpenSpec-shaped loop at the design level. It just does it implicitly, with no formal spec and no verifiable "done" condition. OpenSpec does it explicitly, with scenarios, assessment, and archive. Same pattern, different rigor levels.

**The asymmetry that causes problems**: OpenSpec has a hard archive gate (`missing_design_binding`) that compensates for the fact that implementation can start without a design node. This gate exists because the entry point is wrong — not because the relationship is fundamentally unclear.

### Three-level spectrum of change

**Level 1 — Label change** (zero code cost)
Rename the "OpenSpec" surface in the dashboard and all agent guidelines to "Implementation". The design tree becomes "Design", OpenSpec changes become "Implementation". This alone clarifies the mental model: design work goes in the design tree, implementation work goes in implementation (OpenSpec). The conceptual union becomes visible without touching any code.

**Level 2 — Design Definition of Done** (light code change)
Add formal `acceptance_criteria` to design nodes — written at node creation, evaluated before `decided`. Could be a new `## Acceptance Criteria` section in the document body (like `## Open Questions`) or a YAML field. The agent runs an equivalent of `/assess spec` on the design node before allowing `set_status(decided)`. This closes the "vibes-based decided" gap: "decided" becomes verifiable.

Acceptance criteria for a design node look like:
- "All open questions are answered (open_questions.length === 0)"
- "At least one decision is documented with rationale"
- "Implementation notes include file scope or explicit 'no file changes' note"
- Custom: "The chosen approach is explicitly compared against at least one rejected alternative"

**Level 3 — Full design-phase OpenSpec** (significant architecture change, opt-in)
High-stakes design nodes (major architectural decisions) get their own `openspec/design-changes/<node-id>/` entry running the full propose→spec→plan→execute→verify→archive cycle. The "tasks" are the open questions; the "specs" are the acceptance criteria; the "archive" is when the node hits `decided`. Reserved for nodes where "are we really done thinking?" is a genuine risk. Default flow stays at Level 2.

### Full lifecycle picture under the proposed model

```
DESIGN LAYER                          IMPLEMENTATION LAYER
─────────────────────────────         ─────────────────────────────────────────
Design node: seed                     (not yet in scope)
  ↓ add_research, add_question
Design node: exploring
  ↓ [acceptance criteria written]
  ↓ [open questions answered]
  ↓ /assess design (Level 2+)
Design node: decided
  ↓ design_tree_update(implement)     OpenSpec change: proposed
                                        ↓ /opsx:spec
                                      OpenSpec change: specified
                                        ↓ /opsx:ff → /cleave
                                      OpenSpec change: implementing
                                        ↓ /assess spec
                                      OpenSpec change: verifying
                                        ↓ /opsx:archive
                                      OpenSpec change: archived
                                        ↓ [auto-transition]
Design node: implemented              (terminal)
```

Key invariants enforced:
1. No implementation OpenSpec change exists without a bound design node in `decided`+
2. No design node advances to `decided` without meeting its acceptance criteria (Level 2+)
3. Archive of the implementation change is the ONLY path to `implemented` on the design node — the design node never self-transitions
4. `/opsx:propose` standalone is relegated to untracked/throwaway work only, clearly labeled as such

### Reassessment: why Level 3 as default is correct (2026-03-12)



## Decisions

### Decision: Rename OpenSpec surface to "Implementation" throughout

**Status:** decided
**Rationale:** OpenSpec changes ARE the implementation sub-lifecycle of design nodes. Calling them "Implementation" rather than "OpenSpec" makes the two-layer model legible without any code change. Dashboard, agent guidelines, README, and docs all updated. /opsx:* command names stay unchanged (they're CLI, not UX).

### Decision: Design-tree-first entry point is mandatory for tracked work

**Status:** decided
**Rationale:** Implementation OpenSpec changes must always originate from design_tree_update(implement) on a decided node. /opsx:propose becomes explicitly "untracked/throwaway only" — agent guidelines updated, archive gate updated to reflect this intent rather than compensating for a missing constraint. Removes the missing_design_binding gate as a correctness mechanism (it becomes informational at most).

### Decision: Full design-phase OpenSpec is the DEFAULT for all exploring nodes

**Status:** decided
**Rationale:** Reversed from earlier position. At the implementation layer, tests catch mistakes. At the design layer, only the formal process catches mistakes — making it opt-in defeats the entire purpose. The ceremony IS the forcing function against shallow analysis and unexamined second-order effects. Advancing to `exploring` scaffolds a design OpenSpec change. `seed` is the only escape hatch for quick capture that may never advance. The cost is low: the node document IS the design artifact; the only truly new artifact is spec.md written at exploring time.

### Decision: Design node document IS the design.md artifact — no separate file

**Status:** decided
**Rationale:** docs/<node-id>.md already contains everything that would go in a design OpenSpec's design.md (Research + Decisions sections). Duplicating it into openspec/design/<node-id>/design.md creates sync burden. The design OpenSpec change references the node document as its artifact. openspec/design/<node-id>/ therefore contains only: proposal.md (one-liner + link), spec.md (acceptance criteria), tasks.md (auto-mirrored from Open Questions), assessment.json (from /assess design). The node document is the implementation of its own spec.

### Decision: Separate openspec/design/ subtree for design-phase changes

**Status:** decided
**Rationale:** Separate openspec/design/<node-id>/ and openspec/design-archive/ subtrees make the distinction explicit in git history, grep, and directory listings. A phase marker in proposal.md frontmatter inside a shared changes/ directory would be invisible in git log and requires filtered views everywhere. Explicit directory separation costs nothing and makes the two lifecycles unambiguous at the filesystem level.

### Decision: Retroactive migration for all 46 existing implemented nodes

**Status:** decided
**Rationale:** All existing nodes get design-phase OpenSpec changes generated retroactively from their existing content. The agent can perform this migration autonomously using its own harness — it has the design nodes, the OpenSpec tooling, and the memory system needed to reconstruct spec scenarios from existing Research/Decisions/Implementation Notes. Migration produces archived design changes (since the nodes are already decided/implemented), so the retroactive work goes straight to design-archive/ with a pre-dates-dual-lifecycle marker in the proposal. No node remains exempt — uniformity over convenience.

### Decision: ready query hard-gates on design spec present

**Status:** decided
**Rationale:** A decided node without a design spec is not actually ready — it skipped the formal exploration phase. ready returns only nodes where: status=decided, all deps=implemented, AND a design OpenSpec change exists in decided/archived state. Nodes that are decided but missing their design spec appear in blocked instead, with a synthetic blocking dep: "design spec not present". This enforces the pipeline at query time rather than relying on guideline compliance.

### Decision: implement action hard-gates: design OpenSpec must be in archived state

**Status:** decided
**Rationale:** Consistent with ready hard-gating on design spec present. If implement can be called on a decided node with no design spec, the entire enforcement model collapses. The gate is: design OpenSpec change exists in openspec/design-archive/ for this node. If it exists only in openspec/design/ (still active), implement blocks with "run /assess design and archive before implementing". If it doesn't exist at all, implement blocks with "scaffold design spec first via set_status(exploring)".

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/design-tree/index.ts` (modified) — set_status(exploring) scaffolds design OpenSpec change; set_status(decided) triggers /assess design gate; implement action enforced design-first
- `extensions/design-tree/tree.ts` (modified) — scaffoldDesignOpenSpecChange() new function; no separate design.md generated
- `extensions/openspec/index.ts` (modified) — Support openspec/design/ subtree OR phase: design marker; dashboard label change Implementation vs OpenSpec; /opsx:propose labeled as untracked
- `extensions/openspec/spec.ts` (modified) — computeStage and listChanges filter by phase when querying implementation vs design changes
- `extensions/openspec/archive-gate.ts` (modified) — missing_design_binding becomes informational only — binding now guaranteed by construction
- `extensions/dashboard/index.ts` (modified) — Rename OpenSpec section label to Implementation in footer/overlay
- `openspec/design/` (new) — New directory for design-phase OpenSpec changes (if separate subtree approach chosen)
- `extensions/design-tree/types.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/cleave/assessment.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/cleave/bridge.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/cleave/index.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `docs/design-tree.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- Design node document docs/<node-id>.md is the authoritative design artifact — no design.md in the OpenSpec change directory
- seed status nodes are exempt from design OpenSpec scaffolding — only exploring+ triggers it
- Archive of design OpenSpec change is the ONLY path to decided status, mirroring how archive of implementation change is the only path to implemented
- All 46 existing implemented nodes receive retroactive design-phase OpenSpec changes, archived immediately (pre-dates-dual-lifecycle marker in proposal.md) — no exemptions

## The failure mode I underweighted

My earlier "80% overkill" framing assumed the cost of full OpenSpec for design is ceremony with low return. That's wrong for one specific reason: **at the implementation layer, tests catch mistakes. At the design layer, nothing catches mistakes except the formal process itself.**

When a design node is explored without spec scenarios:
- The agent can declare "decided" based on surface-level analysis that feels complete but isn't
- There's no forcing function to articulate "what would falsify this decision"
- Second/third/fourth-order effects go unexamined — nothing requires documenting them
- The archive gate only checks "is there a decision?" not "is the decision well-reasoned?"

When Given/When/Then scenarios are written BEFORE exploration:
- "Given approach A, when concurrent writes arrive, then..."
- "Given this API shape, when a downstream consumer needs to extend it, then..."
- "Given this architectural choice, when requirement X changes in 6 months, then..."

The scenarios must be satisfiable by the documented decisions before archive is allowed. That's a real correctness check, not ceremony.

On THIS node specifically — if we'd written scenarios first, we'd have caught:
- "Given we apply OpenSpec to design nodes, when an existing project has 50 legacy nodes, then migration path must be defined" (not addressed)
- "Given design OpenSpec archives when decided, when a decision needs revisiting, then re-open path must exist without losing the archive" (not addressed)
- "Given design OpenSpec for all nodes, when a trivial exploration becomes unnecessary, then abandon path must exist without corruption" (not addressed)

Those are real constraints. The scenarios would have surfaced them. They didn't get surfaced because nothing required them.

## Why "80% overkill" was the wrong frame

Looking at the actual design tree (51 nodes, all architectural) — the "trivial decisions" I was imagining don't appear in this project. Every design choice here affects how the agent reasons about future work. `compaction-policy-hotfix`, `dashboard-cleanup`, even small nodes carry downstream effects. There ARE no truly throwaway design decisions in a meta-architecture project.

More fundamentally: the cost of full OpenSpec for design is lower than implied. A design node already HAS most of the content:
- `proposal.md` = one paragraph overview (already written at seed time)
- `spec.md` = 3–5 Given/When/Then scenarios for what "decided" means (this is the gap we're closing)
- `design.md` = the existing Research + Decisions sections (already being written)
- `tasks.md` = the existing Open Questions formatted as tasks (already being maintained)

The overhead is the spec scenarios written at `exploring` time, before the research begins. That's the whole point — writing the falsifiability conditions BEFORE you do the work you're trying to falsify.

## The document-as-artifact optimization

The design node markdown file already IS the design artifact. We don't need a separate `design.md` in the OpenSpec change directory — the node document serves that role. The OpenSpec machinery for design can be lighter than implementation:

```
openspec/design/<node-id>/
  proposal.md       ← one-liner intent + link to docs/<node-id>.md
  spec.md           ← the acceptance criteria for "decided" (THIS is the new thing)
  tasks.md          ← mirrored from Open Questions (auto-generated, kept in sync)
  assessment.json   ← /assess design result
  (no design.md — the node document IS the design artifact)
```

This means the full OpenSpec machinery runs but the total overhead above what already exists is exactly one file written at `exploring` time: `spec.md`. Everything else is either already written or auto-generated.

## Revised Level taxonomy

- **Level 1** — Label change: always, free, do it now
- **Level 2** — Acceptance criteria as body section: REMOVED — subsumed into Level 3, which makes this explicit in spec.md rather than a new body section
- **Level 3** — Full design-phase OpenSpec: THE DEFAULT for all `exploring` nodes. Not opt-in. Opt-out only for `seed`-only capture nodes that never advance.

The seed status is the escape hatch for quick capture. The moment you advance a node to `exploring`, a design OpenSpec change is scaffolded. That's the commitment point: "we are formally exploring this."
