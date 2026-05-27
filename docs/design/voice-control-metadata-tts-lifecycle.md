+++
id = "voice-control-metadata-tts-lifecycle"
title = "Voice control metadata and TTS lifecycle — Issue 98"
status = "exploring"
parent = null
issue_type = "github-issue"
priority = 1
openspec_change = null
tags = ["extensions", "voice", "tui", "status", "0.24", "issue-98"]
aliases = ["issue-98-voice-control", "voice-control-metadata"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Voice control metadata and TTS lifecycle — Issue 98

## Overview

Issue #98 promotes voice transcription control fields and spoken-output state from extension-specific JSON details into deliberate host-side contracts.

The current voice path is intentionally narrow:

```text
omegon-voice JSON-RPC notification
→ ExtensionNotification
→ extensions::voice_bridge
→ DaemonEventEnvelope
→ TUI VoicePrompt
→ SubmitPrompt
```

That path is correct, but it currently drops or under-models control metadata at the prompt-routing boundary. `radio_cue`, `end_of_turn`, `close_session_requested`, and full utterance timing need to survive conversion, queueing, display, and acceptance. TTS/spoken-output state also needs a settled host contract so the footer/status model can represent microphone capture and speaker playback independently.

## Current evidence

- `core/crates/omegon/src/extensions/voice_bridge.rs` converts `voice/transcription` into `DaemonEventEnvelope` with `source = "voice"`, `trigger_kind = "prompt"`, `source_channel = "voice"`, and operator trust fields.
- `voice_bridge.rs` currently preserves `text`, `duration_s`, `utterance_id`, `trust_level`, and `extension`, but not `radio_cue`, `end_of_turn`, or `close_session_requested`.
- `core/crates/omegon/src/tui/mod.rs` maps `voice/transcription` notifications to `TuiCommand::VoicePrompt { text, event_id }`; the enum has no metadata field.
- `core/crates/omegon/src/main.rs` normalizes `VoicePrompt` into `SubmitPrompt` with visible `🎙` decoration and `queue_mode = UntilReady`.
- `core/crates/omegon/src/status.rs` has `VoiceStateStatus { extension, state, mic_open }` and footer rendering for `listening`, `processing`, and `speaking`, but no independent audio-output state.
- `docs/design/extension-event-adapter-anchors.md` and `docs/design/voice-mvp-integration-tests.md` already establish the invariant that voice events use canonical daemon ingress, not a private prompt stream.

## Scope decomposition

This parent node owns the overall issue and links four child design nodes:

1. [[voice-transcription-control-metadata]] — preserve transcription metadata across bridge, TUI command, prompt submission, queueing, and transcript rendering.
2. [[voice-over-and-out-shutdown]] — define deterministic `close_session_requested` handling after prompt acceptance.
3. [[voice-tts-lifecycle-contract]] — decide host contract for spoken-output state, preferably independent from microphone state.
4. [[voice-control-tests-issue-98]] — encode acceptance criteria with focused tests.

## Decisions

### Decision: Keep voice input on canonical daemon ingress

**Status:** accepted

Voice transcription remains an extension event adapter that emits `DaemonEventEnvelope`. Issue #98 should not introduce a second voice-specific prompt bus or agent loop.

### Decision: Treat recognized radio cues as metadata, not prompt text

**Status:** accepted

When `omegon-voice` recognizes cue words and strips them from `text`, Omegon core must preserve the cue metadata and submit only normalized operator text. The host must not re-append or leak trailing `over` / `over and out` into the prompt.

### Decision: Model microphone capture and spoken output independently

**Status:** accepted

Use separate status fields for microphone capture and spoken output. `voice/state` remains the microphone/capture lifecycle. `voice/tts_state` becomes the spoken-output lifecycle. This avoids impossible composite states such as “listening and speaking” being forced into one enum value.

## Boundary model

Issue #98 crosses three boundaries. Treat them separately so the implementation does not sprawl:

1. **Ingress boundary** — `voice_bridge` validates extension notifications and emits canonical daemon envelopes.
2. **Prompt boundary** — TUI/runtime converts voice envelopes into `PromptSubmission` values without losing metadata.
3. **Lifecycle boundary** — status/control paths represent microphone capture, close-session requests, and spoken output.

The first implementation pass should finish the ingress and prompt boundaries before adding broader lifecycle behavior.

## Open questions

- [assumption] `close_session_requested=true` can be handled through an existing extension command/control path without adding a new public HostAction.
- [assumption] `PromptSubmission` or adjacent transcript structures can carry metadata without forcing broad conversation model changes.
- [assumption] The direct TUI idle-notification path and daemon event path can use the same metadata struct.
- Should `voice/tts_state` be the final method name, or should host status use a more general `audio_output` event namespace?
- Should close-after-accept run immediately after queue acceptance, or after the resulting agent turn finishes? Issue #98 says after prompt acceptance/turn handling; implementation needs one concrete lifecycle point.

## Non-goals

- Acoustic echo cancellation.
- Cloud STT/TTS providers.
- Multi-user voice routing.
- Confirm-before-submit for all voice prompts.
