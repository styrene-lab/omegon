---
title: TTS Agent Mode (#100)
status: exploring
tags: [0.25, voice, tts, agent-ux]
---

# TTS Agent Mode (#100)

## Problem

`omegon-voice` can speak using Piper/TTS tools, but agent-mode spoken UX is not deterministic. Operators can enable spoken cues, yet subsequent agent turns may answer only in text or report audio status out of sync with playback.

## Goal

Define a deterministic host/agent behavior contract for short spoken status cues.

## Desired behavior

When spoken status cues are enabled:

- Short operational status may be spoken.
- Long answers remain text-first in TUI.
- Agent speaks short handoff cues for long answers, not full content.
- Failures can produce short failure cues.
- Text and audio timing language must be honest.

## Decisions

### Decision: TTS agent mode depends on #98

Do not define agent cue behavior until voice/TTS lifecycle metadata is settled.

### Decision: Never speak sensitive or high-density content by default

Do not speak code, logs, tables, URLs, hashes, secrets, or long answers.

## Open questions

- [assumption] A session-level state can tell the agent spoken cues are active.
- Where should the active TTS mode live: host status, memory/session metadata, skill state, or extension status?
- Should cue selection be prompt-instruction based, tool-policy based, or explicit host automation?
- How do we prevent repeated/annoying spoken cues during rapid tool loops?

## Acceptance

- Session state exposes `spoken_status_cues`, voice profile, and backend.
- Agent consistently uses `voice_speak_async` or equivalent for short cues when active.
- Long answers are not spoken in full.
- Playback timing language distinguishes queued vs completed audio.
- TTS mode is visible/auditable in the TUI or session details.

## Links

- [[0.25-roadmap-extension-surfaces]]
- [[voice-control-metadata-98]]
