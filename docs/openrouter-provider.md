+++
id = "8bd557c9-4f06-49ce-b86e-94e51c6b3e9c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# OpenRouter as first-class provider — client, credential storage, task-aware model routing

## Overview

Add OpenRouter alongside Anthropic and OpenAI as a first-class provider. Thin client (OpenAI wire protocol with different base URL and model catalog). Separate credential storage (OPENROUTER_API_KEY). Task-aware routing: driver → Qwen3 Coder 480B, cleave children → openrouter/free meta-model, compaction → Nemotron Nano 9B, memory extraction → smallest viable. The 27 free models with tool calling support make this a zero-cost full-stack inference option.

## Open Questions

*No open questions.*
