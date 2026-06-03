---
id: resource-open-flynt-backend
title: "Implement Flynt backend for resource.open@1"
status: seed
tags: [host-actions, extensions, resource-open, flynt]
open_questions:
  - "[assumption] Flynt exposes or can expose a host-side API to open markdown, diagram, and image resources from validated HostActions."
dependencies:
  - resource-open-real-backends-125
related:
  - docs/resource-open-real-backends-125.md
---

# Implement Flynt backend for resource.open@1

## Overview

Follow-up to the limited #125 implementation. `resource.open@1` now validates policy, parses `file://` URIs with `url::Url`, reports preferred/selected backend diagnostics, and routes ebook/pdf resources through the terminal/Bookokrat backend. Markdown, diagram, and image resources currently route to an explicit Flynt unavailable diagnostic.

This node tracks the remaining work to attach a real Flynt resource-open surface for rendered/document resources without exposing Flynt internals to extensions.

## Target resources

- `markdown`
- `diagram`
- `image`

## Initial constraints

- Preserve the existing HostAction policy order: manifest and workspace-root validation must happen before any Flynt handoff.
- The backend should return a `ResourceOpenResult` with backend `flynt`, actual placement, and an opaque Flynt handle such as a tab/document id.
- If Flynt is unavailable in the current runtime, retain explicit unavailable diagnostics rather than falling back silently.
- Do not let extensions address Flynt internals directly; the host owns translation from validated resource URI to Flynt operation.

## Open implementation questions

1. What is the host-side API for opening a Flynt tab/document from core Omegon?
2. Should placement map to Flynt main tab/side pane, or should Flynt decide placement entirely?
3. How should non-file resources be represented if future schemes are allowed?
