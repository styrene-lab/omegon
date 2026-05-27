---
title: Voice Control Metadata and TTS Lifecycle (#98)
status: exploring
tags: [0.25, voice, tts, tui, metadata]
---

# Voice Control Metadata and TTS Lifecycle (#98)

## Problem

`omegon-voice` emits richer `voice/transcription` metadata such as radio cues and session-close requests, but Omegon currently treats the event mostly as prompt text.

The host needs deliberate semantics for:

- `radio_cue`
- `end_of_turn`
- `close_session_requested`
- TTS playback lifecycle

## Goal

Consume voice control metadata without submitting cue words as literal prompt text, and define host-visible TTS lifecycle events.

## Event fields

Example:

```json
{
  "text": "Assess this current directory",
  "duration_s": 3.2,
  "radio_cue": "over",
  "end_of_turn": true,
  "close_session_requested": false
}
```

`over_and_out` should submit the normalized prompt, then close the voice session through an explicit host/extension control path.

## Decisions

### Decision: Cue words are metadata, not prompt text

If the extension recognized `over` or `over and out`, host must not re-add those cue words to the submitted prompt.

### Decision: Voice state remains extension-reported

Host does not infer microphone or playback truth from OS indicators. Extension emits lifecycle state; host displays/acts on it.

## Open questions

- [assumption] `omegon-voice` strips cue words from `text` before emission.
- Should `close_session_requested` trigger `voice_session_stop` directly, queue a tool call, or surface a deterministic operator-visible stop action?
- Should `duration_s` and radio cue metadata be stored in conversation details, audit log, or only trace logs?
- Which TTS lifecycle states are required: `queued`, `speaking`, `interrupted`, `completed`, `failed`?

## Acceptance

- Host preserves voice metadata alongside visible prompt submission.
- `over` submits normally and keeps session open.
- `over_and_out` requests/executes voice session closure after prompt acceptance.
- Cue words are never appended to submitted text.
- TTS lifecycle has a documented state model.

## Links

- [[0.25-roadmap-extension-surfaces]]
- [[tts-agent-mode-100]]
