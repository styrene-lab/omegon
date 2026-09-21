# Verification

Evidence lives outside Git in `../omegon-live-activity-evidence-01/`.

The installed baseline `1251593f6d38c2115054307434bd075ecb28664c54e0e58cc59eaa6869221d01`
failed the held Working capture because activity was absent above the composer.
The inline placement test likewise failed against the old composer-only status.
A separate Full App test reproduced the layout planner discarding the phase row.
Both integration regressions pass after the changes.

Read-only review found and resolved three issues: Full's auxiliary-height veto,
late events replacing Canceling, and an old error resurfacing when loop turn
numbers restart at one. The new RuntimePromptStarted boundary clears only prior
agent transient activity when its runtime identity changes, preserving operator
shell activity and canonical history. The repeated-turn-one regression was
red before that fix. All 12 focused action tests pass, covering phases, current
tools, concurrency, success/error expiry, pending cancellation, authoritative
completion/idle queue recovery, hidden activity, and narrow/sanitized rendering.

Frozen debug `ac1f3248ab116d7c81cabec69c02e8c0e3c6d7988a6d2df5235951dbf63d6549`
passes activity acceptance in inline Active, fullscreen Active, and fullscreen
Full. Each run holds six checkpoints, runs a real bounded bash tool, verifies
its result reaches continuation, checks completion/cancellation, and submits a
recovery turn. Each uses four local requests, zero paid inference, and verifies
owned-process cleanup. SGR captures verify the grey activity surface and tool
summary; inline captures establish placement after the response tail and absence
from native scrollback. The initial tiny final fixture response triggered the
existing text-policy continuation rule; the corrected fixture provides a
substantive completed answer. Earlier failed runs and cleanup evidence remain.

Markdown inline Active also passes four live checkpoints at 120, 72, and 160
columns, including an unfinished paragraph. Styling, word wrapping, and response
retention remain intact with the action area present. All 21 Python fixture tests
pass. `acceptance-summary.json` indexes the manifests; `visual-artifacts.json`
identifies PNG renderings of captured ANSI, which are not native screenshots.

The first full crate run passed 5,274 tests and failed one old assertion expecting
the generic `active turn` label. That assertion now requires the actual Responding
phase and cancellation hint while retaining response/composer visibility and
placement checks. No production change was needed for that assertion.

Final landing gates pass: `just test-crate omegon` with 5,300 passed, zero
failed, and 11 ignored across nine targets; `just clippy-changed --base a611c39c`;
`cargo fmt --all --check`; and `git diff --check`. Tests ran serialized with
`NO_COLOR` and `OMEGON_ASCII_GLYPHS` unset. Installed artifact identity and
release acceptance will be recorded externally after committing this source.
