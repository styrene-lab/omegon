# HostActions Core Runtime

## Intent

Implement Omegon core runtime support for HostActions after the SDK contract from issue #74. This change is the security boundary between extension-declared host side effects and real host behavior.

## Scope

- Parse HostAction capabilities and permissions from extension manifests.
- Extract structured extension tool-result envelopes without breaking existing raw/string outputs.
- Validate HostAction candidates and produce typed `invalid`, `unsupported`, and `denied` outcomes.
- Route declarative tool-result actions and imperative `actions/execute` through one pipeline.
- Add an executor registry seam without implementing `terminal.create@1` process spawning.
- Surface HostActions separately from ordinary tool content in result details/rendering data.

## Non-goals

- Real terminal creation or command spawning. That belongs to issue #76.
- MCP metadata mapping. That belongs to issue #77.
- Rich TUI/ACP action-card UI. This change may expose structured data for rendering, but polished UI is separate.
- Automatic execution for MCP-origin actions.

## Success criteria

- Existing extensions continue to work when returning plain strings, JSON, or `content` arrays.
- Malformed actions never suppress valid ordinary content.
- Both declarative and imperative action paths use identical validation and policy checks.
- No action executes unless manifest permissions and runtime/operator policy allow it.
