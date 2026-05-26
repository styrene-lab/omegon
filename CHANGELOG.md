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

### Added

- Added explicit MCP HostAction policy parsing and manual `needs_approval` outcomes so configured MCP servers can advertise approved action requests without gaining auto-execution.

### Fixed

- Shorten pre-content provider/SSE idle detection from five minutes to 90 seconds, with environment overrides, so stale provider sessions surface as failures instead of apparent hangs.
- Read Codex CLI JWT `exp` claims and refresh OpenAI OAuth tokens five minutes early so adopted CLI credentials do not wait for stale `last_refresh` timestamps before re-authentication.
- Stop promoting persisted OAuth credentials into the parent process environment, so one Omegon session no longer shadows shared auth.json refreshes with a stale per-process token.
- Split well-known secrets into static env credentials and refreshable OAuth session tokens, and only auto-hydrate static credentials into the parent process environment.

## [0.24.2] - 2026-05-25

### Added

- Added OpenSpec-owned task checkbox status updates via `openspec_manage`, with strict numeric task-id matching and ambiguity refusal.

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
- Added host-side voice MVP integration coverage proving fake voice extensions route through the existing daemon event ingress rather than a parallel prompt stream.

### Fixed

- Keep dynamically registered native extension tools visible in the lazy model tool surface after turn 1 so installed extension tools such as `reader_doctor` and `reader_open` can be discovered during an active session.
- Normalize native extension SDK `get_tools` schemas that use `inputSchema` into Omegon's internal tool definitions so installed extensions advertise their tools instead of silently registering zero tools.
- Harden extension tool-result envelope parsing and HostAction policy outcomes.
- Avoid blocking-runtime panics when native HostAction terminal execution starts a local terminal backend from an interactive turn runtime.
- **Completed plans surface in Slim** — completed plan updates now leave the active pinned plan lane clear while keeping a `plan done · view` affordance visible so the last completed plan can be recalled.
- **Completed plans remain recoverable** — completed work plans are now recorded as bounded session state, survive save/resume, and `/plan view` can show the last completed plan even after the active plan has been cleared.
