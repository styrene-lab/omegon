# Credential readiness and bounded refresh recovery

## Intent

An expired Anthropic token can survive a rejected refresh and remain advertised as usable. Concurrent startup and status paths also repeat refresh requests. Make readiness reflect usable credentials and distinguish rejected grants from temporary transport failures.

## Scope

- Resolve stored, external, and environment credentials consistently without returning known expired OAuth tokens.
- Classify refresh failures with sanitized typed outcomes and coalesce requests per provider and credential generation.
- Suppress terminal retries until credentials change or an operator explicitly retries connection setup.
- Keep ordinary provider inventory read-only and remove cached OAuth fallback after failed re-resolution.
- Preserve existing provider endpoints, credential precedence, and explicit interactive login flows.

## Success criteria

- Local fixtures returning invalid_grant never result in a usable expired token or inference request.
- Concurrent resolutions issue one refresh for the same credential generation.
- A fresh external credential, changed stored credential, or explicit connection retry can recover without restarting.
- Opening connection inventory or starting disconnected performs no unrelated OAuth refresh.
- Logs and surfaced failures contain no credentials or raw provider response bodies.
