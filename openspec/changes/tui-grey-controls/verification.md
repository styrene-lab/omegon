# Verification

Evidence is stored outside Git at `../omegon-grey-controls-evidence-01/`.

The rendered menu regression initially failed because both selected and ordinary
rows had a reset background. Its next run caught a partial-row highlight; explicit
row padding corrected that. The final regression passes at 36 and 80 columns,
checks both selection positions, verifies the previous row returns to the panel
background, and runs final-frame cleanup before checking styles.

The old installed release `413c68d34fec62d70dcdcd05095c5f073843393ccff7efa7db84bd9893dd2e4c`
failed the controls palette check after navigation reached every surface. Two
earlier fixture attempts used outdated command-panel markers; their failure
captures and verified cleanup are retained, and the corrected flow uses `/help
all` for the prose command panel.

Frozen debug artifact `227d0229a174ee9808c29cb0cc33834b64a70b7cd3f3b9dc3e7df852aff395be`
passes inline Active and fullscreen Full controls acceptance. Both runs capture
the empty composer, typed draft, slash suggestions, `/connect` with navigation,
`/settings`, `/think`, command inventory, and the help panel. A 36-column menu
capture verifies narrow selection, then restores the original size. SGR checks
confirm panel 235, selection 240, distinct text roles, selection removal from the
previous row, and terminal-default canvas/input colors. Both runs made zero
provider requests and verified owned-process cleanup without GUI windows. All
19 Python fixture tests pass. See `acceptance-summary.json` for capture provenance.

Read-only adversarial review found no blockers. It checked custom-theme defaults,
explicit panel foreground/background pairs, signal preservation, full-width
selection, and both background-cleanup passes. Clippy initially identified an
identical selector label branch; removing that redundant branch leaves rendering
unchanged. `clippy.log` retains the diagnostic, and `clippy-final.log` passes.

The first complete crate run passed 5,261 tests and failed the pre-existing
placeholder assertion that specifically required DIM plus a reset foreground.
That assertion now checks the neutral hint role and its distinction from input,
while retaining the disappearance, default background, and geometry checks.
The final serialized `just test-crate omegon` passed 5,287 tests with 11 ignored
across nine targets. All seven selector tests pass after the redundant-branch
cleanup. Final changed-target Clippy (`--base e8e1882f`), formatting, and diff
checks pass. The final installed-release acceptance and existing-window handoff
are recorded externally after committing this tested source.
