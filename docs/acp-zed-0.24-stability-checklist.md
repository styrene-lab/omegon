+++
id = "acp-zed-0-24-stability-checklist"
kind = "document"
title = "ACP Zed 0.24 Stability Checklist"
status = "exploring"
tags = ["acp", "zed", "release", "stability", "0.24"]
aliases = ["zed-acp-0.24-checklist", "acp-zed-stability"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# ACP Zed 0.24 Stability Checklist

## Purpose

Use this checklist before cutting 0.24.0 to verify that the Zed ACP surface feels like a polished editor-agent integration rather than raw Omegon harness telemetry.

## Plan UX smoke checks

- Start a turn that sets a multi-step plan.
  - Expected: Zed native plan UI shows the checklist.
  - Expected: transcript status is concise, e.g. `Planning mode — edits blocked until approval.`
  - Expected: no literal `_Plan set ..._` markdown artifacts.
  - Expected: the transcript does not duplicate the full plan receipt when the native plan UI already owns the checklist.
- Approve or execute the plan.
  - Expected: status says execution may proceed or plan is executing.
  - Expected: checklist state remains visible through native plan updates.
- Complete, skip, or clear a plan item.
  - Expected: native plan state updates without stale in-progress entries.
  - Expected: transcript receives at most a short progress marker.

## Prompt resource checks

- Mention a file with Zed `@file`.
  - Expected: Omegon receives bounded text context for the file.
- Mention a selection with Zed `@selection`.
  - Expected: Omegon receives exactly the selected text context.
- Mention a directory with Zed `@directory`.
  - Expected: listing is bounded, rooted in the workspace, and excludes binary payloads.
- Try a symlink or traversal escape.
  - Expected: ACP resource handling rejects or suppresses the escape.

## Host capability checks

- Request a host-mediated file write.
  - Expected: Zed permission prompt appears before mutation.
  - Expected: denied writes fail cleanly.
  - Expected: granted writes do not silently bypass host policy.
- Run a terminal-backed command through ACP host capabilities.
  - Expected: output and exit status return without corrupting the editor panel.
- Abort a running turn.
  - Expected: abort is forwarded and rendered cleanly.

## Model/control checks

- Switch model from the Zed ACP model dropdown.
  - Expected: only available providers are listed as selectable.
  - Expected: current unavailable model is labeled stale/unavailable rather than silently dropped.
- Switch thinking and posture.
  - Expected: setting applies to the worker and persists across ACP sessions.

## Release gate

0.24.0 should not claim Zed ACP stability unless the plan UX, prompt resource, host permission, abort, and model-control checks above have been exercised against a current build.
