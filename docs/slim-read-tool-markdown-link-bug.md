---
id: slim-read-tool-markdown-link-bug
title: "Slim read tool Markdown path link bug"
status: deferred
tags: [tui, slim, tool-rendering, bugfix, 0.23.2]
open_questions:
  - "Does `hyperrat::Link` intentionally render a visible terminal affordance glyph in some terminals, or is the orange `i` caused by a width/overlay mismatch in `apply_rendered_links`?"
  - "Should bare path auto-linking be disabled globally, or only for Slim tool rows where the terminal hand cursor does not provide a reliable open action?"
  - "Do explicit `file://` URLs remain useful in assistant output and non-tool content after this change?"
dependencies: []
related:
  - slim-operator-contract
  - tool-card-interaction
  - tool-card-system
---

# Slim read tool Markdown path link bug

## Overview

In Slim mode, an expanded `read` tool row can render a Markdown path with two misleading artifacts:

```text
▸ read
  i design/obsidian-parity-milestone.md
```

Observed behavior:

- A spurious orange `i` appears before the `.md` path.
- Hovering over the `.md` path changes the terminal cursor to a hand, implying it is clickable.
- Clicking the path does not open anything useful in the observed terminal/Zed setup.

This should be treated as a release-polish bug for 0.23.2. The row should render plain, stable text unless the link target is actually actionable.

## Research

### Link detection source

`core/crates/omegon/src/tui/segments.rs` auto-detects links in rendered lines via `detect_links(...)` and reapplies them with:

```rust
hyperrat::Link::new(link.label, link.url)
```

The detector handles explicit schemes:

```rust
https://
http://
file://
```

It also auto-links bare Markdown file paths when they look like paths and end in `.md`:

```rust
let looks_like_path = (label.contains('/') || label.starts_with('.'))
    && label.ends_with(".md")
    && !SCHEMES.iter().any(|scheme| label.starts_with(scheme));
```

When this branch matches, `file_url_for_path(...)` converts the bare path into a `file://...` URL and `apply_rendered_links(...)` overlays a terminal hyperlink on top of the rendered row.

### Why this is bad in Slim tool rows

Slim tool rows are already keyboard-driven. Expanded details are reached through the tool-card focus and `Ctrl+O`; they are not a mouse-first file browser. A terminal hyperlink that changes the cursor to a hand but does not open the file is worse than plain text because it advertises an unavailable action.

The orange `i` is likely tied to hyperlink rendering or overlay width/decoration behavior. Even if the exact glyph source is terminal-dependent, disabling bare path auto-linking for Slim tool rows removes the misleading hyperlink path and should remove the artifact.

### Existing test expectation

`segments.rs` currently has a test named `detects_markdown_file_paths_as_clickable_file_links` that asserts `/tmp/omegon-transcript-20260519.md` becomes a `file://` link. That expectation conflicts with the observed Slim UX.

## Decisions

### Do not auto-link bare Markdown paths in Slim tool rows

**Status:** proposed

**Rationale:** Bare Markdown paths are not reliably actionable in terminal/Zed Slim output. Explicit URLs should remain links, but plain file paths should render as plain text unless a future interaction layer can actually open them.

### Prefer a narrow fix before a hyperlink subsystem rewrite

**Status:** proposed

**Rationale:** The 0.23.2 fix should remove the misleading artifact without changing unrelated rendering. The smallest safe patch is either to remove bare `.md` path auto-linking from `detect_links(...)` or add a flag to disable path auto-linking at Slim tool call sites.

## Implementation Plan

### Option A — simplest global patch

Change `detect_links(...)` so it only detects explicit schemes:

- Keep `https://`, `http://`, and explicit `file://` detection.
- Remove the bare `.md` path detection branch.
- Update the Markdown-path test from positive to negative.

Replacement test:

```rust
#[test]
fn does_not_autolink_bare_markdown_paths() {
    let links = detect_links("Transcript: /tmp/omegon-transcript-20260519.md.");
    assert!(links.is_empty());
}
```

Keep the explicit scheme test:

```rust
#[test]
fn detects_bare_agent_links_without_trailing_punctuation() {
    let links = detect_links("See https://example.com/docs, then file:///tmp/x.");
    assert_eq!(links.len(), 2);
}
```

Pros:

- Minimal code change.
- Removes misleading terminal affordance everywhere.
- Low risk for 0.23.2.

Cons:

- Removes auto-linking for bare Markdown paths outside Slim tool rows too.

### Option B — scoped Slim-only patch

Add an argument or enum to `apply_rendered_links(...)`/`detect_links(...)`, for example:

```rust
enum BarePathLinks {
    Enabled,
    Disabled,
}
```

Call with disabled from Slim tool renderers:

- `render_slim_tool_summary_rows(...)`
- `render_slim_tool_live_rows(...)`

Leave explicit URLs enabled everywhere.

Pros:

- Preserves existing auto-link behavior in non-Slim contexts if desired.

Cons:

- More call-site churn.
- More test cases.
- The only observed behavior is bad, so preserving it may not be worth the complexity.

## Recommended first patch

Use Option A unless there is a known operator workflow that depends on bare `.md` terminal hyperlinks. The observed link does not open, so the feature is currently misleading.

Commit message:

```text
fix(tui): stop autolinking bare markdown paths in slim tools
```

Changelog entry:

```markdown
- **Slim tool rows no longer fake-link Markdown paths** — bare `.md` paths in expanded tool summaries now render as plain text instead of terminal hyperlinks that show a hand cursor without opening.
```

## Validation

Run narrow tests first:

```bash
cargo test -p omegon detects_bare_agent_links_without_trailing_punctuation --bin omegon
cargo test -p omegon markdown --bin omegon
cargo test -p omegon tui --bin omegon
cargo check -p omegon --bin omegon
```

Then run project lint before committing:

```bash
just lint
```

## Open Questions

- Does `hyperrat::Link` intentionally render a visible terminal affordance glyph in some terminals, or is the orange `i` caused by a width/overlay mismatch in `apply_rendered_links`?
- Should bare path auto-linking be disabled globally, or only for Slim tool rows where the terminal hand cursor does not provide a reliable open action?
- Do explicit `file://` URLs remain useful in assistant output and non-tool content after this change?
