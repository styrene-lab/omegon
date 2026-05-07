+++
id = "dfc1acb8-8304-446e-9a85-987ea5115b81"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Context-Aware OSC 8 URIs — Smart File Links in Terminal Output

## Overview

Terminal file output (view tool, render_diagram, generate_image) includes OSC 8 hyperlinks on headers. Instead of always using file://, route to the best available handler based on file type and what's installed/running.

Core principle: all URI schemes are optional. file:// is always the fallback. If an app isn't installed, the link just opens in Finder — no breakage.

URI routing by file type:
- .md/.markdown → mdserve (http://localhost:PORT/path) when running, else file://
- .excalidraw → obsidian:// if vault detected (Obsidian-Excalidraw plugin), else file://
- .ts/.js/.py/code → editor:// (vscode/cursor/zed per user pref), else file://
- images/PDFs/everything else → file:// (Preview.app)

Detection strategy: check what's available at link-render time, not at startup. mdserve running? Use it. Editor preference set? Use that scheme. Otherwise file://.

## Research

### URI Scheme Availability

Verified URI schemes and their requirements:

**Universal (no install needed):**
- `file://` — opens in default OS handler (Preview for images, Finder for dirs)

**Editor schemes (require app installed + registered):**
- `vscode://file/path:line:col` — VS Code
- `cursor://file/path:line:col` — Cursor  
- `zed://file/path` — Zed

**App-specific:**
- `obsidian://open?vault=name&file=path` — Obsidian (rich: open, new, search actions)
- No native `excalidraw://` scheme — Excalidraw is web-only. Desktop access only via Obsidian-Excalidraw plugin using `obsidian://` URI.
- `figma://file/KEY` — Figma (cloud files only, not useful for local)

**Local server:**
- `http://localhost:PORT/path` — mdserve for markdown. Requires process running. Best rendering for .md with wikilinks, graph view, live reload.

**Key insight:** All non-file:// schemes are strictly additive. file:// always works. The routing layer just picks a better handler when one is available.

## Decisions

### Decision: D1: mdserve is the default markdown handler

**Status:** decided
**Rationale:** mdserve auto-starts on session_start if the binary is on $PATH. All .md/.markdown OSC 8 links route to http://localhost:PORT/path. It's a simple local renderer — no data leaves the machine. /vault command becomes status/config only. If mdserve isn't installed, fall back to file://.

### Decision: D2: Editor and URI preferences in `.omegon/profile.json`

**Status:** decided
**Rationale:** Project-local `.omegon/profile.json` stores editor preference (vscode/cursor/zed) and any URI overrides. Same location as tool profiles (`.omegon/profile.json`). Not coupled to scribe's user_preferences — works for any project.

### Decision: D3: Smooth dynamic install path for mdserve

**Status:** decided
**Rationale:** When mdserve isn't found on $PATH, the agent should be able to install it (cargo install from cwilson613/mdserve or download prebuilt binary). Make the install path frictionless — operators will ask for it. The agent handles the menial install; the operator just says yes.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/view/index.ts` (modified) — Add URI resolver to osc8() — route by file type + available handlers
- `extensions/vault/index.ts` (modified) — Refactor: auto-start mdserve on session_start, /vault becomes status/config
- `.omegon/profile.json` — Project-local config — editor preference, URI overrides
- `extensions/view/uri-resolver.ts` (new) — URI resolution logic — detect mdserve, editor scheme, obsidian vault, fallback to file://

### Constraints

- All URI schemes optional — file:// is always the fallback
- No data leaves the machine — mdserve is local only
- mdserve install must be smooth (cargo install or prebuilt binary)
- OSC 8 links must degrade gracefully in terminals that don't support them
