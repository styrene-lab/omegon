# omegon-browser

`omegon-browser` is an Omegon extension that exposes browser automation tools backed by Vercel's `agent-browser` CLI.

The extension does not vendor Chromium or the `agent-browser` daemon. Install `agent-browser` separately, then install this extension from Armory:

```sh
omegon extension install omegon-browser
```

For local development, build from this directory and link the extension:

```sh
cargo build --release
omegon extension install .
```

The manifest expects the local release binary at `target/release/omegon-browser`, which matches a normal build from this directory.

## Tools

- `browser_status` checks whether `agent-browser` is available.
- `browser_open` opens a page, optionally with a session, headed mode, auth state, and domain allowlist.
- `browser_snapshot` returns an accessibility snapshot, optimized for interactive refs by default.
- `browser_click` clicks a selector or `@ref`.
- `browser_fill` fills a selector or `@ref`.
- `browser_wait` waits for a selector, text, URL glob, load state, JavaScript expression, or milliseconds.
- `browser_get` reads text, attribute, HTML, value, or visibility from a selector/ref.
- `browser_screenshot` writes a screenshot to a requested path.
- `browser_batch` runs a bounded list of `agent-browser` commands in one daemon call.

## Security

Browser control can operate on logged-in sessions. Prefer per-project `session_name` values and set `allowed_domains` so `agent-browser` blocks unexpected origins and subresources. State files can contain session tokens; keep them out of git.
