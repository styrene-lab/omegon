+++
id = "79a19ea8-3bee-4dba-8f1a-27f90716ff8e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# View & URI

> Inline file viewing with rich rendering, context-aware OSC 8 hyperlinks, and image zoom/scale controls.

## What It Does

The view extension renders files inline in the terminal with format-appropriate handling:

- **Images** (jpg/png/gif/webp): Rendered via terminal graphics protocol (Kitty/iTerm2) with configurable scale
- **SVGs**: Converted to PNG via `rsvg-convert`, then rendered as image
- **PDFs**: Pages rendered as images via `pdftoppm`, with page selection
- **Documents** (docx/xlsx/epub/etc.): Converted to markdown via `pandoc`
- **D2 diagrams**: Rendered to PNG via `d2` CLI
- **Code files**: Syntax highlighted via pi's markdown renderer

Scale controls: `/view file.png large|full|2x|compact` adjusts rendered width. `/zoom` opens the last viewed image in a fullscreen overlay.

URI resolution adds context-aware OSC 8 hyperlinks to file headers:
- `.md` files → mdserve (`http://localhost:PORT/path`) when running, else `file://`
- `.excalidraw` → `obsidian://` if vault detected
- Code files → editor scheme (`vscode://`, `cursor://`, `zed://`) per user preference
- Everything else → `file://`

## Key Files

| File | Role |
|------|------|
| `extensions/view/index.ts` | Extension entry — `/view`, `/zoom`, `/edit` commands, `view` tool, message renderer, scale presets |
| `extensions/view/uri-resolver.ts` | `resolveUri()`, `detectObsidianVault()`, `osc8Link()`, `loadConfig()` |

## Design Decisions

- **mdserve is the default markdown handler**: When mdserve is running, `.md` links route to it for rendered preview. Falls back to `file://` when not running.
- **Editor and URI preferences in `.omegon/profile.json`**: User sets preferred editor; URI resolution respects it at link-render time.
- **Scale presets**: compact=60, normal=120, large=200, full=terminal-width cells. Multipliers (2x, 3x) also accepted.
- **`/zoom` fullscreen overlay**: Opens last viewed image at terminal-filling size. Press Escape/q to close. All image-producing renderers stash their output for zoom access.

## Constraints & Known Limitations

- Terminal graphics require Kitty or iTerm2 protocol support — other terminals get fallback text
- Image dimensions must stay under 8000px for Anthropic API (when images go into conversation)
- SVG rendering requires `rsvg-convert` (librsvg); PDF requires `pdftoppm` (poppler); docs require `pandoc`
- `maxHeightCells` is never read by the renderer — height is derived from width × aspect ratio

## Related Subsystems

- [Dashboard](dashboard.md) — URI helper used for clickable dashboard items
- [Quality & Guardrails](quality-guardrails.md) — bootstrap probes for pdftoppm, pandoc availability
