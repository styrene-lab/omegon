# Streaming presentation revisions

## Problem

Provider chunks are transport units, not presentation units. Applying every `MessageChunk` directly to the rendered conversation lets transport cadence control layout work. It also allows queued completion events to finalize a message before any accumulated streaming state is drawn.

Markdown makes direct chunk rendering especially unstable: an arbitrary prefix can contain an open fence, partial table, unclosed inline delimiter, incomplete link, or list whose eventual structure is not yet known.

## Contract

The TUI separates four states:

1. **Authoritative text** — every accepted runtime delta, retained losslessly for transcript and copying.
2. **Accumulated presentation** — content observed since the last publication.
3. **Published revision** — a coherent presentation snapshot eligible for drawing.
4. **Drawn revision** — the newest published revision acknowledged by a completed terminal draw.

A completion transition may become visual only when:

```text
published_revision == accumulated_revision == drawn_revision
```

This prevents `MessageEnd`, `MessageAbort`, or `TurnEnd` from overtaking visible content.

## Markdown projection

A published assistant presentation is conceptually:

```text
committed markdown blocks + provisional streaming tail
```

Committed blocks use the normal Markdown renderer and may be cached. The provisional tail uses conservative rendering so incomplete syntax does not repeatedly reinterpret earlier content.

Initial stable-boundary policy should be conservative:

- blank line outside a fence;
- closed fenced-code block;
- completed ATX heading or horizontal-rule line;
- end of message.

Tables, nested lists, incomplete links, and unbalanced inline delimiters remain provisional until a stable boundary or completion. This bounds reflow to the tail rather than the entire response.

## Ownership

- Runtime events remain authoritative and presentation-neutral.
- `ConversationView` owns canonical message text and lifecycle.
- `StreamingPresentationController` owns revision, publication, and completion gating.
- Conversation projection owns stable-block/provisional-tail derivation.
- The widget owns width/theme-specific rendering caches.
- The frame scheduler knows only that a revision is due or drawn; it does not parse Markdown.

## Publication policy

Publication may occur when:

- the normal frame interval elapses;
- a stable Markdown block boundary is crossed;
- a bounded byte/grapheme threshold is reached;
- completion requires a final publication;
- an operator-visible interaction makes the update urgent.

Detached or hidden views may publish less frequently, but canonical text accumulation never pauses.

## Known regression: active conversation separated by a blank viewport

A currently observed failure mode leaves a large, unexplained vertical void between the latest conversation content and the active-turn/status/composer surfaces. A short active transcript may remain near the top of the conversation area while an `active turn` row and composer remain at the bottom. The empty region can occupy most of the terminal height.

This is not acceptable spacing and must not be treated as intentional bottom anchoring. It creates three presentation failures:

- the latest reasoning or assistant output appears disconnected from the controls for the same turn;
- competing progress projections can remain visible at opposite ends of the viewport;
- the operator cannot tell whether output stalled, the viewport detached, or content was omitted.

### Reproduction shape

The observed state has these characteristics:

1. The conversation is shorter than the available viewport.
2. A turn is active and has emitted reasoning or assistant content.
3. The conversation content is rendered near the top of the viewport.
4. The active-turn/status and composer surfaces are rendered at the bottom.
5. The intervening rows are blank even though the operator did not detach or scroll.

Streaming, reflow, completion gating, terminal resize, and transitions between reasoning and assistant content are all suspected triggers. The exact trigger is not yet proven. The visual invariant below applies regardless of trigger.

### Required viewport invariant

When the viewport is attached to the tail:

- the newest published conversation content MUST remain visually adjacent to the bottom interaction surfaces, subject only to deliberate section spacing;
- short transcripts MUST NOT leave a large blank region between the newest content and active-turn/composer surfaces;
- publication or Markdown reflow MUST preserve tail attachment;
- an attached viewport MUST NOT silently acquire a stale scroll offset or false detached anchor;
- only explicit operator scrolling may enter detached mode.

When the viewport is detached:

- the TUI MUST show an unambiguous detached indicator;
- publication MUST preserve the operator-selected semantic anchor;
- returning to the tail MUST restore the attached invariant on the next draw.

### Verification scenarios

Visual regression coverage for the new presentation path must include terminal-buffer snapshots or equivalent row-position assertions for:

- a short active reasoning stream in a tall terminal;
- a short assistant stream after one or more publication revisions;
- final chunk and completion arriving in the same ingestion burst;
- reasoning-to-assistant and assistant-to-tool transitions;
- terminal resize while attached;
- detached scrolling during streaming and explicit return to tail;
- an active stream with compact status and composer surfaces visible.

For attached cases, tests must assert geometry, not merely text presence. At minimum, they must bound the number of blank rows between the final nonblank conversation row and the first active-turn/status/composer row. A snapshot containing the expected strings but retaining the large void does not satisfy this regression.

The blank-space regression is considered quashed only when these tests pass through the published-revision rendering path and a live debug session no longer reproduces the discontinuity.

## Required regressions

- queued chunks and completion require an intermediate drawn revision;
- chunk bursts coalesce into one revision;
- unclosed code fences remain provisional;
- tables do not repeatedly resize while incomplete;
- tool interleaving creates separate assistant presentation streams;
- detached viewports retain their anchor;
- final canonical text exactly equals all accepted deltas;
- final completion reparses only the provisional tail.
