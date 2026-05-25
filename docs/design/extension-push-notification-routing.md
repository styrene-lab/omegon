+++
id = "extension-push-notification-routing"
tags = ["extensions", "voice", "daemon", "host-actions", "0.24", "issue-79"]
aliases = ["issue-79-voice-push-routing"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Extension push notification routing — Issue 79

## Overview

Issue #79 adds a push-notification path from native extensions into Omegon's daemon event queue. The immediate consumer is `omegon-voice`, which emits JSON-RPC notifications such as `voice/transcription` when local speech-to-text completes.

The existing vox bridge polls `vox_route` every 500ms. That is acceptable for chat connectors but too slow and indirect for voice. Voice input is local operator input and should be injected as an operator-trusted prompt as soon as the extension emits the notification.

## Decisions

### Decision: Add a generic notification dispatch seam, with voice as the first consumer

**Status:** decided
**Rationale:** The transport should not hardcode voice semantics. The extension RPC layer should preserve notifications and route them to typed consumers. `voice_bridge` owns voice-specific conversion into `DaemonEventEnvelope`.

### Decision: Gate voice routing on extension capability

**Status:** decided
**Rationale:** Extensions without explicit voice capability should be unaffected. `capabilities.voice = true` is required before `voice/transcription` notifications are accepted for daemon injection.

### Decision: Voice transcriptions are operator-trusted prompts

**Status:** decided
**Rationale:** Voice input is the local operator speaking. It should not use the untrusted external-message containment used for Discord/Slack vox messages.

### Decision: Omegon owns voice state observability, extensions own capture semantics

**Status:** decided
**Rationale:** Omegon should not encode OS-, backend-, USB-, hardware mute-, or privacy-indicator semantics. For 0.24.0, Omegon only accepts the extension-reported `voice/state` payload and surfaces the required TUI-observable lifecycle/mic-open indicator. Lower-level capture details remain the responsibility of voice extensions.

Contract boundary:

- Required host-visible fields: `state` and `mic_open`.
- `mic_open` means the extension reports that an input capture session/stream is active.
- `mic_open` does **not** assert physical USB LED state, hardware mute state, audio energy, OS permission state, or OS privacy-indicator visibility.
- Optional fields such as `backend`, `device`, `permission`, `error`, or audio levels may be passed through later, but they are extension-owned metadata and are not required for 0.24.0.

## Implementation plan

1. Add `voice` to `omegon-extension::Capabilities`, defaulting false and included in host-all/intersection behavior.
2. Add tests proving legacy capability payloads default `voice = false` and explicit payloads parse `voice = true`.
3. Add a notification dispatch seam to extension process handling so JSON-RPC notifications are not silently dropped.
4. Add `extensions::voice_bridge` to convert `voice/transcription` notifications into `DaemonEventEnvelope` entries.
5. Wire voice-capable extensions into daemon startup with the shared daemon event queue.
6. Add minimal `voice/state` observability for the TUI mic indicator path.
7. Validate with `omegon-voice` end-to-end.

## Open questions

- [assumption] The installed `omegon-voice` extension declares or can be updated to declare `capabilities.voice = true` in its manifest/initialize payload.
- [assumption] The daemon event queue used by the vox bridge is the correct initial injection target for voice events.
