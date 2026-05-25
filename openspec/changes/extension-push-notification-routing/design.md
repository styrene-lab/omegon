# Design

See `docs/design/extension-push-notification-routing.md`.

## Architecture

Extension RPC transport receives both responses and notifications on stdout. Requests/responses remain matched by JSON-RPC id. Notifications without an id are routed to notification consumers. `voice_bridge` is the first consumer and only accepts `voice/*` notifications from extensions that declared `capabilities.voice = true`.

Voice transcription events are converted into `DaemonEventEnvelope` with `source = "voice"`, `trigger_kind = "prompt"`, `caller_role = "edit"`, `source_channel = "voice"`, and payload text/trust fields.
