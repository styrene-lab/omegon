+++
id = "6e9e5b57-4548-471b-9e5a-d2d555e7c9e2"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Dashboard Recovery Badge — Actionability

## Overview

The compact footer shows a recovery badge (↺ retry / ↺ offline / ↺ switch / ↺ escalate / ↺ cooldown) after upstream errors. The labels read as imperative directives but the badge is purely informational — no click handler, no slash command, no OSC 8 link. For most actions the system already acted automatically. For 'escalate' the operator genuinely must act but there is no affordance pointing them to what to do.

## Research

### What each action label means vs. what it implies

| Badge | What actually happened | Operator needs to act? |
|---|---|---|
| ↺ retry | System auto-retried same model once | No — already done |
| ↺ switch | System switched to alternate candidate | No — already done |
| ↺ offline | System handed off to local model | No — already done |
| ↺ cooldown | System is waiting out a rate-limit window | Maybe — can `/set-model-tier` to bypass |
| ↺ escalate | All automatic recovery options exhausted | YES — operator must act |
| ↺ recovery | Fallback/catch-all label | Unknown |

All labels use present/imperative tense implying the operator should do something. Only 'escalate' is genuinely operator-blocking. OSC 8 links are used elsewhere in the footer (design tree nodes, openspec changes) but not here.

## Decisions

### Decision: Option B — past-tense status labels + escalate hint

**Status:** decided
**Rationale:** OSC 8 links can only open file/http URIs — they cannot invoke slash commands. So a click-to-act badge is not achievable with the current pi TUI model. Decision: (1) rename all auto-handled badges to past tense (retried, switched, offline, cooling) so they read unambiguously as status; (2) escalate gets ⚠ icon + error color + a dim '→ /set-model-tier' hint appended, making the required operator action explicit; (3) the System tab in /dashboard continues to show full recovery details. This establishes the pattern for organic growth — future work can add a /recover command or a command-palette modal triggered from the overlay.

## Open Questions

*No open questions.*
