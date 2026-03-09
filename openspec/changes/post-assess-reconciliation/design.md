# Post-Assess Reconciliation — Update OpenSpec and Design Tree after Review/Fix Cycles — Design

## Architecture Decisions

### Decision: Assessment outcomes can demote a change from verifying back to implementing

**Status:** decided
**Rationale:** Verification is only trustworthy if it reflects current reality. If `/assess spec`, `/assess cleave`, or an auto-fix review loop finds remaining required work, the lifecycle must demote from verifying to implementing by reopening work in tasks.md or otherwise ensuring task progress no longer reads as complete. A passed assessment keeps the change in verifying; a failed or partially-resolved assessment reopens implementation.

### Decision: Post-assess reconciliation updates both OpenSpec task state and design-tree implementation notes

**Status:** decided
**Rationale:** Assessment does more than say pass/fail — it reveals where planning artifacts no longer match implementation. The reconciler should update OpenSpec tasks.md to reflect reopened or partial work and append design-tree implementation notes when review expands file scope or uncovers new constraints. This keeps design-tree and OpenSpec synchronized as dual views of the same implementation lifecycle rather than letting one lag behind the other.

### Decision: Start with automatic best-effort reconciliation plus explicit warnings, not full semantic task rewriting

**Status:** decided
**Rationale:** A first implementation should be reliable and narrow: reopen lifecycle state when assessment fails, refresh dashboard state, and append scoped implementation-note deltas when new files/constraints appear. Fully rewriting tasks.md from arbitrary reviewer prose is higher-risk and should remain a later enhancement. Best-effort automation paired with explicit warnings preserves trust without over-automating ambiguous interpretation.

## Research Context

### Gap after post-cleave reconciliation

Current lifecycle sync now handles post-cleave task write-back and pre-archive stale-state refusal, but review/fix cycles can still invalidate lifecycle state. A change may enter verifying because all tasks were checked off, then `/assess spec` or `/assess cleave` discovers missing work, widened file scope, or a design constraint that was not captured. Without a post-assess reconciliation step, OpenSpec can remain in verifying while real work has re-opened, and design-tree implementation notes can lag behind the code that review forced us to touch.

### Required checkpoint inputs and outputs

The post-assess reconciler should consume: (1) assessment target (`/assess spec` or `/assess cleave`), (2) result class (pass, warnings, criticals, unresolved failure), (3) files changed during any follow-up fix loop, and (4) the bound OpenSpec change + design-tree node if present. It should produce: (a) an OpenSpec task-state update, including re-opened or partially-complete work when assessment finds remaining requirements, (b) design-tree implementation note updates when follow-up fixes touched files outside prior file scope or introduced new constraints, and (c) refreshed dashboard state so verifying/implementing reflects the post-review reality rather than the pre-review plan.

## File Changes

- `extensions/cleave/index.ts` (modified) — Hook `/assess spec` and `/assess cleave` outcomes into lifecycle reconciliation and dashboard refresh
- `extensions/openspec/` (modified) — Add helpers to reopen or annotate lifecycle task state after failed/partial assessment
- `extensions/design-tree/` (modified) — Append implementation-note deltas when post-assess fixes expand file scope or constraints
- `skills/openspec/SKILL.md` (modified) — Document post-assess reconciliation as part of lifecycle execution
- `skills/cleave/SKILL.md` (modified) — Document that assessment can reopen implementation state

## Constraints

- Do not attempt freeform semantic rewriting of tasks.md from arbitrary reviewer prose in the first version
- Passed assessment should preserve verifying; failed or partially-resolved assessment should reopen implementation state
- Dashboard state must refresh immediately after post-assess reconciliation so verifying/implementing reflects current reality
