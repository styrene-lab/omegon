+++
id = "c81dff72-e890-4884-ab2e-16f0ed433599"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# context-aware-uri — Design

## Spec-Derived Architecture

### uri-resolver

- **URI resolution routes by file type and available handlers** (added) — 7 scenarios
- **mdserve auto-starts on session_start** (added) — 3 scenarios
- **Config file at .pi/config.json** (added) — 3 scenarios
- **OSC 8 links in view tool output** (added) — 2 scenarios

## Scope

In scope:
- New `uri-resolver.ts` module with `resolveUri(absPath)` function
- Config loader for `.pi/config.json` (editor pref, URI overrides)
- Refactor vault extension to auto-start mdserve on session_start
- Wire view tool's `osc8()` to use `resolveUri()` instead of hardcoded `file://`

Out of scope:
- mdserve binary changes (it already works)
- New TUI components
- Changes to render_diagram or generate_image_local (those already produce files in ~/.pi/visuals/ — view tool handles them)

## File Changes

| File | Action | Description |
|------|--------|-------------|
| `extensions/view/uri-resolver.ts` | new | URI resolution: resolveUri(absPath), config loader, editor scheme map, mdserve detection, Obsidian vault detection |
| `extensions/view/uri-resolver.test.ts` | new | Tests for all resolution scenarios |
| `extensions/view/index.ts` | modified | Wire osc8() to use resolveUri(); remove hardcoded file:// |
| `extensions/vault/index.ts` | modified | Auto-start mdserve on session_start; store port in shared state; /vault becomes status/config |
| `.pi/config.json` | new | Project-local config schema: `{"editor": "cursor"}` |

## Key Decisions

- **D1**: mdserve auto-starts if binary on $PATH. Port stored in shared state so uri-resolver can read it at link-render time.
- **D2**: `.pi/config.json` for editor pref. Supported values: `vscode`, `cursor`, `zed`. Unknown values → file:// fallback.
- **D3**: Install path: `cargo install --git https://github.com/cwilson613/mdserve` or prebuilt binary download. Agent handles it when operator asks.

## Editor URI Schemes

```
vscode  → vscode://file/{absPath}:{line}:{col}
cursor  → cursor://file/{absPath}:{line}:{col}
zed     → zed://file/{absPath}
```

## Shared State Key

`mdserve.port` — set by vault extension on auto-start, read by uri-resolver. Absent if mdserve not running.
