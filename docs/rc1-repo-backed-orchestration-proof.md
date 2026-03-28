---
id: rc1-repo-backed-orchestration-proof
title: "RC1: repo-backed orchestration proof"
status: exploring
parent: release-0-15-4-trust-hardening
tags: [release, rc1, cleave, verification]
open_questions: []
dependencies: []
related:
  - orchestratable-provider-model
---

# RC1: repo-backed orchestration proof

## Overview

Release-checklist node for the third rc.1 acceptance criterion: at least one realistic repo-backed orchestrated execution path must succeed end-to-end and leave state that matches what the operator sees. This node exists to avoid repeating the false confidence of synthetic scratch probes that do not exercise the full routing, child execution, and reporting path.

## Decisions

### Decision: the rc.1 repo-backed proof case should be a small real-repo task that exercises child routing and reporting without requiring a large merge-risk change

**Status:** decided

**Rationale:** Rc.1 needs a proof case that is real enough to exercise the full path but small enough not to confound routing validation with large implementation risk. The proof task should be a bounded change inside the actual repo — for example, a targeted doc/test/config adjustment or similarly small edit scope — dispatched through the normal orchestration path so provider resolution, child execution, result reporting, and worktree/merge bookkeeping all run against a real project checkout rather than a synthetic scratch directory.

### Decision: rc.1 orchestration proof acceptance requires end-to-end success, truthful provider/model reporting, coherent artifact outcome, and no bookkeeping contradiction

**Status:** decided

**Rationale:** The proof run passes only if all layers agree. Acceptance means: the child run reaches a coherent terminal state, the concrete provider/model reported to the operator matches the executed route, the repo outcome is explainable (expected file changes or a justified no-op), and worktree/merge bookkeeping does not downgrade the run into apparent failure after successful execution. This directly targets the false-negative failure mode seen in earlier cleave investigations.

### Decision: rc.1 repo-backed orchestration proof should use a bounded documentation or test-surface change in the real repo

**Status:** decided

**Rationale:** For rc.1, the proof task should exercise real checkout/worktree/routing/reporting behavior without coupling success to a large product change. The safest proof shape is a small documentation or test-surface task inside the real repository — for example, updating a design/release doc or adding a narrowly scoped test/assertion — because it still uses the full orchestration path, produces observable repo artifacts, and minimizes merge-risk compared with feature-code changes. That keeps the proof focused on orchestration trust rather than feature implementation complexity.

### Decision: the exact rc.1 repo-backed proof task is a single-file documentation edit under docs/ on the live repository

**Status:** decided

**Rationale:** Stop leaving the proof task abstract. For rc.1, the proof task should be a single-file documentation edit under `docs/` in the live repository — ideally on an existing release-planning or rc.1 design doc that is already part of the current working set. This is the cleanest low-risk proof because it exercises the full real-repo orchestration path (checkout, child routing, child execution, result reporting, worktree/merge bookkeeping) while minimizing the chance that a large code change muddies whether the orchestration system itself is trustworthy. Later RCs can use harder proof tasks; rc.1 should optimize for signal, not bravado.

### Decision: the docs-based rc.1 proof run passes only if the child succeeds, the reported provider/model route matches execution, the expected single-file docs change is produced or a justified no-op is reported, and merge/worktree state remains coherent

**Status:** decided

**Rationale:** Close the last acceptance ambiguity. For the chosen rc.1 proof task, success means: the child reaches a successful terminal state, the concrete provider/model shown to the operator matches the executed route, the repo outcome is coherent (the expected single-file docs change exists, or the run clearly explains why no edit was required), and no bookkeeping layer reclassifies the run as failed after successful child execution. This is the minimum trustworthy end-to-end proof for rc.1.

### Decision: OpenAI/Codex repo-backed docs proof completed successfully end-to-end on the jj workspace backend

**Status:** decided

**Rationale:** The same single-file docs proof shape was rerun with `openai-codex:gpt-5.4` after the jj-native success-path integration was implemented. The child executed successfully, reported the concrete route honestly, produced session/log artifacts, and the final integration back into the parent repo completed successfully via the jj-aware squash path. This satisfies the core rc.1 requirement that at least one realistic repo-backed orchestration path completes end-to-end without bookkeeping contradiction.
