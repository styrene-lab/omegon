# Inline and fullscreen TUI

`om` opens a small inline composer with Active detail. Prompts and streamed
responses enter the terminal's normal scrollback. `omegon` opens the fullscreen
workspace with Full detail. Both use the same session, editor, tools, permissions,
Project browser, and runtime.

| Selection | Terminal layout | Detail |
|---|---|---|
| `om` | Inline, up to eight live rows | Active |
| `omegon` | Fullscreen workspace | Full |
| `om --ui full` | Inline | Full |
| `omegon --ui active` | Fullscreen | Active |

`--tui inline|fullscreen` selects layout independently. Each preference resolves
from the command line, then the selected profile, then the entry default. Running
the binary directly uses the `omegon` defaults. Headless commands do not acquire
terminal modes because of these preferences.

Use `/ui` or `/settings ui` to discover both controls. `/ui terminal inline` and
`/ui terminal fullscreen` change the current session's base layout. `/ui active`
and `/ui full` save an explicit detail preference. Ctrl+G cycles Active and Full.
Legacy `om`, `lean`, and `slim` detail values now select Active.

An explicit profile overrides either entry's corresponding default:

```json
{
  "uiTerminal": "inline",
  "uiPresentation": "full"
}
```

Leave a field absent to retain that entry's default. Starting a session with a
command-line override, or saving an unrelated setting, does not save the inferred
layout or detail. Existing explicit Active/Full preferences continue to apply.

F2 opens Project in fullscreen. Menus, reference pickers, inspectors, tutorials,
and permission decisions also use the shared fullscreen widgets. Closing them
returns to the selected base and preserves the draft. Changing the base while a
rich view is open takes effect after closing that view. Mouse capture is disabled
in inline so the terminal can select text; fullscreen retains the mouse preference.

Active scrollback groups each completed run of tool outcomes. Full includes detailed tool
arguments/results and appends labeled reasoning after the answer when available. Images have text and path
references in scrollback. Full detail in inline does not create persistent
workspace panels; requested panels become available in fullscreen.
Mutable plan snapshots remain in Project/Workbench and fullscreen history;
automatic scrollback omits them as the plan changes.

While the model streams, complete lines and stable wrapped rows publish in bounded
batches. You can scroll back through the response before the turn finishes. Only
the unfinished tail and composer remain in the small live area. Running and
publication status sit in the composer frame, below the response. Returning
from Project catches up without replaying
already published output. `/session-export scrollback` is an explicit snapshot and
can intentionally repeat history; it does not reset automatic publication. After
an uncertain terminal write, automatic publication stops to avoid duplication.
The status points to fullscreen history or `/session-export`. Resuming or replacing
a conversation starts a new publication boundary; existing terminal history remains.
If a source rewrite invalidates the position of already streamed text, publication
pauses with a conversation-changed notice. Fullscreen history and explicit export
retain access to the current source.

Assistant replies retain Markdown formatting in scrollback: headings, emphasis,
inline code, lists, fenced code, and tables use the shared terminal presentation.
Prose wraps between words; a token wider than the available row is split at a
Unicode grapheme boundary. New output follows the current terminal width. Rows
already printed into terminal history keep their original wrapping.

An individual text cluster or unfinished Markdown construct larger than the inline
buffer limit stops automatic publication with a text-limit notice. Complete source
text remains in fullscreen history and explicit export. A new conversation clears
that text-limit condition.

See [captured acceptance](tui-captured-acceptance.md) for automated fixture runs and
[terminal compatibility](tui-terminal-compatibility.md) for native client evidence.

Controls use a neutral grey hierarchy: dark panels, lighter selection bars, bright
labels, and quieter descriptions and hints. Connection/settings menus, selectors,
slash suggestions, and command panels share these styles. The composer and
conversation retain the terminal's base background and input colors. These control
roles are separate from Markdown styles and can be overridden by future themes.
