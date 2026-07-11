+++
title = "Media transformation registry"
tags = ["multimodal","extensions","transforms","media","architecture"]
+++

+++
id = "cc5c7796-bd29-490a-99a8-defc109a2ce3"
kind = "design_node"

[data]
title = "Media transformation registry"
status = "exploring"
issue_type = "architecture"
priority = 2
parent = "9e253602-b48f-4c34-a787-b12fe57804c5"
dependencies = []
open_questions = []
+++

## Overview

# Media transformation registry

# Media transformation registry

## Overview

Define the boundary between core attachment semantics and replaceable extension-backed media interpretation. Core owns acquisition, identity, state, routing, persistence, presentation, and operator decisions. Extensions advertise and execute transformations such as transcription, OCR, frame extraction, scene detection, document extraction, and specialist binary interpretation.

## Decisions

### Transforms are semantic capabilities

Extensions register descriptors rather than adding modality-specific UI. Core uses descriptors to build consistent actions and preflight recommendations.

```rust
struct MediaTransformDescriptor {
    id: TransformId,
    label: String,
    accepts: MediaSelector,
    produces: Vec<MediaProduct>,
    execution: TransformExecutionProfile,
    privacy: TransformPrivacy,
    cost: TransformCost,
    limits: TransformLimits,
}
```

### Outputs are canonical derived attachments

Transform outputs enter managed storage and receive stable attachment IDs. Provenance records source IDs, transform ID/version, parameters, locality, completion time, and hashes. Extensions do not mutate or replace source records.

### Core owns selection and consent

Extensions report capabilities and progress; semantic surfaces choose how actions are displayed. Remote execution, potentially lossy conversion, large uploads, and automatic cardinality expansion require policy checks and, where appropriate, operator confirmation.

### Extensions receive bounded media authority

Inputs are immutable staged assets, bounded IPC bytes, or explicit scoped grants. Extensions do not receive arbitrary filesystem authority merely because an attachment originated from a local path.

### Distribution has three classes

- Core-native: attachment records, basic image handling, preflight, cards, compositor.
- Bundled first-party extensions: supported OCR, PDF extraction, transcription, and FFmpeg-backed derivation where installed.
- Optional specialist extensions: domain-specific imagery, CAD, GIS, scientific data, and vendor transforms.

## Initial transform vocabulary

- `media.image.ocr`
- `media.image.resize`
- `media.document.extract-text`
- `media.document.select-pages`
- `media.audio.transcribe`
- `media.audio.diarize`
- `media.video.extract-keyframes`
- `media.video.extract-clip`
- `media.video.extract-audio`
- `media.video.transcribe`
- `media.archive.list`

IDs describe semantics, not a specific implementation. Multiple providers may satisfy a transform with different locality, quality, cost, and policy attributes.

## Implementation Details

- Add transform descriptors and result/progress contracts to `omegon-traits`.
- Extend extension registration with media-transform inventory and provenance metadata.
- Add a core resolver that matches attachment state, current-route capability, installed transforms, and policy.
- Project ranked transform choices through shared command/editor surfaces.
- Stage immutable inputs into an extension exchange boundary.
- Ingest outputs atomically into managed media storage before marking transforms complete.
- Record cancellation, partial output, and failure without losing source attachments.
- Keep transformation execution off the TUI event loop and expose bounded progress.

## File Scope

- `core/crates/omegon-traits/` — descriptors, selectors, progress, structured results.
- `core/crates/omegon-extension/` — transform advertisement and invocation transport.
- `core/crates/omegon/src/attachments.rs` — resolver integration and provenance.
- `core/crates/omegon/src/extensions.rs` or registry modules — live transform inventory.
- `core/crates/omegon/src/surfaces/editor.rs` — transform action projections.
- `core/crates/omegon/src/conversation.rs` — delivered-evidence and derivation persistence.

## Constraints

- A missing extension degrades to a visible unavailable action, not data loss.
- Transform metadata cannot be trusted as filesystem authority.
- Remote transforms disclose destination and upload size.
- Core does not embed FFmpeg, Whisper, or specialist stacks solely to satisfy the semantic contract.
- Extension-specific metadata remains namespaced and cannot redefine canonical attachment state.

## Open Questions

- [assumption] Transform registration belongs in the existing extension capability exchange rather than a separate plugin subsystem.
- How should competing transform implementations be ranked: locality, quality, latency, cost, operator preference, or a policy-weighted score?
- Which first-party transform extensions ship by default versus on-demand installation?
- What is the cancellation contract for transforms that have already uploaded remote media?

## Open Questions
