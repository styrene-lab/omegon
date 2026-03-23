# Tutorial demo project — self-seeded repo with live cleave demonstration — Design Spec (extracted)

> Auto-extracted from docs/tutorial-demo-project.md at decide-time.

## Decisions

### Tutorial project is a small Rust CLI tool with pre-seeded lifecycle artifacts (decided)

The project needs to be: (1) small enough that cleave branches finish in <60s, (2) interesting enough that the work is visible and inspectable, (3) Rust because that's Omegon's own language and the operator likely uses it. A CLI tool with 3-4 modules (config parser, formatter, validator, CLI interface) gives enough surface for a 3-branch cleave. Pre-seeded: 2-3 design docs (one decided, one exploring), 1 OpenSpec change with specs and tasks ready for cleave, ~10 memory facts giving the project a 'lived-in' feel, a .omegon/milestones.json with a '0.2.0' milestone.

### Tutorial cleave uses retribution tier with a cost warning upfront (decided)

A 3-branch cleave on gloriana could cost $2-5 in tokens — unacceptable for an onboarding flow the operator might run multiple times. Retribution tier is the cheapest cloud option and still demonstrates the full cleave lifecycle. The tutorial overlay shows an upfront notice: 'This demo will use ~$0.30 of API credits for the cleave demonstration. Press Tab to continue or Esc to skip.' If local inference is available, prefer that. The --context-class squad flag already constrains context size.

### Overlay steps reworked into 4 acts (10 steps) with new AutoPrompt trigger (decided)

The current 7 steps are all passive (look at this, press Tab). The new flow has 4 acts: Cockpit (passive UI tour), Agent Works (auto-prompted tasks the operator watches), Lifecycle (live cleave), Ready (wrap-up). This requires a new trigger type — AutoPrompt — that sends a prompt to the agent automatically and waits for the agent turn to complete before the overlay advances. The overlay narrates what's happening while the agent works. ~10 steps total. The overlay and lesson-file systems merge: overlay provides the visual narration, auto-prompts provide the agent instructions.

## Research Summary

### Current tutorial architecture

**Two tutorial systems exist:**

1. **Built-in overlay** (`tui/tutorial.rs`) — 7 compiled steps, game-style tooltip overlay that highlights UI elements. Triggered by `/tutorial` when no lesson files exist. Steps: Welcome → Engine → Inference → Tools → Slash Commands → Focus → Ready. Advances via Tab (passive) or action (command/input triggers).

2. **Lesson-file system** (`TutorialState`) — reads `.omegon/tutorial/01-*.md` files with YAML frontmatter. Each lesson's content is injected as a promp…

### Tutorial flow design — the operator journey

The tutorial should tell a story with the demo project as the stage. The operator doesn't just learn buttons — they watch Omegon do real work on a real (small) project, with the overlay narrating what's happening and why.

**Proposed flow:**

**Act 1 — The Cockpit** (overlay steps, no agent interaction)
1. Welcome — what Omegon is, what you're about to see
2. Engine Panel — model, tier, thinking, context
3. Instruments — context bar, memory strings, tool recency
4. Sidebar — design tree, node st…
