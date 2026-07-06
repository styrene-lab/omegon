---
id: semantic-route-resolution
title: "Semantic Route Resolution — resolve conceptual model intent into concrete provider routes"
status: exploring
parent: provider-route-conceptual-model-matrix
tags: [routing, policy, models]
open_questions: []
dependencies:
  - provider-route-schema
related: []
---

# Semantic Route Resolution — resolve conceptual model intent into concrete provider routes

## Overview

Resolve model intent through conceptual model candidates and then concrete provider route candidates. Unprefixed model IDs express conceptual intent; provider-prefixed specs express concrete route pins. Failover prefers same conceptual model via allowed alternate route before cross-model degradation.

## Decisions

### Decision: provider-prefixed specs remain concrete route pins; unprefixed conceptual IDs express semantic model intent

**Status:** decided

**Rationale:** This preserves backward compatibility for existing model settings while giving operators a stable conceptual selector that can expand to policy-allowed provider routes.
