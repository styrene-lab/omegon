---
title: resource.open@1 HostAction (#83)
status: exploring
tags: [0.25, host-actions, resources, routing, viewer]
---

# resource.open@1 HostAction (#83)

## Problem

Extensions and agents need to request “open this resource for the operator” without embedding app-specific routing logic.

Reader should not decide:

```text
markdown -> Flynt
code -> Zed
ebook -> Bookokrat
```

That belongs to the host because it depends on installed apps, active surfaces, workspace trust, operator policy, and runtime mode.

## Goal

Define `resource.open@1` as a semantic HostAction:

```json
{
  "id": "open-resource",
  "type": "resource.open@1",
  "params": {
    "uri": "file:///workspace/docs/architecture.md",
    "intent": "view",
    "kind": "markdown",
    "placement": "main_tab",
    "reuse_key": "docs/architecture.md",
    "title": "Architecture"
  }
}
```

## Proposed params

- `uri`: initially `file://`; future `omegon://` and `https://` optional.
- `intent`: `view | edit | read | inspect`.
- `kind`: `markdown | code | text | diagram | image | ebook | pdf | directory | unknown`.
- `placement`: `main_tab | side_pane | editor | external | default`.
- `reuse_key`: optional stable host reuse key.
- `title`: optional display title.

## Proposed result

- `resource_id`
- `backend`: `flynt | zed | terminal.create@1/bookokrat | system | built_in`
- `actual_placement`
- `handle`
- `warnings`

## Decisions

### Decision: Route through HostAction policy/executor framework

`resource.open@1` should use the same parse/validate/manifest/runtime/approval pipeline as `terminal.create@1`.

### Decision: Do not make Reader a router

Reader may emit `resource.open@1` or declare a delegated reader surface, but host chooses the backend.

## Dependencies

- [[extension-ui-contributions-101]] for surface vocabulary and delegated surfaces.
- [[terminal-background-session-visibility-104]] for honest terminal fallback visibility.
- [[acp-terminal-delegation-87]] for Flynt/native host placement if ACP client is available.

## Open questions

- [assumption] `file://` is enough for the first slice.
- Should edit intent require stricter approval than view/read intent?
- Should markdown default to Flynt only when Flynt is connected, or can standalone Omegon provide a built-in markdown view?
- How should path traversal/trust boundaries be enforced for file URIs?

## Acceptance

- SDK exposes typed `resource.open@1` params/result.
- Manifest policy can allow/deny resource open actions.
- Host validates file URI trust boundary before opening.
- At least markdown/text/code kinds produce deterministic routed or fallback outcomes.
- Reader can request a resource open without knowing Flynt/Zed routing rules.

## Links

- [[0.25-roadmap-extension-surfaces]]
- [[terminal-background-session-visibility-104]]
- [[extension-ui-contributions-101]]
