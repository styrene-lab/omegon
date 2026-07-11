+++
title = "Modality-aware attachment UX policy"
tags = ["multimodal","ux","attachments","audio","video","image"]
+++

+++
id = "afee60bd-a0fe-4cf4-90a9-be92ed563973"
kind = "design_node"

[data]
title = "Modality-aware attachment UX policy"
status = "exploring"
issue_type = "feature"
priority = 1
parent = "9e253602-b48f-4c34-a787-b12fe57804c5"
dependencies = []
open_questions = []
+++

## Overview

# Modality-aware attachment UX policy

# Modality-aware attachment UX policy

## Overview

Define intentionally asymmetric interaction policies for images, audio, video, documents, and uncommon binary inputs. All are first-class attachments, but their default send strategies differ according to operator intent, payload cost, temporal complexity, and current-route capability.

The product optimizes for the common route rather than the theoretical maximum capability of a model family. Every attachment resolves visibly to one of four states: direct on current route, direct after route switch, usable after an installed transform, or unsupported in the current installation.

## Decisions

### Images are direct evidence

Screenshots and small images use a zero-friction path: paste, preview, and send directly when supported. Bounded orientation correction, decode validation, and provider-limit resizing are core behavior. OCR and specialist interpretation are transforms.

### Voice input and audio evidence are distinct

Voice input produces editable prompt text and does not imply that raw audio belongs to the turn. Audio evidence remains attached because acoustic or conversational content matters. When direct audio is unavailable, transcription is the recommended compatibility bridge, with locality and upload behavior disclosed.

### Video is selection-first evidence

Video intake creates a first-class asset but normally requires a strategy: send original, choose a clip, extract frames, include audio/transcript, or combine derived artifacts. Small videos on directly capable routes may offer direct send prominently, but the transcript must state whether the model received video, still frames, audio, or text.

### UX policy follows route evidence

Conceptual-model marketing claims do not establish support. The current offering/endpoint capability and its evidence govern composer status. Route switching, transforms, and direct transport remain separate choices.

### Delivery truth is visible

After submission, the user turn records exactly which original and derived artifacts were delivered. The assistant must not be presented as having watched a video when it received only still frames or having heard audio when it received only a transcript.

## Default policy matrix

| Modality | Current route capable | Current route incapable | Default |
|---|---|---|---|
| Image | Direct send | Safe conversion or route option | Direct |
| Voice prompt | Transcribe to editable text | Same | Text-first |
| Audio evidence | Direct option | Transcribe or route switch | Transform-first |
| Small video | Direct plus extraction options | Extract or route switch | Explicit strategy |
| Large video | Selection required | Selection required | Evidence-first |
| Document | Direct if supported | Extract text/pages | Direct or extract |
| Unknown binary | Never auto-send | Inspect via extension | Block pending action |

## Composer projections

Image:

```text
▦ screenshot.png · 1440×900 · 612 KB · direct
```

Audio evidence:

```text
♫ meeting.m4a · 03:12 · current route accepts text, not audio
[Transcribe locally] [Switch route] [Remove]
```

Video:

```text
▶ rendering-glitch.mov · 00:18 · 24.7 MB
[Key frames] [Clip range] [Audio transcript] [Original video]
```

Delivered evidence:

```text
Original video retained locally
Sent to model:
  ▦ frame at 00:07
  ▦ frame at 00:12
  ▤ audio transcript
```

## Implementation Details

- Add an intent field or strategy state for audio/video attachments rather than inferring solely from MIME type.
- Keep voice-recording workflow separate from attachment submission state.
- Project route compatibility and recommended actions into the shared editor surface.
- Add a dedicated video evidence-selection surface; do not turn the composer into a video editor.
- Persist delivery manifests linking each turn to original and derived attachment IDs.
- Expose transform locality, expected upload, and approximate output before execution.
- Require explicit confirmation before sending large originals or running remote transforms.

## Constraints

- Common screenshot paste remains no-dialog and low latency.
- Recommendations may use prompt intent but may not silently alter delivered evidence.
- Privacy-sensitive audio/video transforms disclose local versus remote execution.
- Native route support never implies that every frame, sample, or page will be inspected.
- Empty text plus an intentionally configured attachment remains a valid prompt.

## Open Questions

- [assumption] Direct image support is common enough among primary routes to be the default image policy.
- What size/duration thresholds distinguish small direct-send video from selection-required video?
- Should attaching audio trigger a lightweight intent suggestion, or only when accompanying text is ambiguous?
- Which voice-recording implementation is acceptable as a bundled first-party transform?

## Open Questions
