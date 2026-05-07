+++
id = "68220563-be84-4d36-8366-48cd431919f1"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Claude Code expectation friction in Omegon

This note focuses on a specific mismatch: a user arrives from Claude Code expecting a lightweight interactive coding loop, but Omegon is built as a broader systems-engineering harness with memory, lifecycle state, browser surfaces, and worktree-based decomposition.

The main conclusion is simple: the friction is real, and most of it is not a bug. It is a product-shape mismatch. The mitigations therefore need to do two things:

1. reduce avoidable startup and UX surprise for Claude Code-shaped usage, and
2. make the "premium harness" behavior an explicit opt-in instead of an ambient default when the operator wanted a lean chat/edit loop.

## Evidence base

This assessment is grounded in current repo behavior and docs:

- README positions `om` as the fastest interactive path and the comparison profile for mainstream CLI coding agents, while `omegon` remains the richer harness mode (`README.md:59-85`, `README.md:180-188`).
- README also describes default Omegon as a richer harness with durable memory, design tree, OpenSpec, worktrees, `/auspex open`, and `/dash` (`README.md:93-103`, `README.md:123-153`).
- The tutorial itself is not a passive help screen; it can fire real agent turns, read code, store memory, mutate design-tree state, and push OpenSpec work (`core/crates/omegon/src/tui/tutorial.rs:22-62`, `core/crates/omegon/src/tui/tutorial.rs:135-220`).
- The TUI footer exposes system telemetry beyond a normal chat loop: context, memory, provider identity, authorization, model tier, and provider telemetry (`core/crates/omegon/src/tui/footer.rs:33-83`).
- Memory is loaded automatically from project state and registered as a first-class feature when the backend is available (`core/crates/omegon/src/setup.rs:302-405`).
- Lifecycle features are also initialized at startup from repo-root state, not only when explicitly requested (`core/crates/omegon/src/setup.rs:415-420`).
- Omegon intentionally advertises worktree-based parallelism as a core capability (`README.md:145-153`), and earlier design work explicitly frames this as safer but more structured than Claude Code's shared-filesystem delegation model (`openspec/design-archive/2026-03-21-subagent-architecture/spec.md:112-120`).
- Interactive Omegon is not trying to perfectly emulate the old pi/Claude shell. Prior design work deferred a "same UX, different host" bridge and instead accepted a distinct native TUI path (`docs/rust-tui-bridge.md:17-27`, `docs/rust-tui-bridge.md:87-99`).

## Ranked friction points

### 1. Default Omegon feels heavier than Claude Code by design

**Why this is friction**

A Claude Code user usually expects: open terminal, ask for code work, see terse tool activity, iterate quickly. Omegon's default pitch is broader: memory, lifecycle artifacts, telemetry, browser surfaces, worktrees, and system-control knobs. That creates immediate cognitive overhead even when the operator only wanted a coding loop.

**Evidence**

- README's fastest path for quick interactive use is explicitly `om`, not plain `omegon` (`README.md:59-85`).
- README says `om` is the de-facto comparison profile for mainstream CLI coding agents, while default `omegon` is the premium richer mode (`README.md:180-188`).
- Default Omegon is described with persistent memory, design tree, OpenSpec, browser surfaces, and worktree decomposition (`README.md:93-153`).

**Why I think this matters most**

This is the root mismatch. If the operator expected Claude Code and launched plain `omegon`, almost every later complaint will be downstream of this initial mode mismatch.

**Mitigation direction**

- Make mode selection explicit at first-run: **Lean interactive** vs **Systems mode**.
- Bias onboarding, docs, and shell guidance toward `om` for Claude Code migrants.
- In the default TUI, explain early that the extra surfaces are intentional harness features, not incidental clutter.

### 2. Omegon exposes far more visible runtime telemetry than Claude Code users expect

**Why this is friction**

Claude Code users generally expect the agent itself to be the main surface. Omegon keeps a lot of machinery visible: provider identity, context pressure, memory counts, authorization, model tier, thinking level, and provider telemetry. That is useful for debugging and systems work, but it increases visual and conceptual load.

**Evidence**

- README advertises "live engine, memory, and system telemetry" as a core TUI feature (`README.md:97-103`).
- `FooterData` tracks and renders model/provider, context usage, memory state, auth summary, model tier, thinking level, posture, and provider quota telemetry (`core/crates/omegon/src/tui/footer.rs:33-83`).

**Tradeoff**

This is not gratuitous. The README explicitly argues that exposing the true provider/model path prevents debugging blind spots (`README.md:105-121`). The cost is that the UI feels more like a cockpit than a chat tool.

**Mitigation direction**

- In slim mode, collapse telemetry aggressively to only provider, model, and context pressure.
- Defer advanced telemetry until expanded/focused views.
- Add a migration note: "If you want Claude Code-like visual density, use slim mode."

### 3. Memory and lifecycle state are ambient features, not special-purpose add-ons

**Why this is friction**

Claude Code users often assume the session is primarily conversational plus current-repo file access. Omegon instead boots with ambient project memory and lifecycle context. That changes how the system behaves and what it encourages the operator to do.

**Evidence**

- README treats project memory, design tree, and OpenSpec as first-class built-ins, not optional plugins (`README.md:123-143`).
- Startup code auto-selects `ai/memory` or `.omegon/memory`, creates the directory, opens the sqlite backend, imports tracked JSONL on empty DB, and registers memory tools when available (`core/crates/omegon/src/setup.rs:302-405`).
- Lifecycle features are initialized from repo-root state during setup (`core/crates/omegon/src/setup.rs:415-420`).

**What the user experiences**

A Claude Code migrant can reasonably ask, "Why is this agent telling me about memory facts, design nodes, or specs when I just wanted it to patch a file?"

**Mitigation direction**

- Provide a documented "stateless coding loop" profile that suppresses proactive lifecycle/memory guidance unless the operator invokes it.
- Make the first-run explanation explicit: Omegon is opinionated about durable project state.
- Keep slim mode truly slim; don't let lifecycle prompts leak back in by default.

### 4. The tutorial behaves like an active operator-training workflow, not a passive help overlay

**Why this is friction**

Claude Code users typically expect help text or short command hints. Omegon's tutorial can run real prompts, read files, store memory facts, write design-tree research, inspect OpenSpec state, and instruct the user to run `/cleave`. That is a stronger, more opinionated onboarding experience than many users expect.

**Evidence**

- `TutorialMode` explicitly governs whether tutorial steps may fire real agent turns (`core/crates/omegon/src/tui/tutorial.rs:22-40`).
- `tutorial_gate()` distinguishes interactive, consent-required, and orientation-only routes based on provider/auth state (`core/crates/omegon/src/tui/tutorial.rs:42-62`).
- Demo/tutorial steps include prompts that read code, store memory facts, mutate design-tree state, inspect OpenSpec tasks, and direct the user into `/cleave` (`core/crates/omegon/src/tui/tutorial.rs:135-220`).

**Tradeoff**

This is powerful onboarding for Omegon's real workflow. It is also a sharp mismatch for someone who expected "show me the shortcuts and get out of my way."

**Mitigation direction**

- Split tutorial entrypoints into **orientation** and **full guided workflow**.
- Default Claude Code migrants to orientation-only unless they explicitly opt into the interactive demo.
- Make the tutorial copy say upfront: "This tutorial does real work in your repo/project model."

### 5. Omegon pushes worktree-based decomposition where Claude Code users may expect an in-place interactive loop

**Why this is friction**

Claude Code users often expect the same session to keep editing directly in the working tree. Omegon's large-task story is deliberately more structured: complexity assessment, split tasks, isolated worktrees, merge/review. That is safer, but it feels slower and more process-heavy if the user expected direct iteration.

**Evidence**

- README treats parallel work in real git worktrees as a core capability (`README.md:145-153`).
- Prior design work explicitly contrasts Claude Code's shared filesystem delegation with Omegon's isolated worktree model and frames Omegon's approach as safer and more scope-enforced (`openspec/design-archive/2026-03-21-subagent-architecture/spec.md:114-120`).

**Tradeoff**

I think Omegon is right on the systems-engineering merits here. Real isolation, scope enforcement, and merge boundaries are better than shared-session hand-waving for risky multi-file changes. But the price is friction for users expecting conversational immediacy over controlled execution.

**Mitigation direction**

- Keep direct in-place editing as the obvious default for simple/single-file work.
- Present worktree decomposition as the escalation path, not the expected baseline.
- In Claude Code migration docs, explain the why: Omegon is paying process cost to buy file safety and determinism.

### 6. Omegon has multiple operator surfaces, not one obvious canonical UI

**Why this is friction**

Claude Code expectations skew toward a single obvious terminal interaction loop. Omegon exposes terminal UI plus browser surfaces, and the naming itself signals transition: `/auspex open` is the primary browser surface, while `/dash` remains a compatibility/debug path.

**Evidence**

- README advertises both `/auspex open` and `/dash` (`README.md:97-103`).
- The tutorial's browser step explicitly says Auspex is the full browser surface and `/dash` remains a compatibility path until `/auspex` becomes primary (`core/crates/omegon/src/tui/tutorial.rs:119-131`).
- Design docs also preserve `/dash` as a local compatibility/debug protocol while framing Auspex as the primary browser UX (`docs/conversation-rendering-engine.md:50-65`).

**Mitigation direction**

- Collapse the mental model in user-facing docs: "Terminal is canonical; browser is optional observability" or the reverse. Right now it reads transitional.
- Reduce duplicated command vocabulary once `/auspex` is mature enough.
- For Claude Code migrants, keep the migration path terminal-first.

### 7. Interactive parity with Claude Code is not the current architectural goal

**Why this is friction**

Some users will assume Omegon is trying to be "Claude Code plus extra features." Current design evidence says that is not quite true. Omegon accepted a distinct native TUI trajectory instead of preserving exact parity with earlier interactive shells.

**Evidence**

- `docs/rust-tui-bridge.md` concludes that preserving the prior host/renderer path cleanly is not worth it and recommends moving toward a native TUI instead (`docs/rust-tui-bridge.md:87-99`).

**Implication**

Expectation management matters. If users are promised the same feel, they will call the differences regressions. If they are told clearly that Omegon is a different harness optimized for explicit systems control, the same differences become deliberate tradeoffs.

**Mitigation direction**

- Stop implying full interactive equivalence where the architecture has already chosen divergence.
- Market slim mode as the nearest Claude Code-shaped profile, not as proof that default Omegon is the same experience.

## What should happen next

### Near-term

1. **Make `--slim` the migration default in docs aimed at Claude Code users.**
   The codebase and README already support this framing; the product should stop being coy about it.
2. **Split onboarding paths.**
   One path for lean interactive coding, one for systems/lifecycle mode.
3. **Reduce visual entropy in slim mode.**
   Keep the cockpit available, but not ambient.

### Medium-term

1. **Introduce a named profile for stateless coding work.**
   If Omegon wants Claude Code migrants, it needs a first-class low-ceremony mode, not just a comparison flag.
2. **Rationalize browser surfaces.**
   `/auspex` vs `/dash` is a transition story, not a clean product story.
3. **Be explicit about decomposition thresholds.**
   Worktrees are a strength, but only when invoked at the right times.

## Bottom line

The biggest friction for Claude Code users is not one missing shortcut. It is that Omegon is a different kind of tool.

Claude Code expectations are centered on a streamlined interactive coding loop.
Omegon's default experience is a systems harness with visible runtime state, durable project memory, lifecycle machinery, and controlled parallel execution.

That architecture has real advantages. It also imposes real migration cost.

The correct mitigation is not to pretend the products are identical. It is to give operators a clear low-friction entry path (`--slim` / lean interactive mode), then let them opt into the heavier harness features when the task actually needs them.
