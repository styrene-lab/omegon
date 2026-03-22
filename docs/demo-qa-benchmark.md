---
id: demo-qa-benchmark
title: Demo project as QA benchmark — automated validation and capability matrix testing
status: exploring
tags: [qa, benchmark, testing, demo]
open_questions:
  - "Should matrix runs be orchestrated by omegon itself (a /benchmark command that iterates permutations) or by an external script/CI that launches omegon N times with different flags?"
  - "How do we define pass/fail for each phase? Some phases (thinking analysis, design decisions) have subjective quality — do we need an LLM-as-judge evaluator, or are structural checks sufficient (file created, fact stored, node exists)?"
  - "What's the baseline for emergent property detection? We need a control run (no persona, default tools, medium thinking) that subsequent runs are compared against. How many runs constitute a statistically meaningful sample?"
  - "Can instrument telemetry be captured in headless mode? The CIC instruments produce intensity values each frame — logging peak/mean intensity per phase per instrument would give us a quantitative signal for how each subsystem was exercised."
jj_change_id: vkpoqrqrqqvroqyzvtoynxvyukwqotxs
issue_type: epic
priority: 2
---

# Demo project as QA benchmark — automated validation and capability matrix testing

## Overview

Evolve the omegon-demo project from a visual smoke test into a proper QA and benchmarking framework. The demo's 9-phase walk-through already exercises every harness subsystem — extend it to produce structured, comparable results across configuration matrices.

## Research

### Research thrusts — what to measure and why



### Results artifact schema

Each benchmark run produces a JSON file. Runs are comparable when they share the same demo version (git sha of omegon-demo).

```json
{
  "version": "1",
  "demo_sha": "abc123",
  "omegon_sha": "def456",
  "omegon_version": "0.14.1-rc.16",
  "timestamp": "2026-03-22T10:00:00Z",
  
  "config": {
    "model": "claude-opus-4-6",
    "context_class": "Legion",
    "thinking_level": "high",
    "persona": "alpharius",
    "tool_count": 31,
    "tool_profile": "default",
    "memory_state": "warm",
    "initial_facts": 2565
  },
  
  "phases": [
    {
      "id": 1,
      "name": "baseline",
      "status": "pass",
      "wall_time_ms": 3000,
      "turns": 0,
      "tool_calls": 0,
      "tokens_in": 0,
      "tokens_out": 0,
      "context_pct_start": 0.0,
      "context_pct_end": 0.0,
      "memory_ops": { "store": 0, "recall": 0, "archive": 0 },
      "instruments": {
        "context_peak": 0.0,
        "tools_peak": 0.0,
        "thinking_peak": 0.0,
        "memory_peak": 0.0
      },
      "errors": []
    }
  ],
  
  "summary": {
    "total_phases": 9,
    "passed": 9,
    "failed": 0,
    "total_wall_time_ms": 120000,
    "total_turns": 12,
    "total_tool_calls": 23,
    "total_tokens_in": 45000,
    "total_tokens_out": 12000,
    "compactions": 0,
    "peak_context_pct": 34.5
  }
}
```

Results accumulate in `.omegon/benchmark/` and can be compared with a future `/benchmark compare` command or external tooling.

## Open Questions

- Should matrix runs be orchestrated by omegon itself (a /benchmark command that iterates permutations) or by an external script/CI that launches omegon N times with different flags?
- How do we define pass/fail for each phase? Some phases (thinking analysis, design decisions) have subjective quality — do we need an LLM-as-judge evaluator, or are structural checks sufficient (file created, fact stored, node exists)?
- What's the baseline for emergent property detection? We need a control run (no persona, default tools, medium thinking) that subsequent runs are compared against. How many runs constitute a statistically meaningful sample?
- Can instrument telemetry be captured in headless mode? The CIC instruments produce intensity values each frame — logging peak/mean intensity per phase per instrument would give us a quantitative signal for how each subsystem was exercised.

## Thrust 1: Harness Feature Isolation — "Does this knob actually do anything?"

The Omegon harness has ~8 independently configurable dimensions. Most operators set them once and never evaluate whether their choices matter. The benchmark can answer:

**Thinking level impact:**
- Run Phase 4 (architectural analysis) at off/low/medium/high
- Measure: response depth (word count, heading count), token usage, wall time
- Hypothesis: high thinking produces measurably deeper analysis but at 3-5x cost. Is the quality difference worth the cost? At what complexity threshold does thinking level stop mattering?

**Context class impact:**
- Run all 9 phases at Squad (200k) vs Legion (1M)
- Measure: compaction count, turns before compaction, context % at each phase boundary
- Hypothesis: Legion delays compaction but costs more per turn. Is there a session length where Squad actually outperforms because compaction resets context more efficiently?

**Persona impact:**
- Run Phase 3 (code generation) and Phase 4 (analysis) with/without a code-focused persona
- Measure: code correctness (cargo test pass rate), response structure, tool selection patterns
- Hypothesis: personas change tool selection and response style but not correctness. If true, personas are cosmetic. If false, they're a genuine capability amplifier.

**Tool profile impact:**
- Run with full (49 tools) vs default (31) vs minimal (10: bash, read, write, edit)
- Measure: turns to completion per phase, tool call count, error rate
- Hypothesis: more tools = more options = slower decision making. The minimal profile might complete faster because the agent doesn't waste turns considering irrelevant tools.

## Thrust 2: Memory System Efficacy — "Does the knowledge graph actually help?"

**Cold vs warm start:**
- Run the demo on a fresh project (zero facts) vs after 10 sessions (2000+ facts)
- Measure: Phase 5 recall accuracy, injection token cost, agent behavioral differences
- Question: does the agent make different decisions when it has project memory? Does it skip exploration? Does it reference stored facts in its reasoning?

**Injection budget:**
- Vary the injection budget: 0 facts, 10 facts, 50 facts, max
- Measure: response quality in Phase 4 (does more context = better analysis?), token overhead
- Hypothesis: there's a sweet spot. Too few facts = no benefit. Too many = noise drowns signal.

**Working memory vs project memory:**
- Pin specific facts with memory_focus before Phase 4, vs letting the system inject naturally
- Measure: does pinning improve task relevance? Does the agent reference pinned facts more?
- Question: is operator-curated working memory more valuable than algorithm-selected injection?

## Thrust 3: Cross-Model Behavioral Fingerprinting — "Do different models use the harness differently?"

**Tool call patterns:**
- Same demo, Opus vs Sonnet vs Haiku vs local (qwen3:32b)
- Measure: tool calls per phase, tool selection diversity, error rate, retry rate
- Question: does Opus use more tools because it's more capable, or fewer because it needs less exploration? Does Haiku fail more and retry, or does it take simpler paths?

**Context consumption rate:**
- Track context % at each phase boundary across models
- Measure: tokens per turn, context efficiency
- Question: do larger models consume context faster (longer responses) or slower (more efficient)?

**Self-correction behavior:**
- Count: how many times does the agent re-read a file after writing it? How many test failures before passing?
- Question: is self-correction correlated with model capability, or with thinking level?

## Thrust 4: Emergent Properties — "What are we not seeing?"

This is the speculative thrust. Run the matrix, collect the data, look for unexpected correlations:

- Does the combination of {high thinking + persona + warm memory} produce qualitatively different outputs than the sum of each individually?
- Are there failure modes that only appear at specific configuration intersections?
- Does the agent develop session-specific strategies (e.g., always reading README first) that vary by model?
- Does the sequence of phases matter? Would running Phase 5 (memory) before Phase 3 (tools) change Phase 3's behavior?
- Are there "resonance" configurations where the harness produces notably better results than adjacent configurations?

This thrust requires the most runs and the least predetermined structure. It's exploratory data analysis on harness telemetry.

## Use Cases

### 1. Automated QA Regression
Run the demo against every release candidate. Each phase produces a pass/fail + timing + token usage. Diff results between RC builds to catch regressions in tool dispatch, memory injection, context management, etc.

### 2. Capability Matrix Testing
Run the same demo across permutations of:
- **Model**: opus-4-6 / sonnet-4-6 / haiku / local (qwen3:32b)
- **Context class**: Squad / Maniple / Clan / Legion
- **Thinking level**: off / low / medium / high
- **Persona**: none / active persona
- **Tool profile**: full (49) / default (31) / minimal
- **Memory**: cold start / warm (pre-loaded facts)

Each permutation produces a results artifact. Compare: does adding a persona help code generation? Does Legion context class change tool call patterns? Does high thinking actually improve architectural analysis?

### 3. Emergent Property Detection
The demo exercises the harness in a controlled sequence. By running it many times with slight variations, we can detect:
- Does the agent develop different strategies based on context class?
- Do memory injections change the agent's behavior in later phases?
- Are there tool call patterns that correlate with better/worse outcomes?
- Does the thinking level actually affect the depth of analysis, or just the token count?
- Signal vs noise: which harness features produce measurable behavioral changes?

### 4. Sales Demo Customization
Teams fork omegon-demo, replace the Rust project with their own codebase, edit the phase prompts to match their workflows, and run /demo to see Omegon against their real environment. The framework provides the structure — the content is customizable.

## Output Format

Each run should produce a JSON results file:
- Per-phase: pass/fail, wall time, token usage, tool calls, memory ops
- Configuration: model, context class, thinking level, persona, tool count
- Instrument telemetry snapshots: peak intensity per instrument per phase
- Agent behavior: number of turns, retry count, error count

## Dependencies

- omegon-demo repo (styrene-lab/omegon-demo)
- /demo slash command (implemented)
- --initial-prompt-file CLI arg (implemented)
- Headless mode for CI runs (--prompt exists but needs structured output)
