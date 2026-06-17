---
id: composer-semantic-contract
title: "Composer Semantic Contract"
status: implementing
tags: [tui, ui-runtime, composer, surfaces]
open_questions: []
dependencies: []
related: []
---

# Composer Semantic Contract

## Overview

Track the next TUI/UI decoupling phase: extract the operator prompt composer into a semantic projection/action contract while keeping Ratatui as the reference frontend. First pass should add bounded composer draft actions and richer projection hooks without rewriting visual rendering.

## Research

### First-pass implementation notes

First implementation pass added shared UiAction composer mutations in core/crates/omegon/src/ui_runtime/actions.rs: ReplaceComposerDraft, ClearComposerDraft, AttachComposerPath. TUI routes them in App::handle_ui_action without visual rewrite, using existing Editor methods set_text, clear_line, insert_attachment.

### Composer cursor/edit action pass

Second implementation pass added semantic composer cursor/edit actions: MoveComposerCursor with direction/unit and EditComposer with bounded operations. Ratatui routes these to existing Editor movement/edit methods and exits history recall on edit operations. Tests cover word/character movement, word deletion, history recall exit, and rejection of unsupported direction/unit pairs.

## Decisions

### Start with semantic draft actions and projection compatibility

**Status:** accepted

**Rationale:** A narrow first slice can expose replace/clear/attach/remove composer draft operations through UiAction while preserving existing Ratatui rendering and editor internals.

### Keep frontend-local concerns out of composer contract

**Status:** accepted

**Rationale:** Wrapping, scroll row, terminal cursor screen coordinates, keybindings, and Ratatui style remain adapter concerns; shared composer state should describe draft content, tokens, cursor, mode, and capabilities.

### Expose composer cursor and word-edit mutations as semantic UI actions

**Status:** accepted

**Rationale:** Input-area parity should not depend only on raw terminal key handling; shared composer actions let Ratatui, ACP, and future frontends invoke the same bounded cursor/edit operations.
