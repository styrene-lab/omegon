+++
id = "2705b4d9-f83a-4df5-9c05-06bf8f1a5976"
tags = ["extensions", "browser", "automation"]
aliases = ["omegon-browser"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omegon Browser Extension

`extensions/omegon-browser` is a native Omegon extension that wraps Vercel's `agent-browser` CLI. It gives Omegon a browser automation surface without moving browser control into the core runtime.

## Shape

- Extension name: `omegon-browser`
- Runtime: native JSON-RPC over stdin/stdout
- External dependency: an installed `agent-browser` binary
- Install path: `omegon extension install omegon-browser`

The extension is intentionally a wrapper. `agent-browser` owns Chrome/Chromium control, daemon lifecycle, session persistence, snapshots, screenshots, and browser-specific policy. Omegon owns tool exposure, config, and workflow composition.

## Install

Install Vercel `agent-browser` using its upstream instructions, then install the extension from Armory:

```sh
omegon extension install omegon-browser
```

For local development, build and link the extension from the source tree:

```sh
cd extensions/omegon-browser
cargo build --release
omegon extension install .
```

The extension manifest points at `target/release/omegon-browser`, which is the normal release output when building from the extension directory.

## Tool Surface

- `browser_status`: checks whether the configured `agent-browser` binary is available.
- `browser_open`: opens a URL, with optional session, auth state, headed mode, React DevTools, and allowed domains.
- `browser_snapshot`: returns a compact accessibility snapshot with interactive refs by default.
- `browser_click`: clicks a selector, locator, or `@ref`.
- `browser_fill`: fills a form field.
- `browser_wait`: waits for one condition: milliseconds, selector, text, URL glob, load state, or JavaScript expression.
- `browser_get`: reads page state from a target.
- `browser_screenshot`: writes a PNG screenshot to disk.
- `browser_batch`: sends a bounded list of `agent-browser` commands through one daemon call.

## Safety Defaults

Browser automation can interact with authenticated sessions. Use per-project `session_name` values and pass `allowed_domains` on tool calls that touch external sites. If `browser_open` does not receive an allowlist, the extension derives a single-domain allowlist from the URL host.

The extension passes:

- `AGENT_BROWSER_ALLOWED_DOMAINS` when an allowlist is available.
- `AGENT_BROWSER_MAX_OUTPUT` to bound returned page content.
- `AGENT_BROWSER_CONTENT_BOUNDARIES=1` to preserve output boundaries.

State files can contain login material. Keep `state` paths out of git and avoid reusing browser sessions across unrelated projects.

## Config

The manifest declares these extension config fields:

- `agent_browser_binary`: executable name or absolute path. Default: `agent-browser`.
- `default_session`: optional session name used when tool calls omit `session_name`.
- `allowed_domains`: optional comma-separated global allowlist.
- `max_output`: maximum output characters. Default: `50000`.

Tool-call arguments override config for the specific invocation.
