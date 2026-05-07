+++
id = "1c23658b-49f0-4990-98de-3ef5a0fda2ed"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Git-native task management — extend design tree into a full in-repo issue/task system — Design Spec (extracted)

> Auto-extracted from docs/git-native-task-management.md at decide-time.

## Decisions

### Design tree IS the task system — extend, don't replace or add a parallel tracker (exploring)

The temptation is to build a separate "issues" subsystem alongside the design tree. This would be wrong — it creates two competing sources of truth for "what work exists."

The design tree already has richer semantics than any issue tracker: research, decisions, acceptance criteria, readiness scoring, lifecycle doctor. Adding issue-tracker fields (assignee, milestone, due date, estimate) to the EXISTING design node format gives us the best of both worlds: structured design exploration AND operational task management in one system.

The node's status lifecycle already covers the workflow: seed (backlog) → exploring (triage/analysis) → decided (ready) → implementing (in progress) → implemented (done). These are strictly richer than Open/In Progress/Done.

Adding 6 optional fields to DesignNode (milestone, assignee, estimate, actual, due, archived) and a filtered list query makes it a complete task system. No new data model, no new storage format, no new tools. Just wider frontmatter.

### Extract design tree to omegon-design crate — zero external dependencies beyond serde, reusable across binaries (exploring)

The extraction boundary is already clean. The core design tree code has ZERO agent/TUI/provider dependencies:

```
lifecycle/types.rs   — serde + std::path only
lifecycle/design.rs  — std + types only  
lifecycle/doctor.rs  — std + design + types only
lifecycle/spec.rs    — std + types only
lifecycle/capture.rs — std only
```

Only `context.rs` touches `omegon_traits` (for ContextProvider/ContextInjection). That stays in the omegon binary.

The extracted crate:
```toml
[package]
name = "omegon-design"
description = "Git-backed design tree and task management — markdown nodes with structured lifecycle"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"  # frontmatter parsing (currently inline in design.rs)
```

Three consumers:
1. **omegon binary** — `features/lifecycle.rs` uses `omegon-design` for per-project design tree. Agent tools, context injection, TUI dashboard all consume the crate's API.
2. **Standalone task/project app** — uses `omegon-design` + own web UI for multi-project management. Scans multiple repos' `.omegon/design/` directories.
3. **opsx-core** — could depend on `omegon-design::types` for shared type definitions (NodeStatus, IssueType), eliminating the current type duplication between the two crates.

### No platform bridge in v1 — sovereign git (Forgejo) is the primary path, bridge is a future optional add-on (exploring)

The design tree's value proposition is that it lives IN the repo, not in a platform's database. Building a GitHub Issues bridge first would optimize for platform dependence. The sovereign path (Forgejo + omegon-pm) keeps the data portable and platform-independent. A bridge crate (omegon-bridge-github) can come later for teams that want it, but the architecture shouldn't assume or require it.

### Index is gitignored — local cache rebuilt on startup, no merge conflicts (exploring)

The index is a derivative of the markdown files. Tracking it in git creates merge conflicts when multiple agents or sessions mutate nodes concurrently (which happens during cleave runs). Rebuilding from 255 markdown files on startup is sub-100ms in Rust — no perceptible cost. The markdown files are the source of truth; the index is a read optimization. .gitignore it.

### Both tree and board views — hierarchy answers scope, kanban answers workflow (exploring)

They answer different questions. The tree view (existing sidebar) shows what belongs to what — project structure. The board view shows what's in what state — operational status. Both are read views over the same DesignStore. The web dashboard gets both as tabs. The TUI gets `/board` as a command that replaces the sidebar with a status-grouped compact view. Since the data model is the same, this is a rendering decision, not a data model decision.

## Research Summary

### What we already have vs what's missing — the 80/20 gap



### Already implemented (the 80%)

| Capability | Current implementation | Equivalent in GitHub Issues |
|------------|----------------------|----------------------------|
| Node lifecycle | seed→exploring→resolved→decided→implementing→implemented→blocked→deferred | Open→In Progress→Done |
| Hierarchy | parent/children (unlimited depth) | Labels + Milestones (flat) |
| Issue types | epic/feature/task/bug/chore | Labels |
| Priority | 1-5 (P1 critical → P5 trivial) | Priority labels |
| Tags | Freeform string tags | Labels |
| Dep…

### Missing (the 20%)

**1. Filtered queries** (SMALL delta)
Current `list` returns all 255 nodes as JSON. Need:
- `list --status=exploring --priority=1` 
- `list --tag=rust --type=bug`
- `list --milestone=0.15.3`
- `list --assignee=cwilson`

Implementation: add optional filter parameters to the `design_tree` tool's `list` action. ~50 lines of filter logic.

**2. Milestone binding** (SMALL delta)
`.omegon/milestones.json` exists with release tracking. Design nodes have no `milestone` field. Add:
- `milestone: Option<S…

### Comparison with git-bug and git-issue



### git-bug (MichaelMure/git-bug)

**Storage**: Custom git objects in `refs/bugs/` namespace. Not files — git objects accessed via git-bug tooling.
**Pro**: Fully distributed, merge-friendly (CRDTs for comments), bridges to GitHub/GitLab/Jira.
**Con**: Invisible without git-bug binary. Can't browse issues in a text editor or on GitHub. Complex object model.

### git-issue (dspinellis/git-issue)

**Storage**: Files in `.issues/` directory. Each issue is a directory with `description`, `comments/`, `tags/`, `assignee` files.
**Pro**: Simple, file-based, browsable. Bidirectional GitHub/GitLab sync.
**Con**: One-directory-per-issue creates filesystem clutter. No structured metadata beyond simple key-value files.

### Omegon design tree (our approach)

**Storage**: Markdown files with YAML frontmatter in `docs/` (soon `.omegon/design/`). One file per node.
**Pro**: Human-readable, git-diffable, editable in any editor or on GitHub. Rich structured content (research, decisions, acceptance criteria). Agent-native (LLM tools for query/mutation). Already integrated with OpenSpec, cleave, milestones, dashboard.
**Con**: 255 files in a flat directory. No CRDT merge (relies on git merge). No external platform sync (no GitHub Issues bridge — yet).

### Key advantage: agent-native by design

Neither git-bug nor git-issue are designed for AI agents. They're CLI tools for humans. Our design tree IS the agent's working memory for design decisions — it's injected into the system prompt, the agent queries it for context, mutations happen via tool calls during design conversations.

This is not a human issue tracker with agent bolted on. It's an agent-native knowledge structure that happens to also be human-readable. The deltas to make it a "full task system" are about human workflow affo…

### What we DON'T need to replicate

- **Comment threads**: Research sections and decisions already capture structured discourse. No need for an unstructured comment stream.
- **Notifications**: Single-operator system. The dashboard IS the notification surface.
- **Pull request integration**: OpenSpec + branch binding already tracks implementation. Commits reference node IDs in conventional commit scope.
- **Team assignment workflow**: For now, single operator + agents. Assignee field covers the multi-agent case.

### Storage migration: docs/ → .omegon/design/



### Why move

Currently design docs live in `docs/` alongside hand-authored documentation (if any). This conflates two concerns:
- Machine-managed design state (created/mutated by design_tree_update)
- Human-authored reference docs, guides, architecture overviews

Projects using Omegon that also have a `docs/` site (Astro, MkDocs, etc.) would get 255 design node files mixed into their documentation.

### Proposed layout

```
.omegon/
  milestones.json          # already exists
  profile.json             # already exists
  history                  # already exists
  design/
    perpetual-rolling-context.md    # active design nodes
    git-native-task-management.md
    ...
    archive/                        # completed nodes (optional)
      rust-phase-0.md
      ...
  sessions/                # session persistence (already used)
  memory/                  # facts.db, facts.jsonl (via ai/memory symlink)
  agents/…

### Migration

1. Move `docs/*.md` design nodes → `.omegon/design/`
2. Update `LifecycleProvider` to scan `.omegon/design/` instead of `docs/`
3. Fallback: if `.omegon/design/` doesn't exist but `docs/*.md` has frontmatter with `status:`, scan there (backward compat)
4. One-time migration command: `/migrate-design` or auto-detect on startup

### Index file

Add `.omegon/design/index.json` — a lightweight index rebuilt on startup:
```json
{
  "nodes": {
    "perpetual-rolling-context": {
      "status": "exploring",
      "priority": 1,
      "type": "epic",
      "parent": "rust-agent-loop",
      "milestone": null,
      "assignee": null,
      "tags": ["architecture", "context"],
      "modified": "2026-03-27T12:00:00Z"
    }
  },
  "stats": {
    "total": 255,
    "by_status": { "seed": 12, "exploring": 18, "decided": 5, "implemented": 215 },
  …

### Effort estimate for each delta

| Delta | Size | Files touched | Est. effort |
|-------|------|---------------|-------------|
| Filtered `list` queries | S | lifecycle.rs (add filter params) | 2h |
| Milestone binding | S | types.rs, design.rs, lifecycle.rs, milestones | 3h |
| Assignee field | XS | types.rs, design.rs, lifecycle.rs | 1h |
| Activity/history query | S | lifecycle.rs (shell to git log) | 2h |
| Effort estimation fields | XS | types.rs, design.rs | 1h |
| Due date field | XS | types.rs, design.rs, doctor.rs (ove…

### Crate extraction boundary and multi-consumer architecture



### omegon-design crate API surface

```rust
// ─── Core types ────────────────────────────────────────────
pub struct DesignNode { /* existing + new fields */ }
pub enum NodeStatus { Seed, Exploring, Resolved, Decided, Implementing, Implemented, Blocked, Deferred }
pub enum IssueType { Epic, Feature, Task, Bug, Chore }
pub struct DocumentSections { /* research, decisions, questions, impl notes */ }
pub struct DesignDecision { /* title, status, rationale */ }
pub struct ResearchEntry { /* heading, content */ }

// ─── Store ───────…

### Three consumers

**1. omegon binary (existing)**
```
features/lifecycle.rs wraps DesignStore:
  - Registers design_tree/design_tree_update tools
  - Injects focused node into system prompt (context.rs stays in omegon)
  - Renders dashboard sidebar tree from store.list()
  - Runs doctor on startup and reports findings
```

**2. Standalone project management app (future)**
```
omegon-pm (or whatever):
  - Scans multiple git repos for .omegon/design/
  - Serves web UI (axum + htmx or similar)
  - Cross-project depe…

### What stays in omegon (NOT extracted)

- `context.rs` — agent-specific context injection (depends on omegon_traits)
- `capture.rs` — ambient tag parsing from LLM responses (agent-specific)
- `features/lifecycle.rs` — tool registration, event handling (agent integration layer)
- OpenSpec binding logic — crosses omegon-design + opsx-core boundary, orchestrated by the agent

### File layout after extraction

```
core/crates/
  omegon-design/
    Cargo.toml
    src/
      lib.rs
      types.rs      ← from lifecycle/types.rs
      store.rs      ← from lifecycle/design.rs (renamed, expanded)
      doctor.rs     ← from lifecycle/doctor.rs
      spec.rs       ← from lifecycle/spec.rs (OpenSpec spec parsing)
      index.rs      ← NEW: index build/query
      filter.rs     ← NEW: NodeFilter logic
      history.rs    ← NEW: git log integration
  omegon/
    src/
      lifecycle/
        mod.rs       ← re-ex…

### Sovereign project management: Forgejo + omegon-design + multi-repo orchestration



### The stack

```
┌─────────────────────────────────────────────────────┐
│          omegon-pm (project management app)          │
│  Multi-repo design tree aggregation + web dashboard  │
│  Uses: omegon-design crate for each repo's tree      │
├─────────────────────────────────────────────────────┤
│              Forgejo (sovereign git hosting)          │
│  Self-hosted, lightweight, Go binary                 │
│  Repos contain .omegon/design/ directories           │
│  No external GitHub/GitLab dependency  …

### How it works

1. **Per-repo**: Each project has `.omegon/design/` with its design tree. Omegon agent sessions read/write to it. Commits are pushed to Forgejo.

2. **Cross-repo**: omegon-pm scans all repos on the Forgejo instance (or any set of git remotes), clones/pulls `.omegon/design/` from each, and builds a unified view. Cross-project dependencies work because node IDs are `{repo}:{node-id}`.

3. **Dashboard**: omegon-pm serves a web UI showing:
   - All projects' design trees in one view
   - Cross-proje…

### Why Forgejo, not GitHub

- **Sovereign**: Your data, your server, your rules. No platform risk.
- **Lightweight**: Single Go binary, runs on a $5 VPS or a Raspberry Pi.
- **API-compatible**: Gitea/Forgejo API is close enough to GitHub's that tools work.
- **Repo-local state**: The design tree lives IN the repo, not in a platform's database. Moving between Forgejo, GitHub, and bare git repos changes nothing — `.omegon/design/` travels with the code.

### The bridge question is answered

Q: "Should we add a GitHub Issues bridge?"
A: **Not yet, but the architecture doesn't preclude it.** Since the design tree is markdown in a git repo, a bridge is just: "sync DesignNode ↔ GitHub Issue". The `omegon-design` crate exposes the data model; a `omegon-bridge-github` crate could implement bidirectional sync. But the sovereign path (Forgejo + omegon-pm) makes the bridge optional, not essential.

### Why this matters

Most project management tools are SaaS platforms that own your data. The design tree is the opposite: it's files in your repo. The "project management app" is just a viewer/aggregator — the source of truth is always the git repo. You can switch from omegon-pm to reading markdown files in vim and lose nothing.

### Conclusion

The architectural direction is now settled:

1. The design tree becomes the single task/project-management substrate rather than spawning a parallel issue system.
2. The reusable core is extracted into `omegon-design`.
3. Per-project omegon sessions consume that crate locally.
4. A future multi-repo project-management app can consume the same crate for sovereign orchestration, likely alongside Forgejo.
5. Implementation work should proceed in child nodes: crate extraction, task metadata/filterin…
