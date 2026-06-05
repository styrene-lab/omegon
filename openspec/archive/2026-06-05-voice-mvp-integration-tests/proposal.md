# Voice MVP Integration Tests

## Intent

Add autonomous integration coverage for voice push notification routing so the 0.24.0 voice MVP cannot regress without CI catching it.

## Scope

- Fake native extension process tests.
- Voice-capable notification receiver to daemon event bridge validation.
- Non-voice manifest gating validation.
- `voice/state` non-prompt validation.
- Explicit documentation of initialize-negotiation caveat.

## Non-goals

- Real microphone smoke tests.
- Whisper model download or transcription correctness.
- Audible TTS validation.
