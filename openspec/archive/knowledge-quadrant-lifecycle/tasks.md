+++
id = "7c186522-2c84-48a2-8f05-4d73ddbffa44"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Knowledge quadrant lifecycle — guide design progression through the Rumsfeld Matrix — Tasks

## 1. core/crates/omegon/src/lifecycle/design.rs (modified)

- [x] 1.1 Add readiness_score() to DesignNode: count decisions vs open questions (including [assumption] tagged). Parse [assumption] prefix from question text. Add assumption_count() accessor.

## 2. core/crates/omegon/src/features/lifecycle.rs (modified)

- [x] 2.1 Include readiness score in design_tree node query response. /assess design enhancement: prompt the reviewer to surface assumptions as [assumption]-tagged questions.

## 3. core/crates/omegon/src/tui/dashboard.rs (modified)

- [x] 3.1 Show readiness gauge for focused node: decisions/total with ? and ⚠ breakdown. Render below focused node section.

## 4. core/crates/omegon/src/prompt.rs (modified)

- [x] 4.1 Add assumption-surfacing guidance to the design exploration system prompt injection: 'When exploring a design node, actively surface assumptions as [assumption]-tagged open questions.'

## 5. Cross-cutting constraints

- [x] 5.1 Readiness score is advisory — displayed in dashboard and /assess output, never blocks status transitions
- [x] 5.2 Assumptions are open questions prefixed with [assumption] — no new section type, same lifecycle as regular questions
- [x] 5.3 readiness = decisions / (decisions + open_questions) — includes both ? and ⚠ tagged questions in denominator
- [x] 5.4 /assess design should explicitly prompt: 'What assumptions is this design making that haven’t been stated?'

> Implementation note (2026-06-12): readiness_score() and assumption_count()
> landed in `lifecycle/types.rs` (not `lifecycle/design.rs` as originally
> scoped) with unit tests including rejected-decision exclusion. Dashboard
> badges/gauge in `tui/dashboard.rs`, lifecycle feature surface in
> `features/lifecycle.rs`, prompt injection in `prompt.rs`. Task 5.4's
> assess-time assumption question was added to the design-tree prompt
> injection — /assess design is an agent-level workflow, not a code path.
