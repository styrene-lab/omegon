+++
id = "14ede778-eec8-4be6-84f3-8cba3fb623b7"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# lifecycle

### Requirement: implement auto-checkouts the directive branch and sets focus

#### Scenario: implement creates branch, checks it out, focuses design node, and forks mind

Given a design node with id "my-feature" in status "decided"
When design_tree_update(implement) is called for "my-feature"
Then a git branch "feature/my-feature" is created and checked out
And the design-tree focused node is set to "my-feature"
And a mind lifecycle fork request is queued for "directive/my-feature"
And a mind lifecycle activate request is queued for "directive/my-feature"

### Requirement: Session start detects branch↔mind consistency

#### Scenario: Active directive mind matches current branch

Given the active mind is "directive/my-feature"
And the current git branch is "feature/my-feature"
When session_start fires
Then no mismatch warning is emitted

#### Scenario: Active directive mind does not match current branch

Given the active mind is "directive/my-feature"
And the current git branch is "main"
When session_start fires
Then a context message surfaces the mismatch: directive expects "feature/my-feature" but current branch is "main"

#### Scenario: No active directive — no consistency check

Given the active mind is null (default)
When session_start fires
Then no branch consistency check runs

### Requirement: Dashboard shows active directive indicator

#### Scenario: Directive mind is active

Given the active mind is "directive/my-feature"
When the dashboard footer renders
Then the footer includes a directive indicator showing "my-feature"

#### Scenario: No directive mind active

Given the active mind is null (default)
When the dashboard footer renders
Then no directive indicator is shown

### Requirement: Mind system parent-chain inheritance

#### Scenario: forkMind creates lightweight child with zero fact copy

Given the "default" mind has 100 active facts
When forkMind("default", "directive/test", "test") is called
Then a mind record "directive/test" is created with parent="default"
And zero facts are copied (fact count for "directive/test" only = 0)
And getActiveFacts("directive/test") returns all 100 parent facts

#### Scenario: Facts stored in child shadow parent facts with same content

Given "default" has a fact with content "X is Y"
And "directive/test" is forked from "default"
When a fact "X is Z" is stored in "directive/test" (different content_hash)
Then getActiveFacts("directive/test") returns both the parent's "X is Y" and the child's "X is Z"

#### Scenario: Exact duplicate in parent prevents re-creation in child

Given "default" has a fact with content_hash H
And "directive/test" is forked from "default"
When storeFact is called on "directive/test" with the same content (hash H)
Then the parent's fact is reinforced, no new fact created

#### Scenario: ingestMind copies only child-owned facts

Given "directive/test" forked from "default"
And "directive/test" has 3 facts stored directly (not inherited)
When ingestMind("directive/test", "default") is called
Then only those 3 facts are considered for ingestion (not the full inherited set)

#### Scenario: sweepDecayedFacts only sweeps own facts

Given "directive/test" forked from "default"
And "default" has decayed facts below minimum confidence
When sweepDecayedFacts("directive/test") runs
Then only facts directly in "directive/test" are swept, not parent facts
