---
id: subprocess-safety-hardening
title: Subprocess safety hardening
status: implementing
parent: repo-consolidation-hardening
tags: [security, process, subprocess, hardening]
open_questions: []
branches: ["feature/subprocess-safety-hardening"]
openspec_change: subprocess-safety-hardening
---

# Subprocess safety hardening

## Overview

Narrow the repo-consolidation-hardening effort to a first concrete slice that removes risky shell-string execution and broad process-management patterns in browser/server/process helpers, replacing them with safer process spawning and explicit argument handling.

## Research

### Why this is the right first consolidation slice

The parent repo-consolidation-hardening topic is still proposal-only and spans architecture, lifecycle, security, and UX concerns. A subprocess-safety slice is concrete, testable, and cross-cutting enough to deliver immediate hardening without trying to refactor multiple large extensions at once. It aligns with the earlier repo assessment finding to replace broad pkill patterns and shell-string execution with explicit process spawning and argument handling.

## Decisions

### Decision: Start repo consolidation with subprocess/process-management hardening

**Status:** decided
**Rationale:** This slice is small enough to specify and verify cleanly, improves security immediately, and avoids stalling on a repo-wide architecture rewrite before any concrete risk is reduced.

## Open Questions

*No open questions.*
