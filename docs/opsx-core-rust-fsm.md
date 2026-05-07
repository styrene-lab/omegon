+++
id = "f1e580a2-e313-4c2b-bbd3-384404702bda"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# opsx-core — Rust-backed lifecycle FSM for OpenSpec enforcement

## Overview

Replace markdown-as-source-of-truth with a Rust state machine that owns the lifecycle. Markdown becomes the UI/display layer, not the authority. Components: lifecycle FSM (statig), task DAG (daggy/dagcuter), spec validator (jsonschema + garde), state store (sled). Scoped to Omega (enterprise orchestrator), not Omegon (single-operator tool). The single-operator workflow stays git-native markdown; the fleet orchestration layer gets enforcement.

## Research

### jj/git compatibility and synergy



## Decisions

### Decision: JSON files in repo as state store, not sled — jj/git IS the transaction log

**Status:** exploring
**Rationale:** sled adds opacity and a dependency. Structured JSON files in .omegon/lifecycle/ are transparent, diffable, mergeable, and versioned by jj/git for free. The VCS operation log becomes the audit trail. sled is reserved for Omega's fleet-scale ACID requirements. Single-operator Omegon doesn't need an embedded database when the filesystem + VCS already provides persistence, transactions (commits), and conflict resolution (jj conflicts).

### Decision: Shared library crate — both Omegon and Omega depend on opsx-core

**Status:** decided
**Rationale:** Omegon uses opsx-core with JSON file backend (single operator, git-native). Omega uses opsx-core with sled backend (fleet, ACID). The FSM logic, type definitions, and validation are the same — only the storage backend differs. This is a classic trait-based abstraction: trait StateStore with JsonFileStore and SledStore implementations.

### Decision: One-way: JSON state → generated markdown. Operator edits go through FSM commands, not direct markdown editing.

**Status:** decided
**Rationale:** Bidirectional sync is a complexity trap. The JSON files are the source of truth. Markdown is generated/regenerated on state changes. If an operator edits markdown directly, the next FSM operation overwrites it. This is the same contract as generated code — edit the generator input, not the output. jj/git shows the conflict if someone edits markdown while the FSM also updates it.

### Decision: opsx-core is state guardian, markdown is content store — dual storage by design

**Status:** decided
**Rationale:** opsx-core owns: state transitions (FSM validation), milestones (freeze enforcement), audit trail (who changed what when), and referential integrity (parent validation, delete guards). Markdown (docs/*.md) owns: rich content (research, decisions, questions, file scope, overview). Integration: design.rs calls opsx_core::transition_node() to validate before writing markdown. If the FSM rejects, the markdown write doesn't happen. Two stores, one authority per concern.

### Decision: TDD via Testing state (Option A) — first-class lifecycle phase between Planned and Implementing

**Status:** decided
**Rationale:** For harness development, the state machine must literally encode TDD: test stubs written and failing before implementation code. A dedicated Testing state makes this semantically explicit — the operator and agent both know what phase they're in. Proposed → Specced → Planned → Testing → Implementing → Verifying → Archived.

## Open Questions

*No open questions.*

## The layering

```
Layer 3:  opsx-core (lifecycle FSM)     — "what is the state of the work?"
Layer 2:  jj / git (version control)    — "what is the state of the code?"
Layer 1:  filesystem                    — "what files exist?"
```

These layers are naturally synergistic:

**opsx-core ON TOP of jj/git:**
- The lifecycle state store (sled) lives inside the repo (`.omegon/state/`)
- It's versioned by jj/git like any other file
- When you `jj undo` a commit that included a lifecycle transition, the state store rolls back too
- When you `jj squash` implementation commits, the lifecycle state stays consistent because it's just another file in the tree
- Branching in jj/git naturally branches the lifecycle state — a feature branch has its own lifecycle progression

**opsx-core AWARE of jj/git:**
- The task DAG can reference jj change IDs or git commits
- "Task 2.1 was completed in commit abc123" becomes a first-class relationship
- The lifecycle FSM can enforce "you can't transition to `implemented` without a commit that modifies the files in scope"
- Milestone freezes can check the jj/git log: "no commits touching files in this milestone's scope are allowed"

**The conflict resolution question (open question #2) dissolves with jj:**
- jj treats conflicts as first-class — a conflicted state is valid and inspectable
- If an operator edits the markdown display layer while the FSM also updates it, jj shows the conflict naturally
- The resolution: the FSM is authoritative. The markdown is regenerated from FSM state. If the operator edited the markdown directly, jj shows the conflict, and the operator either accepts the FSM's version or uses an FSM command to make the change they wanted.
- This is the same pattern as generated code: the source of truth is the generator (FSM), the output (markdown) is derived.

## sled vs git-native storage

Alternative to sled: store the FSM state AS structured files in the repo, not in an embedded database. Use JSON or a compact binary format. This means:
- No sled dependency
- State is diffable in jj/git
- Merge conflicts are visible and resolvable
- Backup is just `git push`
- The filesystem IS the state store, jj/git IS the transaction log

This is arguably better than sled for the Omegon use case. sled adds a dependency and opacity. JSON files in `.omegon/lifecycle/` are transparent, versionable, and mergeable.

sled makes sense for Omega where you need ACID transactions across multiple concurrent agents. For single-operator Omegon, structured files + jj/git are sufficient.

## jj-specific advantages

- **Operation log**: every `opsx` state change is a jj operation, automatically. No explicit logging needed.
- **Immutable snapshots**: you can inspect "what was the lifecycle state 3 hours ago?" by looking at jj's operation log, not by building a custom audit trail.
- **Concurrent editing**: jj handles concurrent edits to the same file without locks. Two agents editing different parts of the lifecycle won't corrupt each other — jj surfaces the conflict cleanly.
- **First-class conflicts**: if the FSM and an operator disagree about state, jj shows the conflict. No silent data loss.

## Recommendation

opsx-core should use **structured JSON files in the repo** as its state store, not sled. This makes it fully compatible with both jj and git, naturally diffable, and requires no embedded database. The jj/git operation log becomes the transaction log for free. sled is reserved for Omega where ACID matters across a fleet.
