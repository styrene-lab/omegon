+++
title = "Managed media assets and provider adaptation"
tags = ["media","providers","assets","persistence","multimodal"]
+++

+++
id = "f42acc1f-cb8c-4b73-8761-1fe04389c096"
kind = "design_node"

[data]
title = "Managed media assets and provider adaptation"
status = "exploring"
issue_type = "architecture"
priority = 2
parent = "multimodal-conversation-media"
dependencies = []
open_questions = []
+++

## Overview

# Managed media assets and provider adaptation

# Managed media assets and provider adaptation

## Overview

Provide durable-enough asset ownership, validation, transformation provenance, and provider-specific wire conversion for multimodal turns. This layer prevents temp paths and extension guesses from becoming hidden transport contracts.

## Decisions

### Managed assets separate source lifetime from transcript identity

Clipboard and generated media are copied or atomically written into a session-managed asset store before being considered ready. Workspace paths may remain external references but are fingerprinted and revalidated at send time. Transcript metadata survives payload expiry.

### Detection uses bytes and decoders

MIME/type determination uses trusted sniffing or the relevant decoder. Filename extensions contribute display hints only. Dimensions, page count, duration, and other metadata are probed lazily and bounded.

### Provider conversion is explicit

A route capability matrix declares accepted modalities, MIME families, limits, and transport forms. Adaptation produces either a provider payload or a structured incompatibility; it never skips unreadable inputs.

### Retention is explicit

Each record declares ephemeral, session, project-cache, or operator-pinned retention. Session resume can distinguish retained, missing, and expired assets and render appropriate transcript status.

## File Scope

- `core/crates/omegon/src/attachments.rs` (new) — asset store, metadata, fingerprints, retention, probing.
- `core/crates/omegon/src/bridge.rs` (modified) — modality-neutral provider attachment payloads or adapters.
- `core/crates/omegon/src/providers.rs` and provider clients (modified) — capability declarations and conversion.
- `core/crates/omegon/src/main.rs` (modified) — preflight orchestration; remove silent path-read skipping.
- `core/crates/omegon/src/conversation.rs` (modified) — serialized attachment metadata and reconstruction.
- `core/crates/omegon/src/tools/view.rs` (modified) — produce or reference canonical media artifacts.
- `core/crates/omegon/src/session.rs` or session persistence modules (modified) — asset manifest lifecycle.

## Constraints

- Asset writes use bounded sizes and atomic completion; partial files never become ready.
- Paths are validated and normalized without broadening filesystem authority.
- Hashing/probing must not block the TUI event loop.
- Remote payload creation does not log base64 data or secrets embedded in files.
- Provider-specific limits and transformations remain outside semantic surfaces.
- Resume behavior must never pretend an expired payload is still available.

## Open Questions

- [assumption] Session-local managed storage is the safest first default for clipboard assets.
- Should content-addressed deduplication be part of v1 or deferred until asset volume warrants it?
- Which transformations may run automatically under a size threshold, and which always require operator approval?
- Are raw audio/video payloads in initial scope, or should v1 expose only explicit transcription/frame-extraction transforms?

## Open Questions
