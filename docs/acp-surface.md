+++
id = "acp-canonical-integration-surface"
kind = "document"
title = "ACP canonical integration surface"
status = "implementing"
tags = ["acp", "integration", "zed", "flynt"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = [
  "Which ACP client-side resource-fetch method, if any, should Omegon call for virtual zed:///agent resources that are not embedded?",
  "Should ACP session resume hydrate full transcript into clients, or should load_session remain a shallow worker attach until transcript replay is designed?"
]
+++

# ACP canonical integration surface

## Position

ACP is Omegon's canonical rich editor/client integration path outside the native TUI. Zed is the primary compatibility target because it is the most complete external ACP client currently in use. Flynt should adopt the same Omegon ACP semantics instead of relying on private behavior, and other clients can interoperate by implementing the same ACP expectations.

This means Omegon's ACP surface must be treated as product API, not a thin demo adapter:

- no hard-coded model lists outside the model registry;
- no silent dropping of ACP content blocks;
- no misleading capability advertisement;
- no Zed-only behavior that prevents other ACP clients from working;
- client-specific URI affordances are compatibility shims layered under general ACP semantics.

## Current compatibility baseline

### Prompt content

Omegon accepts ACP prompt blocks and converts them into the worker prompt. Supported behavior:

- `Text` blocks are included directly.
- `ResourceLink` blocks include metadata and, when resolvable to local text, bounded file contents.
- embedded `TextResourceContents` include embedded text and can dereference local URI contents.
- binary/blob/image/audio blocks are represented as metadata markers unless future provider paths support them directly.
- unknown future ACP block variants produce explicit unsupported markers rather than disappearing.

Resource text injection is content-based, not extension whack-a-mole:

1. reject binary-looking bytes with `content_inspector`;
2. accept text-ish ACP MIME types;
3. accept text-ish `mime_guess` path inference;
4. accept valid UTF-8 as final text fallback;
5. cap injected content at 50KB / 2000 lines.

This is intentionally ecosystem-friendly for Flynt/Eidolon/Styrene formats (`.canvas`, `.flow`, `.d2`, `.pkl`, `.excalidraw`, etc.) without enumerating every file type.

### Zed mention model

Zed uses two forms relevant to Omegon:

- `ResourceLink(name, uri)` in drafts/tests and some context paths;
- `Resource(TextResourceContents { uri, text })` for user mentions, including `@file`.

Zed file mentions normally serialize as `file:///absolute/path`. Zed directories serialize as `file:///absolute/path/`. Zed also has internal URI forms:

- `zed:///agent/file?path=...`
- `zed:///agent/directory?path=...`
- `zed:///agent/selection?path=...#L...`
- `zed:///agent/symbol/...?...path=...#L...`
- `zed:///agent/diagnostics`
- `zed:///agent/git-diff`
- `zed:///agent/terminal-selection`
- `zed:///agent/merge-conflict`
- `zed:///agent/skill`
- `zed:///agent/pasted-image`

Omegon currently extracts local paths from the file/directory/selection/symbol forms. It intentionally does not pretend virtual Zed resources like diagnostics or git diff are files; those require embedded content or a future client resource-fetch flow.

### Config/session options

The ACP model selector is registry-driven via `ModelRegistry::global()`, with local Ollama discovery prepended and de-duplicated. ACP `SetModel`, `SetThinking`, and `SetPosture` update in-memory settings and persist `.omegon/profile.json`.

ACP startup treats the launch `--model` as a fallback when no profile model exists, not as an unconditional override. This prevents editor launch config from clobbering the user's last model choice.

`_runtime/status` now also exposes `acp.turn.phase` (`idle`, `running`, `cancelling`, or `failed`) and a redacted `acp.turn.last_error`. Reconnecting clients should query this state rather than infer whether a turn is still active from stream timing.

ACP does not currently advertise `load_session`: persisted transcript hydration and replay are not implemented yet, so clients cannot mistake the shallow internal attach path for full resume support.

### Tool/UI behavior

Running tool cards are expandable through the same detail surface as completed cards. Ctrl+O/toggle-pin prefers:

1. explicitly selected tool card;
2. latest running tool card;
3. viewport-focused tool card.


## Tool-call display contract

ACP clients vary in how much of `ToolCall.raw_input` they show in collapsed rows. Zed currently shows the tool call name prominently and hides most input until expansion. Omegon should therefore make the tool-call title itself operator-informative while preserving full structured input in `raw_input`.

Policy:

- `ToolCall::name` should be a compact human label, not only the bare internal tool id.
- `ToolCall::raw_input` remains the canonical structured argument payload.
- labels must be short, whitespace-normalized, and secret-redacted before sending.
- labels should summarize the target/action, not dump JSON.

Examples:

| Tool | Raw target | Collapsed ACP label |
| --- | --- | --- |
| `read` | `{ "path": "docs/acp-surface.md" }` | `read — docs/acp-surface.md` |
| `memory_recall` | `{ "query": "ACP hardening" }` | `memory_recall — ACP hardening` |
| `bash` | `{ "command": "cargo test -p omegon acp" }` | `bash — cargo test -p omegon acp` |
| `edit` | `{ "path": "core/crates/omegon/src/acp.rs" }` | `edit — core/crates/omegon/src/acp.rs` |
| `delegate` | `{ "task": "inspect provider filtering" }` | `delegate — inspect provider filtering` |

This keeps Zed/Flynt collapsed tool streams understandable without requiring each client to reinvent Omegon-specific argument summarization.


### Client expansion affordance

Omegon sends three layers of tool-call information:

1. `ToolCall::title` / visible name — compact operator label for collapsed rows.
2. `ToolCall::raw_input` and `raw_output` — structured detail payloads.
3. `ToolCall::content` — rendered tool artifacts such as text, diffs, terminals, or future rich blocks.

ACP clients should provide an explicit expansion affordance for tool calls that exposes at least `raw_input`, `raw_output`, and `content`. Zed already stores `raw_input_markdown` internally, but as of this review the visible agent panel primarily shows the compact title in collapsed rows. Omegon should not compensate by flooding `content` with full argument JSON unless the client hides that content behind an expander; that would make the stream noisy for clients that render content inline.

Recommended client behavior:

- collapsed row: show compact title;
- focused/expanded row: show raw input, raw output, and content;
- keyboard action: `Toggle Tool Call Details` for the selected tool call;
- mouse action: click/chevron to expand;
- preserve structured input for copy/debug without requiring agents to stringify JSON into titles.

## Known remaining gaps

### Completed: line and symbol slicing

Zed selection and symbol URI fragments (`#L10`, `#L10:20`, and `#L10-L20`) are resolved to bounded line ranges before prompt injection.

### Completed: directory mentions

Resolved directory resources produce a bounded listing rooted in the session workspace, excluding common generated and metadata directories.

### P0: host read fallback clarity

Host delegated file reads can fail even when local reads would succeed. The current path fallback is useful, but the canonical policy should be explicit:

- use host reads for client-owned virtual resources or when local path access is unavailable;
- use local fallback for local files when host read fails;
- preserve exact path diagnostics in errors.

### P1: virtual client resources

Virtual resources (`zed:///agent/diagnostics`, git diff, terminal selection, pasted images, thread/rule/skill) cannot be resolved as local files. Omegon needs one of:

- embedded content from the client;
- an ACP/client resource-read RPC if the protocol exposes one;
- explicit unsupported markers with actionable diagnostics.

No silent fallback to guesses.

### Completed: session load capability honesty

Omegon does not advertise session load support while `load_session` remains shallow. Transcript hydration and replay remain future work, but external clients no longer infer full resume semantics.

### Completed: model availability annotation

The registry-driven model dropdown filters providers through configured or usable unexpired credentials. An active model that becomes unavailable remains visible as `(current, unavailable)` rather than disappearing.

When the host advertises `fs.readTextFile`, Omegon treats host permission denials and ordinary host read failures as authoritative. A direct local fallback is permitted only when the host method is unavailable at runtime and the target is an existing regular file; fallback use and its reason are reported in tool details.

### P1: non-text tool outputs

Tool outputs are currently forwarded to ACP as text blocks. Rich tool outputs (image/resource/blob) need a structured `WorkerEvent` path so ACP clients can render them natively.

## Client contract for Flynt and future ACP clients

Clients that want best behavior should:

1. send user text as `Text` blocks;
2. send local files as embedded `TextResourceContents` when content is available;
3. otherwise send `ResourceLink`/embedded resource URI with a stable local `file://` URI when possible;
4. include MIME type when known;
5. use standard `file://` URIs for files/directories before client-private schemes;
6. embed virtual resource content directly unless/until a client resource-fetch method is standardized;
7. treat Omegon's config options and modes as the canonical runtime control surface.

Flynt should follow this contract rather than introducing a Flynt-private agent prompt format. Flynt-specific resources can still use Flynt URI schemes, but should either embed content or provide a documented resolver path.

## Validation expectations

Every ACP hardening change should include focused tests for the content transformation or state transition it changes. Current useful filters:

```bash
cargo test -p omegon acp
cargo test -p omegon toggle_pin_prefers_latest_running_tool_card
cargo check -p omegon --bin omegon
```

Before linking a Zed/Flynt-consumed binary, register the checkout through the stable launcher instead of writing direct symlinks:

```bash
cargo build --profile dev-release -p omegon
just link
omegon --which
```

For multi-checkout machines, use named channels instead of overwriting `~/.local/bin/omegon`:

```bash
just link acp
OMEGON_CHANNEL=acp omegon --which
```
