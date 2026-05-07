+++
id = "517db4ff-1340-4a11-8532-36b9a463d199"
kind = "document"
title = "Cleave child dispatch quality — progress visibility, prerequisite prompting, and submodule awareness"
status = "implemented"
tags = ["cleave", "dx", "submodules"]
aliases = ["cleave-child-dispatch-quality"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "epic"
open_questions = []
+++

# Cleave child dispatch quality — progress visibility, prerequisite prompting, and submodule awareness

## Overview

Three areas of improvement for cleave child dispatch quality, surfaced by the vault-secret-backend implementation:\n\n1. **Progress visibility**: Children need a task inventory with progress tracking (task count, LoC, ETA) surfaced to the operator during execution.\n2. **Prerequisite prompting**: Children need explicit instructions about library versions, commit procedures, and finalization steps — not just a description and scope list.\n3. **Submodule awareness**: Children working inside git submodules cannot commit their work because worktree git operations only affect the parent repo.

## Research

### Current task file construction (build_task_file in orchestrator.rs)

The task file given to each child contains: root directive, mission (description), scope (file list), dependency note, sibling context, guardrail commands, and a contract section with 5 generic rules. The contract says "Commit your work with clear messages — do not push" but provides **no operational guidance** on:\n\n- How to detect and commit inside submodules\n- What library versions are in use (e.g., mockito 1.x not 0.x)\n- What the test framework is (beyond a one-line language-aware hint)\n- How to finalize — the child doesn't know it needs to `cargo check`, run tests, verify compilation\n- What the merge path looks like — the child doesn't understand that uncommitted work in its worktree will be lost\n\nThe task file is ~60 lines of template. The child agent's system prompt (from its training) fills in the rest — which is where the vault-client child used the wrong mockito API and the wrong crate path.

### Progress system architecture

Progress is emitted via NDJSON on stdout from the Rust orchestrator. Events: WaveStart, ChildSpawned, ChildStatus, ChildActivity (tool calls + turn boundaries parsed from stderr), AutoCommit, MergeStart/MergeResult, Done. The ChildActivity event captures individual tool calls (write, bash, etc.) and turn numbers. But there's **no task-level progress** — the orchestrator doesn't know how many subtasks a child has, which ones are done, or how much code has been written. The task file has a checklist (from tasks.md) but the child's progress through it is opaque. The only signal is tool call volume and turn count.

### Submodule root cause analysis

The worktree.rs `create_worktree` function runs `git worktree add` on the **parent** repo. This creates a worktree that includes the submodule directory (core/) but the submodule's `.git` still points to the original checkout. When the child agent writes files inside `core/crates/...`, those changes are tracked by the submodule's git, not the parent's. The child's `git add . && git commit` in the worktree root only commits the parent-level changes and updates the submodule pointer — but only if the submodule itself has already been committed internally.\n\nThe fix has three possible approaches:\n1. **Prompt engineering**: Tell children to detect submodules and commit inside them first\n2. **Post-dispatch auto-commit**: The orchestrator inspects worktrees for dirty submodules and commits them before merge\n3. **Submodule init in worktree**: Run `git submodule update --init` in the worktree after creation, then detect + commit dirty submodules in a post-child hook\n\nOption 2 is the most elegant — it's transparent to the child, doesn't depend on prompt compliance, and the orchestrator already has a post-child phase where it collects results. The orchestrator can walk git submodules, commit dirty ones, then commit the parent pointer update, all before the merge phase.

## Open Questions

*No open questions.*
