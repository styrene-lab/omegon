# Architecture Reference

## Ownership table

Before implementation, fill this from the repository:

| Resource | Owner in conversation | Owner in exclusive interaction | Acquire | Release | Failure recovery |
|---|---|---|---|---|---|
| primary-screen cells | | | | | |
| alternate screen | | | | | |
| raw mode | | | | | |
| cursor | | | | | |
| mouse capture | | | | | |
| bracketed paste | | | | | |
| keyboard enhancement | | | | | |
| transcript cursor | | | | | |

Unassigned or multiply-owned rows are design defects.

## Projection direction

```text
runtime/domain state
  -> renderer-neutral surface projection
  -> frontend-local interaction state
  -> Ratatui layout and widgets
  -> optional bounded TachyonFX pass
  -> Crossterm backend
```

Do not let terminal geometry leak upward into semantic projections. Do not couple canonical state to one renderer.

## Terminal transition checklist

- The old renderer is flushed or intentionally abandoned.
- Acquisitions happen in an order that can be rolled back.
- Guard state changes after successful I/O.
- The new Terminal has an explicit viewport kind.
- The new owner clears and paints its complete area.
- Event receivers and background tasks are unchanged.
- Returning invalidates stale buffers and recomputes geometry.
- Deferred primary-screen publication occurs only after ownership returns.
- Quit and panic work at every intermediate state.

## Resize checklist

- Query physical dimensions authoritatively.
- Recompute layout capacity and wrapped rows.
- Preserve semantic item identity.
- Clamp selection if identity disappeared.
- Reveal selection under the new capacity.
- Rebind image protocol and animation regions if required.
- Avoid publishing from stale viewport coordinates.

## Async fairness checklist

- Cancellation/interrupt path is non-awaiting or independently serviced.
- Operator input has bounded urgent service.
- Agent/event drains have count and time budgets.
- Extension traffic cannot clobber the current owner.
- Frame pacing coalesces background mutations.
- Expensive syntax/image/effect work is bounded or cached.
- A visible menu does not alter coordinator execution.
