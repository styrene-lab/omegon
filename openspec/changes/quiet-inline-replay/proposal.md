# Quiet inline replay

## Post-merge acceptance — 2026-10-07

[PR #249](https://github.com/styrene-lab/omegon/pull/249) merged at
`77996814af7720a45f15fd482ac8579093ebdd6f`, whose first parent is provider-repair
merge `024a8616`. The bounded inline implementation is accepted. Its tested PR
head is `f21cc8d6`; [verification](verification.md) separates that CI result and
the earlier frozen-binary evidence from combined-main acceptance.

Combined-main headless acceptance passed at exact `77996814`. Post-merge CI
completed with 29 successful jobs and one Rust-build timeout, so it is not an
all-green gate. These results do not validate a later rebased TUI tip.
All seven tasks remain checked, and artifact-derived OpenSpec
state remains `verifying`. The user authorized lifecycle closure, but native
registration and archival reconciliation remain blocked by the
[closure gap](../../../docs/lifecycle-closure-reconciliation.md). The change is
not archived.

## Intent

Keep the native inline composer responsive. The operator approved quiet inline
presentation, canonical session history, transient inspection, and immediate
approval and cancellation controls.

Before this change, inline startup and rendering already suppressed splash and
post-render effects, but `/splash` still requested a blocking fullscreen replay.
That loop did not drain agent events and read terminal input alongside
`TerminalInputPump`. The merged inline guard now returns an immediate notice.

## Scope

The implementation returns an immediate notice for `/splash` when the session
base is inline, including when an inspector borrows fullscreen. It extends the
existing private PTY stress case. The [design](design.md) records bounded parity
with the reference checkout and current owners.

Fullscreen animation redesign and the remaining project-shell work browser are
outside this change. Existing dual-presentation implementation remains the base.

## Success criteria

- Inline `/splash` does not request fullscreen replay or change draft/navigation state.
- A held stream continues through inspection, completion, and an ordinary second turn.
- Real approval denial, cancellation, and owned-terminal cleanup remain covered.
