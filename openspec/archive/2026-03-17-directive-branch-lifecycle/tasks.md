+++
id = "41d5b89c-de14-44ae-86c1-4e607ee4f1d1"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# directive-branch-lifecycle — Tasks

## 1. implement auto-checkouts the directive branch and sets focus

- [x] 1.1 implement creates branch, checks it out, focuses design node, and forks mind
- [x] 1.2 implement publishes activeDirective to shared state

## 2. Session start detects branch↔mind consistency

- [x] 2.1 Active directive mind matches current branch — no warning
- [x] 2.2 Active directive mind does not match current branch — context message emitted
- [x] 2.3 No active directive — no consistency check

## 3. Dashboard shows active directive indicator

- [x] 3.1 Directive mind is active — footer shows directive indicator
- [x] 3.2 No directive mind active — no indicator shown
- [x] 3.3 activeMind published to shared state for dashboard

## 4. Mind system parent-chain inheritance

- [x] 4.1 forkMind creates lightweight child with zero fact copy
- [x] 4.2 Facts stored in child shadow parent facts with same content
- [x] 4.3 Exact duplicate in parent prevents re-creation in child
- [x] 4.4 ingestMind copies only child-owned facts
- [x] 4.5 sweepDecayedFacts only sweeps own facts
- [x] 4.6 resolveMindChain caching tests
