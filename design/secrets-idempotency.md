# Secrets idempotency and bounded repair

## Problem

Harness-managed secrets have two pieces of state:

- A non-secret recipe in Omegon's recipe store, for example `BRAVE_API_KEY -> keyring:BRAVE_API_KEY`.
- A sensitive value in OS secure storage under Omegon's canonical keyring service.

Those can drift. The observed failure was an orphaned `BRAVE_API_KEY` keychain item with no matching recipe. `secret_set` then failed because secure storage reported that the item already existed, while `web_search` could not resolve it because no recipe declared it.

## Design rule

The recipe store is the source of truth for **intent**. The keychain is value storage. Omegon must repair named harness-managed secrets when the operator names them, but it must not scan or preflight the whole keychain.

Startup reads declarations, not guesses.

## Invariants

For a harness-managed keyring secret named `NAME`:

```text
recipe store: NAME -> keyring:NAME
keychain:     service=sh.styrene.omegon, account=NAME, value=<secret>
```

Operations must be idempotent:

- `secret_set(NAME, value)` upserts the keychain value and ensures the recipe exists.
- `secret_set(NAME, recipe)` overwrites the recipe safely when the recipe type is agent-allowed.
- `secret_delete(NAME)` removes the recipe, same-name keychain entry, process env projection, session cache, and redaction cache. Missing recipe/keychain state is success.
- `secret_list` reports only declared recipes and bounded status for those recipes. It never enumerates keychain contents.

## Startup policy

Do not preflight every well-known secret. Preflight only:

- active LLM provider credentials,
- configured project/extension/MCP secrets,
- web-search keys that already have recipe declarations.

This avoids startup latency and keychain prompt storms while still warming credentials that the session is likely to need.

## Repair behavior

If an orphaned keychain item exists without a recipe, `secret_set(NAME, value)` repairs it by treating the secure-storage write as an upsert. On backends that return duplicate-item errors instead of replacing, Omegon retries as delete-then-set and then writes the recipe.

`secret_delete(NAME)` is the cleanup escape hatch. It deletes both sides for the named secret without trying to discover unrelated keychain items.

## Diagnostics

`secret_list` shows configured recipes with bounded resolution status:

```text
BRAVE_API_KEY: keyring:BRAVE_API_KEY [resolves]
TAVILY_API_KEY: keyring:TAVILY_API_KEY [missing]
VAULT_ROOT_TOKEN: vault:secret/data/root#token [deferred]
```

`deferred` means resolution requires async/external context such as Vault and is intentionally not probed by the synchronous list operation.
