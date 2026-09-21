# Implementation and acceptance evidence

Status on 2026-09-05 (America/New_York): shared TUI implementation, scoped acceptance and
launcher installation are complete. The full installation recipe remains blocked
at catalog refresh by the separately tracked home-identity mismatch. Native GUI trials stopped after
the operator reported desktop disruption. All further checks run headlessly.

## Artifact identity

Final captured debug executable SHA-256:
`4b191c7c98b05fdf391b661be4c5e1d961dbe22ce4188eb8cc09d297ff63ea8d`.
Captures identify planning HEAD `181992fa` plus the dirty source inventory; they
must not be described as captures of a later commit or release binary. Each frozen
kit records its binary and support hashes. Each native trial records the driver,
helper, process/window identity, screenshots, timestamps and recording location.

Evidence is outside Git under `/Users/wilson/workspace/styrene-labs/`.
`omegon-dual-evidence-01/sha256.json` indexes retained gate logs and
`native-matrix.json` indexes the 12 completed native trials with their hashes.

## Red-first findings and fixes

- Launcher marker/literal-argument and legacy-density assertions failed before
  independent entry resolution and migration were implemented.
- Profile assertions failed when unrelated saves persisted inferred preferences
  or invocation overrides replaced explicit preferences. Explicit preference
  fields now remain separate from effective session values.
- Ctrl+G failed its two-level cycle assertion before Full returned to Active.
- Stress trial 02 exposed active Ctrl+C not being consumed by the supervisor.
  Active priority ingress now durably admits interruption and starts cancellation.
- Stress trial 03 exposed publication skipping the first reply after `/new`.
  Both source-replacement owners now invalidate the cursor before subsequent events.
- Cleanup regressions exposed unproven window ownership, missing session guards,
  and a process-cleanup exception skipping window cleanup. The headless regression
  suite passes after the fixes.

Other failed observations were fixture issues: the bare detachment PTY did not
answer cursor-position queries; Full readiness used a compact-only label; Apple
Terminal could preload history instead of showing the first-prompt placeholder;
and an outcome assertion raced the separately admitted lifecycle notification.
Those failures are not counted as product acceptance passes.

## Gates

All Rust tests used the canonical glyph environment: NO_COLOR and
OMEGON_ASCII_GLYPHS unset, OMEGON_NERD_FONT=1. Cargo gates ran serially.

| Gate | Recorded result | Retained log in omegon-dual-evidence-01 |
|---|---|---|
| omegon crate and integration suite via just test-crate | 5,125 unit tests passed, 10 ignored; integration groups passed after the startup ordering fix | omegon-dual-crate-final-03.log |
| TUI suite after final outcome/geometry changes | 1,276 passed, 1 ignored | omegon-dual-tui-final-04.log |
| Clippy, omegon all targets | Passed after the startup ordering fix | omegon-dual-clippy-final-07.log |
| Explicit PTY detachment test | Passed: idle and active under both bases | omegon-dual-detachment-02.log |
| Developer script gate and launcher tests | Passed | omegon-dual-dev-scripts.log; omegon-dual-launcher-final.log |
| Pkl Profile evaluation | Passed | omegon-dual-profile-schema.log |
| TUI Python unittest discovery | 13 passed on Python 3.14.7, including native cleanup contracts | omegon-tui-scripts-final-04.log |
| Standalone fixture contract script | Passed | omegon-dual-fixture-final-04.log |

The final crate gate includes the outcome notices and startup ordering fix. The
repository recipe uses libtest scheduling; the earlier crate gate used one test
thread. Cargo gates ran sequentially. No full-workspace or cross-platform gate is claimed.

## Scenario coverage

| Spec scenarios | Verification owner and result |
|---|---|
| Entry defaults; independent precedence; literal arguments | Resolver/settings and actual-launcher tests; om and omegon captured defaults passed |
| Absent/default-equal preferences; legacy migration | Profile capture/apply tests and Active/Full cycle assertions passed |
| Full detail inline; base change under mounted view | Shared App tests plus both mixed combinations in PTY and Ghostty passed |
| Offset origin; shared preparation; narrow decisions | TestBackend at 40/56/90 columns and short heights; shared composer and decision tests; native choice screenshots inspected |
| Completion/second submission; Project permission round trip; queued decisions | Existing App recovery and queued-decision tests parameterized for both bases; normal four-request captures passed |
| Cancellation under browsing/backlog | Six-request stress capture: gated large streaming reply, draft, Project, denial, active cancellation, reset, subsequent reply and exit passed |
| Primary startup; repeated visits; no fullscreen startup; borrowed resize | PTY history/mode checks and repeated native Project/export visits passed; inline startup code omits fullscreen splash/probe/clear |
| Failed entry/restoration | Every tracked mode operation has injected acquisition/release failures; inactive output guard tests passed. Terminal creation/geometry propagation reviewed, without a dedicated fault-injected TerminalBuffers backend |
| Handoff and shutdown | Existing primary-scope success/failure tests plus real PTY detachment in both bases; ordinary captured exits passed. New native shell-command handoff and every signal were not separately replayed |
| Stable streaming; safe terminal-control text; failed/cancelled outcomes | Automatic publication boundary/control tests and authoritative lifecycle outcome-once tests passed; stress capture verifies cancellation notice |
| Resume without history flood; fullscreen-first history | Cursor attach/boundary tests and mode-independent source retention; native/PTY switches passed. Resume UI is not covered by a fresh native campaign |
| Interruptible backlog; oversized Unicode; zero viewport | Bounded source/record/row/cell work, injected cooperative clock, UTF-8 continuation and zero-geometry tests passed; stress input remained usable |
| Once-only settlement; inactive fullscreen rejection; known non-write; ambiguous write | Cursor settlement and active-owner guard tests passed. Ambiguous delivery is injected at the settlement boundary, not through a physically failing native terminal writer |
| Resize/detail during partial record; replacement generation | Cursor continuation/stale-settlement tests passed; stress reset capture passed |
| Explicit export separation; bounded exit | Separate publication owners and cursor tests; captured primary export, preserved composer anchor and exit passed; exit does not drain an unlimited backlog |
| Quiet invocation; ownership-safe cleanup; cleanup exception | Headless native runner contracts passed; no further native GUI validation after the disruption report |

Unicode tests preserve source text with wide and combining characters; they do not
claim full grapheme shaping for pathological, arbitrarily large combining clusters.
The preparation time limit is cooperative, not a hard real-time deadline.

## Captured matrix

All directories below are siblings of this checkout. Standard trials make four
local fixture requests and verify the denied file is absent. No paid inference is
used. Both entry defaults exercise the real launcher script with a frozen binary.

| Layout/detail | PTY evidence | Native evidence |
|---|---|---|
| Inline/Active (om) | omegon-dual-stress-06: six-request stress pass | omegon-dual-native-inline-04/{ghostty,iterm,kitty,wezterm,terminal}: five passes |
| Fullscreen/Full (omegon) | omegon-dual-pty-full-full-01: pass | omegon-dual-native-full-04/{ghostty,iterm,kitty,wezterm} and omegon-dual-native-full-04b/terminal: five passes |
| Inline/Full | omegon-dual-pty-inline-full-01: pass | omegon-dual-native-inline-full-04/ghostty: pass |
| Fullscreen/Active | omegon-dual-pty-full-active-01: pass | omegon-dual-native-full-active-04/ghostty: pass |

Final screenshots inspected include inline Ghostty and WezTerm composers, kitty
permission choices, iTerm fullscreen permission choices, Apple Terminal exit and
Ghostty inline/Full. Choices remain distinct and readable; inline returns to its
small composer. Instrumentation visible in Full remains the baseline for the
separately planned telemetry retirement.

Apple Terminal do-script appends Return: physical Escape and native bracketed
paste are not verified there. Ghostty resize uses font zoom; WezTerm resize uses an
owned split. Recordings/current-view captures establish their supported actions,
not identical physical key coverage across clients.

## Desktop disruption correction

The matrix completed before the operator asked for quiet testing. Its original
PASS results establish fixture outcomes and recorder exit, **not window closure**.
A terminal can remain open after its child exits. Apple Terminal may also group
tabs, so closing a window without session checks is unsafe.

The native runner now requires --interactive-gui and explicit clients, removes
explicit activation commands, records the trial session, attempts exact cleanup
on success and failure, and checks all Spaces for surviving window/PID pairs.
Shared-window adapters refuse closure if other tabs/sessions are present. A cleanup
failure stops subsequent clients. These changes have headless contract coverage;
no new GUI launch was made to validate them. Previously recorded test identities
were checked for surviving owners; unrelated operator terminals were left alone.

Routine iteration uses the private PTY. It needs no operator keystrokes or visible
terminal windows. Native tests are reserved for a dedicated compatibility session.

## Release installation discoveries

`just link` built and installed both release companions and the `om`/`omegon`
launchers. Launcher byte comparisons and --which confirmed the expected checkout.
The recipe then stopped in catalog installation: the stored home device identity
was 16777231 and the current descriptor reports 16777233, with the same path and
inode. This is a pre-existing installation-state mismatch, not a TUI assertion.
No authority state was deleted or rebound. Recovery is tracked in
[maintenance-home-identity-recovery](../maintenance-home-identity-recovery/tasks.md).

Release PTY trial 01 exposed first-turn route admission racing background
discovery. Trials 02/03 narrowed it: an unsupported provider prefix failed provider
selection, and a distinct offering with a supported prefix was absent from the
initial snapshot. Local manifest refresh ran only after background network
discovery. Debug setup had been slow enough to hide the race. Setup now admits
local manifests and cached evidence before spawning network discovery. Six
inference-runtime tests pass, and `omegon-dual-metadata-pty-01` passes the full
four-request headless sequence with the changed debug build.

The fixture now uses the distinct `openai:omegon-tui-fixture` offering, avoiding
overrides of real bundled model entries. Historical captures retain their original
fixture identity and hashes. Final release verification follows this ordering fix;
the earlier failed release trials are retained as diagnostic evidence.

## Final installed artifact

Last production code commit: `1bd50f61`. The later documentation commits do not
change the installed production code. Release SHA-256: `d9e1044954c139aab01b20ec147a6472ded40377c5cdc4125c243c2912bed7b5`.
`omegon-dual-evidence-01/installed-release.json` records installed launcher bytes,
--which resolution, fallback binary equality and both release acceptance manifests.

Both final release runs passed four local requests, denied-file absence and shell
return: `omegon-dual-release-pty-04` (om, inline/Active) and
`omegon-dual-release-pty-full-01` (omegon, fullscreen/Full). These are headless
captures of the corrected release build, with no GUI launches.

The second `just link` rebuilt and installed both companions and launchers, then
again stopped at the existing catalog home-identity mismatch. Thus binary/launcher
installation passed; the whole link recipe did not. Catalog and subsequent
extension-install steps remain with the separate recovery change. Decorative
telemetry retirement also remains planned. This TUI change is left unarchived.

## Persistent notification follow-up

The installation-recovery pass reproduced hidden startup and `/status` output
caused by merging new system text into already-published records. Persistent
notifications now append centrally. A separate red regression reproduced output
loss after notification retention rolled over; typed pruning now preserves cursor
meaning, partial output and stale-batch rejection with bounded bookkeeping.
Adversarial review checked interleaved removals, boundary changes between events,
synthetic notice offsets and generation changes after the cursor.

Focused red/green logs are `omegon-recovery-status-*`,
`omegon-recovery-rollover-*` and `omegon-recovery-prune-metadata-green.log` in the
external installation-recovery evidence directory. The final Omegon crate gate
passed 5,252 tests (11 ignored); affected-target Clippy passed.

Real debug PTY acceptance delivered all 66 additional `/status` responses across
retention rollover, plus the initial status response. Startup and status then
passed again after supported home recovery using the identical binary hash.
Both sessions had zero inference requests, unchanged auth/profile hashes and
verified process cleanup. The recovered session exposes existing trust blocks for
two contributions rather than silently hiding them. See the contribution-loading
verification document for exact artifact identity and capture paths.

The historical catalog/home-identity blocker above is now resolved by
maintenance-home-identity-recovery; catalog and extension installation recipes
completed. Final release handoff evidence is recorded externally after these
verification notes are committed.

## Live streaming correction

The operator's live reply exposed an incorrect publication policy: the inline
viewport replaced its three-line preview while withholding the answer from
primary scrollback until turn completion. Completed text was retained, but the
operator could not read earlier lines during the response.

The deterministic baseline reproduced the defect against release commit
`bc2f08796c195177e3b85fb516e46ca1dce71465`, SHA-256
`03d1a3131387d41ea4d800d2c24473d335384fe4a630777bf483b9f77d1ee712`.
The fixture delivered 32 numbered long Unicode lines, then held the stream before
completion. Only markers 30 and 31 appeared in the live preview; marker 1 was
absent from primary history. The authority journal had no closed turn. The failed
assertion and owned-process cleanup are retained in
`../omegon-streaming-feedback-evidence-01/before/`, with a summary in
`before-observation.json` and the operator capture in `operator-before/`.

The revised contract requires stable streamed rows in primary scrollback before
completion. The acceptance driver checks five held checkpoints, resizing during
streaming, a completed read tool, and a second turn. It verifies all 160 numbered
lines exactly once and in order. These fixtures use local HTTP requests and a
private tmux server; they create no desktop windows or paid inference requests.

Adversarial source review identified and corrected split terminal-control parsing,
Unicode joins across stripped controls, oversized-cluster stalls, duplicate tool
summaries after detail changes, and restart behind a live publication cursor.
The implementation also omits mutable plan snapshots from automatic history so
they cannot block following answers. Their canonical Workbench/fullscreen state
remains intact. Regression fixtures cover these cases, including explicit recovery
from text-limit and unmapped-rewrite pauses.

The final focused suite passed 29 publication tests, 10 inline integration tests,
and two composer tests. The Python acceptance driver tests also passed. An earlier
run exposed a fixture using a 16-byte budget for the production 64 KiB cluster
limit; the corrected fixture checks the production limit. Another compile caught
a missing type qualification in the restored running-tool preview. Both failed
attempts are retained separately from the final passing logs.

The earlier test executable also waited in macOS `_dyld_start` before any tests
ran, then proceeded without intervention. Its on-disk signature check passed.
That focused run finished naturally; no machine security settings changed. The optional
developer-script sweep was stopped separately during unrelated Cargo filters;
it is not counted as a passed complete gate.

The first Active and Full streaming runs passed marker progress and ordering, but
inspection of their raw captures found lost characters in 159 of 160 Unicode
payloads. Those manifests are insufficient evidence of text fidelity; the finding
is retained in `payload-fidelity-review.json`. A plain tmux probe preserved the
same Unicode under both tested UTF-8 locales. Local Ratatui source identified
extra continuation-cell spaces in `insert_before` output as the cause. The
acceptance driver now checks every complete payload as well as markers. The
interim crate run was stopped while waiting in the existing filesystem-watcher
test because this renderer correction required a new validation run.

Earlier fixture-only failures are also retained: joined soft-wrap captures were
needed to read markers after resize, the new streaming fixture inherited a legacy
permission barrier, and saved alternate-screen captures could not include primary
scrollback. Each correction retains raw geometry captures and scoped cleanup.

The corrected insertion adapter passed 11 inline tests and 29 publication tests,
including an actual Crossterm byte regression that was red before the correction.
Final debug SHA-256:
`520a033255ef3e57ca3810d710a24c1334a251900dfca15dc99b32e1c59e41b2`.
`debug-width-artifact.json` identifies the frozen executable and build source.
The final `width-fixed-active` and `width-fixed-full` captures each preserve all
160 complete Unicode payloads after whitespace normalization, exactly once and in
order. Both pass five checkpoints before completion, resize from 120×40 to 72×24,
and complete two turns with a read-tool continuation through three local requests.

`width-fixed-stress` passes browsing with a preserved draft and published prefix,
explicit export, denied write, cancellation, new-session recovery, and clean shell
return through six local requests. The authority journals confirm every started
turn closed. All three sessions restored terminal modes and cleaned up their owned
processes. No GUI test windows or paid inference were used. See
`acceptance-summary.json` for capture hashes and per-scenario evidence. All 14
Python fixture tests passed. Changed-target Clippy, including the workspace
selected by the lockfile change, passed.

The final serialized `just test-crate omegon` gate passed 5,275 tests with 11
ignored, including authoritative lifecycle recovery and all integration targets.
`just clippy-changed --base bc2f0879`, formatting, and diff checks passed. The final
crate run completed the filesystem-watcher test without intervention. Release
installation and operator-window identity are recorded externally after this
tested source is committed, preserving an exact artifact-to-commit handoff.

## Markdown and word-wrapping follow-up

The next operator capture exposed a presentation gap in that retention-only
verification: assistant Markdown was published as raw strings and ordinary words
were split at cell capacity. The original WezTerm capture and installed artifact
identity are retained in `../omegon-markdown-feedback-evidence-01/`. That window
had grown from 117 to 203 columns after the earlier rows were printed; its existing
hard line breaks also illustrate why new-width checks must examine new output.

The added `--markdown` local-provider scenario failed against installed release
`39094ec59b8a5cc3ec399829100186639083fdbf392d8bddf50e6c60db8c56a3`
with `literal Markdown leaked into rendered output: **` before completion.
`before-markdown/` retains the physical rows, SGR capture, process/build identity,
and failed manifest. A stricter `before-markdown-live/` run also fails while a
long paragraph remains unterminated: completed bold text still appears as raw
syntax in primary history. Both runs cleaned up their owned terminals. This is a
separate regression gate from the 160-line payload test.

The corrected frozen debug artifact is
`ec9a294358aa7c42dcb2114615596f9bace7f0357e53161b6683ff9948675f75`.
`debug-artifact.json` identifies its source diff and `acceptance-summary.json`
indexes the passing Markdown Active, Markdown Full, and streaming Active runs.
Each Markdown run passes four held checkpoints: an unfinished long paragraph
followed by formatted blocks at 120, 72, and 160 columns. Physical rows establish
packed whole-word wrapping and table alignment; SGR captures establish bold
headings/emphasis and underlined inline code. Fenced indentation and source order
remain intact. Streaming Active again preserves 160 Unicode payloads through a
read-tool continuation and two turns. `debug-stress/` passes cancellation,
browsing, draft restoration, denial, export, and session reset through six local
requests. All four passing runs verified cleanup without GUI windows or paid
inference. All 16 Python fixture tests pass.

Nine Rust streaming/projector regressions and the terminal inline-code style
regression pass. The combined TUI run initially passed 1,324 tests and failed 12
glyph/snapshot tests under inherited color/glyph environment overrides. That
failed log remains in `logs/tui-focused.log`; the landing crate run explicitly
unsets those overrides and serializes tests. Read-only adversarial review found
no remaining blockers after corrections to stable row tails, table backpressure,
pending previews, width changes, and shared inline-code styling.

The final `just test-crate omegon` run passed 5,285 tests with 11 ignored across
nine targets. The filesystem watcher and daemon integration completed naturally;
no test process was interrupted to obtain a pass. The first full run failed only
because the oversized-text regression still expected the previous limit message;
`logs/crate-first.log` preserves it. The corrected assertion passed in the final
run. `just clippy-changed --base 4ce9c731` passed after removing a redundant local
binding; `logs/clippy-first.log` preserves that initial diagnostic. This cleanup
does not change rendering behavior; all six native Markdown projector tests also
pass after that cleanup. Formatting and diff checks pass. Installed
release evidence and the operator handoff are recorded externally after commit.

## Uninterrupted response flow follow-up

The operator's next capture showed the transient working/cancellation line between
already published text and the live response tail. The unit regression failed on
that ordering, and the captured local-provider scenario failed against installed
release `2f7f89217c5521f3fd8e035d9abbd22d731bb191990645d6ab4f240d7b41071c`
with `working status interrupts the answer above the composer`. Evidence is kept
in `../omegon-status-feedback-evidence-01/`, including both failures.

Working/cancellation and completed-output publication status now occupy the
composer's bottom border. The regression checks Active and Full at 40 and 80
columns, including publication backlog and completion. Frozen debug artifact
`973027d8df305917699aeba2a3adc888ab44d800351fbaba4deaf7fb7f23bd92`
passes captured Active and Full scenarios at four held checkpoints across 120,
72, and 160 columns. The status stays inside the composer and disappears after
completion. Markdown styling, table alignment, code indentation, and whole-word
wrapping checks remain green. Both runs verify cleanup, with no GUI test windows
or paid inference. All 17 Python fixture tests pass. Read-only adversarial review
found no blockers; fullscreen behavior and existing command-hint precedence are
preserved.

The final serialized `just test-crate omegon` passed 5,286 tests with 11 ignored
across nine targets. `just clippy-changed --base ec730c1d`, formatting, and diff
checks passed. Installed release acceptance and the in-place operator handoff
are recorded externally after committing this tested source.
