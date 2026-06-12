---
id: tui-polish
title: "TUI Polish Workstream"
status: exploring
tags: [tui, polish, ratatui, tachyonfx]
open_questions: []
dependencies: []
related: []
---

# TUI Polish Workstream

## Overview

Explore and implement Ratatui widget, TachyonFX, and UI chrome polish for the single-line TUI surfaces recently factored into shared glyph/horizontal-line grammar. Target low-risk visual improvements to workbench, active tool stream, separators, tool surface, footer/status chrome, and peer-agent representation without recoupling surface state.

## Decisions

### Segment presentation hierarchy separates prose from structured output

**Status:** accepted

**Rationale:** Conversation rendering should distinguish role/provenance from content form. Assistant responses and tool results such as reading a markdown file can both be prose/markdown and should share the same prose rendering path, while structured outputs use structured renderers. Segment self-reporting should project both axes: who/what produced the segment and what kind of content it contains.
