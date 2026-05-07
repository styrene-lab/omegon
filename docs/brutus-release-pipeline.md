+++
id = "6c0dc46a-2c06-4e73-9cae-7773b6f62afd"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Brutus release pipeline

## Overview

> Parent: [Rust versioning system — semver, changelog, --version, release workflow](rust-versioning.md)
> Spawned from: "What ArgoCD workflow/event structure already exists on Brutus — Argo Workflows, Argo Events with webhook sensor, or something else?"

*To be explored.*

## Research

### Brutus infrastructure — existing patterns

**Argo Events**: EventSource at `vanderlyn/apps/argo-events/eventsource-github.yaml` receives webhooks from styrene-lab repos. Currently watches: styrened, specularium, styrene-edge, styrene, public-hub. `omegon-core` needs to be added.

**Sensor pattern**: Each repo has a sensor that maps events to workflow triggers. e.g. `sensor-styrene-docs.yaml` maps push-to-main → docs build workflow with commit-sha and repo parameters.

**WorkflowTemplate pattern**: Templates live in `.argo/workflows/` inside each repo. styrened has: release-build, pr-validation, nightly-tests, cron-nightly. These are registered in the argo namespace.

**Release workflow (styrened)**: tag push → parse-version → checkout → build → publish → GitHub release → dispatch meta-package → ntfy notification. Uses DAG for parallelism. Exit hooks for notifications. ghcr-secret for GitHub auth, ntfy-secret for notifications.

**Deploy pattern (styrene-docs)**: build → kaniko push to GHCR → update image tag in `styrene-lab/deploy` repo → ArgoCD refresh. Deployment runs an nginx container serving static files. Staging and production are separate deployments.

**Cross-compilation challenge**: styrened uses Nix for OCI builds. For omegon, we need 4 target binaries (darwin-arm64, darwin-x64, linux-x64, linux-arm64). `cross` (Docker-based cross-compilation) is already proven from the existing GitHub Actions matrix and runs natively in k8s pods.

**mdserve as docs tooling**: mdserve is a Rust binary serving markdown with live reload, Mermaid diagrams, syntax highlighting. The docs/ directory can be served directly. For the release pipeline, the static HTML export or a container running mdserve serves the docs site.

## Decisions

### Decision: Use cross for macOS cross-compilation in k8s pods

**Status:** decided
**Rationale:** cross (Docker-based cross-compilation) is already proven from the existing GitHub Actions matrix. It runs natively in containers on k8s. No osxcross setup or macOS runners needed. The release-build workflow template uses cross for all 4 targets in parallel DAG tasks.

## Open Questions

*No open questions.*
