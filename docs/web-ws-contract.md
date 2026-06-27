# WebSocket Control Contract (`/ws`)

This document is the authoritative contract for the legacy Omegon WebSocket control surface at `/ws`. The newer HTTP/native Web UI contract lives in `docs/web-api.openapi.yaml`; this file covers the message protocol for the compatibility WebSocket.

## Endpoint

```http
GET /ws?token=<web-auth-token>
```

- Transport: WebSocket.
- Authentication: required query token. The server rejects missing or invalid tokens with `401` before upgrade.
- Authorization: each inbound command is classified by `core/crates/omegon/src/control_actions.rs` and checked against the connection/request principal role before it is queued.
- Initial server message: `state_snapshot`.
- Server event messages: JSON text frames derived from agent/runtime events.

## Roles

The legacy `/ws` surface uses `ControlRole` levels:

| Role | Meaning |
|---|---|
| `read` | May view status/snapshots/read-only data. |
| `edit` | May submit prompts, mutate runtime settings, and perform non-admin operations. |
| `admin` | May switch providers/dispatchers, manage auth/vault-sensitive operations, or shut down. |

If a message carries `caller_role`, accepted values are `read`, `edit`, and `admin`. If omitted, the server uses the configured web principal role for the connection. Unknown caller-role labels degrade to `read`; they do not escalate.

## Common inbound envelope

Every client command is a JSON object with a `type` field:

```json
{
  "type": "model_list",
  "caller_role": "read"
}
```

`caller_role` is optional. Specific command payload fields are listed below.

## Inbound commands

| `type` | Required role | Payload | Effect | Response |
|---|---:|---|---|---|
| `request_snapshot` | read | none | Sends a fresh state snapshot. | `state_snapshot` |
| `user_prompt` | edit | `text: string` | Queues user prompt text for the agent. | Agent event stream |
| `cancel` | edit | none | Cancels the current turn. | Agent event stream |
| `slash_command` | classified per slash command | `name: string`, `args: string` | Executes a remote-safe slash command. Local-only slash commands are rejected. | `slash_command_result` |
| `model_view` | read | none | Reads current model/provider state. | `control_result` |
| `model_list` | read | none | Lists available models/providers. | `control_result` |
| `set_model` | edit/admin | `provider?: string`, `model: string` | Same-provider tuning is edit; provider switch is admin. | `control_result` |
| `switch_dispatcher` | admin | `dispatcher: string` | Switches dispatcher. | `control_result` |
| `set_thinking` | edit | `thinking: string` | Sets thinking level. | `control_result` |
| `context_status` | read | none | Reads context status. | `control_result` |
| `context_compact` | edit | optional args | Requests context compaction. | `control_result` |
| `context_clear` | admin | optional args | Clears context. | `control_result` |
| `skills_view` | read | none | Lists skill state. | `control_result` |
| `skills_install` | edit | optional args | Installs bundled skills. | `control_result` |
| `plugin_view` | read | none | Lists plugin status. | `control_result` |
| `plugin_install` | edit | plugin fields | Installs plugin. | `control_result` |
| `plugin_remove` | edit | plugin fields | Removes plugin. | `control_result` |
| `plugin_update` | edit | plugin fields | Updates plugin. | `control_result` |
| `auth_status` | read | none | Reads auth status. | `control_result` |
| `auth_login` | admin | provider/login fields | Starts/records auth login flow. | `control_result` |
| `auth_logout` | admin | provider fields | Logs out/removes auth state. | `control_result` |
| `secrets_view` | edit | none | Lists configured secret metadata/readiness. | `control_result` |
| `secrets_set` | edit | secret fields | Sets secret recipe/value. | `control_result` |
| `secrets_get` | edit | secret name | Reads secret material through the control executor. | `control_result` |
| `secrets_delete` | edit | secret name | Deletes secret material/configuration. | `control_result` |
| `vault_status` | read | none | Reads external vault status. | `control_result` |
| `vault_unseal` | admin | vault fields | Performs vault unseal operation. | `control_result` |
| `vault_login` | admin | vault fields | Performs vault login operation. | `control_result` |
| `vault_configure` | admin | vault fields | Configures vault integration. | `control_result` |
| `vault_init_policy` | admin | vault fields | Initializes vault policy. | `control_result` |
| `cleave_status` | read | none | Reads cleave/workstream status. | `control_result` |
| `cancel_cleave_child` | edit | `label: string` | Cancels a cleave child by label. | `control_result` |
| `delegate_status` | read | none | Reads delegate status. | `control_result` |

Unknown `type` values are ignored after debug logging.

## Slash command rules

For `slash_command`, authorization and remote-safety are derived from `classify_remote_slash_command(name, args)`:

- Remote-safe read examples: `/cleave status`, `/delegate status`, prompt preview/read commands.
- Remote-safe edit examples: `/cleave cancel <label>`, same-provider model tuning.
- Local-only examples: skills install/update/delete, prompt create/update/delete, plugin install/update/remove, provider switch, login/logout, and other host-affecting operations unless explicitly marked remote-safe.

If a slash command is local-only, the server sends a system message instead of queueing it.

## Server messages

### `state_snapshot`

Sent immediately after upgrade and in response to `request_snapshot`.

```json
{
  "type": "state_snapshot",
  "event_name": "state.snapshot",
  "name": "state.snapshot",
  "data": {}
}
```

`data` is the dashboard snapshot built by `web::api::build_snapshot`.

### `control_result`

Control-executor response for most command messages.

```json
{
  "type": "control_result",
  "event_name": "control.result",
  "name": "model_list",
  "accepted": true,
  "output": "..."
}
```

`output` is HTML-escaped before emission.

### `slash_command_result`

Response for slash commands that use the slash-specific path.

```json
{
  "type": "slash_command_result",
  "event_name": "slash.command.result",
  "name": "model",
  "args": "list",
  "accepted": true,
  "output": "..."
}
```

`output` is HTML-escaped before emission.

### `system_message`

Used for authorization and remote-safety denials.

```json
{
  "type": "system_message",
  "role": "system",
  "message": "caller role is insufficient for user_prompt"
}
```

## Security notes

- `/ws` is a legacy compatibility surface. Prefer the native HTTP/session/action/surface APIs in `docs/web-api.openapi.yaml` for new Web UI work.
- Token auth happens once at WebSocket upgrade; per-message authorization still gates command execution.
- `caller_role` is not an escalation mechanism. Omitted roles use the configured web principal; unknown roles degrade to read.
- Secret and vault methods are remotely executable on this surface only because `/ws` already exposes concrete command arms for them; they remain protected by edit/admin role requirements.
- New `/ws` command types must be added to `control_actions.rs` with explicit role and remote-safety classification before use.
