---
id: ui-surface-envelope-runtime
title: "Versioned surface envelope and runtime transport"
status: implemented
parent: ui-surface-action-protocol
tags: [surfaces, protocol, serialization]
open_questions: []
dependencies: []
related: []
---

# Versioned surface envelope and runtime transport

## Overview

Define versioned surface snapshot/update envelopes and action outcome envelopes for external or out-of-process clients while keeping internal semantic structs separate from wire DTOs.

## Decisions

### Introduce internal replay envelopes before external transport

**Status:** accepted

**Rationale:** Commit c4a86c27 added `ui_runtime::envelope` with versioned internal DTOs for surface snapshots, action requests, and action outcomes. These envelopes support replay/runtime boundaries while keeping ACP/Flynt/TS wire contracts as later adapters.
