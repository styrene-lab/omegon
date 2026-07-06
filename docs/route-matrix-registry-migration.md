---
id: route-matrix-registry-migration
title: "Route Matrix Registry Migration — evolve provider-centric registry into conceptual model + provider route matrix"
status: exploring
parent: provider-route-conceptual-model-matrix
tags: [registry, migration, routing]
open_questions: []
dependencies:
  - provider-route-schema
related: []
---

# Route Matrix Registry Migration — evolve provider-centric registry into conceptual model + provider route matrix

## Overview

Evolve the provider-centric model registry toward a conceptual model plus provider route matrix. Start additively by adding conceptualModelId to existing provider model entries before any larger schema split.

## Decisions

### Decision: additive registry migration begins with conceptualModelId on provider model entries

**Status:** decided

**Rationale:** Adding conceptualModelId preserves existing provider:model execution specs, settings, logs, and UI behavior while enabling grouping and semantic lookup. A later split into conceptualModels[] and providerRoutes[] can happen after consumers are migrated.
