+++
id = "voice-over-and-out-shutdown"
title = "Voice over-and-out shutdown"
status = "exploring"
parent = "voice-control-metadata-tts-lifecycle"
issue_type = "implementation-slice"
priority = 1
openspec_change = null
tags = ["extensions", "voice", "lifecycle", "0.24", "issue-98"]
aliases = ["voice-close-session-requested", "over-and-out"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Voice over-and-out shutdown

## Overview

This node scopes deterministic handling for `close_session_requested=true`, normally emitted when the operator says `over and out`.

The core behavior must be deliberate: process the prompt normally, then close or stage closure of the voice session. Dropping the field leaves the microphone open after the operator explicitly requested shutdown.

## Desired behavior

Given:

```json
{
  "text": "Stop listening after this message",
  "radio_cue": "over_and_out",
  "end_of_turn": true,
  "close_session_requested": true
}
```

Then:

1. Submit `Stop listening after this message` as the operator prompt.
2. Render it as voice-originated.
3. Preserve close intent while queued/busy.
4. Close the voice session after prompt acceptance or after the resulting turn is handled.
5. Update voice status to idle / mic closed, or surface a deterministic pending-stop action if direct control is not available.

## Implementation targets

- Locate extension control path for invoking `voice_session_stop` or equivalent host-side stop command.
- Add a small lifecycle hook for “voice stop requested after prompt acceptance”.
- Ensure busy queue mode preserves close intent until the queued prompt is accepted.
- Emit a visible system/status notification if stop cannot be executed directly.

## Lifecycle options

### Option 1: Stop after queue acceptance

Fastest feedback: once the prompt is accepted into the runtime queue, invoke/stage voice shutdown. Cost: the mic may close before the resulting agent turn actually completes.

### Option 2: Stop after turn completion

Closest reading of “after the turn is handled”: carry close intent through prompt execution and stop after `AgentEnd`. Cost: requires metadata to survive deeper into runtime turn state.

### Option 3: Stage agent-visible stop action

If direct host-side extension invocation is not available, emit a deterministic pending action/system notification rather than silently dropping the request. Cost: closure may depend on a follow-up mechanism.

## Decisions

### Decision: `over` and `over_and_out` differ only by close intent

**Status:** accepted

`radio_cue=over` submits normally and keeps listening. `radio_cue=over_and_out` submits normally and requests session closure after safe handoff.

### Decision: Stop after queue acceptance for the first implementation

**Status:** accepted

The first implementation should invoke or stage voice shutdown once the prompt has been accepted into the runtime queue. That satisfies the operator’s explicit “over and out” request with less runtime coupling than carrying a post-turn callback through agent execution. If later UX shows the mic must remain open until `AgentEnd`, this can be revised as a follow-up.

## Open questions

- [assumption] The voice extension exposes a callable `voice_session_stop` tool in the same host process that can be invoked without agent mediation.
- What exact lifecycle point is “safe handoff”: queue acceptance, prompt submission to agent loop, or turn completion?
- If the stop call fails, should the TUI retry, show a persistent warning, or require explicit operator action?

## Acceptance criteria

- `close_session_requested=true` is not silently dropped.
- Queued voice prompts retain close intent.
- `over` leaves mic state unchanged.
- `over_and_out` transitions voice state toward idle/mic closed or exposes a deterministic pending-stop state.
