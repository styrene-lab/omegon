---
id: omega-coordinator
title: Omega coordinator tier — cross-project cleave and multi-instance orchestration
status: seed
parent: omega
open_questions:
  - Is cross-project cleave (dispatching child tasks across multiple project repos simultaneously) a target use case? If yes, a coordinator tier is necessary. If no, the flat catalog + federation model is sufficient and the coordinator can be deferred.
---

# Omega coordinator tier — cross-project cleave and multi-instance orchestration

## Overview

> Parent: [Omega — Rust execution engine and project intelligence daemon](omega.md)
> Spawned from: "Is cross-project cleave (dispatching child tasks across multiple project repos simultaneously) a target use case? If yes, a coordinator tier is necessary. If no, the flat catalog + federation model is sufficient and the coordinator can be deferred."

*To be explored.*

## Research

### K8s as execution scheduler vs. semantic coordinator

K8s handles: pod scheduling, resource limits, restarts, health probes, Job lifecycle (active/succeeded/failed), parallelism (completions + parallelism fields). This subsumes most of what a custom Omega coordinator would do for execution management.

What k8s does NOT handle:
- Git merge conflict resolution across child branches
- OpenSpec lifecycle state aggregation across children
- Cross-project dependency ordering (k8s Jobs within a namespace can depend on each other via init containers, but cross-namespace or cross-cluster ordering requires external coordination)
- The global Dioxus portfolio view (requires reading lifecycle state from multiple Omega instances)
- VRAM budget negotiation for local Ollama inference

Conclusion: k8s is the right execution coordinator for remote/cloud cleave runs. An Omega coordinator tier is still needed for semantic coordination, but its scope is narrower than initially framed — it is NOT in the business of process scheduling. It is in the business of: lifecycle state aggregation, cross-project merge orchestration, and global observability.

## Open Questions

- Is cross-project cleave (dispatching child tasks across multiple project repos simultaneously) a target use case? If yes, a coordinator tier is necessary. If no, the flat catalog + federation model is sufficient and the coordinator can be deferred.
