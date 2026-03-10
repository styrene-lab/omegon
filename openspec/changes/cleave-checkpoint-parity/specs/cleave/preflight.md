# cleave/preflight — Delta Spec

## ADDED Requirements

### Requirement: cleave_run uses the same dirty-tree preflight as /cleave

All cleave execution entrypoints SHALL resolve dirty-tree state through the same preflight policy before worktree creation begins.

#### Scenario: Tool path shows preflight instead of bare clean-tree failure

Given the repository contains non-volatile uncommitted changes
When `cleave_run` is invoked
Then pi-kit evaluates the dirty tree through the preflight workflow
And pi-kit does not fail immediately with only a bare "commit or stash before cleaving" error

#### Scenario: Clean tree still proceeds directly

Given the repository working tree is clean
When `/cleave` or `cleave_run` is invoked
Then pi-kit proceeds without a dirty-tree interruption

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
Then pi-kit offers a volatile-specific resolution path
And pi-kit does not require a checkpoint commit for unrelated volatile artifacts

### Requirement: project-memory avoids rewriting facts.jsonl when export content is unchanged

The memory export path SHALL not dirty the repository merely by rewriting identical JSONL content.

#### Scenario: unchanged export leaves facts.jsonl untouched

Given the current exported memory JSONL matches the existing `.pi/memory/facts.jsonl` content exactly
When project-memory performs its export step
Then pi-kit does not rewrite the file
And the file mtime and git dirty state remain unchanged

### Requirement: checkpoint approval uses a single structured confirmation flow

When cleave preflight prepares a checkpoint commit, the operator SHALL make one explicit approval decision through a structured confirmation surface.

#### Scenario: operator approves checkpoint in one confirmation step

Given preflight identifies related checkpointable files
And pi-kit has prepared a suggested checkpoint commit message
When the operator approves the checkpoint
Then pi-kit stages the prepared file set
And pi-kit creates the checkpoint commit with the approved message
And pi-kit does not require separate free-text approval prompts after the structured confirmation

#### Scenario: operator cancels checkpoint without side effects

Given preflight identifies related checkpointable files
When the operator declines the checkpoint confirmation
Then pi-kit does not stage files
And pi-kit does not create a commit

## MODIFIED Requirements

### Requirement: volatile-only policy default

Volatile-only dirty trees SHALL use a low-friction default path that minimizes repeated interruptions while preserving operator visibility.

#### Scenario: volatile-only dirty tree can be resolved without repeated manual git choreography

Given the only dirty files are volatile artifacts
When cleave preflight resolves the dirty tree
Then the workflow requires at most one operator decision
And pi-kit performs any necessary stash or continue mechanics without asking for unrelated checkpoint details

### Requirement: shared confirmation surface across execution modes

The slash-command and tool-driven cleave paths SHALL share the same checkpoint confirmation behavior.

#### Scenario: /cleave and cleave_run present equivalent checkpoint approval semantics

Given both `/cleave` and `cleave_run` encounter the same checkpointable dirty tree
When each path presents checkpoint approval
Then both paths use the same prepared file scope and suggested commit message semantics
And neither path falls back to a stale multi-prompt approval sequence
