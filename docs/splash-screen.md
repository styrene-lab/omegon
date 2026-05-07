+++
id = "f03ee676-1959-4590-b5b9-23f73ffd793e"
kind = "document"
title = "Branded splash/loading screen for Omegon startup"
status = "implemented"
tags = ["ux", "tui", "startup"]
aliases = ["splash-screen"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
+++

# Branded splash/loading screen for Omegon startup

## Overview

Replace the wall of keybinding hints, changelog, and raw extension output with a branded splash loading screen that shows what's initializing. Two phases: (1) pre-import ANSI splash in bin/omegon.mjs while Node loads the module graph, (2) extension-driven header replacement once pi TUI is active, showing a minimal branded header instead of keybinding dump.

## Decisions

### Decision: Glitch-convergence splash as extension header + pre-import spinner

**Status:** decided
**Rationale:** Two-phase approach: (1) bin/omegon.mjs shows a braille spinner during module import, (2) extensions/00-splash sets a custom header with the full glitch-convergence ASCII logo animation + loading checklist. Force quietStartup=true to suppress the built-in keybinding wall. Other extensions report status via Symbol.for shared state. After animation + loading complete, transitions to minimal branded header.

## Open Questions

*No open questions.*
