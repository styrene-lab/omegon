+++
title = "Modality capability and preflight policy"
tags = ["inference","routing","multimodal","capabilities","preflight"]
+++

+++
id = "64dc492b-bd79-4136-8279-c48f95af4632"
kind = "design_node"

[data]
title = "Modality capability and preflight policy"
status = "exploring"
issue_type = "architecture"
priority = 1
parent = "9e253602-b48f-4c34-a787-b12fe57804c5"
dependencies = []
open_questions = []
+++

## Overview

# Modality capability and preflight policy

# Modality capability and preflight policy

## Overview

Extend offering-level inference inventory so attachment preflight can distinguish direct modality acceptance from available transformation paths. Capability claims are endpoint/route properties with evidence, limits, and transport semantics—not broad assumptions about conceptual model families.

## Decisions

### Audio is a canonical modality

Add `audio` alongside text, image, video, and embedding. Documents and arbitrary files may be represented as explicit modalities or constrained media/file capabilities; the inventory must not collapse them into image or text merely for routing convenience.

### Capability constraints are structured

```rust
struct InputModalityCapability {
    modality: Modality,
    media_types: BTreeSet<String>,
    max_items: Option<u32>,
    max_bytes_per_item: Option<u64>,
    max_total_bytes: Option<u64>,
    max_duration_ms: Option<u64>,
    max_dimensions: Option<Dimensions>,
    delivery: DeliveryMode,
}

enum DeliveryMode {
    InlinePayload,
    ProviderUpload,
    RemoteUri,
    AdapterManaged,
}
```

Every value retains inventory evidence and source provenance consistent with dynamic inference inventory.

### Compatibility precedes quality

Preflight first tests whether the current offering accepts the attachment set within limits. Only then may routing compare quality or grade. A conceptual model's capability does not automatically transfer to every endpoint or adapter exposing it.

### Preflight returns resolutions

The result is not merely compatible/incompatible. It reports direct delivery, constrained conversion, candidate route switch, candidate transform, or unsupported state, with reasons suitable for semantic surfaces.

```rust
enum AttachmentResolution {
    Direct(DeliveryPlan),
    Convert { transform: TransformId, reason: String },
    SwitchRoute { candidates: Vec<RouteCandidate> },
    ChooseStrategy { options: Vec<EvidenceStrategy> },
    Unsupported { reason: String },
}
```

### Common-route UX is measurable

Inventory projections should support telemetry-free local analysis of the operator's configured routes: what fraction directly accept image/audio/video, and which transforms cover the gaps. Product defaults should be based on actual configured inventory rather than unsupported global percentages.

## Implementation Details

- Add `Modality::AUDIO` and parse/merge tests in `inference_inventory.rs` and manifests.
- Replace or augment bare `input_modalities` with evidenced constrained capabilities while preserving migration compatibility.
- Add capability declarations to provider/bootstrap inventory records only where evidence exists.
- Extend route compatibility to accept attachment requirements and delivery constraints.
- Add preflight projection consumed by editor/composer semantic surfaces.
- Record stale/unknown capability separately from explicit unsupported capability.
- Add tests proving model-family capability does not leak across incompatible endpoints.
- Add inventory summary queries for configured-route modality coverage.

## File Scope

- `core/crates/omegon/src/inference_inventory.rs`
- `core/crates/omegon/src/inference_manifest.rs`
- `core/crates/omegon/src/routing.rs`
- `core/crates/omegon/src/route.rs`
- `core/crates/omegon/src/inference_runtime.rs`
- provider/bootstrap inventory declarations and related tests
- `pkl/` inference configuration schemas

## Constraints

- Unknown is not treated as supported.
- Marketing/model-family claims are weaker evidence than endpoint documentation or runtime discovery.
- Existing text-only routes remain valid without fabricating empty media limits.
- Compatibility checks remain deterministic and explainable.
- Capability vocabulary is shared by TUI, ACP, daemon, and future surfaces.
- Route switching may not occur silently because an attachment was added.

## Open Questions

- [assumption] Offering-level inventory is the correct authority for modality support even when adapters add preprocessing.
- Should document/file input be a canonical modality or a transport capability layered over semantic media kinds?
- How are provider upload lifetimes and asynchronous file readiness represented?
- What evidence freshness policy applies to dynamically discovered modality limits?

## Open Questions
