---
id: windshift-integration
title: Windshift PM Integration — Design Tree + OpenSpec sync
status: seed
tags: [integration, project-management, windshift, lifecycle, self-hosted]
open_questions:
  - Does Windshift add personal access token (PAT) support? Without it, programmatic access requires storing a session cookie — fragile and not suitable for an extension.
  - Do we want push-only (design-tree → Windshift on lifecycle transitions) or bidirectional sync? Bidirectional adds complexity but lets the operator triage in the Windshift UI.
  - Should OpenSpec changes map to Windshift Milestones or parent Items? Milestones have a completion semantic but less flexibility; parent Items allow full hierarchy.
  - Wait for Windshift to stabilize (API versioning, PAT support, webhooks) or build against current surface and accept churn? Project is days old.
---

# Windshift PM Integration — Design Tree + OpenSpec sync

## Overview

Explore bidirectional sync between pi-kit's design-tree/OpenSpec lifecycle and a self-hosted Windshift instance. Windshift is a Go+Svelte, SQLite-default work management platform (AGPL-3.0) with a hierarchical Item model, built-in LLM integration, and SCM hooks. The goal is to surface pi-kit's internal lifecycle state (design nodes, OpenSpec changes, cleave tasks) as first-class items in Windshift — giving a human-readable PM view without abandoning the code-native workflow.

## Research

### Windshift API surface (assessed 2026-03-12)

- **Backend**: Go 1.25+, net/http, flat REST routes. No versioning prefix.
- **Frontend**: Svelte 5 + Vite + Tailwind
- **DB**: SQLite (default) or PostgreSQL
- **Auth**: Session + JWT. No personal access tokens visible in routes — blocker for programmatic access.
- **Item model**: Hierarchical (`parent_id`, `children`), `custom_field_values`, `status_id`, `milestone_id`, `iteration_id`, `is_task` flag. Strong mapping to design-tree nodes.
- **AI routes**: `/ai/items/{id}/decompose`, `/ai/items/{id}/catch-me-up`, `/ai/plan-my-day`. OpenAI-compatible LLM client, admin-configurable.
- **SCM**: GitHub, GitLab, Gitea, Bitbucket integration built in.
- **Auth**: OIDC/SSO, WebAuthn/FIDO2, SCIM 2.0.
- **No outbound webhooks found** — extension would need to poll.
- **No API versioning** — breaking changes will be silent.
- **Repo**: github.com/Windshiftapp/core — AGPL-3.0, 3 stars, very early (updated 2026-03-12).

### Concept mapping

| pi-kit concept | Windshift concept |
|---|---|
| Design node | Item (hierarchical, custom fields) |
| Node status (seed/exploring/decided/implemented) | Configurable workspace Status |
| OpenSpec change | Milestone or parent Item |
| tasks.md group | Item with `is_task: true` children |
| Cleave child | Sub-item under the OpenSpec item |
| `feature/*` branch | SCM link on the item |
| Design tree tag | Label |

## Open Questions

- Does Windshift add personal access token (PAT) support? Without it, programmatic access requires storing a session cookie — fragile and not suitable for an extension.
- Do we want push-only (design-tree → Windshift on lifecycle transitions) or bidirectional sync? Bidirectional adds complexity but lets the operator triage in the Windshift UI.
- Should OpenSpec changes map to Windshift Milestones or parent Items? Milestones have a completion semantic but less flexibility; parent Items allow full hierarchy.
- Wait for Windshift to stabilize (API versioning, PAT support, webhooks) or build against current surface and accept churn? Project is days old.
