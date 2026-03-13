---
id: alpha-omega-visual-language
title: α→ω Visual Language — Lifecycle Symbol System
status: exploring
tags: [theming, ux, symbols, lifecycle, 40k, dashboard, design-tree, openspec]
open_questions: []
---

# α→ω Visual Language — Lifecycle Symbol System

## Overview

Bake a coherent α→ω Greek letter symbol system throughout all lifecycle surfaces: design tree node statuses, OpenSpec stages, cleave states, dashboard/footer icons, and the effort tier display. The arc from α (origin/seed) to ω (complete/archived) should be visually readable at a glance — any engineer looking at the dashboard should immediately understand where in the lifecycle something lives without reading the label.

## Research

### Current icon inventory across all surfaces

**Design tree — STATUS_ICONS (types.ts):**
| Status | Current | Meaning |
|---|---|---|
| seed | ◌ | origin, latent |
| exploring | ◐ | half-lit, active investigation |
| decided | ● | full, crystallized |
| implementing | ⚙ | gear, in motion |
| implemented | ✓ | done |
| blocked | ✕ | hard stop |
| deferred | ◑ | half-lit other side, paused |

**OpenSpec — stageIcon (openspec/index.ts):**
| Stage | Current |
|---|---|
| proposed | ◌ |
| specified | ◐ |
| planned | ▸ |
| implementing | ⟳ |
| verifying | ◉ |
| archived | ✓ |

**Dashboard footer — nodeStatusIcon (dashboard/footer.ts):**
Mirrors design tree but with theme colors applied. Also: `◐` used for the dashboard extension badge itself.

**Effort tiers (existing 40k naming):**
Servitor → Adept → Magos → Omnissiah (4 tiers, currently shown as model tier labels not symbols)

### Proposed α→ω symbol mapping

The key insight: α and ω are the terminal bookends. Everything between them uses Greek letters that have semantic resonance with the stage, not just sequential assignment.

**Design Tree — the exploration→decision arc:**
| Status | Symbol | Rationale |
|---|---|---|
| seed | α | Alpha — the origin, the first thought |
| exploring | β | Beta — active investigation, not yet proven |
| decided | Δ | Delta — a decision IS a change, crystallized inflection point |
| implementing | ξ (xi) | Xi — complex, in motion, mid-process (visually busy, fits) |
| implemented | ω | Omega — complete, the full arc closed |
| blocked | ✕ | Keep — universal, not Greek, immediately understood |
| deferred | ∂ | Partial derivative — incomplete, held in potential |

**OpenSpec — the spec→archive arc:**
| Stage | Symbol | Rationale |
|---|---|---|
| proposed | α | Same alpha — new thing entering the system |
| specified | σ | Sigma — summation, the requirements gathered |
| planned | π | Pi — the plan is the ratio, the proportional breakdown |
| implementing | ξ | Xi — same as design tree implementing, consistent |
| verifying | φ | Phi — the golden ratio, verification is the quality gate |
| archived | ω | Same omega — archived is complete |

**Effort tiers — the operator capability arc:**
| Tier | Symbol | Name | Rationale |
|---|---|---|---|
| 1 | α | Servitor | Minimal, rote, mechanical |
| 2 | β | Adept | Competent, standard work |
| 3 | γ | Magos | Expert, complex reasoning |
| 4 | ω | Omnissiah | Maximum, no constraint |

**The coherence principle:**
- α always means "new/origin/entering" — consistent across all systems
- ω always means "complete/closed/done" — consistent across all systems  
- π appears at the planning stage of OpenSpec — the ratio, the proportional breakdown of work. Also a nod to the pi heritage.
- σ for specification — summation of requirements
- φ for verification — the golden ratio, the quality standard
- Δ for decided — a decision is literally a delta, a change in direction
- ∂ for deferred — partial, incomplete, held in potential

### Implementation surface map — files to touch

| File | Change |
|---|---|
| `extensions/design-tree/types.ts` | `STATUS_ICONS` — replace ◌◐●⚙✓◑ with α β Δ ξ ω ∂ |
| `extensions/openspec/index.ts` | `stageIcon()` — replace ◌◐▸⟳◉✓ with α σ π ξ φ ω |
| `extensions/dashboard/footer.ts` | `nodeStatusIcon()` — mirror types.ts; update effort tier display |
| `extensions/dashboard/overlay-data.ts` | Any hardcoded icons in lifecycle pipeline display |
| `extensions/cleave/index.ts` | Cleave phase indicators if any (planning/dispatching/running/merging/complete) |

**Terminal rendering note:** All proposed symbols (α β Δ ξ ω σ π φ ∂) are in Unicode Basic Multilingual Plane and render correctly in any modern terminal (iTerm2, Alacritty, WezTerm, kitty). The existing dashboard already uses Unicode successfully (◐ ◉ etc.).

### Terminal sizing — α vs ω resolved

Capital `Ω` (U+03A9) renders larger than lowercase `α` (U+03B1) in monospace terminals — inconsistent visual weight at the bookends.

**Resolution: use lowercase `ω` (U+03C9) throughout.**

- Same visual weight as `α` — consistent bookends
- `α...ω` is the canonical mathematical convention for full scalar range (lowercase for scalars/indices)
- Set theory uses `ω` as the first infinite ordinal — "the complete arc"
- Capital `Α` (alpha) is visually identical to Latin `A` — not a viable alternative
- Mathematical italic variants (`𝛼`, `𝜔`) live in supplementary Unicode planes — inconsistent terminal rendering

Updated terminal glyph: `ω` replaces `ω` everywhere in the system. The arc reads `α → ω`.

## Decisions

### Decision: Use lowercase ω (U+03C9) not capital Ω throughout

**Status:** decided
**Rationale:** Consistent visual weight with α. α→ω is the mathematical convention for full scalar range. Capital Α would be indistinguishable from Latin A. ω is unambiguous, same size, and mathematically more precise.

## Open Questions

*No open questions.*
