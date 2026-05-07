+++
id = "42dcd703-7512-4117-89ef-66dadb941eaa"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Display tool — agent-driven visual artifacts in the conversation

## Overview

Add a harness-internal `display` tool that lets the agent show the operator a visual artifact inside the conversation stream. The same display segment should render inline images and documents, and reuse the existing browser/service bridge model when a browser-backed surface is required. Auspex is the primary browser target; Omegon's local `/dash` surface remains a compatibility path where needed. V1 target: image and document artifacts rendered in a dedicated display segment within the conversation panel. Follow-on target: agent can start a service, capture a screenshot or selected artifact from it, and display that result back to the operator.

## Research

### Approved operator direction

Operator approvals recorded:
- Parent concept should be named **Conversation Rendering Engine**.
- V1 artifact kinds are limited to **image** and **document**.
- **Video** should render through a browser-backed surface — Auspex by default, or Omegon's local `/dash` compatibility surface where necessary — so the browser/OS handles playback.
- `view` and `display` remain intentionally distinct: `view` for inspection, `display` for operator-facing presentation.

Implementation consequence: `display` should introduce a dedicated display segment in the conversation stream for image/document artifacts, plus a browser-backed handoff path for video, targeting Auspex first and retaining `/dash` as a local fallback.

## Decisions

### Decision: Rename parent scope to conversation rendering engine

**Status:** decided

**Rationale:** The existing `markdown-viewport` name is too narrow. The actual scope already includes segment-based conversation rendering, inline images, and operator-facing artifact presentation. The parent node should be reframed around conversation rendering rather than markdown preview.

### Decision: V1 `display` supports image and document artifacts only

**Status:** decided

**Rationale:** Image and document artifacts are already compatible with the conversation segment architecture and existing rich rendering paths. Limiting V1 to these two kinds keeps the feature shippable and avoids terminal-side media playback complexity.

### Decision: Video display uses browser rendering, not inline terminal playback

**Status:** decided

**Rationale:** Terminal video playback is the wrong complexity surface. Browser-backed display lets the OS and browser handle codecs, controls, and rendering. Auspex should be the primary browser target, while Omegon can still reuse the existing `/dash` web-service model as a local compatibility path. The conversation display segment can show a handoff or poster artifact, but the primary video surface is the browser view.

### Decision: `display` is presentation-oriented and distinct from `view`

**Status:** decided

**Rationale:** `view` remains a file-inspection tool that reads and returns content blocks for agent consumption. `display` is a higher-level harness-internal presentation tool whose job is to show a selected artifact to the operator using a dedicated display segment in the conversation and, when appropriate, an Auspex-backed browser surface with `/dash` as a local compatibility option.

### Decision: One dedicated conversation display segment renders all `display` artifacts

**Status:** decided

**Rationale:** Images and documents should not invent separate ad hoc UI pathways. A single typed display segment in the conversation stream keeps the UX coherent, reuses the segment architecture, and matches the operator mental model of 'the agent is showing me an artifact' regardless of artifact type.
