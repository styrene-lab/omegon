# WEB-UI-WORK pre-0.27.0

Notes for the Omegon agent implementing the daemon-hosted **Omegon Web** single-agent SPA.

## Design target

Omegon Web is the daemon-owned single-page web UI for one persistent Omegon runtime. It should work standalone from the daemon and also be the target that Auspex opens or reverse-proxies for persistent daemon agents.

Do not port Ratatui widgets directly. Reuse the existing semantic surface/action seams and add web transport where missing.

## Existing backend seams found

### Embedded web server exists

`core/crates/omegon/src/web/mod.rs` already documents and implements an embedded server shape:

- `GET /`
- `GET /api/startup`
- `GET /api/healthz`
- `GET /api/readyz`
- `GET /api/state`
- `WS /ws`
- optional `WS /acp`

It also has `WebStartupInfo`, `WebState`, auth state, startup/control-plane metadata, daemon status, and instance descriptor projection.

### ACP WebSocket exists

`core/crates/omegon/src/web/acp_ws.rs` already exposes a network ACP transport at `/acp` with token auth, connection limits, and per-connection session workers.

This is useful for ACP clients, but it is not yet the ideal native Omegon Web protocol because the SPA wants current surface snapshots, incremental surface events, and semantic UI actions.

### Semantic surfaces exist

`core/crates/omegon/src/surfaces/mod.rs` states these are shared semantic projections for TUI, ACP, and future clients. Current modules include:

- `activity`
- `command`
- `command_menu`
- `conversation`
- `dashboard`
- `editor`
- `footer`
- `inline`
- `instruments`
- `layout`
- `memory_status`
- `operations`
- `palette`
- `profile`
- `settings`

These are the correct backend contract vocabulary for Omegon Web.

### Semantic UI actions exist

`core/crates/omegon/src/ui_runtime/actions.rs` already defines renderer-neutral inbound actions:

- `SubmitPrompt`
- `SubmitContinuation`
- `CancelActiveTurn`
- `RespondToPermission`
- `RespondToOperatorWait`
- `RunSlashCommand`
- `SetUiPreset`
- `SetSurfaceVisible`
- `SelectConversationSegment`
- `OpenConversationSegmentDetail`
- `CopyConversationSegment`
- `CopyLatestAssistantResponse`

This is the correct action vocabulary for the web client. The missing work is HTTP/WS serialization and routing to the existing action handler.

## Backend calls to prepare for Omegon Web

### Existing / likely reusable as-is

| Call | Status | Notes |
|---|---|---|
| `GET /api/startup` | existing | Use for bootstrap metadata, auth mode, URLs, control-plane state, instance descriptor. |
| `GET /api/healthz` | existing | Liveness only. Do not treat as chat readiness. |
| `GET /api/readyz` | existing | Readiness probe; web should still distinguish runtime/tool/session readiness. |
| `GET /api/state` | existing | Existing full state snapshot; inspect before deciding whether to supersede or wrap. |
| `WS /ws` | existing | Existing bidirectional JSON protocol; inspect protocol before reusing for surfaces. |
| `WS /acp` | existing | ACP transport; useful fallback/integration, not the preferred native SPA surface protocol. |

### Missing or not confirmed implemented

These should be added before/for 0.27.0 if Omegon Web is expected to expose TUI richness directly.

#### `GET /api/web/surfaces`

Return a full renderer-neutral surface snapshot for the current/default session.

Suggested payload:

```json
{
  "schema_version": 1,
  "session_id": "default",
  "surfaces": {
    "conversation": {},
    "editor": {},
    "command": {},
    "command_menu": {},
    "dashboard": {},
    "footer": {},
    "instruments": {},
    "memory_status": {},
    "operations": {},
    "settings": {}
  }
}
```

Use the existing `surfaces::*` projections. Avoid web-specific field names at this layer.

#### `WS /api/web/surfaces/stream`

Stream surface updates and runtime events to the SPA.

Event types should include at least:

- `snapshot`
- `surface_updated`
- `conversation_segment_added`
- `conversation_segment_updated`
- `permission_requested`
- `operator_wait_requested`
- `tool_started`
- `tool_updated`
- `tool_completed`
- `turn_started`
- `turn_completed`
- `runtime_status_changed`
- `session_resumed`
- `error`

This may reuse existing broadcast channels, but the public contract should be surface-oriented rather than raw TUI events.

#### `POST /api/web/actions`

Accept serialized `UiAction` values from `ui_runtime/actions.rs`.

Suggested envelope:

```json
{
  "schema_version": 1,
  "client_id": "browser-tab-id",
  "session_id": "default",
  "action": {
    "type": "submit_prompt",
    "text": "...",
    "attachments": [],
    "queue_mode": "until_ready"
  }
}
```

Required action mappings:

- submit prompt
- cancel active turn
- respond to permission
- respond to operator wait
- run slash command
- set UI preset/surface visibility
- select/open/copy conversation segment
- copy latest assistant response

Note: copy actions cannot directly write to the browser clipboard from the backend. Return copyable text in the action outcome and let the browser perform clipboard write.

#### `GET /api/web/sessions`

List resumable sessions for the daemon.

Needed for browser reload, explicit session switching, and Auspex deep links.

Current `session_router.rs` tracks caller sessions internally, but I did not find a web-facing session listing projection. Add one rather than exposing router internals.

#### `GET /api/web/sessions/{session_id}`

Return session metadata and current surface snapshot for one session.

Needed for URL-addressable sessions.

#### `POST /api/web/sessions`

Create or resume a session.

Should support the default single-agent case without making users understand caller keys.

#### `GET /api/web/capabilities`

Return the explicit chat/web capability descriptor Auspex should consume.

Suggested fields:

```json
{
  "interactive": true,
  "chat": true,
  "hosted_web_ui": true,
  "surface_api": true,
  "supports_tool_approval": true,
  "supports_operator_wait": true,
  "supports_session_resume": true,
  "supports_attachments": true,
  "supports_auspex_proxy": true
}
```

Do not make Auspex infer chat eligibility from open ports.

#### `GET /api/web/launch-context`

Return whether the page was opened directly, through Auspex, or through another proxy if known.

This can be simple initially:

```json
{
  "mode": "direct",
  "proxied_by": null,
  "back_url": null,
  "policy_owner": "omegon"
}
```

Auspex can later inject headers or query params that the daemon validates and projects here.

#### Attachment endpoints

If web composer supports files/images, add explicit upload/attachment calls instead of sending local filesystem paths from the browser.

Potential calls:

- `POST /api/web/attachments`
- `GET /api/web/attachments/{id}`

The existing `SubmitPromptAction.attachments` uses `PathBuf`, which is appropriate for local TUI but not sufficient for browser uploads. Web needs an attachment token/id that the backend maps to a staged file under daemon control.

## Surface-to-web placement

| Existing surface/module | Web placement | Backend readiness |
|---|---|---|
| `surfaces/conversation` | main transcript | projection exists; needs web serialization/snapshot/stream |
| `surfaces/editor` | composer state | projection exists; browser editing still client-owned |
| `surfaces/command` | toasts/modals/command results | projection exists |
| `surfaces/command_menu` | command palette | projection exists |
| `surfaces/dashboard` | right context rail | projection exists |
| `surfaces/footer` | top bar + context rail | projection exists |
| `surfaces/instruments` | diagnostics/current activity rail | projection exists |
| `surfaces/memory_status` | context/memory rail | projection exists |
| `surfaces/operations` | workbench/delegate/cleave panel | projection exists |
| `surfaces/settings` | settings drawer | projection exists; mutation route needs web action mapping |
| `ui_runtime/actions` | `POST /api/web/actions` | action vocabulary exists; web transport missing/not confirmed |
| `tui/permission_lane` | approval cards/banner | formatting exists; should create semantic permission projection if absent |
| `tui/tool_inspection` | inline tool cards + current-tool rail | TUI exists; confirm semantic projection coverage via conversation/activity/instruments |
| `tui/segment_detail` | segment side panel/modal | action vocabulary exists; web projection missing/not confirmed |

## Web-specific deltas not covered by TUI

- Browser reconnect/resume and stale-stream recovery.
- URL-addressable sessions and segment anchors.
- Auspex direct/proxied launch context.
- Browser notifications/tab attention for approval-needed and turn-complete states.
- Browser-safe file uploads instead of `PathBuf` attachments.
- Clipboard actions returned to browser rather than executed server-side.
- Accessibility semantics, focus management, and mobile drawer behavior.

## Implementation warning

Do not build a second bespoke chat protocol if the semantic surfaces/actions can be serialized. The web API should be a transport adapter over the same projection/action model used by TUI and ACP.

## Full frontend support design plan

Status: planning/design. The current backend exposes launch discovery only. Full frontend support requires a web transport layer over the existing semantic projection/action model, not another TUI-shaped protocol.

### Contract principles

- Use `/api/web/*` as the native browser/Auspex API namespace.
- Keep `surfaces::*` and `ui_runtime/actions::*` as the source vocabularies.
- Add web DTO adapters only where Rust projection types are not yet serde-safe or browser-safe.
- Version every public web envelope with `schema_version` or `protocol_version`.
- Keep session id in every snapshot/action/stream envelope, even while only `default` is implemented.
- Make unsupported features explicit in `/api/web/capabilities`; do not imply support from route existence.
- Browser copy and attachments are client-mediated: backend returns copy text or staged attachment ids, never attempts browser clipboard access or accepts arbitrary local paths from the browser.

### Phase 1 — Web surface snapshot

Add `GET /api/web/surfaces` for the current/default session.

Envelope:

```json
{
  "schema_version": 1,
  "session_id": "default",
  "revision": 0,
  "generated_at": "2026-06-25T00:00:00Z",
  "surfaces": {
    "conversation": { "segments": [] },
    "editor": {},
    "command": {},
    "command_menu": {},
    "dashboard": {},
    "footer": {},
    "instruments": {},
    "memory_status": {},
    "operations": {},
    "settings": {}
  }
}
```

Implementation notes:

- Introduce `web/surfaces.rs` as the adapter module.
- Prefer deriving `Serialize` on semantic projection structs where low-risk.
- For projection types with lifetimes, paths, or non-serde enums, add owned `Web*Projection` DTOs converted from semantic projections.
- Start with surfaces that already have clean snapshot producers: dashboard/session, operations/workbench, activity/instruments, command prompt, settings, and conversation segment projection.
- `conversation` must expose stable segment indexes initially; later add stable ids if transcript persistence supports them.
- `editor` is browser-owned for text input; backend projection should expose placeholder/mode/queue constraints, not mirror every keystroke.

Acceptance criteria:

- Route exists and returns `schema_version`, `session_id`, `revision`, and all expected top-level surface keys.
- Existing `/api/state` remains unchanged for compatibility.
- `/api/web/capabilities.surface_api` flips to `true` only after this endpoint is implemented and tested.

### Phase 2 — Web action endpoint

Add `POST /api/web/actions` as the serialized transport for `UiAction`.

Envelope:

```json
{
  "schema_version": 1,
  "action_id": "browser-generated-id",
  "client_id": "browser-tab-id",
  "session_id": "default",
  "action": { "type": "submit_prompt", "text": "...", "queue_mode": "until_ready", "attachments": [] }
}
```

Outcome envelope should reuse `ui_runtime::envelope::UiActionOutcomeEnvelope` and extend only where needed for web copy results:

```json
{
  "protocolVersion": 1,
  "sessionId": "default",
  "actionId": "...",
  "status": "accepted",
  "revisionAfter": 12,
  "message": null,
  "error": null,
  "copyText": null
}
```

Implementation notes:

- Add serde DTOs for action input; convert into internal `UiAction`.
- Route submit/cancel/slash actions through existing command channels where no TUI instance is required.
- For UI-local actions currently implemented only on `App::handle_ui_action`, split handler logic into a renderer-neutral runtime service before wiring web.
- Copy actions must return text in the response. Browser writes clipboard.
- Permission/operator-wait responses need request-id handling; reject mismatched or stale ids.
- Attachments must accept attachment ids only; raw filesystem paths from browser requests are rejected.

Acceptance criteria:

- Submit prompt, cancel active turn, run slash command, permission response, operator wait response, copy latest response, and segment select/detail/copy have explicit tests.
- Unsupported action types return a structured rejected outcome, not 500.

### Phase 3 — Surface event stream

Add `WS /api/web/surfaces/stream`.

Stream events:

- `snapshot`
- `surface_updated`
- `conversation_segment_added`
- `conversation_segment_updated`
- `permission_requested`
- `operator_wait_requested`
- `tool_started`
- `tool_updated`
- `tool_completed`
- `turn_started`
- `turn_completed`
- `runtime_status_changed`
- `session_resumed`
- `error`

Envelope:

```json
{
  "schema_version": 1,
  "session_id": "default",
  "revision": 13,
  "type": "surface_updated",
  "surface": "conversation",
  "payload": {}
}
```

Implementation notes:

- Subscribe to existing `AgentEvent` broadcast channel.
- Translate raw runtime events into surface-oriented events; do not expose raw TUI events as the public contract.
- Send an initial `snapshot` immediately after auth/connection.
- Add heartbeat/ping and reconnect guidance. Browser reconnect should refetch `/api/web/surfaces` when stream revision is stale or unknown.
- Maintain a per-session monotonic revision counter for accepted actions and emitted surface deltas.

Acceptance criteria:

- Stream auth matches current web token model.
- Initial snapshot arrives before incremental events.
- Tool lifecycle and turn lifecycle events are surfaced without requiring `/ws` command-protocol knowledge.

### Phase 4 — Sessions and resume

Add:

- `GET /api/web/sessions`
- `GET /api/web/sessions/{session_id}`
- `POST /api/web/sessions`

Design:

- Initial implementation may expose only `default` plus saved transcript sessions from `session::list_sessions(cwd)`.
- Do not expose `session_router` internals directly; create a `WebSessionSummary` projection.
- `GET /api/web/sessions/{session_id}` returns metadata plus the same surface snapshot envelope as `/api/web/surfaces`.
- `POST /api/web/sessions` supports `{ "mode": "resume", "session_id": "..." }` and `{ "mode": "default" }` before supporting arbitrary new runtime creation.

Acceptance criteria:

- `/api/web/capabilities.supports_session_resume` flips to `true` only after list/get/resume are implemented.
- Unknown sessions return 404 with structured error.
- Browser reload can recover current/default session state without ACP caller keys.

### Phase 5 — Attachments

Add:

- `POST /api/web/attachments`
- `GET /api/web/attachments/{id}`

Design:

- Browser uploads bytes or multipart form data.
- Backend stages files under daemon-controlled project/runtime temp storage.
- Returned attachment ids are the only values accepted by web `submit_prompt` actions.
- Convert attachment ids to internal `PathBuf`s only after validating ownership, expiry, size, content type, and containment.

Acceptance criteria:

- Path traversal and arbitrary local path submission are impossible through web APIs.
- `/api/web/capabilities.supports_attachments` flips to `true` only after upload, retrieval, expiry, and prompt-conversion tests exist.

### Phase 6 — Frontend readiness extras

Add the non-core but frontend-critical semantics after the main transport exists:

- Browser notification/attention hints in stream events for approval-needed, operator-wait, and turn-complete.
- Segment anchors and stable ids for deep links.
- Accessibility/focus metadata where backend state influences modal/banner behavior.
- Auspex launch context enrichment from validated headers/query params, including `proxied_by`, `back_url`, and policy owner.

### Recommended implementation order

1. `web/surfaces.rs` DTOs + `GET /api/web/surfaces`.
2. Serialize/convert core surface modules; add snapshot tests.
3. `POST /api/web/actions` envelope + submit/cancel/slash minimum.
4. Split remaining UI-local action logic into a renderer-neutral action service.
5. `WS /api/web/surfaces/stream` from `AgentEvent` translation.
6. Session list/get/resume.
7. Attachment staging and prompt attachment conversion.
8. Auspex launch enrichment + browser attention metadata.

### Known gaps to resolve before implementation

- Several `surfaces::*` projection structs are not currently `Serialize`; decide per module whether to derive serde or introduce web-owned DTOs.
- `App::handle_ui_action` owns important behavior but is TUI-local. Web actions need a shared runtime action handler rather than constructing a TUI app.
- Conversation segment indexes are not stable across pruning/merge; acceptable for v1 snapshot, but deep links need stable segment ids.
- The existing `/ws` protocol is command/control oriented and broad. It should remain compatibility/control-plane protocol, not become the native SPA surface stream.
