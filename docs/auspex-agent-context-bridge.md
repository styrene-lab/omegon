+++
id = "cbef778d-9c73-431c-b840-d62c6edfaef8"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Auspex agent context bridge — pass active portal view and selection with prompt submissions

## Overview

When the operator sends a prompt from the Auspex/mdserve browser portal, the agent should receive lightweight structured context about the active view (tab/route), visible artifact scope, and current selection so queries like 'what does this graph show?' are grounded without manual restatement. This is bridge/protocol work between the browser portal and Omegon, not static docs generation.

## Decisions

### Decision: Browser-originated agent context belongs to the Auspex bridge layer, not the static docs/site pipeline

**Status:** decided

**Rationale:** The problem is interactive prompt grounding from a live browser portal: the agent needs to know what route/tab/selection the operator is looking at when a prompt is submitted. That is runtime bridge/protocol work between Auspex/mdserve and Omegon. It is unrelated to static docs generation and should not live under public site automation nodes.

## Open Questions

- What is the minimal safe context envelope for browser-originated prompts: route/tab only, or also visible node IDs, selected entity IDs, viewport filters, and graph focus state?
- How is browser view context transported into the agent loop: appended hidden prompt metadata, a structured side-channel field on the WebSocket/HTTP prompt command, or a session-scoped shared-state lookup keyed by dashboard client?
- What redaction/privacy boundary applies to browser-originated context so the portal does not dump excessive UI state into every prompt?
