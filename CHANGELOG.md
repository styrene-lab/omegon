+++
id = "75315b06-0947-44f3-ba98-90348120509d"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Changelog

All notable changes to Omegon are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/). Versioning: [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.24.1] - 2026-05-25

### Added

- Started the plan-refinement lifecycle: small work plans now have a compatibility wrapper, central mutation action API, session-scoped visible plan projection metadata, read-only `/plan list` surfaces for operators and agents, initial registry projection types, and OpenSpec-owned task checkbox status updates while preserving existing `/plan` snapshot fields.
- Surface voice-capable extension `voice/state` notifications in harness status/footer summaries using extension-reported `state` and `mic_open` only.
- Route `terminal.create@1` execution through a terminal backend registry so visual hosts can satisfy placement requests while portable PTY remains the background fallback.

### Fixed

- Preserve voice transcription `utterance_id` metadata when routing voice-capable extension notifications into daemon prompt events.

## [0.24.0] - 2026-05-25

### Added

- Added `omegon-extension` HostAction SDK types, host action capabilities, typed `ToolResult` actions, `HostProxy::execute_action()`, and `terminal.create@1` protocol structs for extension-side host side-effect declarations.
- Added manifest policy parsing and host-side validation for declarative HostActions, including terminal create permission checks and structured policy outcomes.
- Added native extension HostAction execution through the canonical executor registry, including the `terminal.create@1` backend adapter.
- Preserve HostActions across MCP metadata using `_meta["omegon/hostActions"]` for native extension MCP exposure and MCP-origin tool results.
- Added deny-by-default MCP HostAction metadata handling so MCP-origin actions are preserved, validated, surfaced as outcomes, and never auto-executed without a future explicit policy layer.
- Added an extension `voice` capability flag as the first substrate for push-based local voice notification routing.
- Route voice-capable extension `voice/transcription` notifications into operator-trusted daemon prompt events.
- Route voice-capable extension `voice/state` notifications into daemon prompt events so extensions can report mic/listening/processing/speaking state.
- Added daemon extension notification routing into the interactive agent input stream.
- Added generic extension notification subscriptions and `notify` JSON-RPC handling, allowing daemon/TUI subscribers to receive extension-originated events.
- Added an `ExtensionDaemon` helper for long-lived notification-capable extension processes.
- Added `omegon run action` for bounded JSON action contracts with timeout handling and CI-oriented validation tests.
- Added a `process.spawn@1` host action contract and policy validation for command execution declarations.
- Added `ActionDescriptor` and host-action types to `omegon-traits` so agent-facing outputs can carry structured host requests without coupling to the extension SDK.
- Added OpenSpec/design lifecycle artifacts for host action runtime, terminal creation, extension push notification routing, voice MVP integration tests, and the 0.24.0 release checklist.
- Added embedded runtime defaults for the OpenAI bridge and upstream Codex user-agent version checks for 0.24.x.
- Added plan-history and focus-mode TDD slices covering Slim plan detach and live-tail behavior.

### Fixed

- Detach completed Slim plans so finished checklist snapshots do not keep reappearing at the bottom of the TUI.
- Open focus mode at the live tail so the focused conversation view no longer starts at stale history.
- Improve Codex error formatting for forbidden/missing-scope responses.
- Bridge missing MCP OAuth resource metadata through the Anthropic provider without panics.

### Changed

- Promote extension host actions as the 0.24 release line, with release memory/changelog moved to the 0.24.0 section.

## [0.23.9] - 2026-05-25

### Fixed

- Detach completed Slim plans so finished checklist snapshots no longer stay pinned at the bottom of the TUI after completion.
- Preserve active/incomplete legacy plan progress in the Slim bottom pin while keeping completed plans in history.
