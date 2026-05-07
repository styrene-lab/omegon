+++
id = "bb412f7d-62da-4881-955a-2fc388a31df1"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# cleave/checkpoint

### Requirement: Confirmed checkpoints must verify post-checkpoint cleanliness before cleave continues

A confirmed checkpoint action MUST not report success and return control to cleave execution until the repository has been re-checked for remaining dirty paths.

#### Scenario: checkpointed files clean the tree and cleave continues
Given the operator selects `checkpoint`
And the approved checkpoint commit succeeds
And no dirty paths remain afterward
When dirty-tree preflight completes
Then cleave continues to worktree creation
And no subsequent generic dirty-tree blocker is shown for that checkpoint attempt

#### Scenario: checkpoint leaves excluded files dirty
Given the operator selects `checkpoint`
And the checkpoint commit succeeds for the approved related files
But unrelated, unknown, or otherwise excluded files still remain dirty afterward
When dirty-tree preflight re-checks the repository
Then preflight does not return success yet
And the operator is shown an explicit post-checkpoint diagnosis naming the remaining dirty paths and why they were not checkpointed
And cleave does not fall through to a later generic clean-worktree failure for the same attempt

### Requirement: Checkpoint failures must surface precise execution errors

When the checkpoint action cannot produce a valid commit boundary, cleave MUST report the concrete failure reason from the git operation instead of implying that the checkpoint succeeded.

#### Scenario: git commit fails during checkpoint creation
Given the operator approves a checkpoint message
And the underlying `git commit` command fails
When dirty-tree preflight handles the failure
Then the operator sees the git failure reason in the preflight surface
And the workflow remains in preflight resolution rather than pretending the checkpoint succeeded

#### Scenario: no approved checkpoint files remain stageable
Given the operator selects `checkpoint`
But the approved checkpoint file set is empty or no longer stageable by the time git runs
When dirty-tree preflight handles the checkpoint request
Then it reports that the checkpoint scope no longer produces a valid commit
And it does not exit preflight as though the checkpoint cleared the dirty tree
