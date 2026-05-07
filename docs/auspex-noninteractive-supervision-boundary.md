+++
id = "a9a39c66-636d-46b3-93e3-4bc872ffd20f"
kind = "document"
title = "Auspex Non-Interactive Supervision Boundary"
status = "decided"
tags = ["architecture", "auspex", "supervision", "cleave"]
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Auspex Non-Interactive Supervision Boundary

## Decision

**Auspex owns durable non-interactive child supervision.**

Omegon retains a local bootstrap supervisor for interactive and same-host recovery cases, but durable lifecycle authority for non-interactive cleave/daemon execution belongs in Auspex.

## Why

Omegon's current local supervisor model is intentionally limited:

- same-host only
- lease/fingerprint based
- degraded restart recovery
- no true subprocess stream reattachment
- no cryptographic authority model

That is good enough for:

- interactive operator control
- local daemon restart recovery
- last-resort PID-based cancel
- surfacing truthful degraded state (`attached`, `recovered_degraded`, `lost`)

It is not the right place to carry:

- durable non-interactive ownership
- long-lived restart-safe supervision
- cross-boundary authority
- identity-backed leases/capabilities
- mTLS-secured control channels

Those belong in Auspex.

## Boundary

### Omegon owns

- interactive operator UX
- local bootstrap child spawn
- local cleave progress rendering
- degraded recovery from persisted state
- transport/control surface presentation
- fallback same-host cancel when operating without Auspex

### Auspex owns

- durable non-interactive child registry
- long-lived child ownership across Omegon restarts
- authoritative cancel/restart/reap semantics
- persistent supervision workers
- future Styrene Identity / mTLS trust plane
- signed or otherwise authoritative supervisor leases/capabilities

## Expected handoff shape

For non-interactive execution, Omegon should evolve toward acting as a client of Auspex:

1. Omegon submits a child/run request
2. Auspex creates or adopts the durable child supervisor context
3. Auspex returns a durable child identity / handle
4. Omegon renders status and issues control requests against that handle
5. Auspex reports lifecycle and progress updates back to Omegon

## Local fallback remains valid

Even with Auspex as the durable supervisor, Omegon should keep the local bootstrap supervisor for:

- development
- offline same-host operation
- interactive operator sessions
- degraded local recovery when Auspex is unavailable

That fallback should be treated as **bootstrap continuity**, not the final authority model.

## Migration implication

The current Omegon local supervisor fields (`pid`, worktree/model fingerprint, supervisor lease token, supervision mode, activity logs) are still useful. They become:

- local fallback metadata
- bootstrap adoption hints
- UI/runtime evidence

They should not be mistaken for the final durable supervision contract once Auspex is present.
