+++
id = "8276aa92-6c22-4a85-8267-fc8123a89010"
kind = "document"
title = "Footer idle state — engine border, useful content in empty panels before first turn"
status = "implemented"
tags = []
aliases = ["footer-idle-state"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
+++

# Footer idle state — engine border, useful content in empty panels before first turn

## Overview

The footer renders 12 rows of mostly void before the first LLM turn. The engine panel has no border (visually inconsistent), the tools panel shows '0/0 active' in a huge box, and the inference panel has a single dotted memory line in empty space. These panels should show useful content at idle: engine gets a border, tools shows a quick reference, inference shows a ready-state indicator.

## Open Questions

*No open questions.*
