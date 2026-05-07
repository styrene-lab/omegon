+++
id = "e4ce1f26-8e77-4c5d-ae97-b30ced3a0b37"
kind = "design_node"
title = "Agent loop test harness — mock LlmBridge and deterministic state machine testing"
status = "resolved"
tags = []
aliases = ["agent-loop-test-harness"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "rust-agent-loop"
+++

# Agent loop test harness — mock LlmBridge and deterministic state machine testing

## Overview

> Parent: [Rust-native agent loop — middle-out replacement of pi's orchestration core](rust-agent-loop.md)
> Spawned from: "What is the testing strategy for the agent loop itself — mock LlmBridge that returns scripted responses, allowing deterministic tests of the state machine (tool dispatch ordering, error handling, multi-turn conversation flow) without real LLM calls?"

*To be explored.*

## Decisions

### Decision: Mock LlmBridge with scripted responses for deterministic loop testing

**Status:** decided
**Rationale:** The agent loop's LlmBridge trait enables a MockBridge that returns scripted LlmEvent sequences. Tests define: given this conversation state, when the loop calls the bridge, it receives these events (text deltas, tool calls, done). This tests the state machine deterministically: tool dispatch ordering, error handling, multi-turn flow, steering message injection, context decay, ambient capture parsing, and lifecycle phase transitions — all without real LLM calls. The mock also verifies what the loop sends TO the bridge (correct system prompt assembly, decayed message history, proper tool definitions). Standard Rust `#[tokio::test]` with the mock injected via the trait.

## Open Questions

*No open questions.*
