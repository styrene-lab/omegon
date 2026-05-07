+++
id = "8bd557c9-4f06-49ce-b86e-94e51c6b3e9c"
kind = "document"
title = "OpenRouter as first-class provider — client, credential storage, task-aware model routing"
status = "implemented"
tags = ["providers", "openrouter", "routing", "free-tier", "0.15.1"]
aliases = ["openrouter-provider"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "free-tier-tutorial"
priority = "1"
+++

# OpenRouter as first-class provider — client, credential storage, task-aware model routing

## Overview

Add OpenRouter alongside Anthropic and OpenAI as a first-class provider. Thin client (OpenAI wire protocol with different base URL and model catalog). Separate credential storage (OPENROUTER_API_KEY). Task-aware routing: driver → Qwen3 Coder 480B, cleave children → openrouter/free meta-model, compaction → Nemotron Nano 9B, memory extraction → smallest viable. The 27 free models with tool calling support make this a zero-cost full-stack inference option.

## Open Questions

*No open questions.*
