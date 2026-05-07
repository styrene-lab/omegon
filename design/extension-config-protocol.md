+++
id = "a7db5e42-0653-40ec-b773-269323c09dec"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Extension config & secret management protocol surface

## Problem

Flynt needs to render extension settings (config + secrets) in its GUI and
send user-provided values to omegon for storage. Secret values must never
touch Flynt's storage or be visible in ACP message logs. Config values are
non-sensitive and stored in plaintext.

## Transport: ACP extension methods

ACP already has `ext_method` (request/response) and `ext_notification`
(fire-and-forget) on both sides of the connection. Methods are prefixed
with `_` on the wire. Both `ClientSideConnection` and `AgentSideConnection`
support them.

All extension management calls use `ext_method` — Flynt sends a request,
omegon responds with a result.

## Protocol methods

### `_extensions/list`

List installed extensions with their manifest metadata, config schema,
secret declarations, and resolution status.

**Request:**
```json
{}
```

**Response:**
```json
{
  "extensions": [
    {
      "name": "vox",
      "version": "0.2.0",
      "description": "Voice and messaging bridge",
      "enabled": true,
      "config_schema": {
        "signal_phone": {
          "type": "string",
          "label": "Signal phone number",
          "required": true,
          "pattern": "^\\+[1-9]\\d{1,14}$",
          "placeholder": "+14155551234",
          "current_value": "+14155551234"
        },
        "webhook_enabled": {
          "type": "boolean",
          "label": "Enable webhook",
          "default": "false",
          "current_value": "true"
        }
      },
      "secrets": {
        "required": [
          { "name": "SIGNAL_AUTH_TOKEN", "resolved": true, "source": "keyring" },
          { "name": "IMAP_PASSWORD", "resolved": false, "source": null }
        ],
        "optional": [
          { "name": "WEBHOOK_HMAC_SECRET", "resolved": false, "source": null }
        ]
      }
    }
  ]
}
```

Notes:
- `config_schema` comes from the manifest's `[config]` section
- `current_value` is read from per-extension config.toml (non-sensitive, plaintext)
- Secret `resolved` is a boolean — omegon checks if the recipe resolves, but
  **never sends the secret value** over ACP
- Secret `source` is the hint ("keyring", "env", "vault", "cmd", or null if missing)

### `_extensions/config_set`

Set a config value for an extension. Stored in
`~/.omegon/extensions/{name}/config.toml`.

**Request:**
```json
{
  "extension": "vox",
  "key": "signal_phone",
  "value": "+14155551234"
}
```

**Response:**
```json
{ "ok": true }
```

Omegon validates the value against the manifest schema (type, pattern, enum
values) before writing. Returns an error if validation fails.

### `_extensions/config_get`

Read all config values for an extension.

**Request:**
```json
{
  "extension": "vox"
}
```

**Response:**
```json
{
  "config": {
    "signal_phone": "+14155551234",
    "webhook_enabled": "true"
  }
}
```

### `_extensions/secret_set`

Store a secret for an extension. Omegon stores it in the OS keychain via
`set_keyring_secret()`. **The value is transmitted once over the ACP pipe
(stdin/stdout between Flynt and omegon on the same machine) and then
immediately stored in the keychain. It never hits disk in plaintext, never
appears in config files, and is scrubbed from any subsequent ACP output by
the redaction engine.**

**Request:**
```json
{
  "extension": "vox",
  "name": "IMAP_PASSWORD",
  "value": "hunter2"
}
```

**Response:**
```json
{ "ok": true, "source": "keyring" }
```

**Security considerations:**
- ACP runs over stdin/stdout of a local child process — not a network socket.
  The pipe is process-local and not observable without ptrace/root.
- Once stored, the value is in the OS keychain (macOS Keychain, Linux
  secret-service, Windows Credential Manager).
- Omegon's redaction engine (`Redactor`) immediately adds the value to its
  Aho-Corasick automaton. Any subsequent ACP output (tool results, assistant
  messages) that accidentally contains the value will be scrubbed to
  `[REDACTED]`.
- The value never appears in `_extensions/list` responses — only the
  `resolved: bool` flag.
- Flynt should clear the value from its input field immediately after sending.

### `_extensions/secret_delete`

Remove a secret from the keychain and recipe store.

**Request:**
```json
{
  "extension": "vox",
  "name": "IMAP_PASSWORD"
}
```

**Response:**
```json
{ "ok": true }
```

### `_extensions/enable`

Enable a disabled extension.

**Request:**
```json
{ "extension": "vox" }
```

**Response:**
```json
{ "ok": true }
```

### `_extensions/disable`

Disable an extension (sets `enabled = false` in state.toml, kills process).

**Request:**
```json
{ "extension": "vox" }
```

**Response:**
```json
{ "ok": true }
```

## Security model

### What crosses the ACP pipe

| Data | Direction | When | Sensitive? |
|------|-----------|------|-----------|
| Config schema | omegon → flynt | `_extensions/list` | No — labels, types, defaults |
| Config values | omegon → flynt | `_extensions/list`, `config_get` | No — user preferences |
| Config writes | flynt → omegon | `_extensions/config_set` | No |
| Secret status | omegon → flynt | `_extensions/list` | No — only `resolved: bool` + source hint |
| Secret value | flynt → omegon | `_extensions/secret_set` | **Yes** — one-shot, over local pipe |
| Secret value | omegon → flynt | **Never** | N/A |

### Redaction guarantees

After `secret_set`, the value is immediately added to the `SecretsManager`
redaction set. Any subsequent text emitted over ACP (assistant messages,
tool output) that contains the value will be replaced with `[REDACTED]`
before it leaves omegon. This includes:

- `PromptResponse` content
- `ToolCall.raw_input` and tool results
- `SessionNotification` text deltas
- Error messages

This means if an extension accidentally echoes a secret in a tool result,
Flynt will see `[REDACTED]`, not the value.

### What Flynt must do

1. Never persist secret values to disk (no config.toml, no ui-state.json)
2. Clear password input fields immediately after `secret_set` succeeds
3. Use `type="password"` for secret input fields
4. Never log or display the value after sending

### What Flynt does NOT need to do

1. Encrypt anything — omegon handles keychain storage
2. Manage secret lifecycle — omegon resolves at extension spawn time
3. Validate secrets — omegon checks resolution; Flynt only shows status

## Flynt GUI rendering

Given the `_extensions/list` response, Flynt renders each extension as:

```
┌─ Vox ──────────────────────────────────────┐
│  v0.2.0  Voice and messaging bridge        │
│                                            │
│  Configuration                             │
│  Signal phone    [+14155551234     ]       │
│  Enable webhook  [✓]                       │
│                                            │
│  Secrets                                   │
│  SIGNAL_AUTH_TOKEN   ● keyring    [Clear]  │
│  IMAP_PASSWORD       ○ missing   [Set...] │
│  WEBHOOK_HMAC_SECRET ○ optional  [Set...] │
│                                            │
│  [Disable]                    [Remove]     │
└────────────────────────────────────────────┘
```

Config fields are rendered dynamically from the schema:
- `string` → text input (with pattern validation)
- `number` → number input
- `boolean` → checkbox
- `enum` → select dropdown (values from schema)
- `text` → textarea

Secret fields show status dots:
- ● green + source label = resolved
- ○ red + "missing" = required but unresolved
- ○ muted + "optional" = optional and unresolved

## Implementation order

1. **Omegon: `ext_method` handler** — route `_extensions/*` methods in `acp.rs`
2. **Omegon: config storage** — read/write `~/.omegon/extensions/{name}/config.toml`
3. **Omegon: secret status** — query `SecretsManager` for resolution status per name
4. **Flynt: ACP client** — add `ext_method` calls to `AcpSession`
5. **Flynt: generic config UI** — render from schema, replace hardcoded Vox component
6. **Extensions: add `[config]` to manifests** — Vox, Scry, Aether declare their fields
