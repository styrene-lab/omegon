+++
id = "9f9e782e-06e7-403c-9f55-1933a9973d50"
kind = "document"
tags = ["secrets", "security", "vault", "keyring", "acp"]
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
design_docs = []
last_updated = "2026-05-13"
openspec_baselines = []
subsystem = "secrets"
+++

# Secrets

Omegon stores operator credentials through recipes and the OS keyring so secret values do not need to live in prompts, command transcripts, project files, or shell profiles.

## Operator Flows

### Hidden TUI Entry

Use name-only `/secrets set` for raw values:

```text
/secrets set VAULT_ROOT_TOKEN
```

Omegon switches the editor into hidden input mode, stores the value in the OS keyring, and writes a `keyring:VAULT_ROOT_TOKEN` recipe. The typed value is not echoed in the conversation.

`/secrets set` or `/secrets configure` with no name opens the selector for common provider and infrastructure secrets. Selecting a direct-value secret also uses hidden input.

### Recipes

Recipes describe where a secret comes from. They are stored in `~/.omegon/secrets.json`; values are not.

```text
/secrets set GITHUB_TOKEN cmd:gh auth token
/secrets set AWS_SECRET_ACCESS_KEY env:AWS_SECRET_ACCESS_KEY
/secrets set PROD_DB_PASSWORD vault:secret/data/prod/db#password
/secrets set VAULT_TOKEN keyring:VAULT_ROOT_TOKEN
```

Use `keyring:OTHER_NAME` to create aliases without copying values. This is useful when an external tool expects a conventional name such as `VAULT_TOKEN`, but the operator wants to store the durable credential as `VAULT_ROOT_TOKEN`.

### CLI

The CLI supports the same storage model:

```sh
omegon secret set VAULT_ROOT_TOKEN --stdin
omegon secret set VAULT_TOKEN --recipe keyring:VAULT_ROOT_TOKEN
omegon secret list
omegon secret delete VAULT_ROOT_TOKEN
```

Prefer `--stdin` for raw values so credentials do not enter shell history or process listings.

## Vault Token Bootstrap

Vault token auth can load a token from an Omegon-managed secret. Create `~/.omegon/vault.json`:

```json
{
  "addr": "https://vault.example.com:8200",
  "auth": {
    "method": "token",
    "secret_name": "VAULT_ROOT_TOKEN"
  },
  "allowed_paths": ["secret/data/omegon/*"],
  "denied_paths": []
}
```

At startup, Omegon resolves `VAULT_ROOT_TOKEN` through the secrets engine and injects it into the Vault client only in memory. It is not exported into the process environment.

`VAULT_TOKEN` and `~/.vault-token` still work as standard Vault fallbacks when `auth.secret_name` is not configured.

## ACP Methods

ACP clients can manage operator-owned secrets without going through extension manifests:

| Method | Parameters | Returns |
|---|---|---|
| `secrets/list` | `{}` | configured secret names and recipes |
| `secrets/set_value` | `{ "name": "...", "value": "..." }` | stores value in keyring |
| `secrets/set_recipe` | `{ "name": "...", "recipe": "keyring:..." }` | stores recipe only |
| `secrets/check` | `{ "name": "..." }` | whether the secret resolves; never the value |
| `secrets/delete` | `{ "name": "..." }` | removes recipe and best-effort keyring entry |

Extension-specific `extensions/secret_set` remains available for extension onboarding, but generic operator secrets should use `secrets/*`.

## Safety Invariants

- `/secrets get NAME` checks whether a secret resolves. It does not print the value.
- Raw TUI secret entry uses hidden input.
- Recipes contain references only, not values.
- Resolved values are inserted into the redaction set immediately.
- Vault paths are still constrained by `allowed_paths` and `denied_paths`.
- Agent-facing secret tools should not be used as the primary path for operator-entered raw values.

## Related Subsystems

- [Secrets Engine](secrets-engine.md) — implementation details and resolution order
- [Vault Secret Backend](vault-secret-backend.md) — Vault policy model and client behavior
- [Operator Profile](operator-profile.md) — provider authentication status
