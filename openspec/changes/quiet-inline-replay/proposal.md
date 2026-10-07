# Quiet inline replay

## Intent

Keep the native inline composer responsive. The operator approved quiet inline
presentation, canonical session history, transient inspection, and immediate
approval and cancellation controls.

Current inline startup and rendering already suppress splash and post-render
effects. However, `/splash` still requests a blocking fullscreen replay. That loop
does not drain agent events and reads terminal input alongside `TerminalInputPump`.

## Scope

Return an immediate notice for `/splash` when the session base is inline, including
when an inspector borrows fullscreen. Extend the existing private PTY stress case.
Record bounded parity with the reference checkout and current owners in design.md.

Fullscreen animation redesign and the remaining project-shell work browser are
outside this change. Existing dual-presentation implementation remains the base.

## Success criteria

- Inline `/splash` does not request fullscreen replay or change draft/navigation state.
- A held stream continues through inspection, completion, and an ordinary second turn.
- Real approval denial, cancellation, and owned-terminal cleanup remain covered.
