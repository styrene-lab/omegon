---
title: ACP Terminal Delegation (#87)
status: exploring
tags: [0.25, acp, terminal, host-actions, flynt]
---

# ACP Terminal Delegation (#87)

## Problem

Flynt has an integrated terminal surface, but Omegon still often creates its own internal/background PTY for terminal use cases.

This splits behavior:

- Extension HostActions can request `terminal.create@1`.
- Built-in `terminal` tool creates Omegon-owned background sessions.
- ACP permission flow can review actions, but host terminal execution is not the canonical path.

## Goal

When an ACP host advertises terminal capability, Omegon can delegate terminal creation to the host and receive structured outcomes.

## Use cases

1. Extension/MCP emits `terminal.create@1` and ACP host renders approval card, then executes locally.
2. Built-in `terminal start` can delegate to ACP host terminal backend when available.
3. Tool-result metadata preserves raw candidates and outcomes for host UI cards.

## Decisions

### Decision: #104 is still required

ACP delegation is not a replacement for standalone visibility. If no ACP terminal backend exists, fallback background sessions must still be inspectable.

### Decision: terminal delegation is host-owned, not extension-owned

Extensions declare intent; Flynt/Omegon decide placement and execution.

## Open questions

- [assumption] ACP permission request metadata can carry terminal.create candidates with sufficient fidelity for Flynt.
- How does ACP host advertise terminal backend capability?
- Does host execute and return `TerminalCreateResult`, or does Omegon execute after approval?
- How should built-in `terminal` tool expose delegation vs internal background backend?

## Acceptance

- ACP contract documents terminal delegation.
- Flynt can distinguish terminal HostAction candidates from ordinary transcript text.
- Host can approve and execute a terminal create request locally.
- Built-in terminal tool has a host-delegation path when capability exists.
- Fallback to Omegon background PTY remains intact.

## Links

- [[0.25-roadmap-extension-surfaces]]
- [[terminal-background-session-visibility-104]]
- [[resource-open-host-action-83]]
