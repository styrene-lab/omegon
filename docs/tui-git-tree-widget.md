+++
id = "f9798293-4dc6-4fcc-8da6-ccbecc38f0fc"
kind = "document"
title = "Git branch tree widget — interactive, color-coded, scrollable"
status = "exploring"
tags = []
aliases = ["tui-git-tree-widget"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "tui-hud-redesign"
+++

# Git branch tree widget — interactive, color-coded, scrollable

## Overview

Replace the bare "system" footer card with a git branch tree in the dashboard sidebar. Show branch topology as an actual tree — not a flat `git branch` list. Color-code by convention: cleave branches (cyan), feature (green), fix (amber), refactor (blue), main/trunk (white). Show current branch highlighted. Overflow scrolls via mouse wheel. Hotkey to focus the panel for keyboard navigation.

Data source: `git for-each-ref` for branches, `git log --graph` for topology. Refresh on file system events or on a timer.

Interactive: when focused, arrow keys navigate, Enter checks out, Delete deletes (with confirmation).

## Open Questions

*No open questions.*
