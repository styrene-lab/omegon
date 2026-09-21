# Shared TUI presentation design

## Product and configuration contract

Use two independent concepts: terminal presentation (`inline`, `fullscreen`) and
detail (`active`, `full`). Keep presentation policy in the main crate; there is no
new public runtime protocol or shared-contract crate in this change.

| Entry with no explicit preference | Terminal presentation | Detail |
|---|---|---|
| `om` | inline | active |
| `omegon`, direct Cargo binary, `just run` | fullscreen | full |

Add interactive CLI overrides `--tui inline|fullscreen` and `--ui active|full`.
Resolve each axis independently: explicit CLI value, explicit selected-profile
value, entry default. Preserve existing profile selection/precedence. Rust
profile fields are `ui_terminal` and existing `ui_presentation` (JSON
`uiTerminal` and `uiPresentation`); update the
editor-facing Pkl vocabulary alongside the runtime parser.

The installed launcher sets an internal entry marker, `OMEGON_LAUNCH_NAME`, to
its own recognized basename for the target process. It does not prepend CLI
arguments or inspect prompt contents. The binary uses the recognized marker,
then a recognized argv[0], then `omegon`. Do not let an inherited marker make
an `omegon` launcher act as `om`. Child `om`/`omegon` launchers overwrite it.
Headless, remote, maintenance, help, and version execution do not initialize a
terminal; existing `--which` continues reporting the resolved artifact.

Keep resolved values separate from explicit persisted preferences. Launch defaults
and CLI overrides are invocation-local. An unrelated settings update or exit save
must preserve absent preferences. `/ui active` and `/ui full` remain explicit
persisted detail selections. `/ui terminal inline|fullscreen` changes the current
session's base presentation only; file configuration controls persistent terminal
preference. `/ui` reports both axes and whether detail came from a default or an
explicit preference. No duplicated registry command family is needed: extend the
existing UI command path, retaining frontend availability metadata.

Read legacy `om`, `lean`, and `slim` detail values as Active. Accept their existing
slash aliases during migration; help and newly written preferences use Active/Full.
This intentionally retires the old quiet Om density. Missing profile values retain
entry defaults. Existing explicit Active or Full values continue to override them.
Keep canonical conversation/export semantics stable while migrating internal enum
references. Do not rewrite unrelated fields or files during startup.

## Shared application, two compositions

Retain one App, one event loop, one editor state, one InteractionState, and one
canonical conversation. Extract only reusable pieces needed by both layouts:
frame preparation, composer, bounded live activity/preview, and decision rendering.
Existing browser/menu/diff/inspection widgets remain their current owners.

Move state refresh out of layout-specific branches. Input/event admission,
supervisor completion reconciliation, stream scheduling, and commands run before
presentation work. Frame preparation runs once per scheduled frame. Rendering
can retain geometry/hit-area registration; this is not a whole-App purity refactor.
Continue existing stream acknowledgement semantics only after a successful active
draw. Runtime completion must remain independent of whether text was displayed.

Inline initially reserves eight rows, clipped to actual terminal height. Give the
shared editor up to four rows including its borders and primary hint, and allocate
the remaining rows to status and the unfinished response tail. Stable answer text
is published above this region while it streams; the viewport is never the answer
container. Long drafts scroll inside
the same editor. Decisions use the existing shared decision widget with scrolling
context. Borrowed screens use a clean canvas rather than the fullscreen workspace.
The first implementation always borrows fullscreen for a decision: the
complete shared widget needs more than the remaining live rows. This avoids a
second permission layout or input policy. At unusably tiny geometry,
show a resize indication and never render misleading partial action labels.

Inline Full increases evidence/detail in newly published completed
output, and inspectors. It does not mount persistent dashboard/instrument panels.
Fullscreen Active retains the workspace layout while reducing detail. Requested
surface visibility remains stored independently of layout eligibility; returning
fullscreen restores those preferences. Existing `/ui show|hide|toggle` must explain
when a requested panel is available in the fullscreen workspace.

Slash autocomplete stays attached to the shared composer with geometry clamped to
the live area. File-reference pickers use their existing fullscreen selector. Rich navigation (Project, menus, process/diff/copy inspectors, extension
modals, tutorial, command panels) borrows fullscreen from inline. This grants space
to the owning widget without rendering the transcript, composer, or workspace
behind it. Explicit fullscreen retains the full workspace composition. Images use textual
alt/path references in published history and existing rich inspection when available.

## Navigation and terminal ownership

Separate base presentation from effective presentation. A mounted rich root or a
decision that needs more space can require fullscreen temporarily. Derive required
space from shared interaction state; do not create another input priority chain.
The root's fullscreen requirement survives covering decisions and nested surfaces.
Return to the base only after the root and any fullscreen-required decision close.
Changing the base while covered takes effect after the covering requirement ends.

Adapt the neighbor's two Terminal buffers into the local TerminalSessionHandle.
The coordinator selects buffers; the existing TerminalModes ledger owns terminal
mode truth. One lock serializes draw, publication, mode changes, and primary-screen
handoffs. The inline Terminal remains alive across visits. A fullscreen-first launch
creates inline lazily on first return to primary, rather than reserving inline rows
before entering fullscreen. There is never more than one active stdout writer.

Inline uses raw mode and supported paste/keyboard enhancements, without mouse
capture or alternate screen. Fullscreen uses alternate screen and the operator's
mouse preference. Borrowing fullscreen does not permanently change that preference.
Inline startup must bypass fullscreen splash, global background fill, and Clear(All),
including the current unconditional setup in run_tui. Style only owned live cells
and published content; restore default output attributes on return to the shell.
Defer image-protocol discovery until a rich image surface needs it. An explicitly
requested tutorial can borrow fullscreen through the same navigation contract.
Retain file-based interactive logging; route live diagnostics through existing
semantic notifications rather than write uncoordinated stdout/stderr into the UI.
Failure tracks only successfully changed modes. If rollback cannot restore the
expected surface, stop rendering and restore the terminal through existing cleanup;
never claim a usable inline state solely because an enum was reset.

On successful return, autoresize the preserved inline Terminal before publication,
invalidate its live buffer, and redraw. Do not clear saved history. Guarded I/O must
reject a publication call against fullscreen: Ratatui otherwise permits a silent
no-op. Existing shell/tutorial handoff, explicit export, signals, normal exit, and
panic restoration use the same owner. Uncatchable termination is outside restoration
guarantees. Synchronous primary scopes restore preferences on operational failure.

## Incremental publication

Canonical conversation remains the data source. Retain only bounded prepared text
plus a cursor describing attachment, source generation, finalized range, and offset.
Extend the existing publication owner and settlement vocabulary rather than add a
second pending transcript. Do not serialize or hash the full conversation on each
frame. Initial discovery and generation replacement must also yield under budgets.

Publish accepted user input once. During a running turn, publish stable append-only
assistant prefixes, retaining only an unfinished tail in the live area. Complete
logical lines and stable wrapped display rows must reach scrollback without waiting
for MessageEnd or TurnEnd. Retain grapheme-safe boundaries when a delta may extend
the final grapheme. Full streams the same answer first and appends completed thinking
as labeled evidence afterwards; this avoids freezing a thinking field that could
receive late deltas. Canonical content and provenance are unchanged.

Publish completed contiguous tool runs using shared outcome/evidence rules before
subsequent assistant text, rather than waiting for the entire turn to aggregate
all tools. In-progress metadata remains mutable and unpublished. Completion,
cancellation and failure flush only remaining text and truthful final outcomes.
The existing cursor owns progress through streamed and completed records; do not
add another transcript or replay earlier prefixes at completion. Remaining eligible
text drains within existing budgets even when a provider pauses, without spinning
on an unfinished tail.
Standalone informational records can publish once stable. Active deferred operations
remain inspectable; later terminal outcomes publish as new observations.

On attachment/resume, print a bounded session identity summary and begin publication
at the current finalized boundary; do not automatically dump old saved history.
The shared fullscreen transcript exposes history. A fullscreen-first session uses
the attachment boundary if it later switches inline, publishing work completed
since attachment in bounded batches. Temporary visits retain the committed cursor.

Prepared chunks carry the source generation, detail revision, width, and source
offsets. Detail or width changes discard only uncommitted formatting. Published text
stays as originally printed. Once a record starts publishing, retain its chosen
detail until its remaining chunks settle; later records use the new preference.
Split long logical lines and Unicode content without dropping or duplicating text.
Measure wrapped display rows, not newline count, and split before converting to u16.
Apply existing safe display treatment to provider/tool text before terminal output;
raw ESC/OSC control content must not become terminal commands. Keep producer and
content provenance intact when selecting that treatment.

Budget discovery and formatting together: at most 64 KiB source text, 64 records,
1,000 rendered rows, 65,536 buffer cells, and a cooperative 5 ms preparation slice.
Stop at the first limit, retaining a within-record cursor. Bound scratch allocation
before parsing/wrapping; never preformat a megabyte record to discover it is too big.
Inject clock/budget accounting for deterministic tests. These are application work
limits, not a hard deadline for blocking OS writes. Perform at most one publication
batch per event-loop cycle after admitting input, decisions, and lifecycle events.

Settle Committed only after insertion and backend flush succeed. KnownFailure
means no terminal output was attempted and leaves the cursor available for retry
on a subsequent relevant event. Any error after writes may begin is Ambiguous:
disable automatic publication for this attachment, show a persistent degraded
indication, and retain managed transcript/export access. Do not automatically reset
the attachment to retry uncertain content. Successful repeated draws and retries of
known non-writes cannot duplicate output; physical exactly-once is not promised.

Session replacement and canonical-history replacement invalidate prepared chunks
through an explicit generation change at the mutation owner. Explicit attachment
establishes a new boundary without replaying replacement history. If an in-place
rewrite invalidates an already streamed cursor without a precise mapping, pause
automatic publication and show a conversation-changed notice. The old finalized
boundary may now precede printed text and cannot serve as a safe restart. Do not use source
indices across generations. Late settlement from an old generation is rejected.

Explicit `/session-export scrollback` remains an operator-requested snapshot action.
It must serialize through terminal ownership and re-anchor inline geometry afterward.
It does not reset the automatic cursor or silently count as automatic delivery.
An explicit snapshot may intentionally repeat visible history and must be labeled.
Normal exit drains one bounded pending slice and reports any remaining backlog as
available in the saved/managed transcript; it does not block exit to print everything.

## Implementation order and completion boundary

First write resolution and terminal-boundary tests, then extract shared rendering
under existing fullscreen regressions. Wire inline behind the explicit flag and
complete the captured transition slice before changing installed entry defaults.
Follow with failure/backlog coverage, configuration migration, and all native clients.
Keep feature, formatting, and generated-state commits separate.

No speculative dependency update, protocol change, plugin framework, or universal
widget abstraction is required. Empirical flicker and terminal geometry questions
are acceptance work. The prior macOS loader stall can block runtime verification,
but does not require a different TUI architecture or prevent implementation start.

## Implementation refinements

Automatic delivery stores a generation, canonical segment index, field/byte cursor,
and a bounded operation-summary accumulator. The shared semantic outcome reducer
consumes at most 512 bytes from each tool name/result. Active aggregation scans
incrementally, preserving its scan position between budgets; no committed prefix
is exported or hashed. Source rewrites after the unpublished frontier invalidate
only unfinished scanning. Rewrites touching committed content pause automatic
publication until an explicit new attachment, without replaying previous history.

Explicit primary output clears the owned live rows first. It increments the
existing presentation revision, causing the coordinator to acquire a new inline
anchor at the resulting cursor. Ordinary alternate visits retain the original
inline Terminal and resize it before insertion. A failed acquisition propagates
to the existing session cleanup; there is no speculative second rollback ledger.

Normal exit performs the ordinary one-batch publication opportunity and reports
remaining completed output if the bounded cursor has not caught up. It does not
block shutdown until an arbitrary transcript has drained.

## Subsequent operator direction

Live operator feedback rejected the original finalized-turn-only publication
policy: a three-line moving preview inside eight reserved rows made responses
unreadable while they streamed. The incremental-prefix policy above supersedes
that behavior. Validation must pause a long response and inspect primary terminal
history before completion; a final-response capture alone does not establish this
contract. The already-locked unicode-segmentation dependency provides grapheme
boundaries under the TUI feature, avoiding a custom Unicode segmentation algorithm.

Decorative footer inference/tool telemetry is now OBE. The current shared-layout
slice preserves existing Full widgets only as a migration baseline. Their core
retirement, including exclusive code and animation scheduling, is planned in
[tui-telemetry-addon-retirement](../tui-telemetry-addon-retirement/proposal.md).
Active/Full remain evidence preferences; future telemetry is an optional addon
capability rather than a reason to retain core instrument panels.

## Markdown and wrapping follow-up

Operator captures after the live-publication fix exposed a separate presentation
failure: the automatic adapter passed plain strings to native insertion and split
them at cell capacity. Consequently it printed Markdown delimiters and split
ordinary words. Text-retention checks did not establish readable presentation.

Keep the canonical cursor and delivery-settlement boundary. Carry styled lines
through native insertion, reuse shared Markdown presentation, and retain bounded
unfinished syntax/context across preparation cycles. Prose should prefer word
boundaries; code and table structure need their own existing rendering semantics.
No whole-response wait or replay-at-completion is acceptable. Capture styled
terminal output as well as plain payloads, including a held response, normal words
near a wrap boundary, split Markdown delimiters, and a width change. New output
uses the current width; immutable physical history is not retroactively reflowed.

Reuse the shared heading and inline-style helpers, but do not reuse a preview
table's cell truncation for permanent history. A table must retain its values
through wrapping or a narrow-width stacked presentation. Bound retained table
source and rendered scratch before expansion. The header and separator establish
column widths; completed body rows publish without waiting for the table to end.
A width change or a row that cannot fit the pinned layout uses labeled cells
without dropping values. Keep each paragraph's final wrapped
row pending until further text or a logical boundary makes it stable; publishing
that short row at every transport boundary would fragment otherwise intact prose.

Source consumption and styled output have separate byte limits: a small source
slice can expand into a wider physical row through indentation or table padding.
Stop source admission behind publishable output that cannot drain within the
current cycle. A single Unicode cluster or unfinished syntax construct exceeding
the retained-text limit uses the existing explicit text-limit degradation path.

## Activity status placement

Keep transient activity after the live response tail, beside the composer. The
`tui-live-activity` follow-up provides an explicit grey action strip there, shared
with fullscreen; the prior compact Working/cancellation fallback remains in the
composer when activity is hidden. Publication/degradation notices remain in the
composer border. A status row between native history and the live response tail
interrupts reading even if never committed to history; neither presentation may
insert activity there or publish it as conversation.

## Physical insertion fidelity

The Ratatui insertion adapter also needs physical cell fidelity. In the pinned
version, `insert_before` can send every temporary buffer cell to the backend,
including blank cells covered by a wide glyph. Those blanks consume additional
terminal columns and truncate text at row boundaries. Normalize only covered
cells in the temporary insertion buffer to empty symbols before emission. Keep
ordinary blank cells, the canonical source, and the shared fullscreen renderer
unchanged. Enabling scrolling regions alone is insufficient: its full-height
viewport path still uses the same complete-cell draw for the first inserted row.
Verify emitted bytes with the real Crossterm backend and compare captured payloads,
not merely line markers.

## Persistent notification retention

Persistent system notifications append centrally so later control responses and
local notices cannot modify an already-published record. Explicit mutable plan
snapshots retain their existing replacement behavior. Automatic native history
omits these mutable snapshots and advances past them; Workbench, fullscreen history,
and explicit export retain the current plan. Immutable notices and lifecycle
records remain eligible during a turn so they cannot block subsequent answer text.

Notification pruning records at most 64 chronological removal coordinates and
advances source generation. Native publication consumes this typed change to
rebase the cursor and finalized boundary while preserving surviving partial
field/byte/detail and scan state. An evicted current record resets its content
offsets; a partially emitted synthetic attachment notice retains its own offsets.
Generation changes reject stale prepared batches even when deletion occurs after
the cursor. Reconciliation runs before terminal event boundary assignments and
per frame, including fullscreen. Clear, replacement and arbitrary removals
invalidate pruning coordinates and retain conservative rewrite handling. No
second transcript or unbounded pruning queue is introduced.
