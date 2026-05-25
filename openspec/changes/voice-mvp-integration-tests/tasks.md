# Voice MVP Integration Tests Tasks

## 1. Combined voice bridge integration test
<!-- specs: extensions/voice-mvp-tests -->

- [x] 1.1 Add fake native extension helper that can emit JSON-RPC notifications before `get_tools` response.
- [x] 1.2 Add test for voice-capable fake extension → `voice_notification_rx` → `voice_bridge` → existing daemon event queue.
- [x] 1.3 Assert trusted local operator payload fields and source/caller metadata.
- [x] 1.4 Assert the bridge emits ordinary `DaemonEventEnvelope` values and does not create a parallel voice prompt stream.

## 2. Negative routing tests
<!-- specs: extensions/voice-mvp-tests -->

- [x] 2.1 Add non-voice manifest test proving `voice_notification_rx` is absent.
- [x] 2.2 Add `voice/state` no-daemon-event test at bridge level.
- [ ] 2.3 Record initialize-negotiation caveat in #81 closeout comment.

## 3. Validation
<!-- specs: extensions/voice-mvp-tests -->

- [x] 3.1 Run `cargo test -p omegon voice -- --nocapture`.
- [x] 3.2 Run `cargo test -p omegon extensions::tests:: -- --nocapture`.
- [x] 3.3 Run `cargo test -p omegon-extension -- --nocapture`.
- [x] 3.4 Run `just lint`.
- [x] 3.5 Post acceptance trace to #81.
