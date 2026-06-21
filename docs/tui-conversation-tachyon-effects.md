+++
title = "TUI Conversation Tachyon Effects"
tags = ["design","tui","effects"]
+++

+++
id = "26dc7609-a433-4a5b-8dda-5579216ab318"
kind = "design_node"

[data]
title = "TUI Conversation Tachyon Effects"
status = "exploring"
issue_type = "design"
priority = 2
dependencies = []
open_questions = []
+++

## Overview

# TUI Conversation Tachyon Effects

---
title: TUI Conversation Tachyon Effects
status: exploring
tags: [design, tui, effects]
---

# TUI Conversation Tachyon Effects

## Overview

The conversation surface now has basic action feedback: selected segment hints, copy toasts, footer pulse, and a conversation-wide action pulse. Richer tachyon-style effects should build on that without polluting copyable transcript text or creating hidden gesture semantics.

## Design Nodes

### 1. Localized selected-segment pulse

**Status:** seed

**Intent:** Trigger an effect on the selected segment rectangle after copy or expand, instead of pulsing the whole conversation area.

**Open questions:**

- [assumption] The conversation renderer can expose the selected segment `Rect` after clipping without destabilizing scroll anchoring.
- Should clipped segments pulse only the visible portion or the full logical segment when it re-enters view?
- Should the effect live in `Effects` as a zone-subrect effect or in `ConversationWidget` as render-time style modulation?

**Acceptance:**

- Copy/expand visibly pulses only the selected item.
- Transcript copy text remains unchanged.
- No per-frame layout recomputation beyond existing segment height cache behavior.

### 2. Expansion reveal sweep

**Status:** seed

**Intent:** When a collapsed tool card expands, run a short reveal/sweep so the operator can visually track the new detail region.

**Open questions:**

- [assumption] Expansion events can tag the segment id/index for one or two frames after mutation.
- Should the sweep emphasize the header, the newly revealed body, or both?
- How should pinned expansion differ from double-click expansion, if at all?

**Acceptance:**

- Double-click expansion gets immediate visual confirmation beyond the toast.
- The effect does not make routine streaming tool updates noisy.
- The effect degrades safely when the expanded card is partly off-screen.

### 3. Selection armed-state shimmer

**Status:** seed

**Intent:** Use a subtle selected-item shimmer to communicate that the highlighted segment is actionable before the operator double-clicks.

**Open questions:**

- [assumption] A low-frequency shimmer will not distract while reading long assistant responses.
- Should copyable and expandable armed states use distinct hue/marker language?
- Should keyboard-selected and mouse-selected segments shimmer identically?

**Acceptance:**

- Selected copyable/expandable segments are easier to identify at a glance.
- The shimmer pauses or remains subtle during active streaming.
- The selected hint text remains the semantic source of truth for the action.

### 4. Copy confirmation overlay refinement

**Status:** seed

**Intent:** Evaluate whether normal toast rendering is sufficient or whether copy needs a dedicated bottom-right/center transient overlay.

**Open questions:**

- [assumption] Existing toast visibility varies enough by layout/theme that a stronger overlay may be warranted.
- Should copy confirmation share command toast infrastructure or be a separate action-confirmation surface?
- What duration balances noticeability with non-interruption?

**Acceptance:**

- Copy success is visible in Slim and Full UI modes.
- Confirmation is non-modal and disappears automatically.
- Repeated copy actions refresh rather than stack noisy overlays.

## Open Questions
