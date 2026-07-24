+++
title = "Tool Provenance in Conversation Surfaces"
tags = ["design","extensions","provenance","tui"]
+++

# Tool Provenance in Conversation Surfaces

## Intent

Expose the authoritative producer of every executed tool without changing the callable tool name. Built-in tools retain their current compact presentation. Extension-owned tools render an actionable suffix such as `bash (recro-coe-agent)` so operators can identify and disable the responsible extension.

## Decisions

1. **Resolved ownership is authoritative.** Provenance comes from the winning `EventBus` feature after duplicate-name arbitration. Renderers must not infer ownership from name prefixes or extension manifests.
2. **Provenance is semantic data.** Add a shared tool-provenance value to runtime tool lifecycle events and conversation projections. Keep producer metadata independent from display form.
3. **Callable names do not change.** `bash` remains the model/API name. The extension name is presentation and audit metadata.
4. **Compact rendering is additive.** Built-ins render exactly as today. Extension tools render `tool (extension-name)`, with only the suffix using a muted extension accent.
5. **Every surface agrees.** TUI, ACP, IPC/MQTT, audit logging, and durable transcript projections preserve the same producer evidence.
6. **Collision behavior is explicit.** Tests prove that the winning feature is reported and a losing extension advertiser is never mislabeled as executor.

## Implementation plan

### Phase 1 — ownership model

- Introduce a serializable semantic provenance type with built-in and extension variants.
- Teach extension-backed features to expose their extension identity without parsing feature names.
- Extend `EventBus`’s finalized tool cache with resolved ownership/provenance.
- Add lookup tests for built-ins, extension tools, and duplicate-name collisions.

### Phase 2 — runtime propagation

- Resolve provenance before dispatch.
- Add provenance to tool start/end lifecycle events with backward-compatible serde defaults where persisted contracts require them.
- Preserve it in audit, IPC, MQTT, ACP, and conversation segment projections.
- Ensure internal/hidden tools have deterministic built-in provenance unless explicitly extension-owned.

### Phase 3 — presentation

- Add a shared display-label projection: built-in `bash`; extension `bash (recro-coe-agent)`.
- Render the suffix in a muted extension accent in compact and detailed TUI rows.
- Preserve plain-text labels in copy/export surfaces.
- Add snapshot/behavior tests for narrow and wide layouts.

### Phase 4 — verification

- Run focused bus, event, projection, and TUI tests.
- Run `cargo test -p omegon --locked`, `just lint`, and `just link`.
- Exercise the Recro opt-in host litmus and confirm its tools display the extension producer.

## Acceptance criteria

- An extension-owned tool visibly identifies the extension in conversation rows.
- Built-in tool rows are visually unchanged.
- Duplicate tool names show the producer that actually executed.
- Disabling the displayed extension removes that producer on the next registry refresh.
- ACP, IPC/MQTT, audit, and transcript consumers receive equivalent provenance.
- Existing persisted events lacking provenance remain readable if those event contracts are serialized durably.

## Risks

- `AgentEvent` is broad and adding fields will touch many constructors and match arms; use compiler-guided migration and avoid renderer-local fallbacks.
- Feature names are not guaranteed to equal extension package names; extension identity needs an explicit interface.
- Width pressure in Slim mode requires suffix truncation that preserves the tool name first and extension identity second.
