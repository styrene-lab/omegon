# Voice State Observability — Delta Spec

## ADDED Requirements

### Requirement: Voice state is observable without becoming a prompt

Omegon SHALL accept `voice/state` notifications from voice-capable extensions and expose a minimal host-visible state suitable for a TUI mic indicator without injecting a daemon prompt event.

#### Scenario: Voice state updates host-visible lifecycle
Given a voice-capable extension emits `voice/state` with `state` set to `listening` and `mic_open` set to true
When the host voice bridge handles the notification
Then the latest host-visible voice state is `listening`
And the latest host-visible mic-open value is true
And no daemon prompt event is injected.

#### Scenario: Voice state idle closes mic indicator
Given a voice-capable extension emits `voice/state` with `state` set to `idle` and `mic_open` set to false
When the host voice bridge handles the notification
Then the latest host-visible voice state is `idle`
And the latest host-visible mic-open value is false
And no daemon prompt event is injected.

### Requirement: Omegon does not define low-level microphone semantics

Omegon SHALL treat `mic_open` as extension-reported capture-session state only and SHALL NOT infer physical USB LED state, hardware mute state, OS privacy indicator state, audio energy, or OS permission state.

#### Scenario: Minimal host contract ignores backend-specific fields
Given a voice-capable extension emits `voice/state` with `state`, `mic_open`, and optional backend-specific fields
When the host parses the notification
Then `state` and `mic_open` are sufficient for the TUI indicator
And backend-specific fields are not required for correctness.

#### Scenario: Malformed voice state is ignored without prompt injection
Given a voice-capable extension emits malformed `voice/state` params
When the host voice bridge handles the notification
Then the host-visible voice state is unchanged
And no daemon prompt event is injected.
