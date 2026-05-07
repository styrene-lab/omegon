+++
id = "27f06a8d-889f-47f5-8502-8e8a8949c9d2"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# lifecycle/reconciliation — Delta Spec

## ADDED Requirements

### Requirement: Cleave reconciles OpenSpec task state after successful merge

After a cleave run completes against an OpenSpec change, the harness must reconcile tasks.md so the change status reflects the merged reality rather than the original plan.

#### Scenario: Completed cleave groups are checked off
Given an OpenSpec change with a tasks.md file containing multiple task groups
And cleave runs with openspec_change_path pointing at that change
When one or more child groups complete and merge successfully
Then the tasks belonging to the completed groups are marked done in tasks.md
And unfinished groups remain unchecked
And the resulting OpenSpec stage reflects the updated checkbox counts

#### Scenario: Cleave reports stale reconciliation when completed work cannot be written back
Given a cleave run completed successfully for an OpenSpec change
And one or more completed children do not map back to any task group in tasks.md
When the cleave report is produced
Then the report includes a lifecycle reconciliation warning
And the warning explains that tasks.md no longer matches the implementation plan

### Requirement: Archive enforces lifecycle reconciliation before closing a change

Archiving an OpenSpec change must protect dashboard trust by refusing to silently close changes whose lifecycle metadata is obviously stale.

#### Scenario: Archive blocks unreconciled incomplete tasks
Given an OpenSpec change has tasks.md with incomplete tasks
When the operator attempts to archive the change
Then the archive is refused
And the response explains that the lifecycle state is stale until tasks are reconciled or completed

#### Scenario: Archive blocks changes with no design-tree binding
Given an OpenSpec change is being archived
And no design-tree node is bound to the change by openspec_change or matching node id
When the archive is attempted
Then the archive is refused
And the response explains that the change must be bound to a design-tree node before closing the lifecycle

### Requirement: Skills and lifecycle prompts require reconciliation checkpoints

The harness guidance for OpenSpec and cleave must describe reconciliation as a required lifecycle step so agent behavior stays aligned with the runtime automation.

#### Scenario: Cleave guidance includes post-execution reconciliation
Given the cleave skill or tool guidance is read by the agent
When it describes the OpenSpec lifecycle
Then it states that tasks.md must be reconciled after cleave execution
And it instructs the agent to refresh lifecycle state before archive

#### Scenario: OpenSpec guidance includes reconciliation checkpoints
Given the OpenSpec skill or tool guidance is read by the agent
When it describes implementation and archive flow
Then it states that lifecycle reconciliation is automatic at known checkpoints
And it warns that archive will refuse obviously stale lifecycle state
