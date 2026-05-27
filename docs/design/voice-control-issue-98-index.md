+++
id = "voice-control-issue-98-index"
title = "Issue 98 design index"
status = "exploring"
parent = "voice-control-metadata-tts-lifecycle"
issue_type = "index"
priority = 1
openspec_change = null
tags = ["extensions", "voice", "index", "0.24", "issue-98"]
aliases = ["issue-98-design-index"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Issue 98 design index

Design nodes for #98:

- [[voice-control-metadata-tts-lifecycle]] — parent scope and issue framing.
- [[voice-transcription-control-metadata]] — metadata preservation across bridge/TUI/prompt queue.
- [[voice-over-and-out-shutdown]] — deterministic `close_session_requested` handling.
- [[voice-tts-lifecycle-contract]] — spoken-output status contract.
- [[voice-control-tests-issue-98]] — deterministic host-side test plan.

Related prior nodes:

- [[voice-mvp-integration-tests]] — #81 voice bridge trust-boundary tests.
- [[extension-event-adapter-anchors]] — canonical daemon ingress invariant.
