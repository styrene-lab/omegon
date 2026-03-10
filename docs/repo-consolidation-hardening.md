---
id: repo-consolidation-hardening
title: Repo Consolidation, Security Hardening, and Lifecycle Normalization
status: exploring
tags: [architecture, security, lifecycle, consolidation, pi-kit]
open_questions: []
---

# Repo Consolidation, Security Hardening, and Lifecycle Normalization

## Overview

Reduce internal sprawl across major extensions, tighten process safety around subprocess and shell usage, normalize lifecycle state across design-tree/OpenSpec/dashboard, and improve presentation/data-model coherence for pi-kit as it matures into a platform.

## Research

### Repo-wide assessment findings

Top opportunities: (1) break up oversized extension entrypoints such as project-memory/index.ts, cleave/index.ts, openspec/index.ts, and design-tree/index.ts into thinner registration files over explicit domain/store/ui/bridge layers; (2) consolidate repeated dashboard and lifecycle-emitter plumbing into shared publishers; (3) harden subprocess management by replacing broad pkill patterns and shell-string execution; (4) normalize lifecycle state so design-tree, OpenSpec, dashboard, and memory derive from one canonical resolver; (5) unify model-control responsibilities currently split across effort, model-budget, offline-driver, local-inference, and lib/model-routing.

## Open Questions

*No open questions.*
