# Voice MVP Tests — Delta Spec

## ADDED Requirements

### Requirement: Voice-capable extension notifications inject trusted daemon prompts

Omegon SHALL test the complete fake extension notification path into the daemon event queue without microphone or model dependencies.

#### Scenario: Voice transcription reaches daemon queue
Given a fake native extension declares `capabilities.voice = true`
And it emits `voice/transcription` with text `summarize the current project`
When the host starts the voice bridge for the extension receiver
Then exactly one daemon event is produced
And the event source is `voice`
And the trigger kind is `prompt`
And the payload trust level is `operator`
And the payload text is `summarize the current project`.

### Requirement: Non-voice extensions cannot inject voice prompts

Omegon SHALL not attach a voice notification receiver for extensions that do not declare voice capability.

#### Scenario: Non-voice extension emits voice notification
Given a fake native extension does not declare `capabilities.voice = true`
When it emits `voice/transcription`
Then the spawned extension has no voice notification receiver
And no voice bridge can inject a daemon prompt event.

### Requirement: Voice state is not prompt input

Omegon SHALL not convert `voice/state` notifications into prompt events.

#### Scenario: Voice state notification is ignored by prompt bridge
Given a voice bridge receives `voice/state`
When the bridge processes the notification
Then no daemon prompt event is produced.
