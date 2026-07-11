+++
title = "Terminal media compositor"
tags = ["tui","media","rendering","kitty","architecture"]
+++

+++
id = "c9880385-7ccd-49c2-ae69-0ebb77570529"
kind = "design_node"

[data]
title = "Terminal media compositor"
status = "exploring"
issue_type = "architecture"
priority = 1
parent = "multimodal-conversation-media"
dependencies = []
open_questions = []
+++

## Overview

# Terminal media compositor

# Terminal media compositor

## Overview

Own native terminal media as a frame-level composition concern. Conversation segments emit semantic placement requests; they do not directly execute Kitty, Sixel, iTerm2, or fallback rendering. The compositor resolves final geometry, clipping, occlusion, protocol lifecycle, and fallback after every surface layout is known.

## Decisions

### Placement requests replace direct rendering

Conversation rendering emits `MediaPlacement` records. The final draw phase renders them only after menus, selectors, panels, prompts, modals, and other occluders have declared geometry.

```rust
struct MediaPlacement {
    media_id: AttachmentId,
    source_revision: u64,
    rect: Rect,
    clip: Rect,
    layer: SurfaceLayer,
    policy: PreviewPolicy,
}
```

### The compositor owns previous-frame state

It diffs current placements against the previous frame by stable media ID and source revision. It detects movement, resize, disappearance, occlusion, tab changes, and source replacement. Protocol state is never keyed by projected segment index.

### Conservative suppression precedes perfect clipping

The first implementation suppresses native previews whenever an overlapping surface, animation, or unsupported partial clip is active. The media card remains visible. Exact rectangle subtraction can follow once protocol behavior is verified across Kitty, Sixel, and iTerm2.

### Dedicated preview is the reliable fallback

`Enter` on a media card opens a dedicated preview surface. Inline preview may fall back to half-blocks, metadata-only, or hidden state. The dedicated surface owns the viewport and supports open-external, dimensions, path/source details, and future zoom/pan.

## Frame contract

1. Surfaces render buffered content and emit placements/occluders.
2. Layout resolves final rectangles.
3. Compositor intersects placements with their clips.
4. Occlusion policy suppresses or clips placements.
5. Placement diff invalidates stale/moved/resized protocols.
6. Protocol adapter renders eligible placements.
7. Encoding/protocol failures update preview diagnostics without damaging media-card content.

## Implementation phases

### Phase 0: immediate stabilization

- Keep the bottom-anchor coordinate correction.
- Suppress inline native media while any overlapping modal/menu/selector/panel/effect is active.
- Clear/rebuild protocol state on terminal resize, presentation-mode change, and active-tab transition.
- Log protocol encoding failures with media identity and dimensions.

### Phase 1: placement projection

- Add frame-owned `MediaCompositionState`.
- Have conversation projection return placements after segment layout.
- Replace `(projected_segment_idx, path)` cache keys with stable media IDs and revisions.
- Unit-test movement, resize, disappearance, and occlusion diffs independent of a real terminal.

### Phase 2: protocol matrix

- Integration-test Kitty placeholders and cursor restoration under resize and overlays.
- Exercise Sixel/iTerm2 behavior where available.
- Define protocol-specific deletion or full-redraw behavior.
- Add a deterministic half-block fallback for unsupported or unstable protocols.

## File Scope

- `core/crates/omegon/src/tui/media_compositor.rs` (new) — placements, occlusion, frame diff, protocol lifecycle.
- `core/crates/omegon/src/tui/image.rs` (modified) — protocol adapter only; stable media cache keys and diagnostics.
- `core/crates/omegon/src/tui/conv_widget.rs` (modified) — emit placement geometry aligned with segment layout.
- `core/crates/omegon/src/tui/mod.rs` (modified) — final composition phase and occluder collection.
- `core/crates/omegon/src/tui/segment_components/image.rs` (modified) — semantic media card/fallback status.
- `core/crates/omegon/src/tui/tests.rs` and compositor-local tests (modified/new) — surface and lifecycle regressions.

## Constraints

- Ratatui buffered chrome and native terminal pixels must never have independent unexplained positions.
- A hidden, off-screen, inactive-tab, or covered image must not leave stale terminal state.
- Rendering failure must degrade to a usable card, never a blank unexplained box.
- Effects and overlays may not mutate image placeholder cells after native composition.
- Do not assume Kitty semantics apply to Sixel or iTerm2.
- Testable geometry and lifecycle logic must remain independent of terminal availability.

## Open Questions

- [assumption] Suppressing native inline previews during overlapping surfaces is acceptable as the first correctness-first UX.
- Does ratatui-image expose sufficient protocol cleanup for each backend, or must Omegon force terminal redraw/cache reconstruction on disappearance?
- Should final native composition happen after every buffered overlay, or should all overlays instead publish occlusion before the native pass?
- Which animation/effect states are safe for native media, if any?

## Open Questions
