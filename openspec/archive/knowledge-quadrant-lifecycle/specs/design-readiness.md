# Design Readiness — Delta Spec

Retroactive spec (2026-07-01): this change was implemented and verified before spec registration; scenarios below document the shipped behavior for baseline merge.

## ADDED Requirements

### Requirement: Design nodes expose a readiness score

A design node's readiness is the ratio of accepted decisions to total knowledge items (decisions + open questions + assumptions). Rejected decisions are excluded.

#### Scenario: Node with mixed knowledge state
Given a design node with 3 accepted decisions, 1 rejected decision, 2 open questions, and 1 [assumption]-tagged question
When readiness_score() is computed
Then the score is 3 / (3 + 2 + 1) = 0.5
And the rejected decision does not contribute to numerator or denominator

#### Scenario: Fully resolved node
Given a design node whose only knowledge items are accepted decisions
When readiness_score() is computed
Then the score is 1.0

### Requirement: Assumptions are [assumption]-tagged open questions

Assumptions reuse the open-question lifecycle with an [assumption] text prefix — no new section type.

#### Scenario: Counting assumptions
Given a design node with open questions "[assumption] git is installed" and "how should errors render?"
When assumption_count() is computed
Then the count is 1
And both questions contribute to the readiness denominator

### Requirement: Readiness is surfaced in lifecycle queries and dashboard

The readiness score appears in design_tree node query responses and as a dashboard gauge for the focused node.

#### Scenario: Dashboard gauge for focused node
Given a design node is focused in the TUI
When the dashboard renders
Then a readiness gauge shows decisions/total with open-question and assumption breakdown

### Requirement: Readiness is advisory, never blocking

The score guides progression but must not gate status transitions.

#### Scenario: Transition with low readiness
Given a design node with readiness below 1.0
When the operator transitions it to decided
Then the transition succeeds
And readiness remains display-only

### Requirement: Exploration prompts surface assumptions

The design exploration prompt injection instructs the agent to surface assumptions as [assumption]-tagged open questions and to ask what unstated assumptions a design is making during assessment.

#### Scenario: Design exploration prompt
Given a design node is being explored
When the system prompt is assembled
Then it includes guidance to record assumptions as [assumption]-tagged open questions