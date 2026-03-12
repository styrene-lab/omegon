---
id: design-change-reopen
title: Design Change Re-open — revisiting an archived design decision
status: decided
parent: dual-lifecycle-openspec
open_questions:
  - When a design decision needs revisiting after archive (decided → re-exploring), what happens to the archived design OpenSpec change? Re-open it (breaks archive immutability), create a new revision change, or treat the node re-open as a new design cycle with a new change?
---

# Design Change Re-open — revisiting an archived design decision

## Overview

> Parent: [Dual-Lifecycle OpenSpec — Design Layer + Implementation Layer](dual-lifecycle-openspec.md)
> Spawned from: "When a design decision needs revisiting after archive (decided → re-exploring), what happens to the archived design OpenSpec change? Re-open it (breaks archive immutability), create a new revision change, or treat the node re-open as a new design cycle with a new change?"

*To be explored.*

## Research

### Options for revisiting an archived design decision



## Decisions

### Decision: Option C — New cycle same ID, prior archive preserved

**Status:** decided
**Rationale:** Archive immutability is non-negotiable — it's the audit trail. Re-opening archives (Option A) breaks this. Versioned sibling names (Option B) create binding ambiguity that compounds. A new cycle at the same ID preserves the archive exactly, keeps binding trivial, and matches how implementation archives already work. Scaffolding copies prior spec.md as a starting point so revision doesn't feel like starting from scratch. Amendment records (Option D) are allowed only for non-substantive corrections with an explicit guard in the tooling.

## Open Questions

- When a design decision needs revisiting after archive (decided → re-exploring), what happens to the archived design OpenSpec change? Re-open it (breaks archive immutability), create a new revision change, or treat the node re-open as a new design cycle with a new change?

## Option A — Re-open the archived change (move back to active)

`git mv openspec/design-archive/2026-03-12-<id>/ openspec/design/<id>/`

Reset stage by removing the assessment record. Node transitions back to `exploring`.

**Pros:** Simple. One change, everything is back in place. The existing spec scenarios are the starting point for the revision — you update them rather than starting over.

**Cons:** Mutates git history in a confusing way. The archive timestamp becomes a lie (it's no longer archived at that date). Anyone reading git log sees a directory appearing, disappearing, reappearing. Archive is meant to be a record of what was decided and when — breaking that breaks auditability.

---

## Option B — New revision change (versioned sibling)

Create `openspec/design/<id>-r2/` (or `<id>-rev-2026-03-14/`). Original stays archived.
New change starts from a copy of the original spec, updated for the revision.

**Pros:** Full audit trail — original decision preserved, revision is a discrete new artifact with its own timeline. You can diff the two spec files to understand what changed in the thinking.

**Cons:** Node ID and change name diverge (`my-feature` node → `my-feature-r2` design change). Binding logic needs to handle versioned names. The "current" design change for a node becomes ambiguous — is it the latest revision, or the first?

---

## Option C — New cycle, same ID (archive absorbs the old, fresh directory)

The archive already timestamps entries as `YYYY-MM-DD-<id>`. Re-opening means:
1. The existing archived entry stays at `openspec/design-archive/2026-03-12-<id>/` — untouched
2. A fresh `openspec/design/<id>/` is scaffolded — new proposal.md, new spec.md, new tasks.md
3. The new spec.md explicitly references the prior archived cycle as context

**Pros:** Identical to how implementation re-opens work (or would work). Archive is truly immutable — the old decision is preserved exactly as it was. The new cycle is a clean slate that can revise the spec scenarios without carrying forward the old ones implicitly. Git history shows the old archive directory, then a new active directory — clear timeline. Binding by node ID always resolves to the single active design change.

**Cons:** If the revision is minor (small clarification), starting from an empty spec.md feels heavy. Mitigated by: the scaffolding can pre-populate from the prior archived spec as a starting point.

---

## Option D — Amendment record (append, don't replace)

Don't re-open at all. Instead, append an `amendment.md` to the archived change directory documenting what changed and why. The node gets a new `## Amendment` section rather than cycling back to exploring.

**Pros:** Lowest friction for small corrections. No directory movement, no new lifecycle.

**Cons:** The amendment is unverified — there's no scenario assessment for the revised position, no formal "done" for the amendment. This is exactly the vibes-based pattern we're trying to eliminate. Only appropriate for trivial factual corrections, not substantive decision changes.

---

## Analysis

Option C is the correct default. It preserves archive immutability (same model as implementation archives), keeps binding simple (always `openspec/design/<node-id>/` for the active change), and creates a clean audit trail in git. The scaffolding copying from the prior archived spec mitigates the "starting from scratch" cost.

Option D is acceptable for trivial corrections (typos, factual updates) but must be explicitly not allowed for substantive decision changes — needs a guard.

Option A breaks archive semantics. Option B creates naming complexity that compounds over multiple revisions.
