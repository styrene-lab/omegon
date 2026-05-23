+++
id = "tui-pane-focus-input-model"
kind = "document"
title = "TUI Pane Focus and Input Model"
status = "seed"
tags = ["tui", "input", "focus", "keyboard", "mouse", "pane"]
aliases = ["tui-pane-focus-input-model"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
parent = "tui-surface-substrate-reevaluation"
dependencies = ["tui-surface-substrate-reevaluation", "tui-surface-inventory-taxonomy"]
open_questions = [
  "What is the canonical focus order between conversation, editor, inspectors, overlays, and embedded PTY panes?",
  "Which keybindings are global, which are pane-local, and which are child-process passthrough?",
  "How should mouse events be routed when a pane contains a child TUI with its own mouse mode?",
  "How does Omegon avoid terminal keybinding assumptions that fail outside Kitty/Ghostty enhanced keyboard protocols?",
  "What visible affordances tell the operator which pane owns input?"
]
related = ["tui-surface-substrate-reevaluation", "reader-workspace-embedded-pty-alternatives", "tui-design-tree-widget"]
+++

# TUI Pane Focus and Input Model

## Overview

Define the focus, keyboard, mouse, and lifecycle rules required before Omegon adopts pane-like TUI surfaces.

This node exists because pane substrates can make layout easier while making input ownership harder. A substrate decision without an input model would be incomplete.

## Required concepts

- Global command mode: Omegon shortcuts and slash commands.
- Editor focus: text entry and prompt composition.
- Conversation focus: scroll, expand/collapse, copy/select.
- Inspector focus: tree/list navigation, search, expand/collapse.
- Child PTY focus: raw-ish passthrough to hosted TUI processes.
- Overlay focus: modal capture until dismissed.

## Research tasks

1. Inventory current keybindings and focus assumptions.
2. Identify collisions with child TUI applications.
3. Define a focus stack or focus graph.
4. Define escape hatches:
   - return focus to editor;
   - close focused pane;
   - toggle pane visibility;
   - send literal key to child process.
5. Define mouse routing policy for normal widgets and child PTYs.
6. Define visual focus indicators that work in slim mode.
