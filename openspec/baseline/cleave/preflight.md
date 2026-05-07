+++
id = "ea198db6-13e2-45d3-b8c3-9d3867fd643e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# cleave/preflight

### Requirement: Cleave runs a dirty-tree preflight before worktree dispatch

When `/cleave` is invoked with a dirty working tree, Omegon must treat that as a workflow preflight step rather than failing immediately with a bare error.

#### Scenario: Clean tree proceeds without preflight interruption
Given the working tree is clean
When the operator invokes `/cleave`
Then cleave proceeds to its normal planning or dispatch flow
And no dirty-tree checkpoint prompt is shown

#### Scenario: Dirty tree shows classified preflight choices
Given the working tree contains uncommitted changes
When the operator invokes `/cleave`
Then Omegon produces a preflight summary that distinguishes related changes, unrelated or unknown changes, and volatile artifacts
And the operator is offered actions to checkpoint, stash, continue without cleave, or cancel

### Requirement: Volatile artifacts do not block cleave by default

Approved volatile artifacts such as `.pi/memory/facts.jsonl` must remain visible but should not derail parallel execution the same way substantive implementation drift does.

#### Scenario: Volatile artifacts are visible but separately handled
Given the only dirty path is `.pi/memory/facts.jsonl`
When `/cleave` runs preflight
Then the preflight summary lists the file as volatile
And Omegon does not treat it as substantive unrelated work
And the operator can choose a one-step volatile-only stash path

### Requirement: Checkpointing is an explicit operator-approved action

Cleave may prepare checkpoint actions, but it must not auto-commit accumulated work without explicit operator approval.

#### Scenario: Checkpoint action prepares a scoped commit
Given preflight identifies files confidently related to the active change
When the operator chooses checkpoint
Then Omegon stages the related files
And proposes a conventional commit message scoped to the active change
And it performs the commit only after operator approval

### Requirement: Preflight handles transient low-confidence classification conservatively

When Omegon cannot confidently classify dirty files as related to the active change, it must bias toward asking, stashing, or canceling rather than silently bundling them into the checkpoint.

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
Then Omegon still summarizes volatile and non-volatile changes
And it offers checkpoint, stash, continue-without-cleave, or cancel actions using generic git-state classification

### Requirement: cleave_run uses the same dirty-tree preflight as /cleave

All cleave execution entrypoints SHALL resolve dirty-tree state through the same preflight policy before worktree creation begins.

#### Scenario: Tool path shows preflight instead of bare clean-tree failure

Given the repository contains non-volatile uncommitted changes
When `cleave_run` is invoked
Then Omegon evaluates the dirty tree through the preflight workflow
And Omegon does not fail immediately with only a bare "commit or stash before cleaving" error

#### Scenario: Clean tree still proceeds directly

Given the repository working tree is clean
When `/cleave` or `cleave_run` is invoked
Then Omegon proceeds without a dirty-tree interruption

### Requirement: volatile-only dirty trees are handled separately from substantive drift

Tracked volatile operational artifacts SHALL remain visible but should not force the same workflow as substantive feature changes.

#### Scenario: facts.jsonl-only drift is classified as volatile

Given the only dirty path is `.pi/memory/facts.jsonl`
When `/cleave` or `cleave_run` evaluates preflight
Then the path is classified as volatile
And the operator is not forced through the unrelated-file checkpoint flow

#### Scenario: volatile-only resolution does not require substantive checkpointing

Given all dirty paths are on the volatile allowlist
When cleave preflight runs
Then Omegon offers a volatile-specific resolution path
And Omegon does not require a checkpoint commit for unrelated volatile artifacts

### Requirement: project-memory avoids rewriting facts.jsonl when export content is unchanged

The memory export path SHALL not dirty the repository merely by rewriting identical JSONL content.

#### Scenario: unchanged export leaves facts.jsonl untouched

Given the current exported memory JSONL matches the existing `.pi/memory/facts.jsonl` content exactly
When project-memory performs its export step
Then Omegon does not rewrite the file
And the file mtime and git dirty state remain unchanged

### Requirement: checkpoint approval uses a single structured confirmation flow

When cleave preflight prepares a checkpoint commit, the operator SHALL make one explicit approval decision through a structured confirmation surface.

#### Scenario: operator approves checkpoint in one confirmation step

Given preflight identifies related checkpointable files
And Omegon has prepared a suggested checkpoint commit message
When the operator approves the checkpoint
Then Omegon stages the prepared file set
And Omegon creates the checkpoint commit with the approved message
And Omegon does not require separate free-text approval prompts after the structured confirmation

#### Scenario: operator cancels checkpoint without side effects

Given preflight identifies related checkpointable files
When the operator declines the checkpoint confirmation
Then Omegon does not stage files
And Omegon does not create a commit
