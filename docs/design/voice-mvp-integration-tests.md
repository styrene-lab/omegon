+++
id = "voice-mvp-integration-tests"
tags = ["extensions", "voice", "testing", "0.24", "issue-81"]
aliases = ["issue-81-voice-mvp-tests"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Voice MVP integration tests — Issue 81

## Overview

Issue #81 hardens the voice MVP by adding deterministic host-side integration coverage for push notification routing without depending on microphone access, macOS TCC prompts, Whisper models, or audible TTS.

The already implemented slices cover the components independently:

- Extension transport can capture JSON-RPC notifications into `voice_notification_rx`.
- `voice_bridge` can convert synthetic `voice/transcription` notifications into trusted local `DaemonEventEnvelope` prompt events.

The remaining gap is combined-path coverage: fake extension process → notification receiver → voice bridge → daemon event queue.

## Current evidence

- `voice_bridge` unit tests assert valid transcription conversion, malformed/empty handling, and `voice/state` non-routing.
- `voice_capable_extension_notification_does_not_break_get_tools_response_matching` asserts a fake voice-capable extension can emit a notification before the `get_tools` response without breaking response matching.
- `omegon-voice` local validation confirms the extension registers five `voice_*` tools and loads `models/ggml-base.en.bin` successfully when the model is present.

## Test design

### Test 1: fake voice extension injects daemon event through bridge

Create a temporary native extension script with manifest:

```toml
[capabilities]
voice = true
```

The script emits one `voice/transcription` notification before its `get_tools` response:

```json
{
  "jsonrpc": "2.0",
  "method": "voice/transcription",
  "params": {
    "text": "summarize the current project",
    "utterance_id": "test-u1",
    "duration_s": 1.2
  }
}
```

Host test flow:

1. `spawn_from_manifest(temp_dir, &[])`
2. take `spawned.voice_notification_rx`
3. start `voice_bridge::start_voice_bridge(rx, daemon_tx, cancel)`
4. read one event from daemon channel
5. assert event fields:
   - `source = "voice"`
   - `trigger_kind = "prompt"`
   - `source_channel = "voice"`
   - payload text preserved
   - payload utterance_id preserved
   - payload duration_s preserved
   - payload extension_name preserved
   - payload trust_level is `operator`
   - caller role is `edit`

### Test 2: non-voice extension cannot inject daemon prompt

Use the same fake extension script but omit `capabilities.voice = true` from the manifest. The extension may still emit `voice/transcription`, but host startup must not create `voice_notification_rx`, so no bridge can attach and no daemon prompt event can be injected.

Assertions:

- `spawned.voice_notification_rx.is_none()`
- registered tools still work if provided, proving the extension is otherwise usable

### Test 3: voice/state does not inject daemon prompt

Use a synthetic receiver/channel or fake extension emitting `voice/state`. Start the bridge and assert no event arrives before a short timeout.

### Test 4: capability intersection caveat

Current installed-extension startup uses manifest capabilities plus `get_tools`; it does not appear to perform initialize-response capability negotiation for native extension discovery. Therefore, the exact #81 requirement “manifest declares voice but initialize response has voice=false” is not testable against this path until the host adopts initialize negotiation for extension startup.

Decision: record this as a scoped caveat for 0.24.0 rather than silently claiming coverage. If 0.24.0 requires initialize negotiation, create a separate design/implementation issue; otherwise close #81 with evidence that manifest-gated voice routing prevents non-voice impersonation on the current startup path.

## Acceptance criteria for closing #81

- Host-side tests run autonomously in CI with no mic/TTS/model dependency.
- Combined fake-extension → notification receiver → voice bridge → daemon event test passes.
- Non-voice fake extension cannot create a voice notification receiver.
- `voice/state` produces no daemon prompt event.
- #81 comment records the initialize-negotiation caveat explicitly.

## 0.24.0 targeting posture

The 0.24.0 release should treat voice notification routing as a tested local-operator input path, not a best-effort extension experiment. Tests should encode the trust boundary: only voice-capable extensions get a bridge, and only `voice/transcription` becomes a prompt.
