+++
id = "e37064bf-e43a-4175-a3f1-fe64b636ef91"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Interactive /tutorial system — structured onboarding replacing /demo

## Intent

Replace the old `/demo` (which blasted through all phases because pacing was advisory) with a structurally-enforced tutorial system. Two layers:

1. **Overlay engine** — compiled step arrays rendered as floating TUI callouts. The harness controls pacing via triggers (Tab to advance, Command to wait for slash command, AutoPrompt to auto-send and wait for agent completion). The agent never sees more than one step at a time.

2. **Lesson runner** — markdown files in `.omegon/tutorial/` queued as prompts one at a time. Simpler fallback for projects with custom tutorial content.

The demo mode uses a pre-seeded sprint board project (4 bugs, 6 design nodes, OpenSpec specs) to showcase the full lifecycle: read code → store memory → make design decisions → write specs → parallel fix → verify → browser.

## Status

Implemented in rc.16 on `feature/tutorial-system`. Overlay engine fully wired into TUI with input passthrough, AutoPrompt lifecycle, and Command triggers. Step content rewritten for junior engineer accessibility. Demo project content seeded.

Remaining: lesson markdown files for the lesson runner path are deferred to the composable-tutorial-plugins design node (0.16.0 milestone feature).
