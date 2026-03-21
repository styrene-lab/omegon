---
id: rust-phase-3
title: "Phase 3 — Native LLM providers: reqwest-based Anthropic/OpenAI, Node.js fully optional"
status: deferred
parent: rust-agent-loop
tags: [rust, phase-3, providers, reqwest, standalone]
open_questions: []
---

# Phase 3 — Native LLM providers: reqwest-based Anthropic/OpenAI, Node.js fully optional

## Overview

Implement reqwest-based streaming clients for Anthropic and OpenAI directly in Rust. The Node.js bridge remains for long-tail providers (Bedrock, Vertex, Gemini) but is not spawned in the common case (>95% of sessions). After Phase 3: Omegon is a single Rust binary with zero Node.js dependency for the common case. Installable via `brew install omegon` or `curl | sh` without Node.js.

## Open Questions

*No open questions.*
