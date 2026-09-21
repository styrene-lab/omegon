# Verification

The initial regression failed because HarnessStatus omitted contribution health:
`/tmp/omegon-recovery-ui-red.log` (expected an explicit empty collection, got no
field). The final focused gate `cargo test -p omegon contribution_health --locked`
passed eight tests: `/tmp/omegon-recovery-ui-green.log`.

The fixtures cover absent skill directories, malformed project maintenance state
with a successful independent user scope, reload recovery, an actual legacy home
device mismatch during extension discovery, malformed extension metadata, and
recovery to an absent extension directory. Shared status tests check the typed
error code, root path, cause chain, and Markdown projection. Notice tests check
single aggregate reporting, duplicate suppression, and recovery. Oversized and
multibyte error messages remain bounded while preserving the root cause.

Per-entry plugin and extension failures now remain visible alongside successful
scope counts. Single-extension replacement updates only that entry's health;
full rediscovery replaces the kind's scope and entry results. No guard is bypassed
or cleared by these diagnostics.

The focused gates emitted only the existing warnings:
the macOS linker unwind-size warning and dependency future-incompatibility notice.

The first real-HOME capture at revision `6291e51e6631994343beb61e99c38f0e53ffe1f5`
exposed an inline publication defect: the new warning merged into a session
notification already marked as published. The native publication regression
reproduced the missing warning (`/tmp/omegon-recovery-notice-red.log`). Contribution
notices now append a separate bounded record; the same regression verifies visible
publication once and duplicate suppression (`/tmp/omegon-recovery-notice-green.log`,
one test passed). The failed capture is preserved outside Git under
`../omegon-installation-recovery-evidence-01/before-home-recovery-01`; auth/profile
hashes stayed unchanged, the journal contained only `session.created`, and owned
PTY cleanup completed. Subsequent corrections and accepted runtime evidence follow.

The corrected startup capture exposed the same merge defect for `/status` output,
which arrives through the generic SystemNotification event. The status regression
failed in `/tmp/omegon-recovery-status-red.log`. The final fix removes generic
notification merging centrally and removes the temporary separate-append API.
The before-home-recovery-02 capture preserves one visible startup notice, unchanged
protected files, zero inference, and no OAuth refresh, but fails status visibility.

Focused validation of the central correction passed all seven inline tests and
all 53 conversation tests (`/tmp/omegon-recovery-status-green.log`), including
/status event publication, direct local notices, duplicate suppression, mutable
plan snapshot replacement, and bounded notification history.

Final debug acceptance passed before and after supported home recovery. Both runs
used source `092cca8c` and binary SHA-256
`abba1003b59527beae3ac3b3ea23d6b2f80ad31eb5b07ff7da2059a2e6061a0c`.
The before run displayed all three blocked user scopes and delivered 66 additional
`/status` responses (67 total) in 2.72 seconds without duplicate or missing output.
The after run loaded 13 user skills and admitted plugin/extension scope discovery.
It correctly exposed separate trust requirements for `auspex-cop` and
`omegon-codescan`; those contributions did not execute. Both runs preserved
auth/profile hashes, recorded only `session.created` in the authority journal,
made zero inference requests, logged no OAuth refresh, and cleaned up all observed
owned processes. These runs do not claim a running native-extension lifecycle
test because trust blocked execution.

The accepted directories are `before-home-recovery-debug-02/` and
`after-home-recovery-debug/` under the external installation-recovery evidence
directory. The earlier debug-driver variable collision is retained separately as
an invalid capture; corrected driver identity and ownership tracking are recorded
in the accepted manifests.

Final source validation: `just test-crate omegon` passed 5,252 tests (11 ignored),
and `just clippy-changed --base 0d50ee5c` passed all affected targets. Logs are
`omegon-recovery-final-omegon-tests.log` and `omegon-recovery-publication-clippy.log`.
Final release installation and capture are the external handoff gate described
in the maintenance recovery verification document.
