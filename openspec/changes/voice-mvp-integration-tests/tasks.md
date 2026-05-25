# Voice MVP Integration Tests Tasks

## 1. Combined voice bridge integration test
<!-- specs: extensions/voice-mvp-tests -->

- [ ] 1.1 Add fake native extension helper that can emit JSON-RPC notifications before `get_tools` response.
- [ ] 1.2 Add test for voice-capable fake extension → `voice_notification_rx` → `voice_bridge` → existing daemon event queue.
- [ ] 1.3 Assert trusted local operator payload fields and source/caller metadata.
- [ ] 1.4 Assert the bridge emits ordinary `DaemonEventEnvelope` values and does not create a parallel voice prompt stream.

## 2. Negative routing tests
<!-- specs: extensions/voice-mvp-tests -->

- [ ] 2.1 Add non-voice manifest test proving `voice_notification_rx` is absent.
- [ ] 2.2 Add `voice/state` no-daemon-event test at bridge level.
- [ ] 2.3 Record initialize-negotiation caveat in #81 closeout comment.

## 3. Validation
<!-- specs: extensions/voice-mvp-tests -->

- [ ] 3.1 Run `cargo test -p omegon voice -- --nocapture`.
- [ ] 3.2 Run `cargo test -p omegon extensions::tests:: -- --nocapture`.
- [ ] 3.3 Run `cargo test -p omegon-extension -- --nocapture`.
- [ ] 3.4 Run `just lint`.
- [ ] 3.5 Post acceptance trace to #81.
