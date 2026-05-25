# Voice Transcription Routing — Delta Spec

## ADDED Requirements

### Requirement: Voice transcription injects an operator prompt

Omegon SHALL convert voice transcription notifications into daemon prompt events with operator trust.

#### Scenario: Valid transcription becomes daemon event
Given a voice-capable extension emits `voice/transcription` with text `open the reader`
When the voice bridge handles the notification
Then it injects a `DaemonEventEnvelope`
And the envelope source is `voice`
And the trigger kind is `prompt`
And the payload text is `open the reader`
And the payload trust level is `operator`
And the caller role is `edit`.

#### Scenario: Empty transcription is ignored
Given a voice-capable extension emits `voice/transcription` with empty text
When the voice bridge handles the notification
Then no daemon event is injected.

#### Scenario: Malformed transcription is ignored without panic
Given a voice-capable extension emits `voice/transcription` with malformed params
When the voice bridge handles the notification
Then no daemon event is injected
And the extension transport remains usable.
