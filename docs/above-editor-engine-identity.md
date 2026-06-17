---
id: above-editor-engine-identity
title: "Move engine identity into above-editor status row"
status: exploring
parent: tui
tags: [tui, statusline, engine, slim-ui]
open_questions:
  - "[assumption] The above-editor status row has enough horizontal budget to show provider/model/thinking in a compact form at common terminal widths without hiding context percent, turn, and token counts."
  - "[assumption] The lower engine row can safely become exception-oriented without removing information operators currently rely on during normal operation."
dependencies: []
related: []
---

# Move engine identity into above-editor status row

## Overview

Consolidate normal provider/model/thinking-level identity into the above-editor status row where the current model is already displayed, reducing pressure on the lower engine/status bar. The lower engine row should become exception-oriented: offline/fallback/route warning/drift/auth issues rather than repeating ordinary engine identity.

## Research

### Initial code findings

`core/crates/omegon/src/tui/statusline.rs` already syncs `model_short`, `model_provider`, `model_tier`, `thinking_level`, `posture`, `runtime_brand`, `principal_id`, `authorization`, and `provider_connected` from `FooterData` via `StatusLine::sync_from_footer`. The pinned lifecycle row currently renders context percent, turn, `model_short`, and session input/output tokens. `engine_row_needed()` currently returns true only for disconnected providers or drift, while `project_engine_row()` contains normal identity metadata including posture, provider, tier, and thinking level.

## Open Questions

- [assumption] The above-editor status row has enough horizontal budget to show provider/model/thinking in a compact form at common terminal widths without hiding context percent, turn, and token counts.
- [assumption] The lower engine row can safely become exception-oriented without removing information operators currently rely on during normal operation.
