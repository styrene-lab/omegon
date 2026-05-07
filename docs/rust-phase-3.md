+++
id = "9527c348-3d07-4a02-93d6-2e5e1ccf8e7f"
kind = "document"
title = "Phase 3 — Native LLM providers: reqwest-based Anthropic/OpenAI, Node.js fully optional"
status = "deferred"
tags = ["rust", "phase-3", "providers", "reqwest", "standalone"]
aliases = ["rust-phase-3"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "rust-agent-loop"
+++

# Phase 3 — Native LLM providers: reqwest-based Anthropic/OpenAI, Node.js fully optional

## Overview

Implement reqwest-based streaming clients for Anthropic and OpenAI directly in Rust. The Node.js bridge remains for long-tail providers (Bedrock, Vertex, Gemini) but is not spawned in the common case (>95% of sessions). After Phase 3: Omegon is a single Rust binary with zero Node.js dependency for the common case. Installable via `brew install omegon` or `curl | sh` without Node.js.

## Open Questions

*No open questions.*
