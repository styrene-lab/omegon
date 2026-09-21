# Deliver idle Ctrl+C without weakening active cancellation

## Intent

Restore the documented idle-composer Ctrl+C behavior: clear a nonempty draft,
then allow the existing double-Ctrl+C quit gesture from an empty editor.

A private PTY trial after selecting an anonymous Zen model left the retained draft
unchanged when Ctrl+C was pressed. This is an existing terminal-input defect,
independent of provider selection: `tui/terminal_input.rs::route_input` sends every
Ctrl+C exclusively to the priority runtime interrupt channel. The relay in
`main.rs` forwards only interrupts with an active turn identity. Idle chords are
therefore discarded before `tui/input.rs` can apply its draft-clear/quit behavior.
Direct App input tests bypass that ingress boundary and cannot detect this defect.

Observed evidence is preserved outside the checkout under
`../omegon-provider-onboarding-evidence-01/om-free-connection-03/`, including
`free-02-connected-draft.txt`, `failure.txt`, `manifest.json`, and `omegon.log`.
This change is proposed; no interrupt-routing implementation is included in the
provider-onboarding pass.

## Scope

Define ownership of each Ctrl+C chord at the priority ingress/relay boundary and
deliver idle chords to the composer. Preserve priority cancellation when the
ordinary input lane is congested, active-turn identity fencing, overlay ownership,
and queued-draft preservation. Keep idle clearing and double-quit behavior in the
existing semantic/input owners.

Simply mirroring every chord into both channels is insufficient: an active turn
may complete between priority admission and UI delivery, causing the same chord
to cancel a turn and then clear its queued draft. Resolve that race explicitly
before choosing a channel or acknowledgement design. Also distinguish ingress
at idle followed by a newly admitted turn; a stale idle chord must not cancel that
new turn.

## Success criteria

- A real terminal Ctrl+C clears a nonempty idle draft without submitting it or
  quitting; the existing empty-editor double-quit gesture remains available.
- Active Ctrl+C cancels exactly the intended turn once and preserves queued draft
  text, attachments, and cursor placement, including completion/delivery races.
- Saturating ordinary input does not delay priority cancellation or cause an idle
  chord to become a cancellation for a later turn.
- Overlay and permission ownership retain their declared dismissal/cancellation
  behavior; the composer does not consume another surface's chord.
- Deterministic tests cover terminal ingress through the runtime relay into the
  UI, including idle, active, both transition directions, saturated channels, and
  delayed delivery. Direct `handle_terminal_event` tests alone are insufficient.
- A private headless PTY trial captures idle clearing, deliberate quit, active
  cancellation, and draft retention with bounded loopback inference and verified
  process cleanup. No desktop terminal windows are required.
