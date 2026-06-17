---
title: Delegate Result Continuations
status: exploring
tags: [design, delegate, subagents, runtime]
---

# Delegate Result Continuations

## Overview

Background delegates should not rely on the operator to notice completion and prompt the main agent to fetch results. A completed delegate is child work owned by the orchestrating agent, so completion should create pending reconciliation work for the parent loop.

Current behavior is visibility-oriented: delegate completion can update notifications/workbench state and `delegate_result` can retrieve the result, but completed output is not automatically injected into the parent agent's reasoning context.

## Problem

When a background delegate completes, the main agent often needs to act independently on the result: inspect findings, validate changes, update the active plan, run follow-up checks, or report a blocker. Requiring the operator to say "those finished, go check their results" breaks orchestration and makes background delegation feel detached from the main task.

## Proposed Direction

Treat completed delegate results as first-class unreconciled work.

- Store reconciliation metadata on delegate tasks, such as `reconciled_at`, `reconciliation_turn`, parent session/task affinity, and a continuation policy.
- When a delegate completes, enqueue a semantic continuation or pending-context item for the parent session rather than only sending a UI notification.
- Before the next parent turn, inject unreconciled completed delegate results into context with an instruction to reconcile before claiming completion.
- Mark results reconciled after context injection is consumed or after an explicit `delegate_result` call.
- Keep `delegate_result` as a manual retrieval/debug tool, not the primary completion path.

## Continuation Policy

Initial policy should be conservative:

1. `NotifyOnly` — current behavior for low-relevance or stale results.
2. `InjectResult` — default: hydrate the parent context with completed result content and reconciliation instructions.
3. `ResumeParent` — future behavior: wake the parent loop automatically when safe.

## Guardrails

Automatic reconciliation must not recreate the old invisible auto-delegation failure mode.

A completion should only trigger parent continuation when:

- the parent session/task is still active;
- the delegate belongs to that parent session;
- the result has not already been reconciled;
- the runtime is not waiting on explicit operator approval;
- a bounded continuation budget allows it;
- repeated failures do not cause blind delegate retry loops.

Empty successful results must be surfaced as empty/non-evidence, not treated as successful verification. Mutating delegate results must require parent diff/test validation before completion claims.

## Open Questions

- [assumption] Delegate tasks can be reliably associated with a parent conversation/task id.
- [assumption] The runtime has or can add a semantic continuation queue distinct from UI notifications.
- What exact event/request type should carry pending delegate reconciliation into the parent loop?
- Should context injection itself mark a result reconciled, or only a subsequent parent action/turn?
- How should multiple completed delegates be batched into one continuation?

## Initial Implementation Sketch

1. Add reconciliation fields to delegate task state.
2. Mark a task reconciled when `delegate_result` retrieves it.
3. During context build, inject unreconciled completed delegate summaries/results for the active parent session.
4. Add Workbench state for `completed but unreconciled`.
5. Later, add a bounded runtime wakeup path for `ResumeParent` once context injection is stable.
