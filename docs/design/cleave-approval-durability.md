# Cleave Approval Durability — Level 1/2 Path

## Goal

Raise cleave approval recovery from ephemeral session state to a release-safe durability model for 0.27.1 without pretending the final approval UX is complete.

The approval gate protects expensive, long-running child-agent work. Losing pending approvals on restart weakens operator trust and makes Workbench visibility transient. Durability must therefore be part of the release surface, not follow-up polish.

## Current state

Implemented before this durability slice:

- `cleave_assess` advertises approval metadata.
- `cleave_run` requires approved pending state.
- Approval is digest-bound to `directive + plan_json + max_parallel`.
- High-cost approvals require final-gate confirmation.
- Pending approvals emit Workbench workstream rows without clobbering active plans.
- Primary advertised actions are release-safe: review, approve/run, deny, evidence, reassess.

Remaining weakness:

- Pending approvals live in memory only.
- Restart/reload loses pending approvals and Workbench rows.
- Previously approved plans have no recovery semantics.

## Level 1 — persisted pending approval recovery

Level 1 is the minimum release bar.

### Requirements

1. Persist unresolved cleave approvals under project-local runtime state.
2. Reload approvals when `CleaveFeature` starts.
3. Restore pending approval Workbench projections after reload when a bus sink is available.
4. Persist every approval mutation immediately.
5. Use atomic write (`tmp` + rename) to avoid truncated JSON state.
6. Do not persist secrets or inherited environment.
7. Treat `Approved` as unsafe after restart: downgrade to `ApprovalRequired` and clear high-cost confirmation.
8. Corrupt approval state must not panic feature initialization.

### State path

Use:

```text
.omegon/cleave/approvals.json
```

This keeps approval control-plane state separate from child workspace execution state:

```text
.omegon/cleave-workspace/state.json
```

### Persisted schema

```rust
struct CleaveApprovalsState {
    version: u32,
    approvals: Vec<PendingCleaveApproval>,
}
```

`PendingCleaveApproval` persists:

- `id`
- `directive`
- `plan_json`
- `max_parallel`
- `children`
- `state`
- `modification_request`
- `plan_digest`
- `high_cost_confirmation`

### Conservative reload semantics

On reload:

- `ApprovalRequired` remains `ApprovalRequired`.
- `Modified` remains `Modified`.
- `Approved` becomes `ApprovalRequired` and `high_cost_confirmation = false`.
- `Denied`, `Phased`, and `Saved` may be retained for evidence but must not appear as pending approvals or pass the run gate.

The key invariant:

```text
No approved cleave run may silently survive process restart.
```

## Level 2 — recoverable approval lifecycle

Level 2 is the target after Level 1 lands, still suitable for the next release if small.

### Additions

- `created_at_unix_ms`
- `updated_at_unix_ms`
- optional `expires_at_unix_ms`
- optional `recovery_note`
- `/cleave status` warning for recovered approvals
- stale approval expiry/reconfirm behavior

### Level 2 reload semantics

On reload, recovered approvals should tell the operator why review is needed:

```text
Approval was recovered from disk after restart; review and approve again.
```

Stale approvals should require reassessment rather than approval.

## Release acceptance

0.27.1 can ship when Level 1 is implemented and tested:

- pending approval survives restart;
- modified approval survives restart;
- approved approval reloads as review-required and cannot run;
- corrupt state file does not panic;
- Workbench projection can be reconstructed from reloaded state;
- full crate tests pass.
