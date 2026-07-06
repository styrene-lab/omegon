---
id: github-copilot-first-class-provider
title: "GitHub Copilot as First-Class Meta-Provider — OAuth broker serving top-tier model routes"
status: exploring
parent: provider-route-conceptual-model-matrix
tags: [github-copilot, providers, routing, subscription]
open_questions:
  - "[assumption] The installed authenticated copilot CLI can be used as an initial diagnostics/probe surface even if the final provider bridge uses a direct API or ACP integration."
dependencies:
  - provider-route-schema
related: []
---

# GitHub Copilot as First-Class Meta-Provider — OAuth broker serving top-tier model routes

## Overview

Make github-copilot a first-class subscription/meta-provider whose concrete routes serve conceptual Claude, GPT, Gemini, and other model classes without masquerading as direct Anthropic/OpenAI/Google routes.

## Research

### Local Copilot CLI diagnostics surface

Local probe found /Users/cwilson/.local/bin/copilot. The binary supports non-interactive prompt mode (-p/--prompt), --model selection, --output-format text|json, --context default|long_context, --effort levels, and --available-tools. A silent no-tools prompt returned gpt-5.5, indicating the CLI is authenticated and usable for diagnostics. This proves execution access, not full catalog discovery.

## Decisions

### Decision: github-copilot is a provider ID, not an alias for upstream vendors

**Status:** decided

**Rationale:** Copilot owns a distinct auth, entitlement, transport, quota, and diagnostic surface. Its routes may serve the same conceptual models as Anthropic/OpenAI/Google direct routes, but execution identity must remain github-copilot:<provider-model-id>.

## Open Questions

- [assumption] The installed authenticated copilot CLI can be used as an initial diagnostics/probe surface even if the final provider bridge uses a direct API or ACP integration.
