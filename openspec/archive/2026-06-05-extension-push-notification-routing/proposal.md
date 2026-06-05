# Extension Push Notification Routing

## Intent

Route trusted push notifications from native extensions into Omegon's daemon event queue, starting with `omegon-voice` `voice/transcription` notifications.

## Scope

- Add a `voice` extension capability flag.
- Preserve and dispatch extension JSON-RPC notifications instead of dropping them.
- Convert `voice/transcription` notifications from voice-capable extensions into operator-trusted `DaemonEventEnvelope` prompt events.
- Keep non-voice extensions unaffected.
- Add deterministic tests for capability parsing, notification conversion, and malformed payload handling.

## Non-goals

- MCP HostAction approval policy (#78).
- Replacing the existing polling vox bridge.
- Full rich TUI mic indicator if transcription routing is not yet complete.
