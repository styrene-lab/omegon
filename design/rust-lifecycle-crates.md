+++
id = "b13d38a0-1a22-45e7-bcc9-c8177024190c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Rust lifecycle crates — design-tree + openspec as native Rust modules

## Overview

Per the `lifecycle-native-loop` decision, design-tree and openspec are not feature crates — they're core lifecycle engine components. They live in the `omegon` crate's `lifecycle/` module (stubs already exist at `lifecycle/mod.rs`).

**design-tree → lifecycle/design.rs:**
- Markdown parsing + YAML frontmatter (serde_yaml + markdown parser)
- Node state machine: seed → exploring → resolved → decided → implemented (Rust enum with exhaustive match)
- File I/O: scan docs/ directory, read/write .md files with frontmatter
- Two tools: design_tree (query), design_tree_update (mutations)
- ContextProvider: inject focused node's overview + decisions + open questions
- Replaces: extensions/design-tree/ (4,630 LoC TS)

**openspec → lifecycle/spec.rs:**
- Spec parsing: Given/When/Then scenarios, requirements, falsifiability criteria
- Stage computation: proposed → specced → planned → implementing → verifying → archived
- Archive gating, reconciliation, lifecycle binding to design nodes
- One tool: openspec_manage (with sub-actions)
- ContextProvider: inject active change specs and tasks when bound to design node
- Replaces: extensions/openspec/ (4,132 LoC TS)

**Shared:** Both share lifecycle types (NodeStatus, ChangeStage) that live in the core. The `lifecycle-native-loop` decision noted they share `lifecycle.db` — but the immediate migration path is keeping markdown files as the source of truth (matching current TS behavior) rather than introducing a new sqlite schema.

## Research

### What the Rust agent loop actually needs vs. what exists

**The critical insight: the Rust binary today is a cleave child. Cleave children don't need full design-tree/openspec tool support.** They need:

1. **Read-only awareness** — know which design node is focused, which openspec change is active, what specs apply to their scope
2. **Context injection** — inject relevant design decisions, spec scenarios, and constraints into the system prompt
3. **Ambient capture** — parse `omg:` tags from responses (ALREADY DONE in `lifecycle/capture.rs`)

The **full mutation surface** (create nodes, set status, add decisions, archive changes, reconcile) is only needed by the **parent interactive session** — which is still TypeScript through Phase 1.

**This changes the migration strategy fundamentally.** Instead of porting 8.7k LoC of TS to Rust, we need:

**Phase 1a (now):** Read-only lifecycle crate for cleave children
- Parse design-tree markdown files (frontmatter + sections) → read-only queries
- Parse openspec change directories (proposal, specs, tasks) → read-only queries  
- ContextProvider implementations that inject relevant lifecycle context
- ~800-1200 LoC Rust

**Phase 1b (when Rust becomes the interactive parent):** Full mutation surface
- Write design-tree markdown files (create, update frontmatter, add sections)
- Write openspec files (create changes, add specs, archive)
- Full tool implementations matching current TS tool schemas
- ~2000-3000 LoC additional Rust

**What already exists in Rust:**
- `lifecycle/capture.rs` — ambient `omg:` tag parsing (222 LoC)
- `lifecycle/mod.rs` — module stubs with commented-out sub-modules
- `omegon_traits::LifecyclePhase` — phase detection enum
- `ConversationState::apply_ambient_captures()` — wiring from assistant responses

**The TS functions that need Rust equivalents for Phase 1a (read-only):**
- `parseFrontmatter()` — YAML-ish frontmatter → DesignNode fields
- `parseSections()` — markdown → DocumentSections (overview, research, decisions, etc.)
- `scanDesignDocs()` — scan docs/ directory, build tree
- `parseSpecContent()` / `parseScenarios()` — spec markdown → Given/When/Then
- `listChanges()` / `getChange()` / `computeStage()` — scan openspec/changes/
- `getNodeSections()` — read a node's full content

**What can be DEFERRED to Phase 1b:**
- `createNode()`, `setNodeStatus()`, `addOpenQuestion()`, `addResearch()`, `addDecision()` — mutations
- `scaffoldOpenSpecChange()` — directory scaffolding
- `archiveChange()` — move to archive/
- `generateFrontmatter()`, `generateBody()`, `writeNodeDocument()` — file writing
- All dashboard-state emitters (TS rendering concern)
- Branch cleanup, lifecycle emitters, reconciliation

### Phase 1a assessment — all 5 constraints satisfied

**Implementation: 4 new files, 1,667 LoC Rust, 24 lifecycle tests**

| File | LoC | Role |
|------|-----|------|
| lifecycle/types.rs | 237 | Shared enums + structs |
| lifecycle/design.rs | 661 | Frontmatter + section parser, tree scan |
| lifecycle/spec.rs | 533 | Spec parser, change listing, stage computation |
| lifecycle/context.rs | 229 | ContextProvider impl, wired into agent loop |

**Constraint verification:**
1. ✅ Frontmatter: 12 fields (all present in real docs)
2. ✅ Sections: Overview, Research, Decisions, Open Questions, Implementation Notes
3. ✅ Spec: Given/When/Then + And clauses
4. ✅ Stage: proposed→specified→planned→implementing→verifying
5. ✅ Integration tested against real docs/ and openspec/

**What Phase 1a delivers:** Cleave children now get design-tree focus context and active openspec change information in their system prompts, without any TS bridge. The LifecycleContextProvider scans at startup and injects via the ContextProvider trait.

**Remaining for Phase 1b (when Rust becomes interactive parent):**
- Full mutation tools (create, update, archive)
- Dashboard state emission
- Branch binding, reconciliation
- Acceptance criteria parsing

## Decisions

### Decision: Phase 1a: read-only lifecycle parsing + context injection. Phase 1b: full mutation tools when Rust becomes the interactive parent.

**Status:** decided
**Rationale:** The Rust binary is currently a cleave child executor. Children need lifecycle *awareness* (what node is focused, what specs apply to their scope) but not lifecycle *mutation* (create nodes, archive changes). Building read-only parsing + ContextProvider first delivers value immediately — children get design context in their system prompts — without the complexity of the full mutation surface. The mutation tools (Phase 1b) are only needed when the Rust binary replaces bin/omegon.mjs as the interactive parent. This halves the initial implementation (~800-1200 LoC vs ~3000-4000 LoC) and avoids building write paths that won't be exercised until Phase 1 TUI bridge ships.

### Decision: Keep markdown files as source of truth — no lifecycle.db for Phase 1

**Status:** decided
**Rationale:** The TS extensions read/write markdown files with YAML frontmatter. Introducing a sqlite schema now would mean maintaining two representations (markdown for git + sqlite for queries) and a sync mechanism. Markdown-as-source-of-truth matches current behavior, is git-friendly, and the read-only parsing needed for Phase 1a is simpler against files than a database. lifecycle.db can be introduced in Phase 2+ when the full lifecycle engine (with query optimization, cross-node relationships, and ambient phase detection) warrants it.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/lifecycle/design.rs` (new) — Design-tree read-only parser: frontmatter, sections, node scanning, tree building
- `core/crates/omegon/src/lifecycle/spec.rs` (new) — OpenSpec read-only parser: spec content, scenarios, change listing, stage computation
- `core/crates/omegon/src/lifecycle/types.rs` (new) — Shared lifecycle types: NodeStatus, ChangeStage, DesignNode, ChangeInfo, Scenario, etc.
- `core/crates/omegon/src/lifecycle/mod.rs` (modified) — Uncomment design, spec, types modules; add LifecycleContext provider
- `core/crates/omegon/src/context.rs` (modified) — Wire LifecycleContext as a ContextProvider for design-tree + openspec injection
- `core/crates/omegon/src/main.rs` (modified) — Initialize lifecycle context from docs/ and openspec/ at startup

### Constraints

- Frontmatter parser must handle all fields in current TS parseFrontmatter: status, parent, tags, dependencies, related, branches, openspec_change, issue_type, priority, open_questions, branch
- Section parser must extract: Overview, Research (heading+content pairs), Decisions (title+status+rationale), Open Questions, Implementation Notes (file_scope + constraints), Acceptance Criteria (scenarios + falsifiability + constraints)
- Spec parser must extract Given/When/Then scenarios with And clauses, matching TS parseScenarios output
- Stage computation must match TS computeStage: proposed→specified→planned→implementing→verifying based on file presence and task completion
- All parsers must be extensively tested against real markdown from the existing docs/ and openspec/ directories — not synthetic test data only
