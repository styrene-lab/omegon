+++
id = "9f9e782e-06e7-403c-9f55-1933a9973d50"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Secrets

> Secure API key and credential management with selector-backed hidden input, 1Password integration, and shell command evaluation.

## What It Does

The secrets extension manages provider API keys and credentials needed by Omegon's model routing layer. It supports three input modes:

1. **Selector + hidden input**: Run `/secrets configure` or `/secrets set`, pick a known secret, then paste into hidden input mode so the value is never echoed in the transcript
2. **1Password references**: Store `op://vault/item/field` references that resolve at runtime via 1Password CLI
3. **Shell command evaluation**: Store `$(command)` patterns that evaluate at runtime (e.g., `$(aws secretsmanager get-secret-value ...)`)

Secrets are stored in `~/.config/omegon/auth.json` and the configured Omegon secrets backend, with mode-appropriate handling. The extension probes for clipboard commands (`pbpaste`, `xclip`, `xsel`, `wl-paste`) at runtime.

## Slash Command UX

- `/secrets` — inspect configured secrets
- `/secrets set` — open the selector of common secret names
- `/secrets configure` — alias for the same selector-backed flow
- `/vault` or `/vault status` — inspect Vault connectivity
- `/vault configure` — open an interactive selector that primes either `/vault configure env` or `/vault configure file`

Direct-value secrets switch the editor into hidden input mode so pasted credentials do not appear on screen. Dynamic recipes such as `GITHUB_TOKEN -> cmd:gh auth token` are applied immediately after selection.

## Key Files

| File | Role |
|------|------|
| `extensions/00-secrets/index.ts` | Extension entry — `/secrets` command, `promptForSecretValue()`, `detectClipboardCommand()`, `readClipboard()` |

## Design Decisions

- **Hidden editor input over transcript-visible entry**: the TUI now routes direct secret entry through hidden editor state so paste works without exposing the value in the conversation log.
- **Selector-backed common secret names**: `/secrets set` and `/secrets configure` both open the known-secret selector to reduce typing and avoid malformed command entry.
- **Interactive vault setup**: `/vault configure` primes specific follow-up commands instead of dumping instructions only.
- **Fallback to direct input with warning**: If no clipboard command is available, falls back to `ctx.ui.input()` with a security warning.
- **Non-secret inputs use standard input**: 1Password references and shell commands (not actual secrets) still use `ctx.ui.input()`.

## Constraints & Known Limitations

- Clipboard-based input requires `pbpaste` (macOS), `xclip`/`xsel` (Linux X11), or `wl-paste` (Wayland)
- `ExtensionUIDialogOptions` supports only `signal` and `timeout` — no `secret`/`password` field
- Secrets state is Omegon-owned under `~/.config/omegon/` and the configured backend, not per-project

## Related Subsystems

- [Operator Profile](operator-profile.md) — provider authentication status
- [Model Routing](model-routing.md) — consumes API keys for provider access
