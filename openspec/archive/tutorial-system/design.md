+++
id = "fc92290c-cbde-4aef-ba2a-325d48a73bb0"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Interactive /tutorial system — Design

## Architecture Decisions

### Compiled overlay engine as primary tutorial experience (decided)
The main tutorial is a compiled Rust overlay with `Step` structs containing title, body, anchor, trigger, and highlight. Two arrays: `STEPS_DEMO` (9 steps) and `STEPS_HANDS_ON` (7 steps). Triggers enforce pacing: `Enter` (passive advance), `Command` (wait for slash command with input passthrough), `AutoPrompt` (auto-send to agent, auto-advance on completion). The agent never sees more than one step.

### Lesson runner as fallback for custom content (decided)
Projects with `.omegon/tutorial/*.md` use the simpler `TutorialState` system: markdown files with YAML frontmatter, queued as prompts one at a time via `/next`. Progress persisted in `progress.json`. This exists alongside the overlay — the overlay is the default when no lesson files are present.

### Harness-controlled pacing (decided)
Both systems enforce structural pacing. The overlay controls advancement via trigger types. The lesson runner queues one file at a time. The agent never decides when to advance.

### Sandbox tutorial project (decided)
`/tutorial demo` clones `styrene-lab/omegon-demo` into `/tmp/omegon-tutorial` and exec's omegon there. The source content lives in `test-project/` in this repo.

### Junior-friendly content rewrite (decided, rc.16)
Step text rewritten for accessibility: collapsed cockpit tour to 1 step, removed jargon (no tier names, no "inference instruments", no "design tree nodes"), made cleave a Command trigger (overlay stays visible), moved dashboard to optional/post-action, added time estimates and recovery text.

## File Scope

| File | Role |
|---|---|
| `core/crates/omegon/src/tui/tutorial.rs` | Overlay engine: Step, Tutorial, Trigger, Anchor, Highlight types; STEPS_DEMO and STEPS_HANDS_ON arrays; rendering with smart anchoring; input passthrough logic |
| `core/crates/omegon/src/tui/mod.rs` | TUI integration: tutorial_overlay field, draw(), event loop interception, AgentEnd hook, slash command hooks, handle_tutorial/next/prev, TutorialState lesson runner |
| `core/crates/omegon/src/tui/segments.rs` | Image placeholder rendering (📎 filename card) |
| `test-project/` | Demo sprint board: index.html, src/board.js, ai/docs/*.md, ai/openspec/changes/fix-board-bugs/, ai/memory/facts.jsonl |

## Constraints

- Overlay must not block editor input on Command/AnyInput steps — input passthrough is load-bearing for the /cleave and /dash steps
- AutoPrompt steps must auto-advance on AgentEnd — manual /next for auto-prompts breaks the "watch the AI work" experience
- Demo project must be self-contained — no external dependencies, no build step, viewable in browser via file:// protocol
- Cost is not a concern for the tutorial — each auto-prompt is a full agent turn; this is the correct approach
