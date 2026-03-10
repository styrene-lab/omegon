---
id: design-doc-lifecycle
title: Design Doc Lifecycle — Closing the Loop from Exploration to Reference Documentation
status: implemented
tags: [documentation, design-tree, lifecycle, architecture, dx]
open_questions: []
---

# Design Doc Lifecycle — Closing the Loop from Exploration to Reference Documentation

## Overview

The design tree has 33 implemented nodes — each is an exploration journal that captured the why, research, decisions, and implementation notes for a feature. But once implemented, these documents serve no structured purpose: they're not reference docs, not indexed for agent retrieval, and not connected to the code they describe. This exploration asks: how do we close the loop so completed design explorations become permanent, evolving reference documentation that future agents and humans can always find?

## Research

### Current Knowledge Sources — What Exists Today

Five distinct knowledge sources, each with different granularity, audience, and lifecycle:

| Source | Count | Granularity | Audience | Lifecycle |
|--------|-------|-------------|----------|-----------|
| **Design docs** (`docs/*.md`) | 34 files, 60-400 lines each | Exploration journal — research, decisions, rejected alternatives | Agent + human during active design | Frozen at "implemented" — never updated post-implementation |
| **OpenSpec baselines** (`openspec/baseline/`) | 32 spec files | Given/When/Then scenarios — behavioral contracts | Agent during verify/assess | Updated on archive (delta-merge) — always current |
| **Project memory** (`.pi/memory/facts.jsonl`) | 967 facts | Atomic facts — one sentence each | Agent context injection | Continuously updated, archived when stale |
| **README.md** | 1 file | High-level overview, extension list | Human onboarding, GitHub visitors | Manually maintained — already drifting |
| **CHANGELOG.md** | 1 file | Release-oriented summaries | Human + agent for version context | Updated per release — always current |

**The gap**: No structured *reference documentation* that says "here's what subsystem X does, how it works, what files it touches, and what decisions shaped it." Design docs come closest but they're exploration journals frozen at implementation time — they capture the *journey* but not the *current state*. README gives a surface-level overview. Project memory has the facts but scattered across 967 entries with no narrative structure.

### Design Doc Anatomy — What's Actually in These Files

Sampled 5 representative implemented docs. Consistent structure via frontmatter + markdown sections:

**Frontmatter**: `id`, `title`, `status`, `tags`, `open_questions`, `branches`, `openspec_change`, optionally `parent`, `related`.

**Sections** (not all present in every doc):
- **Overview**: 1-3 paragraphs stating the problem and proposed solution
- **Research**: Exploration findings — architecture options, feasibility assessments, API audits. Often the longest section. Contains tables, code snippets, rejected alternatives.
- **Decisions**: Formal decision records with title, status (decided/rejected), rationale. This is the highest-value content — the *why* behind the code.
- **Open Questions**: Empty in implemented nodes (questions resolved before implementation).
- **Implementation Notes**: File scope (which files were created/modified) and constraints. Added by the `implement` action when bridging to OpenSpec.

**Size range**: 72 lines (harness-upstream-error-recovery) to 398 lines (unified-dashboard). Average ~150 lines.

**Key observation**: The *Decisions* and *Implementation Notes* sections contain durable value. The *Research* sections contain valuable context but also a lot of exploration noise (rejected options, intermediate thinking). The *Overview* is usually a good summary but written for the exploration context, not as reference documentation.

### What Future Agents Actually Need

When a future agent session starts and needs to work on pi-kit, it needs to answer:

1. **"What does this subsystem do?"** — Concise description, not an exploration journal
2. **"What files implement it?"** — File scope, entry points, key exports
3. **"Why was it built this way?"** — Design decisions with rationale (not the rejected alternatives)
4. **"What contracts does it satisfy?"** — OpenSpec scenarios, behavioral guarantees
5. **"What are the known constraints?"** — Limitations, gotchas, things to watch out for
6. **"How does it connect to other subsystems?"** — Dependencies, event flows, shared state

Today's design docs answer #3 well (decisions) and #1 partially (overview). They don't reliably answer #2 (only if implementation notes were added), #4 (separate OpenSpec baselines), #5 (scattered in project memory), or #6 (no cross-reference structure).

**The ideal output**: A per-subsystem reference page that distills the design doc's durable content, links to the spec baseline, lists the file scope, and connects to related subsystems. Think of it as the design doc *after* the exploration noise is stripped away and the implementation reality is folded in.

### Architecture Options



### Option A: Transform Design Docs In-Place

When a node transitions to "implemented", rewrite the design doc into reference format:
- Strip research noise, keep decisions and constraints
- Add current file scope from the actual codebase (not just implementation notes)
- Link to OpenSpec baseline scenarios
- Add a "Related" section linking to parent/child/dependency nodes

**Pros**: No new files, docs/ stays as the single source. Simple mental model.
**Cons**: Loses the exploration history (research, rejected alternatives). Destructive — can't go back. The journey *is* valuable for understanding complex decisions.

### Option B: Two-Layer Architecture (Design Archive + Reference Docs)

Keep design docs as-is (frozen exploration journals). Generate a *second layer* of reference docs from them:
- `docs/design/` — existing exploration docs (renamed from `docs/`)
- `docs/reference/` — generated/maintained reference pages per subsystem

Reference docs are distilled from design docs + OpenSpec baselines + code reality. They're the "current truth" while design docs are the "historical record."

**Pros**: Preserves exploration history. Clean separation of concerns. Reference docs can be regenerated.
**Cons**: Two places to look. Duplication risk. Who maintains the reference layer?

### Option C: Design Docs + Structured Frontmatter Index

Keep design docs as-is. Add a structured index that extracts the durable content:
- A single `docs/INDEX.md` (or `docs/architecture.md`) that's auto-generated from all implemented nodes
- Each entry: title, one-line summary, key decisions, file scope, links to spec baselines
- The index is the "table of contents" — design docs are the "chapters"

**Pros**: Minimal new structure. Index can be regenerated. Design docs untouched.
**Cons**: Index can drift from reality. Doesn't solve the "exploration noise" problem for individual docs.

### Option D: Design Docs Evolve Through Lifecycle Stages

Add new sections to the design doc template that get filled at different lifecycle stages:
- `seed/exploring`: Overview, Research, Open Questions
- `decided`: Decisions added
- `implementing`: Implementation Notes, File Scope, Constraints
- `implemented`: **Reference Summary** added — a new top-level section that distills the doc into agent-friendly reference format
- `archived` (new status): Moves to `docs/archive/`, reference summary promoted to `docs/reference/`

The design doc is a living document that *gains* sections as it matures, culminating in a reference summary.

**Pros**: Single file through entire lifecycle. Natural progression. Reference section is explicitly written at close-out.
**Cons**: Files get long. Mixing exploration and reference in one doc.

### Option E: Reference Header + Design Archive (Hybrid B/D)

Combine the strengths of B and D with minimal file churn:

1. **Design docs move to `docs/design/`** — the full exploration journals are preserved as historical archive
2. **Reference pages live in `docs/`** — one per subsystem, answering the 6 agent questions in a standard template
3. **The reference page links back** to `docs/design/<id>.md` for historical context
4. **A `close-out` step** in the design tree lifecycle generates the initial reference page by distilling the design doc + scanning actual file scope from the codebase + linking OpenSpec baselines
5. **Reference pages are living documents** — when a subsystem changes, the reference page is updated (not the archived design doc)

**Reference page template:**
```markdown
---
subsystem: effort-tiers
design_doc: design/effort-tiers.md
openspec_baselines: [effort.md, routing.md]
last_updated: 2026-03-10
---
# Effort Tiers
> One-line: Global knob controlling local-vs-cloud inference ratio across the harness.

### Secondary Pass: Memory Ingestion — Closing the Loop Completely

The reference doc is a human-readable artifact. But agents don't browse `docs/` — they get facts injected from project memory. The final step in the close-out pipeline must extract structured facts from the distilled reference page and `memory_store` them.

**Three-stage pipeline:**
1. **Archive**: Design doc moves to `docs/design/` (frozen exploration journal)
2. **Distill**: Reference page generated in `docs/` (answering the 6 agent questions)
3. **Ingest**: Key facts extracted from reference page → stored in project memory sections

**What gets ingested per subsystem:**
- **Architecture section**: "Subsystem X does Y. Key files: a.ts, b.ts. See docs/x.md" — pointer facts with enough context for semantic retrieval
- **Decisions section**: "Decision: X chosen over Y because Z. See docs/x.md" — rationale-carrying facts
- **Constraints section**: "Constraint: X cannot do Y because Z" — limitation facts
- **Patterns section**: "Pattern: X uses Y approach for Z" — convention facts

**Why this matters:**
- `memory_recall("dashboard")` immediately surfaces "Dashboard extension provides compact/raised footer modes with live cleave progress. Key files: extensions/dashboard/. See docs/dashboard.md" — no file scanning needed
- Facts are semantically searchable — "what handles upstream errors?" finds the recovery controller facts
- Facts survive context compaction — they're in the persistent store, not conversation history
- Facts can be superseded when subsystems change — `memory_supersede` updates the pointer

**Why reference docs are still needed alongside memory:**
- Memory facts are atomic (one sentence each) — they can't carry the full narrative
- Reference docs provide depth when an agent needs to understand *how* something works, not just *what* it does
- The pointer pattern ("See docs/x.md") bridges memory → docs when depth is needed
- Humans can't read facts.jsonl — they need the markdown docs

**The complete lifecycle:**
```
seed → exploring → decided → implementing → implemented → close-out
                                                            ├── docs/design/x.md (archive)
                                                            ├── docs/x.md (reference)
                                                            └── memory facts (injected)
```

This means a future agent session gets:
1. **Automatic**: relevant facts injected via context pressure system
2. **On-demand**: `memory_recall("subsystem X")` for targeted retrieval
3. **Deep dive**: agent reads `docs/x.md` when pointer facts indicate relevance
4. **Historical**: agent reads `docs/design/x.md` when understanding rejected alternatives matters
5. **Contractual**: agent reads `openspec/baseline/` for behavioral guarantees

No scanning required. No exploration noise in the critical path. Full provenance chain preserved.

### Proposed Subsystem Groupings (33 nodes → ~15 reference pages)

| Reference Page | Design Nodes | Key Files |
|---|---|---|
| **Dashboard** | unified-dashboard, dashboard-wide-truncation, non-capturing-dashboard, clickable-dashboard | extensions/dashboard/ |
| **Cleave** | cleave-dirty-tree-checkpointing, cleave-title-progress-sync | extensions/cleave/ |
| **Model Routing** | codex-tier-routing, effort-tiers, provider-neutral-model-controls, compaction-fallback-chain, compaction-policy-hotfix | extensions/model-budget.ts, extensions/lib/model-routing.ts, extensions/effort-tiers.ts |
| **Error Recovery** | harness-upstream-error-recovery | extensions/model-budget.ts, extensions/lib/model-routing.ts |
| **Operator Profile** | operator-capability-profile, guardrail-capability-probe, bootstrap | extensions/bootstrap/, extensions/lib/operator-fallback.ts |
| **Design Tree** | design-tree-lifecycle | extensions/design-tree/ |
| **OpenSpec** | openspec-assess-lifecycle-integration, lifecycle-hygiene-verification-substates, assess-bridge-completed-results | extensions/openspec/, extensions/cleave/openspec.ts |
| **Lifecycle Reconciliation** | lifecycle-reconciliation, post-assess-reconciliation, lifecycle-artifact-versioning | extensions/openspec/reconcile.ts |
| **Project Memory** | memory-lifecycle-integration, memory-mind-audit, cheap-gpt-memory-models | extensions/project-memory/ |
| **Slash Command Bridge** | agent-assess-tooling-access, bridge-all-slash-commands | extensions/lib/slash-command-bridge.ts, extensions/cleave/bridge.ts |
| **Quality & Guardrails** | deterministic-guardrails, extension-type-safety | extensions/bootstrap/deps.ts |
| **Cost Reduction** | cost-reduction | (cross-cutting — strategies executed via child nodes) |
| **View & URI** | context-aware-uri | extensions/view/ |
| **Native Diagrams** | native-diagram-backend-mvp | extensions/render/ |
| **Tool Profiles** | smart-tool-profiles | extensions/tool-profiles/ |
| **Markdown Viewport** | markdown-viewport | (deferred — external repo) |

That's 16 reference pages covering 34 design nodes. Some nodes like `cost-reduction` are cross-cutting meta-strategies rather than subsystems — their reference page would describe the strategy and link to the subsystem pages that implement individual cost reduction tactics.

## Decisions

### Decision: Preserve exploration history in docs/design/, distill reference docs into docs/

**Status:** decided
**Rationale:** Exploration journals contain valuable context (rejected alternatives, research, intermediate reasoning) that helps when revisiting complex subsystems. Destroying them (Option A) loses irreplaceable context. The two-layer approach (Option E) preserves history while giving agents and humans a clean reference layer. Design docs move to docs/design/ as frozen archives. Reference pages live in docs/ as the current truth.

### Decision: Reference docs are per-subsystem, grouping related design nodes

**Status:** decided
**Rationale:** Many design nodes are incremental refinements of the same subsystem (e.g. unified-dashboard + dashboard-wide-truncation + non-capturing-dashboard + clickable-dashboard are all "Dashboard"). Per-node reference pages would create 33 files, many thin and overlapping. Per-subsystem grouping collapses these into ~12-15 cohesive reference pages. Each reference page lists which design nodes contributed to it, linking back to the individual docs/design/ archives for provenance. The subsystem grouping matches how agents actually think about the codebase — "the dashboard", "the cleave system", "model routing" — not individual feature increments.

### Decision: Three-stage close-out: archive → distill → ingest to memory

**Status:** decided
**Rationale:** Reference docs alone don't close the loop — agents get facts from project memory, not file scans. The close-out pipeline must: (1) archive the design doc to docs/design/, (2) generate a reference page in docs/ using a standard template, (3) extract pointer facts from the reference page and store them in project memory. This gives three retrieval tiers: automatic injection (memory facts), targeted search (memory_recall), and deep dive (read the reference doc). The pointer pattern ("X does Y. See docs/x.md") keeps facts atomic while preserving access to depth.

### Decision: Batch-generate initial reference pages, then incremental close-out going forward

**Status:** decided
**Rationale:** 33 implemented nodes need close-out. Waiting for each to be touched organically means most never get reference pages. Batch generation (agent reads design doc + scans code → drafts reference page → human reviews) is feasible in a single session. Going forward, close-out happens as part of the design tree lifecycle when a node transitions to implemented — either automated or via a /close-out command.

## Open Questions

*No open questions.*

## What It Does

[2-3 paragraphs — current behavior, not exploration framing]

## Key Files

| File | Role |
|------|------|
| `extensions/effort-tiers.ts` | Core extension — tier resolution, /effort command |
| `extensions/model-budget.ts` | Consumes tier to set model caps |

## Design Decisions

[Distilled from design doc — title + rationale only, no rejected alternatives]

## Behavioral Contracts

[Links to OpenSpec baseline scenarios]

## Constraints & Known Limitations

[From design doc + project memory]

## Related Subsystems

[Links to other reference pages — model-budget, cleave, etc.]
```

**Pros**: Clean separation (archive vs. current). Standard template makes agent retrieval predictable. Reference pages can be regenerated from design doc + code. Design history preserved.
**Cons**: 33 reference pages to generate initially (can be batched with local model). Two directories to understand. Need a convention for which layer to update when.
