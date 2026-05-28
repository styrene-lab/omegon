# Extension Push Notification Routing Tasks

## 1. Capability substrate
<!-- specs: extensions/push-notifications -->

- [x] 1.1 Add `voice` to `omegon-extension::Capabilities` with default false.
- [x] 1.2 Add tests for legacy payloads defaulting `voice = false`.
- [x] 1.3 Add tests for explicit `voice = true` and host/intersection behavior.

## 2. Notification dispatch seam
<!-- specs: extensions/push-notifications -->

- [x] 2.1 Add tests proving notifications do not break in-flight RPC response matching.
- [x] 2.2 Add a host-side notification representation/channel for extension processes.
- [x] 2.3 Route unknown notifications safely without daemon injection.

## 3. Voice bridge conversion
<!-- specs: extensions/voice-transcription-routing -->

- [x] 3.1 Add `voice_bridge` tests for valid transcription conversion.
- [x] 3.2 Add tests for empty/malformed transcription handling.
- [x] 3.3 Implement `voice_bridge` conversion into `DaemonEventEnvelope`.

## 4. Daemon wiring and validation
<!-- specs: extensions/push-notifications, extensions/voice-transcription-routing, extensions/voice-state-observability -->

- [x] 4.1 Wire voice-capable extension notifications into the daemon event queue.
- [x] 4.2 Validate `omegon-voice` transcription injection locally.
- [x] 4.3 Run `cargo test -p omegon-extension`.
- [x] 4.4 Run `cargo test -p omegon`.
- [x] 4.5 Run `just lint`.
- [x] 4.6 Implement minimal `voice/state` observability for a TUI mic indicator using only extension-reported `state` and `mic_open`.
- [x] 4.7 Document that OS/backend/USB/hardware mute/privacy indicator semantics remain extension-owned and are not inferred by Omegon.
- [x] 4.8 Post acceptance trace to #79 and close only after end-to-end validation.
