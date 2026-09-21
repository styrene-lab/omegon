# Captured TUI acceptance

Run the actual TUI with deterministic local streaming replies before and after shell changes. The runner requires Python 3.11+ and tmux on macOS or Linux. It does not install dependencies or call a paid inference provider.

```sh
just test-tui-captured /tmp/omegon-tui-run-001
```

The recipe builds the current debug executable, then launches it on a private tmux server. Use a new output directory for each changed hypothesis. To exercise a separately built artifact:

```sh
python3 scripts/tui_acceptance.py --binary target/dev-release/omegon --tui inline --ui active --entry om --output /tmp/omegon-tui-run-002
```

The runner uses a temporary workspace and explicit user environment. Before sending the first draft, it opens F2, inspects the current session, switches to Work, and returns to the preserved draft. It submits two prompts through terminal keystrokes, checks for distinct fixture replies, resizes from 120×40 to 90×30, and captures the resulting terminal cells. It invokes `/session-export scrollback`, verifies the saved primary screen contains the second reply, and checks that fullscreen content and terminal mode preferences are restored. It then opens Settings during a gated provider request, switches to the Project browser Work tab, denies a real write-tool permission prompt, verifies that the Work tab is restored, and checks that the denied file remains absent. The isolated profile explicitly requires write approval; temporary paths alone do not require it. A timeout fails the run and retains the last screen. Cleanup closes only the owned tmux server and terminates its process group if terminal closure cannot stop it.

Inspect the numbered `.txt` screens, `omegon.log`, and `manifest.json`. The manifest identifies source revision and dirty files, executable path and SHA-256, process/start identity, capture times and dimensions, hashes, and request count. Keep these artifacts outside Git. The log may contain local fixture prompt/context data.

The fixture contract tests run without tmux or Cargo:

```sh
python3 scripts/tests/test_tui_acceptance.py
```

This foundation covers fresh-session startup, terminal input, streaming completion, second submission, resize, native transcript printing and fullscreen restoration, denied tool execution, project Sessions/Work navigation, draft preservation, approval visibility above the Project browser, and return to the same Work tab. It uses four local provider requests and no paid inference. Add `--stress` to cover gated large streaming output, active-turn cancellation while browsing, `/new`, and a successful subsequent turn using six local requests. Saved-session resume UI, populated work-item execution/evidence drill-down, colors, and terminal-emulator portability require their own scoped checks. The active reconstruction plan is `openspec/changes/tui-project-shell/`.

## Selecting a presentation

For live inline scrollback, add `--streaming` to the PTY command. This scenario
holds the provider stream at five checkpoints before completion. Each checkpoint
requires the earliest delivered lines in primary terminal scrollback, beyond the
visible screen. It checks 160 numbered Unicode lines across two turns and a real
read-tool continuation, resizes from 120×40 to 72×24 during the first stream, and
verifies final ordering without duplicates and compares each complete payload
after removing terminal padding and wrap whitespace. Joined semantic captures and
raw physical rows are retained separately. The fixture uses three local provider
requests. Captures and failures retain the exact checkpoint and artifact identity.

Use `--markdown` instead of `--streaming` to check inline presentation. This local
fixture splits Markdown across transport chunks and holds an unfinished paragraph
while the runner checks physical rows and SGR styles. It exercises headings, emphasis,
inline code, lists, a table, and indented fenced code at 120, 72, and 160 columns.
The checks reject raw delimiters, ordinary words split across rows, missing code
indentation, stale wrapping width, and replay after completion. This complements
the long-stream retention check; preserving payload alone does not prove readable
formatting. Both scenarios use private headless terminals.

Pass `--tui inline|fullscreen --ui active|full` to either the PTY runner or the
operator kit's `prepare` command. `--entry om|omegon` instead exercises the actual
launcher script against the frozen executable without passing layout/detail flags;
set the two expected axes to the entry defaults for observation. The manifest
records those selections. Both entries use the same local provider fixture.

The PTY runner now verifies a prelaunch primary marker, exactly one copy of each
automatically published reply before explicit export, and `/quit` returning to a
shell with alternate screen and mouse capture disabled. Native trials can be run
entirely by the agent with `scripts/tui_native_acceptance.py --interactive-gui --clients <chosen-client> --usability`.
Native GUI trials are opt-in and must be reserved for a dedicated compatibility
session: they can interrupt desktop work. Routine iteration uses the headless PTY
runner. The native driver attempts closure in `finally`, checks whether the owned
window disappeared, and records cleanup separately from recorder/process exit.
Apple Terminal and iTerm windows must still contain only the recorded trial
session; cleanup refuses to close a window containing other tabs or sessions.
A cleanup failure stops the matrix before another client opens. The cleanup
changes have headless contract coverage; they have not been rerun in native GUI
clients following the operator's report of desktop disruption.

See [presentation controls and migration](tui-presentations.md).
