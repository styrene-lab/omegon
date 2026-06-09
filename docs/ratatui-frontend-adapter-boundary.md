---
id: ratatui-frontend-adapter-boundary
title: "Ratatui frontend adapter boundary"
status: exploring
parent: ui-surface-action-protocol
tags: [tui, ratatui, adapter]
open_questions: []
dependencies: []
related: []
---

# Ratatui frontend adapter boundary

## Overview

Refactor the native Ratatui TUI to act as an in-process frontend adapter that consumes semantic surfaces and emits UiActions, without changing visible default behavior.

## Decisions

### Keep frontend-local controls out of the first adapter slice

**Status:** accepted

**Rationale:** Conversation scroll/pin, selector navigation, tutorial overlay navigation, secret input editing, mouse hit testing, terminal copy mode, and layout behavior remain Ratatui-local. This keeps the first seam limited to portable operator intent.
