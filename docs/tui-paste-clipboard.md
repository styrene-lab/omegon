+++
id = "802962d8-2b50-42e0-8a5d-c47333e4bfab"
kind = "document"
title = "TUI paste and clipboard handling — images, files, multiline"
status = "implemented"
tags = ["tui", "ux", "clipboard"]
aliases = ["tui-paste-clipboard"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "tui-visual-system"
+++

# TUI paste and clipboard handling — images, files, multiline

## Overview

The TUI needs robust paste handling: multiline text, images, file references, and potentially binary content. Current handling strips control chars and inserts printable chars into the single-line editor. Future work: multiline editor, image paste detection (iTerm2/Kitty protocol), file path extraction.

## Open Questions

*No open questions.*
