+++
id = "extension-event-adapter-anchors"
tags = ["extensions", "events", "daemon", "0.24", "architecture"]
aliases = ["extension-event-adapters", "canonical-daemon-ingress"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Extension event adapter anchors

## Purpose

Omegon extensions can increasingly produce push-style events: voice transcription, reader selections, browser DOM changes, file drops, chat mentions, and future sensor or desktop events. These sources must not grow separate prompt paths.

The architectural anchor is:

```text
extension/domain notification
→ host-owned event adapter
→ canonical DaemonEventEnvelope
→ existing daemon queue and ordinary agent/runtime handling
```

## Non-negotiable invariants

1. **No extension gets a private prompt stream.** Every push source that can affect the agent crosses a host-owned adapter.
2. **Transport is not trust.** JSON-RPC notification receipt only proves bytes arrived from an extension process. Trust is assigned by the adapter after capability, method, and payload validation.
3. **One daemon ingress.** Adapters emit ordinary `DaemonEventEnvelope` values into the existing daemon event queue. They do not create durable domain buses, secondary agent loops, or prompt dispatchers.
4. **Domain semantics stay at the edge.** `voice/transcription`, `reader/selection`, and `browser/dom_event` can remain domain-specific at the notification boundary, but the core runtime sees normalized daemon envelopes.
5. **High-volume streams coalesce before ingress.** Adapters throttle, debounce, batch, or summarize noisy sources before emitting daemon events.

## Current anchors in code

- `ExtensionNotification` — transport-level notification captured from an extension process.
- `voice_notification_rx` — per-extension receiver; a transport adapter boundary, not a voice bus.
- `extensions::voice_bridge` — first concrete event adapter.
- `extensions::vox_bridge` — existing extension bridge that also emits `DaemonEventEnvelope`.
- `DaemonEventEnvelope` — canonical daemon ingress format.

## Future abstraction target

The current implementation is intentionally concrete. If a second or third push-event domain appears, extract a generic adapter seam along these lines:

```rust
trait ExtensionEventAdapter {
    fn capability(&self) -> &'static str;
    fn accepts(&self, notification: &ExtensionNotification) -> bool;
    fn convert(&self, notification: ExtensionNotification) -> Option<DaemonEventEnvelope>;
}
```

Do not extract this abstraction for voice alone. Extract when at least one additional domain proves the shared shape.

## Capability domains

Prefer coarse capability gates aligned to trust boundaries, not one flag per notification method:

- `voice` — local operator speech input and voice state.
- `reader_events` — document/page/selection context, normally context not prompt.
- `browser_events` — browser/page state, usually context or explicit operator actions.
- `desktop_events` — file drops/window events, usually operator-confirmed.
- `external_messages` — Slack/Discord/chat-style messages, externally contained.
- `workspace_events` — filesystem or repository events, often signal/context.

## Trust examples

### Voice transcription

```text
voice/transcription
→ capability: voice
→ trigger_kind: prompt
→ trust_level: operator
→ caller_role: edit
→ source_channel: voice
```

### Reader selection

```text
reader/selection
→ capability: reader_events
→ trigger_kind: context
→ trust_level: local_context
→ caller_role: read
→ source_channel: reader
```

### Chat mention

```text
chat/message
→ capability: external_messages
→ trigger_kind: prompt
→ trust_level: external
→ caller_role: ask
→ source_channel: slack|discord
```

## Testing anchor

Every new adapter needs tests that prove:

1. capability-gated sources are accepted;
2. missing capability blocks injection;
3. unsupported methods do not emit prompt events;
4. malformed payloads are ignored without panics;
5. emitted events use `DaemonEventEnvelope` and the shared daemon queue;
6. trust/caller/source metadata is explicit and domain-appropriate.

## 0.24 posture

For 0.24.0, voice is the proving slice. Keep the implementation minimal but use issue #81 tests to lock the invariant that voice uses canonical daemon ingress rather than a parallel event stream.
