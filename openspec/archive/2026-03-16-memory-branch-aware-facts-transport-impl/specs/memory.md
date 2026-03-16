# memory — Delta Spec

## ADDED Requirements

### Requirement: startup import still seeds live memory from tracked transport

The project-memory extension MUST continue to import `.pi/memory/facts.jsonl` into the SQLite fact store automatically at startup so durable knowledge remains portable across clones, branches, and machines.

#### Scenario: startup import seeds an empty or stale DB

Given `.pi/memory/facts.jsonl` contains durable facts
And the local SQLite fact store is empty or stale
When Omegon starts and the project-memory extension initializes
Then the tracked JSONL facts are imported into the live DB without requiring an explicit operator action

### Requirement: tracked facts transport is not rewritten on ordinary session shutdown

The project-memory extension MUST NOT rewrite tracked `.pi/memory/facts.jsonl` as an automatic side effect of ordinary session shutdown.

#### Scenario: branch-local session work does not dirty tracked transport by default

Given the session stores or reinforces durable facts in the SQLite fact store during ordinary branch work
And no explicit memory export or reconciliation action is invoked
When the session ends
Then `.pi/memory/facts.jsonl` is left unchanged on disk

### Requirement: memory transport can be exported explicitly

The system MUST provide an explicit path to rewrite tracked `.pi/memory/facts.jsonl` from the live SQLite fact store when the operator or lifecycle flow intends to reconcile durable memory transport.

#### Scenario: explicit export writes deterministic tracked transport

Given the live SQLite fact store contains durable facts that are not reflected in tracked `.pi/memory/facts.jsonl`
When the operator or a lifecycle reconciliation flow invokes the explicit memory transport export path
Then `.pi/memory/facts.jsonl` is rewritten deterministically from the store export
And repeated exports without intervening durable changes do not rewrite the file again

### Requirement: memory transport drift is reported separately from lifecycle artifact blockers

Readiness and lifecycle checks MUST distinguish `.pi/memory/facts.jsonl` drift from hard blockers involving untracked or missing durable lifecycle artifacts under `docs/` or `openspec/`.

#### Scenario: incidental memory drift does not masquerade as a lifecycle-doc failure

Given the repository has `.pi/memory/facts.jsonl` changes caused by live DB drift
And there are no untracked durable lifecycle artifacts under `docs/` or `openspec/`
When readiness-oriented checks run
Then the result reports memory transport drift as a separate state or warning
And it does not classify that drift as the same hard failure used for missing lifecycle documentation
