+++
id = "voice-control-tests-issue-98"
title = "Voice control tests — Issue 98"
status = "exploring"
parent = "voice-control-metadata-tts-lifecycle"
issue_type = "test-plan"
priority = 1
openspec_change = null
tags = ["extensions", "voice", "testing", "0.24", "issue-98"]
aliases = ["issue-98-tests"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Voice control tests — Issue 98

## Overview

This node scopes deterministic tests for voice metadata and lifecycle behavior. Tests must not require a microphone, Whisper model, macOS TCC approval, or audible TTS.

## Test inventory

### 1. Bridge preserves transcription metadata

Synthetic `voice/transcription` notification includes:

```json
{
  "text": "Assess this directory",
  "radio_cue": "over",
  "end_of_turn": true,
  "close_session_requested": false,
  "duration_s": 2.1,
  "utterance_id": "u1"
}
```

Assert the resulting `DaemonEventEnvelope` preserves all metadata and retains canonical voice ingress fields.

### 2. TUI notification routing preserves metadata

`voice_prompt_from_notification` should return a voice prompt command with normalized text and metadata. Empty/cue-only text remains ignored.

### 3. `over` keeps session open

A prompt with `radio_cue=over` and `close_session_requested=false` submits normally and does not stage a stop action.

### 4. `over_and_out` stages shutdown

A prompt with `radio_cue=over_and_out` and `close_session_requested=true` submits normally and stages or invokes voice shutdown at the chosen lifecycle point.

### 5. Busy/queued routing preserves close intent

When a voice prompt is queued because the agent is busy, metadata survives until acceptance and close intent is still honored.

### 6. Status-only events stay status-only

`voice/state` and `voice/tts_state` update harness status but never create daemon prompt events.

### 7. TTS lifecycle updates and clears status

Synthetic TTS start/completion notifications update `HarnessStatus` and broadcast `HarnessStatusChanged`.

## Implementation targets

- `core/crates/omegon/src/extensions/voice_bridge.rs` unit tests.
- `core/crates/omegon/src/tui/mod.rs` unit tests near existing voice prompt tests.
- `core/crates/omegon/src/status.rs` tests for footer/status serialization if status shape changes.
- Focused main-loop tests only if close-after-accept requires runtime behavior that cannot be isolated.

## Open questions

- [assumption] Close-after-accept behavior can be unit-tested without launching a full TUI runtime.
- Which existing test seam best represents busy/queued prompt acceptance?

## Acceptance criteria

- All issue #98 acceptance criteria have at least one host-side deterministic test.
- Tests encode that voice state and TTS state are status-only.
- Tests encode that cue metadata is not submitted as prompt text.
