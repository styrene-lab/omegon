+++
id = "c66114f0-f39b-43fd-8a69-b4314b580a21"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# cleave/preflight — Delta Spec

## ADDED Requirements

### Requirement: Cleave runs a dirty-tree preflight before worktree dispatch

When `/cleave` is invoked with a dirty working tree, pi-kit must treat that as a workflow preflight step rather than failing immediately with a bare error.

#### Scenario: Clean tree proceeds without preflight interruption
Given the working tree is clean
When the operator invokes `/cleave`
Then cleave proceeds to its normal planning or dispatch flow
And no dirty-tree checkpoint prompt is shown

#### Scenario: Dirty tree shows classified preflight choices
Given the working tree contains uncommitted changes
When the operator invokes `/cleave`
Then pi-kit produces a preflight summary that distinguishes related changes, unrelated or unknown changes, and volatile artifacts
And the operator is offered actions to checkpoint, stash, continue without cleave, or cancel

### Requirement: Volatile artifacts do not block cleave by default

Approved volatile artifacts such as `.pi/memory/facts.jsonl` must remain visible but should not derail parallel execution the same way substantive implementation drift does.

#### Scenario: Volatile artifacts are visible but separately handled
Given the only dirty path is `.pi/memory/facts.jsonl`
When `/cleave` runs preflight
Then the preflight summary lists the file as volatile
And pi-kit does not treat it as substantive unrelated work
And the operator can choose a one-step volatile-only stash path

### Requirement: Checkpointing is an explicit operator-approved action

Cleave may prepare checkpoint actions, but it must not auto-commit accumulated work without explicit operator approval.

#### Scenario: Checkpoint action prepares a scoped commit
Given preflight identifies files confidently related to the active change
When the operator chooses checkpoint
Then pi-kit stages the related files
And proposes a conventional commit message scoped to the active change
And it performs the commit only after operator approval

### Requirement: Preflight handles transient low-confidence classification conservatively

When pi-kit cannot confidently classify dirty files as related to the active change, it must bias toward asking, stashing, or canceling rather than silently bundling them into the checkpoint.

#### Scenario: Unknown files are not silently included in checkpoint scope
Given the working tree contains files outside the active change scope and outside the volatile allowlist
When preflight classifies the dirty tree
Then those files are marked unrelated or unknown
And they are not automatically included in the checkpoint file set

### Requirement: Preflight works without an active OpenSpec change

Dirty-tree checkpointing must still function when `/cleave` runs outside an active OpenSpec lifecycle.

#### Scenario: Generic classification works without OpenSpec context
Given `/cleave` is invoked without an active OpenSpec change
And the working tree is dirty
When preflight runs
Then pi-kit still summarizes volatile and non-volatile changes
And it offers checkpoint, stash, continue-without-cleave, or cancel actions using generic git-state classification
