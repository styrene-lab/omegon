---
id: local-inference-onboarding
title: "Local inference onboarding — smooth Ollama experience via /tutorial"
status: exploring
parent: tutorial-system
dependencies: [startup-systems-check]
tags: [local-inference, ollama, onboarding, tutorial, ux, 0.15.1]
open_questions:
  - What is the minimum model that can meaningfully drive a tutorial — can a 4B model do single-tool-call steps, or do we need 14B+ for reliable tool use?
jj_change_id: kvywttuknzuoxmkzorsyqmsrwqvwpnku
priority: 2
---

# Local inference onboarding — smooth Ollama experience via /tutorial

## Overview

Make local inference a first-class guided experience. If Ollama is available, the tutorial should demonstrate it — model pulling, delegation, cost-free operation. If it's not installed, the tutorial should offer to set it up or gracefully skip. The goal: a user with a beefy machine and no API key should still have a complete, impressive first experience.

## Research

### Current local inference state in Omegon

Omegon already has:
- `tools/local_inference.rs`: `manage_ollama` tool (start/stop/status/pull) and `ask_local_model` tool (delegation)
- `list_local_models` tool: lists what's loaded in Ollama
- Bootstrap panel: shows Ollama availability and model count
- Agent can delegate work to local models via `ask_local_model`
- Compaction fallback chain: tries local model first for context summarization

What's not smooth:
- No guided Ollama installation if it's missing
- No model recommendation based on hardware ("you have 32GB, pull qwen3:14b")
- `manage_ollama` with action `pull` requires the user to know model names
- No tutorial step demonstrates local inference
- No way to set a local model as the *driver* (primary agent model) from the tutorial
- The bootstrap panel shows "Ollama: 7 models" but doesn't show which ones or their sizes

## Decisions

### Decision: Show the install command and wait — don't auto-run curl scripts

**Status:** decided
**Rationale:** Running `curl | sh` without explicit operator consent is a security violation. The tutorial step shows the command, opens the Ollama website if possible, and waits (Command trigger on 'ollama' or manual confirmation). The operator pastes and runs it themselves. Trust boundary is the operator's terminal, not ours.

### Decision: 14B minimum for driver, 4B usable for delegation/compaction — test empirically before shipping

**Status:** exploring
**Rationale:** Qwen3 14B and Devstral 24B can do multi-step tool use on Apple Silicon. 4B-8B models (Qwen3 4B, Llama 3.1 8B) can handle single-tool delegation tasks and compaction. Need empirical testing with the actual tutorial prompts to find the floor — run each auto-prompt step against each model size and record success rates. This is a QA task, not a design decision.

### Decision: Mode flag on existing steps, not a separate array — tutorial adapts expectations per step

**Status:** decided
**Rationale:** A separate STEPS_LOCAL array means maintaining three parallel step sequences. Instead, the Tutorial struct gets an `inference_tier` field (from the systems check CapabilityTier). AutoPrompt steps check the tier and adjust: lower tiers get simpler prompts, shorter expectations, and recovery text if the model fails. The step body text adapts too: "Running locally — this may take longer." Same overlay engine, same rendering, different prompt complexity per tier.

## Open Questions

- What is the minimum model that can meaningfully drive a tutorial — can a 4B model do single-tool-call steps, or do we need 14B+ for reliable tool use?
