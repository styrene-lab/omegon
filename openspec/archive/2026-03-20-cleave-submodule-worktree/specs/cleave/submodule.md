+++
id = "d86afa2f-a4e7-4819-bf40-65ba6f105629"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# cleave/submodule — Delta Spec

## CHANGED Requirements

### Requirement: Submodule commits on both success and failure paths

The orchestrator must attempt to commit dirty submodule changes regardless of child exit status.

#### Scenario: Successful child with submodule edits

Given a child completes successfully (exit 0)
And it modified files inside a git submodule
When the orchestrator harvests the child
Then commit_dirty_submodules runs
And the submodule changes are committed inside the submodule
And the pointer update is committed in the parent worktree

#### Scenario: Failed child with submodule edits

Given a child fails (exit 1 or timeout -1)
And it had modified files inside a git submodule before failing
When the orchestrator harvests the child
Then commit_dirty_submodules still runs
And whatever work the child produced is preserved

#### Scenario: Failed child with no submodule edits

Given a child fails (exit 1)
And it made no changes to any files
When the orchestrator harvests the child
Then commit_dirty_submodules runs but produces zero commits
And the merge reports "no new commits"

### Requirement: Worktree health check after submodule init

After submodule_init, the orchestrator verifies that at least one scope file is accessible.

#### Scenario: Scope file inside submodule is accessible

Given a child with scope ["core/crates/omegon-secrets/src/vault.rs"]
And submodule_init succeeds
When verify_scope_accessible runs
Then the check passes
And the child proceeds to dispatch

#### Scenario: Scope file inside submodule is NOT accessible

Given a child with scope ["core/crates/missing/file.rs"]
And submodule_init ran
When verify_scope_accessible runs
Then the check fails
And the child is marked as failed with error "scope file not accessible after submodule init"
And the child is NOT dispatched

#### Scenario: Child with no scope skips health check

Given a child with scope []
When verify_scope_accessible runs
Then the check passes (vacuously)

### Requirement: Submodule context in task files

Task files include a note about submodule structure when scope crosses a submodule boundary.

#### Scenario: Scope includes submodule path

Given a child with scope ["core/crates/omegon/src/lib.rs"]
And "core" is a registered git submodule
When the task file is generated
Then it includes a note mentioning "core/ is a git submodule"
And it says "edit files normally — the orchestrator handles submodule commits"

#### Scenario: Scope does not cross submodule boundary

Given a child with scope ["extensions/cleave/index.ts"]
And no submodule contains that path
When the task file is generated
Then no submodule note is included

## ADDED Requirements

### Requirement: Dirty-tree preflight submodule classification

The TS dirty-tree preflight recognizes submodule paths in git status output.

#### Scenario: Modified submodule detected in git status

Given git status shows " m core" (modified submodule content)
And .gitmodules declares "core" as a submodule
When inspectGitState parses the output
Then the entry for "core" is classified with submodule: true

#### Scenario: No submodules in repo

Given a repo with no .gitmodules
When inspectGitState parses the output
Then no entries have submodule: true
