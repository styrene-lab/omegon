# 0.25 Extension Surfaces Design

## Overview

This change captures the 0.25 planning map. It does not implement runtime behavior directly; implementation proceeds in issue-specific branches/changes.

## Design nodes

- `docs/design/0.25-roadmap-extension-surfaces.md`
- `docs/design/extension-ui-contributions-101.md`
- `docs/design/terminal-background-session-visibility-104.md`
- `docs/design/resource-open-host-action-83.md`
- `docs/design/acp-terminal-delegation-87.md`
- `docs/design/voice-control-metadata-98.md`
- `docs/design/tts-agent-mode-100.md`
- `docs/design/sdk-lockstep-contract-102.md`
- `docs/design/sdk-repo-extraction-103.md`

## Decisions

- Treat #101, #104, and #83 as core 0.25.0 candidates.
- Treat #87 as stretch/adjacent unless Flynt terminal delegation becomes required for 0.25.0.
- Treat #98/#100 as 0.25.x voice UX unless release scope expands.
- Treat #102/#103 as SDK stabilization after schema settles, with #103 blocked on #102.
