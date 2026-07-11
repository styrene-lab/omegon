+++
title = "Multimodal conversation media architecture"
tags = ["architecture","multimodal","attachments","conversation","tui"]
+++

+++
title = "Multimodal conversation media architecture"
tags = ["architecture","multimodal","attachments","conversation","tui"]
+++

# Multimodal conversation media architecture

+++
id = "9e253602-b48f-4c34-a787-b12fe57804c5"
kind = "design_node"

[data]
title = "Multimodal conversation media architecture"
status = "decided"
issue_type = "architecture"
priority = 1
dependencies = []
open_questions = []
+++

## Overview

Establish one modality-neutral path from operator intake through model transport, transcript persistence, and surface-specific presentation. Core owns attachment identity, state, routing, persistence, and human-facing affordances. Extensions own replaceable interpretation and transformation implementations.

Native pixels, transcripts, extracted frames, PDFs, and other representations are projections over durable attachment or artifact identity; none individually serves as universal identity.

## Decisions

### Semantic attachment records are canonical

Every attachment receives a stable `AttachmentId`. Paths and bytes are sources, not identity. Records carry semantic kind, detected MIME type, display metadata, state, provenance, sensitivity, retention, and optional artifact identity.

### Artifact identity, semantic kind, and delivery representation are separate

Workspace objects such as Flynt documents retain artifact identity and revision. Semantic kind describes what information they contain. Delivery representation records what the route actually receives.

Flynt Markdown is the native default for textual reasoning. Typst/PDF are authoritative publication materializations used when layout, publication output, or exact page evidence matters. Ordinary Formal Document attachment is Markdown-first; representation choice is promoted only when intent requires it.

### Intake, transport, persistence, and presentation are separate

- Intake creates and validates records.
- Preflight resolves current-route delivery, transform, route-switch, or unsupported state.
- Transport adapts an approved representation.
- Persistence records source identity, revision, derivation, and delivered evidence.
- Presentation projects media cards and optional previews.

### Preflight is fail-visible

A composer-visible attachment is delivered, explicitly transformed, or rejected before submission. It is never silently omitted. Route switching and lossy derivation require explicit operator action.

### Media cards survive preview failure

The transcript always retains semantic identity, metadata, status, and actions. Native terminal preview is optional enhancement.

### Derived artifacts preserve authority and provenance

Publication materialization is distinct from conversational projection. OCR, transcription, frame extraction, compression, and summaries create derived records and never silently replace authoritative sources.

### Temporal media uses release-valve thresholds

- Audio up to 5 minutes and 10 MiB may prominently offer direct send on an exactly capable route.
- Larger audio recommends transcription or clipping and requires explicit acceptance before costly processing.
- Video up to 30 seconds and 20 MiB may prominently offer direct send on an exactly capable route.
- Exceeding either video threshold requires an explicit evidence strategy.
- Video over 2 minutes or 100 MiB defaults to evidence selection; original delivery requires manual acceptance.
- Prompt intent ranks recommendations but never acts automatically.

### Core/extension boundary

Core provides records, basic image integrity and resizing, managed assets, preflight, cards, compositor, and consent UX. A bundled and enabled `omegon-media` first-party extension advertises common PDF, OCR, audio, and video transforms when host helpers exist. Heavy models and specialist stacks install on demand.

### Transform ranking

Hard-filter by compatibility, policy, sensitivity, requirements, and cost ceilings. Then honor explicit project/operator choice, prefer adequate local execution, then already-entitled remote execution, then metered services by fit, quality, latency, and reliability. Persist defaults only through explicit action.

### Remote cancellation is phase-aware and best-effort

Before upload, cancellation guarantees no remote transfer. After upload begins, job cancellation and asset deletion are separate capabilities and outcomes. Core never claims deletion without provider confirmation. Large, sensitive, metered, or non-deletable uploads require consent. Successful transforms request remote source deletion by default when supported unless retention is explicitly selected.

### Capability evidence has per-claim freshness

Revision/generation changes invalidate claims immediately. Provider modality/MIME evidence defaults to 24 hours; byte/duration/page limits to 6 hours; upload readiness to 5 minutes; extension health to 60 seconds. Large or costly operations require fresh limits or explicit acceptance. Small corroborated image sends may proceed on stale evidence while refreshing.

### Release ownership is split between 0.28 repair and 0.29 architecture

`release/0.28` receives only surgical repairs to the existing inline-image workflow. These preserve the current `image_paths`, editor-token, transcript-segment, and `ratatui-image` contracts. The bottom-anchor alignment correction and any bounded stale/disappearing-state repair ship with regression tests and an `[Unreleased]` changelog entry.

The generalized architecture in this node targets 0.29+ on `feature/0.29-multimodal-media`, based from `main` after 0.28 hardening fixes are forward-merged. Canonical attachments, managed assets, constrained capability preflight, composer tray, frame-level compositor redesign, transformation registry, `omegon-media`, and Flynt representation selection do not enter 0.28. `release/0.29` is cut only after feature integration completes and hardening begins.

## Canonical contracts

```rust
struct AttachmentRecord {
    id: AttachmentId,
    source: AttachmentSource,
    artifact: Option<ArtifactReference>,
    kind: MediaKind,
    media_type: String,
    display_name: String,
    metadata: MediaMetadata,
    state: AttachmentState,
    provenance: AttachmentProvenance,
    retention: AttachmentRetention,
    sensitivity: MediaSensitivity,
}

enum MediaKind {
    Text,
    Image,
    Audio,
    Video,
    Document,
    StructuredData,
    Archive,
    Binary,
}

enum ArtifactRepresentation {
    Markdown,
    PlainText,
    TypstSource,
    Pdf,
    Html,
    PageImages,
    StructuredAst,
    OriginalBytes,
}
```

Native inference modalities remain text, image, audio, and video. Document acceptance and generic file upload are constrained offering capabilities, not flat modalities. Upload purpose distinguishes direct model input, retrieval, code-interpreter use, and provider-managed extraction.

## Implementation groups

### Group 1 — Inline image correctness and media compositor foundation

Stabilize the production image path before broadening attachment semantics.

- Retain and test bottom-anchor overlay correction.
- Suppress native previews during overlapping surfaces and unsafe effects.
- Invalidate protocol state on resize, tab, presentation-mode, and source transitions.
- Introduce frame-owned placements keyed by stable media identity.
- Diff visibility, movement, resize, disappearance, and occlusion.
- Preserve semantic card fallback and dedicated preview.

Files: `tui/image.rs`, `tui/conv_widget.rs`, `tui/mod.rs`, `tui/segment_components/image.rs`, new `tui/media_compositor.rs`, tests.

### Group 2 — Canonical attachments and managed assets

- Add IDs, records, source variants, metadata, sensitivity, retention, state, and provenance.
- Add session-managed atomic asset storage and bounded probing.
- Add artifact references pinned to revision.
- Preserve `image_paths` through compatibility adapters during migration.
- Replace silent path-read omission with structured preflight failure.

Files: new `attachments.rs`, `clipboard.rs`, `main.rs`, `conversation.rs`, session persistence, tests.

### Group 3 — Offering capabilities and preflight

- Add canonical audio modality.
- Add constrained native modality, document, and file-upload capabilities.
- Add delivery modes, purposes, evidence freshness, and generation invalidation.
- Return direct/convert/switch/choose-strategy/unsupported resolutions.
- Add configured-inventory coverage summaries.

Files: `inference_inventory.rs`, `inference_manifest.rs`, `routing.rs`, `route.rs`, `inference_runtime.rs`, provider inventory, Pkl schemas, tests.

### Group 4 — Semantic composer tray and transcript delivery truth

- Replace raw-path editor ownership with stable attachment references.
- Add keyboard-first tray projection and typed paste outcomes.
- Add preflight actions and progressive disclosure.
- Distinguish voice composition from audio evidence.
- Add video evidence-strategy surface.
- Persist and render exact delivered-evidence manifests.
- Default Flynt artifact references to pinned Markdown; expose publication representations in details.

Files: `surfaces/editor.rs`, `surfaces/conversation.rs`, `tui/editor.rs`, `tui/conversation.rs`, `tui/mod.rs`, Flynt/artifact integration, tests.

### Group 5 — Media transformation registry

- Add transform descriptor, selector, progress, result, cancellation, lifecycle, and provenance contracts.
- Extend extension registration and invocation.
- Add policy filtering and ranked resolution.
- Stage immutable inputs and atomically ingest outputs.
- Persist remote operation and cleanup state.

Files: `omegon-traits`, `omegon-extension`, attachment orchestration, extension registry, semantic surfaces, tests.

### Group 6 — Bundled `omegon-media` extension

Ship enabled by default but without heavyweight helper installation or model downloads.

Initial transforms:

- PDF text extraction with page references.
- PDF page rendering and selection.
- Explicit and evenly sampled video frame extraction.
- Video clip and audio-track extraction.
- OCR adapter when Tesseract exists.
- Transcription adapter with configured local backend.

Probe FFmpeg, Poppler, Tesseract, and transcription backends independently. Missing helpers advertise unavailable actions and Nex-reviewed install plans.

### Group 7 — Verification and migration closure

- Cross-surface semantic projection tests.
- Provider capability and no-leak routing scenarios.
- Resume/expiry/cleanup tests.
- Kitty/Sixel/iTerm2 protocol matrix where available.
- Security tests for staging authority, path containment, payload logging, and partial outputs.
- Remove compatibility-only `image_paths` after all producers migrate.
- Document operator workflows and extension installation.

## Dependency order

1. Group 1 can proceed immediately as a bounded stabilization track.
2. Group 2 establishes the shared identity/storage substrate.
3. Group 3 can proceed alongside Group 2 once shared attachment requirements are fixed.
4. Group 4 depends on Groups 2 and 3.
5. Group 5 depends on Group 2 contracts and integrates with Group 3 resolution.
6. Group 6 depends on Group 5.
7. Group 7 closes all prior groups.

## Constraints

- Preserve zero-dialog screenshot paste.
- Markdown remains the portable native representation for Flynt knowledge documents.
- Unknown capability is not support.
- Never broaden filesystem authority through media handling.
- Never log payload bytes or secrets embedded in media.
- Hashing, probing, uploads, and transforms do not block the TUI event loop.
- No silent route switch, lossy transform, remote upload, or attachment omission.
- Semantic projections are shared across TUI, ACP, daemon, web, and future surfaces.

## Open Questions

None blocking implementation. Numeric thresholds and freshness defaults are configuration defaults and may be tuned from runtime evidence without changing the architecture.
