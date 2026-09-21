# Verification

Evidence is external in `../omegon-inline-menu-evidence-01/`.

The installed baseline `842efcce` (SHA-256
`e4c2c93d270c17bfc25e3081e893c76f6a40679ff2ab3a3c0426e0826854fa02`)
failed `before-02` after saving and resuming a local session. Its settings capture
shows historical content, composer, and workspace chrome outside the menu.
The earlier `before` run exposed a fixture cleanup assumption about an already
closed seed window; it is retained as a fixture diagnostic. Owned cleanup was
verified. Neither run used paid inference or GUI test windows.

`red-render.log` records three expected failures: history under menus/selectors,
conversation measurement during a borrowed visit, and workspace hit rectangles.
The explicit-fullscreen positive case passed. The first extraction passed all
four focused render cases (`green-render.log`).

Adversarial review identified settings-to-footer synchronization hidden inside
workspace rendering. `red-settings-sync.log` reproduces stale `minimal` after
shared settings changed to `high`. Synchronization belongs in shared frame
preparation so inline labels update without another context event. The captured
fixture also selects high from the borrowed selector and checks the returned
inline composer without another inference request.

Final focused checks pass: all five borrowed-screen regressions and the compact
composer fixture. The fixture now supplies authoritative settings instead of
manually populating stale footer fields. Adversarial review approved the final
extraction and shared synchronization with no remaining blockers.

Frozen debug SHA-256
`ee6f70049c13864578e3b9c0cd87fb2800e5cd35b82ad8bf0791022f896eebd7`
passes `debug-menu-backdrop`: actual saved-session resume, clean Settings/Model/
Thinking backdrops, immediate thinking-setting update, Escape/primary preservation,
Project draft retention through resize, and explicit-fullscreen history. It uses
one local inference request. Existing controls acceptance passes with zero
requests. All 23 Python tests pass, and owned-process cleanup is verified.
`visual-artifacts.json` identifies the PNGs rendered from captured ANSI; these are
not native screenshots. No GUI test windows or paid inference were used.

The preliminary full crate run passed before the shared settings-sync correction.
Final landing gates are rerun against the corrected source before commit; installed
artifact identity and acceptance are recorded externally after commit.

Final landing gates passed on the corrected source: `just test-crate omegon`
with 5,305 passed, zero failed, and 11 ignored across nine targets;
`just clippy-changed --base 842efcce`; `cargo fmt --all --check`; and
`git diff --check`. The crate gate ran serialized with `NO_COLOR` and
`OMEGON_ASCII_GLYPHS` unset.
