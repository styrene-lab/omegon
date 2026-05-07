+++
id = "cb190045-35c8-4251-84d7-d19fd40973d8"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# cleave-checkpoint-failure-clarity — Design

## Spec-Derived Architecture

### cleave/checkpoint

- **Confirmed checkpoints must verify post-checkpoint cleanliness before cleave continues** (added) — 2 scenarios
- **Checkpoint failures must surface precise execution errors** (added) — 2 scenarios

## Scope

Tighten the dirty-tree checkpoint execution path at the preflight boundary so cleave never leaves preflight under the false assumption that a confirmed checkpoint fully cleaned the worktree. In scope: post-checkpoint cleanliness verification, explicit diagnosis for remaining excluded dirty paths, and clearer propagation of concrete git add/commit failures. Out of scope: changing the dirty-path classifier policy itself, redesigning merge conflict handling, or broad changes to unrelated cleave worktree lifecycle behavior.

## Design Decisions

1. **Fail closed inside preflight after checkpoint attempts**
   - After a checkpoint action succeeds at the git layer, preflight must immediately re-read `git status --porcelain` before returning `continue`.
   - If any dirty paths remain, preflight stays in the resolution loop and surfaces a dedicated post-checkpoint diagnosis instead of deferring to `ensureCleanWorktree()` later.

2. **Explain what remains dirty and why**
   - The post-checkpoint diagnosis should list the remaining dirty paths and distinguish files excluded from the checkpoint plan from files that unexpectedly remained dirty.
   - This keeps operator trust by making the checkpoint boundary explainable rather than opaque.

3. **Surface concrete checkpoint execution failures verbatim enough to act on**
   - `git add` / `git commit` failures should be reported in the preflight update stream with the actual stderr/stdout-derived reason.
   - Empty or no-longer-stageable checkpoint scopes should be treated as explicit preflight failures, not silent success.

## File Changes

- `extensions/cleave/index.ts` — re-check dirty state after checkpoint attempts, keep failures inside preflight, and emit precise post-checkpoint diagnostics
- `extensions/cleave/index.test.ts` — add regression coverage for successful checkpoints, remaining excluded dirt, git commit failure, and empty-stageable checkpoint scopes
