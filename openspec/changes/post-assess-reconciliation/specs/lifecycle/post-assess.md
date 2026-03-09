# lifecycle/post-assess — Delta Spec

## ADDED Requirements

### Requirement: Failed or partial assessment reopens implementation lifecycle state

Assessment must be able to move a change back out of verifying when review discovers remaining required work.

#### Scenario: Spec assessment reopens a verifying change
Given an OpenSpec change has all tasks checked off and is currently in verifying
And `/assess spec` finds unresolved required work
When post-assess reconciliation runs
Then the change no longer appears fully complete for lifecycle purposes
And the lifecycle state reflects that implementation work has reopened
And the dashboard refreshes to show the updated state

#### Scenario: Cleave assessment warnings reopen lifecycle state
Given an OpenSpec-backed implementation completed and tasks were reconciled
And `/assess cleave` reports unresolved warnings or critical issues that require code changes
When post-assess reconciliation runs
Then the change is moved back to implementing state
And the operator is told that assessment reopened implementation work

### Requirement: Post-assess reconciliation updates design-tree implementation notes when scope expands

When review or fixes expand implementation beyond the original design-tree implementation notes, the design tree must stay synchronized with what actually changed.

#### Scenario: Follow-up fixes append new file scope entries
Given a bound design-tree node has implementation notes with an existing file scope
And post-assess fixes touch files not already listed in that file scope
When post-assess reconciliation runs
Then the design-tree implementation notes include the newly touched files
And the added entries are marked as reconciliation-driven implementation deltas

#### Scenario: Follow-up fixes append new constraints
Given assessment reveals a new implementation constraint that was not recorded in the design-tree node
When post-assess reconciliation runs
Then the implementation notes include the new constraint
And the constraint is appended without deleting existing notes

### Requirement: Post-assess reconciliation is best-effort and explicit

The first version should update lifecycle state conservatively and warn when human judgment is still required.

#### Scenario: Ambiguous assessment does not rewrite tasks semantically
Given an assessment result contains reviewer prose that cannot be safely mapped to specific OpenSpec tasks
When post-assess reconciliation runs
Then it does not attempt freeform semantic rewriting of tasks.md
And it emits an explicit warning describing the ambiguity

#### Scenario: Passed assessment preserves verifying state
Given an OpenSpec change is in verifying
And `/assess spec` or `/assess cleave` passes without reopening work
When post-assess reconciliation runs
Then the change remains in verifying
And dashboard state is refreshed without reopening implementation
