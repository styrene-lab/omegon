+++
id = "9abb47a2-0e1c-46b7-9558-6e5422a67f71"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# openspec/lifecycle-status

### Requirement: Task-complete changes expose concrete verification substates
OpenSpec status reporting MUST distinguish concrete post-implementation verification substates for task-complete changes instead of collapsing them all into a generic `verifying` label.

#### Scenario: missing assessment is surfaced explicitly
Given an OpenSpec change has all tasks completed
And no persisted `assessment.json` exists for that change
When lifecycle status is queried
Then the coarse stage remains compatible with existing verification semantics
And the reported verification substate is `missing-assessment`
And the next action points the operator or agent to run `/assess spec <change>`

#### Scenario: stale assessment is surfaced explicitly
Given an OpenSpec change has all tasks completed
And a persisted assessment record exists
And the assessment snapshot no longer matches the current implementation snapshot
When lifecycle status is queried
Then the reported verification substate is `stale-assessment`
And the next action points the operator or agent to refresh assessment for the current snapshot

#### Scenario: reopened work is surfaced explicitly
Given an OpenSpec change has all tasks completed
And the latest persisted assessment outcome is `reopen`
When lifecycle status is queried
Then the reported verification substate is `reopened-work`
And the next action points the operator or agent to complete follow-up work and reassess

#### Scenario: archive-ready state is surfaced explicitly
Given an OpenSpec change has all tasks completed
And the latest persisted assessment outcome is `pass`
And lifecycle binding and other archive gates are current
When lifecycle status is queried
Then the reported verification substate is `archive-ready`
And the next action points the operator or agent to archive the change

### Requirement: Missing lifecycle binding is reported as a first-class blocker
OpenSpec status and archive-readiness reporting MUST identify missing lifecycle binding separately from assessment freshness so operators can see why an otherwise passing change still cannot archive.

#### Scenario: unbound design-tree state blocks archive readiness
Given an OpenSpec change has all tasks completed
And its latest persisted assessment outcome is `pass`
And no valid design-tree binding can be established for that change
When lifecycle status is queried
Then the reported verification substate is `missing-binding`
And the next action instructs the operator or agent to bind the change to a design-tree node before archive

### Requirement: Binding truth is computed consistently across lifecycle surfaces
OpenSpec status, archive gating, and design-tree lifecycle metadata MUST agree on whether a change is bound by using the same binding rule set.

#### Scenario: explicit openspec_change binding is recognized everywhere
Given a design-tree node declares `openspec_change: my-change`
When lifecycle status, archive readiness, and design-tree metadata are evaluated
Then all three surfaces report that `my-change` is bound

#### Scenario: fallback id-based binding is recognized everywhere
Given a design-tree node id matches the OpenSpec change name `my-change`
And no explicit `openspec_change` field is present
When lifecycle status, archive readiness, and design-tree metadata are evaluated
Then all three surfaces report that `my-change` is bound
And they do not disagree about `boundToOpenSpec`

### Requirement: Coarse stage compatibility is preserved while adding substate detail
The lifecycle model MUST preserve the existing coarse stage semantics while adding explicit verification substate detail for newer status surfaces.

#### Scenario: verification substate augments but does not replace coarse stage
Given an OpenSpec change is task-complete and awaiting final lifecycle completion
When lifecycle status is queried through existing commands or tools
Then the change still reports a verification-compatible coarse stage
And an additional explicit substate communicates the concrete blocker or readiness state
