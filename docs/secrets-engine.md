+++
id = "e126186f-5db3-410e-980a-3cf57010b1a7"
kind = "document"
title = "Secrets Engine — encrypted storage, dynamic resolution, and output redaction"
status = "implemented"
tags = ["secrets", "security", "vault", "keyring", "redaction", "guards"]
aliases = ["secrets-engine"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Secrets Engine

Omegon's secrets engine manages sensitive values across three layers: **storage** (where secrets live), **resolution** (how they're retrieved at runtime), and **protection** (how they're kept out of output). The engine is implemented in the `omegon-secrets` crate (~3100 lines across 8 modules).

## Operator Interface

### `/secrets` command

```
/secrets                    show inventory + usage
/secrets set                open common-secret selector
/secrets set NAME           enter hidden input for a raw value
/secrets set NAME RECIPE    store a recipe such as env:, cmd:, keyring:, file:, vault:
/secrets get NAME           check resolution; never prints the value
/secrets delete NAME        remove a secret
```

### Storage approaches (in order of preference)

**1. Dynamic resolution (cmd:) — always fresh, never stale**

The secret is resolved by running a shell command each time it's needed. The value is never stored — it's fetched on demand. This is the right approach for any token that has a CLI tool:

```
/secrets set GITHUB_TOKEN cmd:gh auth token
/secrets set NPM_TOKEN cmd:npm token get
/secrets set K8S_TOKEN cmd:kubectl config view --raw -o jsonpath='{.users[0].user.token}'
/secrets set GCLOUD_TOKEN cmd:gcloud auth print-access-token
```

**2. Environment variable (env:) — for CI/CD and shell-injected values**

The secret is read from an environment variable at resolution time. Useful when the value is injected by a CI runner, Docker entrypoint, or shell profile:

```
/secrets set AWS_SECRET env:AWS_SECRET_ACCESS_KEY
/secrets set DATABASE_URL env:DATABASE_URL
```

**3. Vault (vault:) — for enterprise secret management**

The secret is fetched from HashiCorp Vault's KV v2 engine. Requires vault.json configuration (see Vault section below):

```
/secrets set PROD_DB_PASS vault:secret/data/production/db#password
```

**4. Keyring aliases (keyring:) — reuse operator-owned values without copying**

The secret is read from another OS keyring entry managed by Omegon. Use this when an external integration expects a conventional secret name, but you want the durable stored item to keep an operator-owned name:

```
/secrets set VAULT_ROOT_TOKEN
/secrets set VAULT_TOKEN keyring:VAULT_ROOT_TOKEN
```

The first command captures the token through hidden input. The second stores only an alias recipe.

**5. Direct value → OS keyring — last resort**

When no CLI, env var, or vault path exists, store the raw value. It goes into the OS keyring (macOS Keychain, Linux Secret Service, Windows Credential Manager), encrypted at rest:

```
/secrets set OPENROUTER_KEY
/secrets set ANTHROPIC_API_KEY
```

This creates a `keyring:NAME` recipe pointing to the OS keyring entry.

### `/login` for provider credentials

Provider API keys have a dedicated flow that handles OAuth, token refresh, and credential storage:

```
/login anthropic    OAuth flow → browser → callback → auth.json
/login openai       OAuth flow → browser → callback → auth.json
/login openrouter   API key prompt → auth.json (encrypted, 0600)
```

Use `/login` for provider keys. Use `/secrets` for everything else.

## Architecture

### Module map

| Module | Lines | Purpose |
|---|---|---|
| `lib.rs` | 271 | `SecretsManager` — top-level orchestrator |
| `recipes.rs` | 199 | `RecipeStore` — persistent recipe definitions in `secrets.json` |
| `resolve.rs` | 459 | Resolution chain: env → recipe (cmd/keyring/file/vault) |
| `store.rs` | 612 | `SecretStore` — AES-256-GCM encrypted SQLite |
| `vault.rs` | 1125 | HashiCorp Vault client with path enforcement |
| `redact.rs` | 163 | Aho-Corasick multi-pattern output redaction |
| `guards.rs` | 204 | Tool guards — block/warn on sensitive file access |
| `audit.rs` | 111 | Append-only audit log of guard decisions |

### Resolution priority

When `resolve("NAME")` is called:

1. **Session cache** — warmed/resolved values for deterministic runtime use
2. **Recipe** — if a recipe exists for NAME, execute it:
   - `env:VAR` → read `$VAR`
   - `cmd:COMMAND` → run shell command, use stdout
   - `keyring:NAME` → read from OS keyring
   - `file:/path` → read first line of file
   - `vault:path#key` → fetch from Vault KV v2
3. **Environment variable** — check `$NAME` directly when no recipe resolves
4. **Well-known env vars** — hydrate provider/search credentials into the redaction set when present

### Well-known secret environment variables

These are automatically detected and added to the redaction set even without a recipe:

```
ANTHROPIC_API_KEY, OPENAI_API_KEY, OPENROUTER_API_KEY, MOONSHOT_API_KEY,
BRAVE_API_KEY, TAVILY_API_KEY, SERPER_API_KEY, FIRECRAWL_API_KEY,
GITHUB_TOKEN, GITLAB_TOKEN, GH_TOKEN,
AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_SESSION_TOKEN,
NPM_TOKEN, DOCKER_PASSWORD, IGOR_API_KEY
```

## Protection

### Output redaction

Every resolved secret value is compiled into an Aho-Corasick automaton. Before any tool result reaches the agent or the conversation view, it passes through the redactor. Secret values are replaced with `[REDACTED:NAME]`.

- Minimum redaction length: 8 characters (avoids false positives on short values)
- Longer secrets are matched first (prevents partial matches)
- Automaton is rebuilt when secrets change
- Zero false negatives — if the exact value appears anywhere in output, it's caught

### Tool guards

Before a tool executes, the guards check if the requested path accesses sensitive files. Guard decisions:

| Pattern | Action | Why |
|---|---|---|
| `.env.production` | **Block** | Production credentials |
| `.ssh/id_*` | **Block** | SSH private keys |
| `.aws/credentials` | **Block** | AWS credentials |
| `.gnupg/` | **Block** | GPG keyring |
| `.vault-token` | **Block** | Vault auth token |
| `vault.json` | **Block** | Vault config with auth |
| `.netrc` | **Block** | Network credentials |
| `.pypirc` | **Block** | PyPI credentials |
| `.p12`, `keystore.jks` | **Block** | Certificate stores |
| `.env`, `.env.local` | **Warn** | May contain secrets |
| `.npmrc` | **Warn** | May contain tokens |
| `.ssh/config` | **Warn** | May reveal infrastructure |
| `.kube/config` | **Warn** | May contain tokens |
| `.docker/config.json` | **Warn** | May contain registry auth |
| `.git/config` | **Warn** | May contain tokens |
| `.pem` | **Warn** | Certificate/key file |

Blocked tool calls return an error to the agent. Warned tool calls execute but log to the audit trail.

### Audit log

Every guard decision (block or warn) is logged to `~/.omegon/audit.jsonl`:

```json
{"timestamp":"2026-03-24T...","tool":"read","path":"/home/user/.ssh/id_ed25519","decision":"block","reason":"SSH private key"}
```

## Vault integration

### Configuration

Create `~/.omegon/vault.json`:

```json
{
  "addr": "https://vault.example.com:8200",
  "auth": {
    "method": "token",
    "secret_name": "VAULT_ROOT_TOKEN"
  },
  "allowed_paths": ["secret/data/myproject/*"],
  "denied_paths": ["secret/data/myproject/admin/*"]
}
```

`auth.secret_name` is optional. When present, Omegon resolves that name through the secrets engine and uses it as the in-memory Vault token. This supports operator flows such as:

```
omegon secret set VAULT_ROOT_TOKEN --stdin
```

without requiring `VAULT_TOKEN` to be exported into every shell.

### Authentication methods

| Method | Config | Source |
|---|---|---|
| Token | `{"method": "token", "secret_name": "VAULT_ROOT_TOKEN"}` | Omegon secret, then `$VAULT_TOKEN` or `~/.vault-token` |
| AppRole | `{"method": "approle", "role_id": "...", "secret_id_key": "..."}` | Secret ID from OS keyring |
| Kubernetes | `{"method": "kubernetes", "role": "..."}` | Service account JWT |

### Security properties

- **Fail-closed**: empty `allowed_paths` → all paths denied
- **Deny overrides allow**: denied paths take precedence
- **Path traversal rejected**: `..`, null bytes, URL-encoded traversal blocked before glob matching
- **No redirects followed**: prevents token exfiltration via crafted 3xx responses
- **Tokens in SecretString**: zeroized on drop, never logged
- **Error bodies sanitized**: token patterns stripped from error messages
- **Child token minting**: cleave operations get scoped, non-renewable tokens with TTL and use limits

### Encrypted local store (SecretStore)

For direct value storage, `~/.config/omegon/secrets.db` uses:

- **AES-256-GCM** encryption at rest
- **Argon2id** key derivation (64MB memory, 3 iterations, 4 parallelism)
- **Key backends**: OS keyring (default), passphrase, or Styrene Identity (future)
- **SQLite WAL mode** for concurrent reads + atomic writes
- **File permissions**: 0600 enforced on creation
- **WAL/SHM files**: also set to 0600

### auth.json

Provider OAuth tokens and API keys are stored in `~/.config/omegon/auth.json`:

- File permissions: 0600 enforced on every write
- OAuth tokens: auto-refreshed when expired (5-minute safety margin)
- API keys: stored with `expires: u64::MAX` (no expiry)
- Provider isolation: each provider gets its own JSON key
