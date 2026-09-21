# Design

Use an extracted TUI action_area owner for a shared read-only projection and
renderer. Agent activity, phase, and tool identity already exist in App, with
authoritative lifecycle reconciliation. Do not add a second lifecycle or timers.
Running tools override stale thinking/response phases. Finished tools cannot be
reported as running; errors may use the existing bounded expiry. Inactive turns
always hide the strip, even if advisory completion or tool-end events were lost.

Inline reserves one phase row and, when needed, one tool row inside its existing
eight-row live viewport. Order: live response tail, spacer, action strip, composer.
Stable response rows still publish through native insertion. Remove the bare-tool
fallback from the response tail and the duplicate Working composer title. Keep
publication/degradation notices in the composer; hiding activity retains a compact
cancellation fallback there. Fullscreen reuses the existing engine-status slot
with a one-row version; richer tool cards remain in their existing surfaces.

Use Theme control roles, full-width grey surface, bounded sanitized strings, and
Unicode display-width truncation. Never render raw argument JSON or thinking text
as activity. Honor the existing activity visibility setting. Terminal capture
fixtures gate provider events and a bounded real tool without paid inference.
