+++
id = "voice-tts-lifecycle-contract"
title = "Voice TTS lifecycle contract"
status = "exploring"
parent = "voice-control-metadata-tts-lifecycle"
issue_type = "contract"
priority = 2
openspec_change = null
tags = ["extensions", "voice", "tts", "status", "0.24", "issue-98"]
aliases = ["voice-spoken-output-state", "voice-tts-state"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Voice TTS lifecycle contract

## Overview

This node scopes the host-side contract for spoken-output lifecycle notifications. The goal is to make TTS state visible without conflating it with microphone capture state or corrupting terminal output.

## Contract options

### Option A: Expand `voice/state`

Allow extension state values such as:

```text
idle
listening
processing
speaking
error
```

Payload example:

```json
{
  "state": "speaking",
  "mic_open": true,
  "audio_output": true,
  "backend": "macos",
  "voice": "Reed (English (US))"
}
```

Cost: a single state enum can blur independent mic and speaker lifecycles.

### Option B: Separate `voice/tts_state`

Use a separate notification for spoken output:

```json
{
  "state": "speaking",
  "audio_output": true,
  "backend": "macos",
  "voice": "Reed (English (US))"
}
```

and completion:

```json
{
  "state": "idle",
  "audio_output": false
}
```

Benefit: microphone state and speaker state remain independently knowable.

## Proposed decision

### Decision: Use `voice/tts_state` for spoken-output status

**Status:** accepted

Adopt Option B. `voice/state` remains capture/mic status. `voice/tts_state` reports speaker playback with `state`, `audio_output`, optional `backend`, and optional `voice`. The host may accept expanded `voice/state` as a compatibility fallback later, but the documented contract is separate.

## Implementation targets

- `core/crates/omegon/src/extensions/voice_bridge.rs`
  - Parse `voice/tts_state` as status-only, not as a prompt event.
- `core/crates/omegon/src/status.rs`
  - Add a separate `VoiceTtsStatus` / audio-output status field with backend and voice metadata.
  - Footer/status summary can render `voice speaking` when audio output is active while preserving mic status.
- TUI/status event propagation
  - Broadcast `HarnessStatusChanged` when spoken output starts and clears.

## Open questions

- [assumption] `omegon-voice` can emit `voice/tts_state` for async speech start/completion.
- Should the host also accept expanded `voice/state` as a compatibility fallback?
- Should status include queued TTS output, or only currently active playback?

## Acceptance criteria

- TTS start updates host status to show active audio output.
- TTS completion clears audio-output state.
- `voice/tts_state` never creates an agent prompt.
- Mic state remains separately visible from speaker state.
