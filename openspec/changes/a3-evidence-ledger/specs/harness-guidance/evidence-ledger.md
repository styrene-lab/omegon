# Harness Guidance Evidence Ledger — Delta Spec

## ADDED Requirements

### Requirement: Evidence ledger tracks discovery rate

The guidance loop must maintain a per-session evidence ledger derived from normalized observation events.

#### Scenario: Novel reads record discovery
Given a successful read observation for a path not previously observed
When guidance state updates from tool calls
Then the evidence ledger records the path as novel for that turn
And the turn's novelty rate is greater than zero

#### Scenario: Repeated reads record revisit churn
Given a path was already read in an earlier turn
When a later turn reads the same path without an intervening mutation or validation
Then the evidence ledger records a revisit for that turn
And the turn can contribute to low-novelty streaks

#### Scenario: Mutation and validation interrupt revisit decay
Given the evidence ledger has low-novelty revisit turns
When a mutation or validation observation is recorded
Then the low-novelty streak is interrupted

### Requirement: Evidence convergence uses novelty decay

The guidance loop must not treat a small number of read files as actionable evidence by itself.

#### Scenario: First targeted read is not actionable by count alone
Given no files have been modified
And the agent reads one targeted file successfully for the first time
When evidence is assessed
Then local evidence is targeted, not actionable
And global evidence is not actionable

#### Scenario: Repeated low-novelty revisits become actionable
Given no files have been modified
And the agent repeatedly rereads a known targeted file across multiple turns without mutation or validation
When evidence is assessed for that target
Then local evidence is actionable
