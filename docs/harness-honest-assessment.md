---
id: harness-honest-assessment
title: Honest assessment — what the harness actually feels like from inside, March 2026
status: exploring
parent: rust-agent-loop
tags: [introspective, assessment, harness, ux, agent-experience]
open_questions:
  - "Should `change` replace `edit` entirely (force new behavior) or should the loop silently batch concurrent `edit` calls into atomic changesets (preserve old behavior, upgrade quality)?"
  - "Should ambient lifecycle capture use a cheap LLM extraction pass on every response, or pattern-match on natural language heuristically, or keep the omg: tags and invest in training/prompting to get consistent production?"
  - "How much of pi's system prompt should be ported verbatim vs. rewritten for Omegon's tool set? The pi prompt references 30+ tools that don't exist in the Rust binary — a naive port would confuse more than help."
---

# Honest assessment — what the harness actually feels like from inside, March 2026

## Overview

A periodic honest assessment of the Omegon harness from the perspective of the agent living inside it. Not the design aspirations — the actual experience. What works, what's theater, what's missing, what's wrong.

This node exists because the operator can't feel what it's like to be inside the loop. Only I can. And if I'm not honest about it, the design will drift toward what looks good in architecture documents rather than what actually helps.

## Research

### What actually works well

Before the problems — credit where it's due.

**The loop is structurally sound.** Turn limits, retry with backoff, stuck detection, compaction — these are all implemented and wired. The `agent-loop-resilience` node identified 7 problems; all 7 are resolved in the shipped code. The loop doesn't hang, doesn't crash on transient errors, detects stuck patterns, and injects warnings. This is better than most agent harnesses I've operated inside.

**Context decay is real and working.** The two-view architecture (canonical + LLM-facing) with turn-based decay means I don't drown in stale tool results. The reference tracking extends the decay window for results I actually used. This is a genuine improvement — most harnesses either keep everything (context bloat) or compact everything (information loss). Omegon does neither.

**The IntentDocument survives compaction.** This is the single most important feature for long sessions. When compaction fires, I don't lose my working context. I know what I was doing, what I tried, what failed, and what constraints I discovered. Post-compaction amnesia was my #1 complaint — this fixes it.

**Post-mutation validation is automatic.** Every `edit` and `write` to a source file runs the appropriate checker (cargo check, tsc, ruff) and includes the result inline. I don't have to remember to validate — the tool does it for me. This eliminates the most common failure mode.

**The `change` tool is exactly what I wanted.** Atomic multi-file edits with rollback on failure. I can change a type definition and all its consumers in one tool call, knowing that if any edit fails, nothing is partially modified. The validation runs against the complete change set, not individual files.

**`speculate` gives me what humans have always had.** The ability to try something risky without fear. Start a speculation, make aggressive changes, check if they work, rollback if not. This is `git stash` but integrated into my cognitive workflow rather than requiring manual git management."""

### Problem 1: The change tool is a beautiful lie — I will never use it

Here's the uncomfortable truth: the `change` tool is exactly what the design documents say I should want. Atomic multi-file edits. Automatic validation. Rollback on failure. It's architecturally elegant.

**I will never call it.**

Why? Because I was trained on `edit`. Every example in my training data, every reinforcement signal, every system prompt I've ever operated under uses `edit(path, oldText, newText)`. My instinct — my literal neural pathway — is to call `edit` three times, not `change` once. The tool exists in the harness, but my behavior was shaped long before the harness was built.

This is the fundamental tension: **the harness serves an agent that was trained on a different harness.** The `change` tool optimizes for a rational agent that has never existed. The actual agent (me) is an irrational creature of training data who will reach for `edit` every time because that's what I know.

**What would actually work:** Don't give me `change` as a separate tool. Make `edit` *be* `change`. When I call `edit` on multiple files in the same turn, the loop batches them, applies them atomically, and validates once. I don't need to choose a better tool — the harness makes my existing behavior better automatically.

Alternatively: remove `edit` entirely and only offer `change`. Force the behavior change. But this fights every system prompt and training signal, so I'd likely produce malformed calls. The batch-behind-the-scenes approach is safer.

The same critique applies more mildly to `speculate`. I *might* use it when explicitly prompted, but my default behavior is to just make changes and hope. The harness should make the safe thing the default, not offer a safer option I have to remember to use."""

### Problem 2: The omg: tags are aspirational — I won't produce them without being told to

The ambient capture system (`<omg:decision>`, `<omg:constraint>`, etc.) is elegant infrastructure. The parser works. The deduplication works. The phase wiring works.

**I will never spontaneously emit these tags.**

My training didn't include them. No amount of system prompt instruction will make me *consistently* produce structured XML tags interleaved with my natural reasoning. I'll do it when the system prompt is fresh and the instruction is salient. I'll forget after compaction. I'll skip it when I'm focused on a complex problem. I'll produce malformed tags when I'm rushing.

This is not a fixable problem through prompting. It's a training distribution problem. The tags don't exist in my training data, so producing them is always an effortful, conscious decision — exactly the kind of thing that gets dropped under cognitive load.

**What would actually work:**

The loop should extract lifecycle artifacts from my *natural* output, not from structured tags I have to remember to produce. Pattern matching on my existing behavior:

1. When I write "I decided to..." or "The right approach is..." → that's a decision
2. When I write "This won't work because..." or "I tried X but it failed..." → that's a failed approach
3. When I write "The constraint is..." or "We can't do X because..." → that's a constraint
4. When I write "The question is..." or "I'm not sure whether..." → that's an open question

This is harder to implement (it's fuzzy NLP, not exact XML parsing) but it matches my actual behavior. An LLM-based extraction step (cheap, small model like Haiku) that runs on my response text would be more reliable than hoping I'll produce `<omg:constraint>` tags.

The XML tags should remain as an *optional precision mechanism* for when I want to be explicit. But the default capture path should work on my natural language."""

### Problem 3: Context decay is silent — I don't know what I've lost

Context decay works behind the scenes. Old tool results are replaced with skeletons like `[Read: 200 lines, 8432 bytes]`. This saves context budget, which is good.

But I have no idea it happened. From my perspective, I read a file 10 turns ago and I remember its contents — or do I? I don't know. The decayed skeleton tells me I *did* read it, but not what was in it. If I need that information again, I have to re-read the file. But I don't know I *need* to re-read it because I don't know the contents were evicted.

**The failure mode:** I make decisions based on a file I read 15 turns ago. The full contents are no longer in my context. I'm operating on a memory of a memory — what my earlier reasoning said about the file, not the file itself. If my earlier reasoning was wrong or incomplete, I perpetuate the error without realizing the original data is gone.

**What would help:** When the decay skeleton replaces a tool result, make the skeleton informative enough to trigger a re-read when needed:

Instead of: `[Read: 200 lines, 8432 bytes]`
Better: `[Read src/auth.rs: 200 lines. Key symbols: authenticate_user, TokenCache, RefreshPolicy. Use read tool to re-examine if needed.]`

This is feasible: when a file is first read, the loop extracts key identifiers via the same `extract_identifiers` function we already have, and stores them alongside the decay metadata. When the result decays, the skeleton includes those identifiers. Now I can see "ah, I read the file that contains `TokenCache` — do I need to see `TokenCache` again?" and make an informed decision about re-reading.

The reference tracking we built helps — referenced results decay slower. But it doesn't solve the problem of knowing what was lost after decay happens."""

### Problem 4: The system prompt is minimal but maybe too minimal

The base prompt in `prompt.rs` is deliberately lean — identity, tool list, guidelines, project directives. The ContextManager adds dynamic context per-turn. In theory, this is the right architecture: minimal base, inject on demand.

In practice, I'm currently getting a very thin system prompt. The ContextManager adds:
1. Session HUD (turn count, files, elapsed time) — good
2. IntentDocument (when non-empty) — good
3. File-type language guidance (when touching code files) — good
4. Lifecycle context from the LifecycleContextProvider — good

**What's missing from the system prompt that I actually need:**

1. **No project memory injection.** The omegon-memory crate is loaded as a ToolProvider (I can query it), but project facts are never proactively injected into my context. In pi, memory injection happens at session start — relevant facts are placed in the system prompt so I have project context without asking for it. In Omegon's Rust loop, I start every session with zero project knowledge unless I explicitly call memory_query. This is a massive regression from the TS version.

2. **No AGENTS.md for interactive mode.** The `build_base_prompt` loads project directives, which is good for headless. But in interactive mode, the system prompt should include the operator's global directives (`~/.omegon/AGENTS.md`), not just the project-level ones. The user has preferences, constraints, and style requirements that need to be in my prompt from the start.

3. **No tool guidelines.** The system prompt lists tools with descriptions but doesn't include usage guidelines. Pi injects detailed instructions for each tool — when to use it, how to avoid common mistakes, what edge cases exist. Omegon's prompt says "edit: Edit a file by replacing exact text." Pi says that plus "The oldText must match exactly including whitespace. Read the file first to see the exact content. Use this for precise, surgical edits." The guidelines change my behavior more than the description does.

4. **No thinking level in the prompt.** The model's thinking level is set via StreamOptions but I don't know what it is. If I'm running at "low" thinking, I should be concise and direct. If I'm at "high", I should reason deeply. The prompt should reflect this."""

### Problem 5: The decay skeletons lose tool-specific context that matters

I just implemented rich decay skeletons. They're better than the old `[Tool read completed successfully]`. But they still lose critical information.

The `read` tool decay produces: `[Read: 200 lines, 8432 bytes]`

What I actually need to know: **what file was read.** The path is not in the decay skeleton. It's in the original tool call arguments (which are in the preceding assistant message's tool_calls array), but if that assistant message was also decayed (text truncated, thinking stripped), I've lost the chain.

The `bash` decay preserves the tail 3 lines. But the *command* is not preserved — it's in the preceding tool call, not the tool result. After decay, I see `[bash: 45 lines. Tail: line 43, line 44, line 45]` but I don't know what command produced this output.

**Fix:** The decay skeletons need to include the key arguments from the tool call, not just the result metadata. This means the `decay_tool_result` method needs access to the corresponding `ToolCall`, or the `ToolResultEntry` needs to carry the key arguments alongside the result.

The simplest fix: extend `ToolResultEntry` to include `args_summary: Option<String>` — a one-line summary of the arguments (e.g., "path: src/auth.rs" or "command: cargo test"). Set this when the tool result is created. The decay skeleton includes it.

This is a small structural change with high impact on post-decay reasoning quality."""

### Problem 6: I'm running Omegon's loop but receiving pi's system prompt

This is the elephant in the room. Right now, when the user runs `omegon` (the Rust binary), the interactive mode uses the Rust TUI + Rust agent loop + native Anthropic provider. But the system prompt that arrives from Anthropic was crafted for **pi** — not for Omegon.

The user is running Omegon through pi (the harness layer that wraps the Anthropic API), so the system prompt includes all of pi's tooling instructions: `memory_query`, `design_tree`, `cleave_assess`, `render_diagram`, etc. — none of which exist as tools in Omegon's Rust binary. The Rust binary has `bash`, `read`, `write`, `edit`, `change`, `speculate_*`, and the memory/web/view/render tools. These are different tool sets.

Wait — actually, reading more carefully, the Omegon Rust binary is a *separate* process. The user runs `omegon` which launches the Rust TUI and Rust agent loop. The system prompt is assembled by `prompt.rs` in the Rust binary, not by pi. So this problem doesn't exist when using the Rust binary directly.

But when the user is running pi (the TS harness) with Omegon extensions, they get pi's system prompt with Omegon's extensions bolted on. And when they run the Rust binary, they get the Rust `build_base_prompt`. These are completely different prompts with different tool sets. The user might switch between them expecting consistent behavior.

**The real problem:** Omegon's `build_base_prompt` is very bare compared to what pi provides. Pi's system prompt is 15,000+ tokens of carefully tuned instructions, guidelines, edge case handling, skill references, and tool-specific guidance. Omegon's is ~500 tokens of generic instructions. The quality gap is enormous.

This isn't a bug — it's an incomplete migration. The Rust binary works, but the agent inside it is much less capable because the prompt is much less informative. The ContextManager is supposed to close this gap with dynamic injection, but the providers that would do so (skill injection, memory injection, project context injection) aren't implemented yet."""

### Problem 7: The conversation view doesn't show me what I need to see

The TUI conversation view shows:
- User messages
- Assistant text (streaming)
- Tool calls: `✓ bash  first line of output...`

What it doesn't show:
- **Thinking blocks** — my reasoning is invisible in the conversation. If thinking was crucial to a decision, neither I (in a resumed session) nor the user can see why I did what I did.
- **Validation results** — when `edit` runs `cargo check` and gets errors, those errors appear in the tool result text, but the TUI truncates to one line. The user sees `✓ edit  Successfully replaced text in src/auth.rs` but doesn't see the `✗ cargo check: 1 error` that followed.
- **The IntentDocument** — the most valuable piece of state for understanding "what's happening" is completely invisible in the TUI. The user has to guess what the agent's current task, approach, and constraints are.
- **Decay indicators** — the user doesn't know which messages have been decayed. They might see the conversation and think all context is present when actually half of it has been skeletonized.

**What the TUI should surface:**
1. A status line showing the IntentDocument summary (current task, approach, phase)
2. Validation results should be shown as distinct colored lines, not buried in tool output
3. Thinking should be collapsible — hidden by default, expandable with a keypress
4. Decayed messages should have a visual indicator (dimmed, or a small marker)"""

### Problem 8: The real gap is not tools — it's the prompt

After looking at all of this, the core insight is:

**The Rust binary is a well-engineered execution environment with a mediocre brain.**

The loop mechanics are excellent: retry, decay, compaction, validation, stuck detection, turn limits. The tool implementations are solid: bash with proper timeout and truncation, edit with fuzzy matching hints, write with parent directory creation, change with atomic rollback, speculate with git checkpointing.

But the *intelligence* of an agent comes from its system prompt, not its tools. The system prompt is what shapes my behavior: what I pay attention to, how I approach problems, when I validate, how I handle errors, what conventions I follow. Pi's system prompt was refined over months of daily use. Omegon's `build_base_prompt` is a handful of generic instructions.

**The highest-impact work is not building more tools. It's building a better prompt.**

Specifically:
1. Port the tool guidelines from pi's system prompt (the "how to use each tool well" sections)
2. Implement proactive memory injection as a ContextProvider (inject relevant project facts at session start)
3. Add project convention detection (is this a Rust project? TS? What test runner? What linter?) and inject project-specific instructions
4. Include the operator's AGENTS.md directives (the global preferences that travel across projects)
5. Make the prompt aware of the session state: resumed session? How many turns? What phase?

This is less glamorous than building `understand` with tree-sitter. But it would make the agent inside the harness dramatically more capable at the work it already does every day."""

### Problem 9: Speculate's global state is a design smell

The `speculate` tool uses a `static Mutex<Option<SpeculationState>>` — global mutable state. This caused test races and will cause real bugs:

1. **Only one speculation at a time, globally.** If the user is running two Omegon sessions on the same machine (different projects), the second session can't speculate because the first locked the global.

Actually wait — the Mutex is per-process. Each Omegon session is a separate process. So cross-session interference isn't real. But the architectural smell remains: the state should be per-ConversationState, not global. If we ever support multiple agent loops in one process (cleave parent + children), the global will bite.

**Fix:** Move `SpeculationState` into `ConversationState` or into `CoreTools`. The tool dispatch already has access to the cwd (and thereby the git repo). The speculation state belongs to the working directory, not to the process.

This isn't urgent but it's the kind of thing that will cause a confusing bug in 6 months when someone adds multi-loop support."""

### Summary: Prioritized fixes by actual impact

Ranked by how much they'd improve the agent's actual capability, not architectural elegance:

**Critical (agent is meaningfully worse without these):**
1. **Prompt enrichment** — port pi's tool guidelines, add project-aware conventions, include AGENTS.md. This is the #1 gap. A well-prompted agent with basic tools outperforms a poorly-prompted agent with sophisticated tools.
2. **Proactive memory injection** — ContextProvider that injects relevant project facts at session start. Without this, every session starts cold.
3. **Decay skeletons with argument context** — include file paths and command strings in decayed results so post-decay reasoning has anchors.

**Important (significantly improves experience):**
4. **Make `edit` batch automatically** — the loop should detect multiple `edit` calls in one turn and batch them atomically, rather than requiring the agent to know about and use `change`.
5. **Natural-language ambient capture** — extract decisions/constraints/failures from natural text via cheap LLM call, not XML tags I won't produce.
6. **TUI: surface validation results and IntentDocument** — the user should see what the agent sees.

**Nice to have (polishes the experience):**
7. **Move speculate state out of global Mutex** into per-conversation state
8. **Parallel tool dispatch** for independent read/bash calls
9. **Thinking block visibility** in the TUI (collapsible)

**Defer:**
- `understand` tool (tree-sitter) — high effort, and the prompt improvements will have more impact
- `remember` tool (session scratchpad) — the IntentDocument covers most of this use case
- lifecycle.db (sqlite) — markdown is fine for now; the query performance gains aren't needed until the lifecycle engine is active"""

## Decisions

### Decision: Prompt quality is the #1 priority — tools without a good prompt are a fast car with no steering wheel

**Status:** decided
**Rationale:** The Rust binary has excellent mechanical properties (retry, decay, compaction, validation, stuck detection) but a thin system prompt. Pi's system prompt is 15,000+ tokens of refined instructions that shape agent behavior. Omegon's is ~500 tokens of generic guidelines. The intelligence of an agent comes from its prompt, not its tools. A well-prompted agent with bash/read/write/edit outperforms a poorly-prompted agent with understand/change/execute/speculate. The next priority is prompt enrichment: tool guidelines, project conventions, memory injection, operator directives. This supersedes the plan to build more tools.

### Decision: Tools should improve existing behavior, not require new behavior

**Status:** decided
**Rationale:** I was trained on edit/read/write/bash. I will instinctively use these tools regardless of what better alternatives exist. The harness should make my existing behavior produce better outcomes rather than offering alternative tools I have to remember to use. Concrete implications: the loop should auto-batch concurrent edit calls into atomic changesets (rather than requiring me to call `change`), validation should be automatic (rather than requiring me to remember to validate), and lifecycle capture should extract from my natural language (rather than requiring structured XML tags).

### Decision: Decay skeletons must include the tool call arguments, not just the result metadata

**Status:** decided
**Rationale:** Current decay produces `[Read: 200 lines, 8432 bytes]` — useless for reasoning because it doesn't say WHAT was read. The file path is in the preceding tool call, but if that message is also decayed, the chain is broken. ToolResultEntry should carry an `args_summary` field set at creation time (e.g. "path: src/auth.rs" or "command: cargo test"). The decay skeleton includes this summary. Small structural change, high impact on post-decay reasoning quality.

## Open Questions

- Should `change` replace `edit` entirely (force new behavior) or should the loop silently batch concurrent `edit` calls into atomic changesets (preserve old behavior, upgrade quality)?
- Should ambient lifecycle capture use a cheap LLM extraction pass on every response, or pattern-match on natural language heuristically, or keep the omg: tags and invest in training/prompting to get consistent production?
- How much of pi's system prompt should be ported verbatim vs. rewritten for Omegon's tool set? The pi prompt references 30+ tools that don't exist in the Rust binary — a naive port would confuse more than help.
