---
id: cost-reduction
title: Token Cost Reduction — Local Inference Offload Strategy
status: decided
tags: [cost, local-inference, architecture]
open_questions: []
---

# Token Cost Reduction — Local Inference Offload Strategy

## Overview

Assess and implement strategies to reduce cloud API token consumption by ≥50% by leveraging the M1 Max 64GB for local inference. Current state: 19% weekly budget burned by Saturday of week 1, $6.83 extra usage accrued, with significant token spend on background processes and routine tasks that don't need frontier models.

## Research

### Current Token Sinks — Where the Money Goes

**Hardware**: M1 Max, 64GB unified memory, 10 cores. Can run 24-30B parameter models at reasonable speed (~20-40 tok/s).

**Available local models**: nemotron-3-nano:30b (24GB), qwen3:30b (18GB), devstral-small-2:24b (15GB code-specialist), plus embedding models.

**Usage snapshot** (Saturday, day 6 of week):
- 19% weekly budget (all models)
- 11% weekly budget (Sonnet only) — meaning ~8% is Opus
- $6.83 / $100 extra usage spent

**Identified token sinks, ranked by estimated impact**:

### 1. Background Fact Extraction (HIGH — ~15-20% of spend)

`project-memory` spawns a full `pi` subprocess with `--model claude-sonnet-4-6` for every extraction cycle. Each extraction sends the entire recent conversation + all current facts through Sonnet. This runs periodically throughout every session.

**Fix**: Change `extractionModel` default from `claude-sonnet-4-6` to a local model. devstral-small-2 or qwen3:30b can extract structured JSONL facts from conversation transcripts — this is a well-defined, constrained output task.

### 2. Cleave Child Processes (HIGH — ~20-30% during /cleave)

Each cleave child spawns a full pi agent session. With 3-8 children per cleave, this multiplies token consumption. Currently defaults to Sonnet even for simple tasks (single-file edits, test writing, boilerplate).

**Fix**: `prefer_local: true` flag already exists but defaults to `true` only in cleave_run params. The `resolveExecuteModel` function already supports local routing. Need to:
- Make local the default for leaf tasks that touch ≤2 files
- Reserve Sonnet for complex multi-file tasks
- Reserve Opus only for review phase

### 3. Episodic Memory Generation (MEDIUM — ~3-5% per session)

`generateEpisodeDirect` already uses local Ollama (qwen3:30b) — **this is already optimized**. Falls back to pi subprocess only if direct call fails.

### 4. Tool Description Bloat (MEDIUM — ~5-10% of context per turn)

~60 tools × ~200 tokens each = ~12K tokens injected into every single API call. Tool profiles exist but could be more aggressive.

**Fix**: Tighten profile switching. Scribe tools (30 tools, ~6K tokens) should only load when explicitly needed, not just when mcp.json exists.

### 5. Extended Thinking on Routine Tasks (MEDIUM — ~10-15%)

Opus + high thinking for file reads, status checks, and simple edits wastes budget. The `set_model_tier` and `set_thinking_level` tools exist but agent discipline is inconsistent.

**Fix**: This is a prompt/behavioral issue — the agent (me) should be more aggressive about downgrading. Could also add heuristics in an extension.

### 6. Compaction (LOW-MEDIUM — ~2-5%)

Already has local fallback but tries cloud first. Should be local-first since summarization is a good local model task.

### 7. Review Loop (VARIABLE — during /assess)

The adversarial review in cleave uses Opus for review passes. Each review round is a full conversation turn with Opus.

**Fix**: First-pass review with local model, escalate to Opus only for findings that need deep reasoning.

### Local Model Capability Assessment — M1 Max 64GB

**What fits in 64GB with headroom for OS/apps (~45GB available for inference)**:

| Model | VRAM | Quality | Speed (est.) | Best For |
|-------|------|---------|-------------|----------|
| devstral-small-2:24b | 15GB | Very good for code | ~30 tok/s | Single-file edits, tests, boilerplate |
| qwen3:30b | 18GB | Good general + reasoning | ~25 tok/s | Fact extraction, summaries, planning |
| nemotron-3-nano:30b | 24GB | Good general, 1M context | ~20 tok/s | Long-context tasks, compaction |

**Key insight**: All three models fit simultaneously (~57GB) but you'd want at most two loaded for performance. Ollama auto-manages model loading/unloading.

**What local models CAN handle well** (offload candidates):
- ✅ Structured JSON/JSONL extraction from conversation (fact extraction)
- ✅ Single-file code edits with clear specs
- ✅ Test generation from existing implementations
- ✅ Boilerplate/template generation
- ✅ Conversation summarization (compaction, episodes)
- ✅ First-pass code review (lint-level, pattern matching)
- ✅ Config file generation
- ✅ Documentation drafts

**What local models CANNOT reliably handle** (keep on cloud):
- ❌ Multi-file architectural reasoning (cross-cutting changes)
- ❌ Complex debugging with subtle interactions
- ❌ Security review requiring deep domain knowledge
- ❌ Novel algorithm design
- ❌ Nuanced spec interpretation across multiple domains

### Concrete Reduction Estimates — Path to 50%

Assuming current weekly spend breakdown (estimated from usage patterns):

| Category | Est. % of Spend | Offload Feasibility | Est. Savings |
|----------|----------------|--------------------:|-------------:|
| Background extraction (Sonnet) | 15-20% | 100% → local | **15-20%** |
| Cleave children (Sonnet) | 20-30% | ~70% → local | **14-21%** |
| Interactive session (Opus/Sonnet) | 25-35% | ~20% via tier mgmt | **5-7%** |
| Tool descriptions (context) | 5-10% | ~50% via profiles | **2.5-5%** |
| Extended thinking | 10-15% | ~40% via discipline | **4-6%** |
| Compaction | 2-5% | 100% → local | **2-5%** |
| Review loop | 5-10% | ~50% → local first | **2.5-5%** |
| **TOTAL ESTIMATED SAVINGS** | | | **40-69%** |

**The two highest-ROI changes that alone get us to ~35%**:
1. **Switch extractionModel to local** — one config change, immediate ~18% savings
2. **Default cleave children to local** — architectural but infrastructure exists, ~17% savings

**Adding tier discipline and tool profiles gets us over 50%**.

### Local model ranking for this harness (M1 Max 64GB)

Key harness requirements that thin out local candidates: reliable structured JSON tool calls (24+ schemas), multi-step orchestration, 16-32K effective context with memory injection, strict instruction following, Rust+TS code quality.

Role-specific recommendations (fit in 64GB unified memory):
- **Daily driver / sonnet-tier orchestration**: Qwen3 32B Q8 (~35GB) — community #1 for agentic tool use, 128K ctx, thinking-mode toggle
- **Deep reasoning / opus-tier**: Qwen2.5 72B Q5_K_M (~48GB) — more capacity, but ~10-15 tok/s on M1, noticeable latency on multi-call loops
- **Leaf/child tasks in cleave**: Qwen2.5-Coder 32B Q8 (~35GB) — purpose-built for code + function calling, better Rust/TS output, weaker as root orchestrator

Even Tier 1 local is ~60-70% of Sonnet on complex orchestration. Failure modes: malformed tool JSON (occasional even with Qwen3), missed system-prompt directives, repetitive tool loops, speed.

Models that don't fit: Mistral Large 2 123B Q4 (~65-70GB, too tight with KV cache), Llama 3.3 70B Q8 (~75GB).
Models already wired (offline-driver): nemotron-3-nano:30b, devstral-small-2:24b, qwen3:30b — all functional for leaf tasks, insufficient for complex orchestration.

## Decisions

### Decision: Switch extractionModel to local immediately

**Status:** decided
**Rationale:** One-line change, highest ROI. devstral-small-2 handles structured JSONL extraction from conversation text. No config indirection needed — just change the default.

### Decision: Cleave autoclassification with no-fail-past ground rule

**Status:** decided
**Rationale:** Auto-classify by scope size (≤2 files → local). Critical ground rule: once classified as local, the task STAYS local. If local model fails, retry locally or fail the task — never silently escalate to cloud. This prevents the classification from being a leaky abstraction that degrades to cloud spend under pressure.

### Decision: Flip compaction to local-first

**Status:** decided
**Rationale:** Summarization is a well-suited local model task. nemotron-3-nano with 1M context is ideal. Flip the order: try local first, fall back to cloud only if Ollama is unreachable.

## Open Questions

*No open questions.*
