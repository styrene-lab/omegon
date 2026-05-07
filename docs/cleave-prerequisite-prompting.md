+++
id = "56cc3327-69d8-4f11-9873-1c5a0579d9b9"
kind = "document"
title = "Cleave prerequisite prompting — project context and finalization instructions"
status = "implemented"
tags = []
aliases = ["cleave-prerequisite-prompting"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "cleave-child-dispatch-quality"
priority = "2"
+++

# Cleave prerequisite prompting — project context and finalization instructions

## Overview

> Parent: [Cleave child dispatch quality — progress visibility, prerequisite prompting, and submodule awareness](cleave-child-dispatch-quality.md)
> Spawned from: "What project-specific context should be injected into the task file? Cargo.lock dependency versions? Key API patterns from existing code? Or should this be delegated to the mind system's project memory?"

*To be explored.*

## Research

### Failure modes from vault-secret-backend cleave

Three distinct failures caused by missing prerequisite context:\n\n1. **Wrong crate path**: Child created files at `crates/omegon-secrets/` instead of `core/crates/omegon-secrets/`. The scope paths in the task file were correct (after our fix), but the child's first attempt (before the fix) had stale paths. Even after fixing, the child didn't understand the submodule boundary.\n\n2. **Wrong library API**: Child used `mockito::mock()` (0.x API) instead of `mockito::Server::new_async()` (1.x API). The Cargo.toml specifies `mockito = \"1\"` but the child never read it — it relied on training data which defaulted to the older API.\n\n3. **Uncommitted work**: Child wrote files inside the `core/` submodule but only the parent repo's git saw the changes as a dirty submodule pointer. The child's contract says \"commit your work\" but the child doesn't know about submodule commit boundaries.\n\nAll three are addressable with better up-front context in the task file.

### Proposed task file enrichment layers

The `build_task_file` function in orchestrator.rs should be extended with several new sections. These fall into three categories:\n\n### 1. Structural context (auto-discovered per project)\n- **Repo layout**: Is there a submodule? What's the actual working directory? Run `git submodule status` and include the result.\n- **Existing file contents**: For each file in scope that already exists, include a brief signature summary (public API, struct definitions, function signatures). The child needs to see what it's modifying.\n- **Dependency versions**: Parse Cargo.toml/package.json/pyproject.toml for the dependency versions relevant to scope. If the child's scope touches `crates/omegon-secrets/`, include the relevant `[dependencies]` and `[dev-dependencies]` sections from that crate's Cargo.toml.\n\n### 2. Convention context (from project memory / mind system)\n- **Test patterns**: Read an existing test in the same crate/package and include it as an example. \"Here's how tests are written in this crate: [example]\". This is far more effective than \"Write tests as #[test] functions\".\n- **Import patterns**: What crates/modules are commonly used? If `mockito` is a dev dependency, show how it's used in an existing test.\n- **Memory facts**: Query project memory for facts relevant to the child's scope. The mind system already has semantic search — use `memory_recall(child.description)` and include the top 3-5 relevant facts.\n\n### 3. Finalization contract (explicit, non-negotiable)\nReplace the current vague \"Commit your work with clear messages\" with:\n```\n## Finalization (REQUIRED before completion)\n\n1. Run the guardrail checks listed above and fix any failures\n2. Ensure all new files are `git add`-ed\n3. If working inside a git submodule:\n   a. `cd <submodule_path>`\n   b. `git add -A && git commit -m \"<your commit message>\"`\n   c. `cd ..` back to the worktree root\n   d. `git add <submodule_path> && git commit -m \"chore: update <submodule> submodule\"`\n4. Verify no uncommitted changes: `git status` should be clean\n5. Update the Result section below with status=COMPLETED\n```\n\nThe submodule instructions are only included when `git submodule status` detects active submodules in the worktree.

## Decisions

### Decision: Three-layer enrichment: structural auto-discovery + convention sampling + explicit finalization contract

**Status:** exploring
**Rationale:** Each layer addresses a different failure class. Structural context prevents wrong-path errors (submodule detection, dependency versions). Convention sampling prevents API misuse (show a real test, not just say 'write tests'). The finalization contract prevents uncommitted work and untested code. The layers are additive — each can be implemented and tested independently. Structural discovery runs once per cleave (shared across children). Convention sampling is per-child (scope-specific). Finalization is a static template expanded with project-specific paths.

### Decision: All three sources — Cargo.toml versions, code samples, and memory facts — each solving distinct failure classes

**Status:** decided
**Rationale:** Dependency versions from Cargo.toml prevent API version mismatches (mockito 0.x vs 1.x). Code samples from existing tests prevent convention drift. Memory facts provide project-specific knowledge that neither source captures. The 4K token budget is sufficient for all three when extracted surgically — dep sections are ~20 lines, one test example is ~30 lines, 3-5 facts are ~15 lines. Memory recall is best-effort (skip if unavailable in child mode).

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/cleave/orchestrator.rs` (modified) — Extend build_task_file with: dependency version extraction from Cargo.toml/package.json for scope crates, existing file signature summaries for in-scope modified files, submodule instructions when detected, explicit finalization checklist.
- `core/crates/omegon/src/cleave/context.rs` (new) — New module: project context discovery. Functions: extract_dependency_versions(scope, repo_path), sample_test_convention(scope, repo_path), detect_submodules(repo_path). Called by build_task_file.
- `core/crates/omegon/src/cleave/mod.rs` (modified) — Add context module

### Constraints

- Dependency extraction must not include the full Cargo.lock — only relevant crate versions from Cargo.toml [dependencies] and [dev-dependencies]
- Test convention sampling should include at most one existing test function as example — not the whole test file
- File signature extraction should be function/struct/trait signatures only — not implementation bodies
- Total task file size should stay under 4K tokens to avoid crowding the child's context window
- Memory recall integration should be optional — skip if memory DB is unavailable in child mode
