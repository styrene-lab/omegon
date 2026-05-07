+++
id = "b945013c-e4fb-4cc0-bd52-b55491ffe21b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Tutorial System — Tasks

## 1. Tutorial overlay engine (tui/tutorial.rs)

- [x] 1.1 Step/Tutorial/Trigger/Anchor/Highlight types
- [x] 1.2 STEPS_DEMO array (9 steps: cockpit, read code, design decision, fix plan, cleave, verify, dashboard, wrapup)
- [x] 1.3 STEPS_HANDS_ON array (7 steps: cockpit, read code, design notes, write spec, dashboard, wrapup)
- [x] 1.4 Overlay rendering with smart anchoring (upper/center/large-centered for active AutoPrompt)
- [x] 1.5 Input passthrough: Command/AnyInput steps let keys through to editor; Enter/AutoPrompt steps block
- [x] 1.6 Project-choice widget (step 0 on empty project — Demo vs My Project)
- [x] 1.7 AutoPrompt lifecycle: pending_auto_prompt → mark_auto_prompt_sent → on_agent_turn_complete → advance

## 2. TUI integration (tui/mod.rs)

- [x] 2.1 Add tutorial_overlay field to App, initialize as None
- [x] 2.2 Render overlay in draw() after effects, before toasts
- [x] 2.3 Event loop: Tab/Esc/ShiftTab interception, Command/AnyInput passthrough
- [x] 2.4 AgentEnd fires on_agent_turn_complete for AutoPrompt auto-advance
- [x] 2.5 Slash command dispatch calls check_command for Command steps
- [x] 2.6 User message dispatch calls check_any_input for AnyInput steps
- [x] 2.7 /tutorial creates overlay (hands-on); /tutorial demo creates demo overlay
- [x] 2.8 /tutorial status/reset work with overlay; /next /prev delegate

## 3. Lesson runner (TutorialState in tui/mod.rs)

- [x] 3.1 TutorialState: load markdown files from .omegon/tutorial/, parse frontmatter
- [x] 3.2 /tutorial loads lessons when .omegon/tutorial/ dir exists, queues first lesson
- [x] 3.3 /next advances lesson, queues content as prompt
- [x] 3.4 /prev goes back, queues content
- [x] 3.5 Progress persisted in progress.json

## 4. Step content rewrite (rc.16)

- [x] 4.1 Collapse 3 cockpit tour steps (Engine/Instruments/Sidebar) → 1 "Your Cockpit"
- [x] 4.2 Remove all Omegon jargon from first encounter
- [x] 4.3 Cleave step: Command("cleave") trigger — overlay stays visible
- [x] 4.4 Web Dashboard moved after action, made optional/skippable
- [x] 4.5 Time estimates on every AutoPrompt step
- [x] 4.6 Recovery text on cleave step

## 5. Replace /demo with /tutorial

- [x] 5.1 /demo alias routes to /tutorial handler
- [x] 5.2 launch_tutorial_project clones styrene-lab/omegon-demo, exec's omegon

## 6. Adjacent fixes (rc.16)

- [x] 6.1 /dash opens dashboard directly (no /dash open subcommand needed)
- [x] 6.2 Clipboard image paste: fixed osascript format matching (PNGf not public.png)
- [x] 6.3 Image placeholder: show 📎 filename in card border, no full temp path
- [x] 6.4 Mouse capture: EnableMouseCapture restored for scroll-wheel

## 7. Demo project content (test-project/)

- [x] 7.1 Sprint board with 4 seeded bugs (board.js)
- [x] 7.2 6 design nodes (ai/docs/*.md)
- [x] 7.3 OpenSpec change fix-board-bugs with specs and 4-task plan
- [x] 7.4 5 seeded memory facts (ai/memory/facts.jsonl)
- [x] 7.5 README explaining the project and bugs

## 8. Lesson files for .omegon/tutorial/ (NOT YET DONE — deferred to composable-tutorial-plugins)

- [ ] 8.1 Write lesson markdown files for the lesson runner path
- [ ] 8.2 Determine if lesson runner is subsumed by composable pack format
