+++
id = "voice-transcription-control-metadata"
title = "Voice transcription control metadata"
status = "exploring"
parent = "voice-control-metadata-tts-lifecycle"
issue_type = "implementation-slice"
priority = 1
openspec_change = null
tags = ["extensions", "voice", "tui", "metadata", "0.24", "issue-98"]
aliases = ["voice-radio-cue-metadata"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Voice transcription control metadata

## Overview

This node scopes the metadata-preservation slice of issue #98. `voice/transcription` already becomes a trusted local operator prompt; the missing piece is preserving control metadata deliberately across every boundary that currently reduces the event to plain text.

## Required metadata

For a normalized transcription payload:

```json
{
  "text": "Assess this directory",
  "duration_s": 2.1,
  "radio_cue": "over",
  "end_of_turn": true,
  "close_session_requested": false
}
```

The host should submit/display:

```text
🎙 Assess this directory
```

and preserve metadata equivalent to:

```json
{
  "source_channel": "voice",
  "radio_cue": "over",
  "end_of_turn": true,
  "close_session_requested": false,
  "duration_s": 2.1
}
```

## Implementation targets

- `core/crates/omegon/src/extensions/voice_bridge.rs`
  - Copy optional `radio_cue`, `end_of_turn`, and `close_session_requested` into `DaemonEventEnvelope.payload`.
  - Keep `duration_s` and `utterance_id` behavior.
  - Add explicit payload `source_channel = "voice"` so downstream consumers do not need to infer it from envelope fields after conversion.
- `core/crates/omegon/src/tui/mod.rs`
  - Introduce a small `VoicePromptMetadata` struct with `event_id`, `duration_s`, `radio_cue`, `end_of_turn`, and `close_session_requested`.
  - Replace `TuiCommand::VoicePrompt { text, event_id }` with `VoicePrompt { text, metadata }`.
  - Update `voice_prompt_from_notification` to preserve recognized cue fields when notifications are routed directly in TUI idle mode.
  - Replace `queued_prompts: VecDeque<(String, Vec<PathBuf>)>` with a queued prompt struct if metadata must survive TUI-side queueing.
- `core/crates/omegon/src/main.rs`
  - Preserve metadata when normalizing `VoicePrompt` into `SubmitPrompt`.
  - Preserve metadata in runtime busy/queued routing (`PromptQueueMode::UntilReady`).
- Prompt transcript/details rendering
  - Keep visible prompt voice-originated via `🎙`.
  - Do not append cue words to submitted text.
  - Make cue metadata available in details/audit only if the current prompt model has a narrow hook; otherwise preserve it in runtime metadata first and defer UI details.

## Decisions

### Decision: Cue words stay out of submitted text

**Status:** accepted

The extension owns cue recognition and strips cue words from `text`. Core should trust the normalized `text` field and carry cue details only as metadata.

### Decision: Metadata is optional and backwards-compatible

**Status:** accepted

Older voice extensions may emit only `text` and `duration_s`. Host parsing should treat the new fields as optional and should ignore malformed optional fields rather than rejecting otherwise valid transcriptions.

### Decision: Constrain known radio cues at behavior boundaries

**Status:** accepted

Preserve unknown `radio_cue` strings for audit/debug metadata, but only `over` and `over_and_out` drive host behavior. Unknown values must not trigger shutdown or other control actions.

## Open questions

- [assumption] Existing prompt submission/history types can tolerate an optional JSON metadata field without large storage migrations.
- Where is the narrowest transcript/details surface for exposing prompt metadata today?
- Should `radio_cue` be constrained to known values (`over`, `over_and_out`) or preserved as an arbitrary string for forward compatibility?

## Acceptance criteria

- `radio_cue=over` survives bridge conversion and TUI prompt normalization.
- `duration_s`, `end_of_turn`, and `close_session_requested` survive queueing.
- Voice prompt visible text remains `🎙 <normalized text>`.
- Recognized cue text does not leak into submitted prompt text.
