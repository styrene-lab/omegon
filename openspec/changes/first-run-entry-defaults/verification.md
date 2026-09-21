# Verification

Production change: `f8875795` (`fix(startup): retire the first-run posture wizard`).
Test isolation follow-up: `229846f0`; no production behavior changed in that commit.

## Behavioral evidence

- The profile-free non-child `om` probe fails on the preceding binary with
  `fresh startup exposed legacy posture wizard`; the old questionnaire is captured.
- The rebuilt `om` and `omegon` probes pass using real launcher defaults, with no
  UI/detail overrides, no profile files, and no OMEGON_CHILD marker. Both reach the
  editor without setup input, type/clear a draft, create no profile before exit,
  perform zero inference requests, and exit cleanly. Captures show inline/Active
  and fullscreen/Full respectively; layout resolution tests cover these defaults.
- The existing WezTerm window runs the frozen production binary. Its old pane was
  closed gracefully before removing the accidentally created user posture profile
  and the exit-time project snapshot. The native capture shows the editor without
  the wizard. Neither profile exists after startup. Credentials/history remain.
- Capture hashes, binary hash, process identity, geometry, and source revision are
  recorded outside the checkout at
  `/Users/wilson/workspace/styrene-labs/omegon-first-run-evidence-01/`.

## Checks

- `cargo test -p omegon surfaces::layout --locked -- --test-threads=1`: 7 passed.
- `just test-crate omegon`: passed, including 5,150 main-binary tests and integrations.
- `just clippy-changed --base origin/main`: passed, repeated after the test-only fix.
- `python3 scripts/tests/test_tui_acceptance.py`: passed.
- OpenSpec structural validation: passed.

## Test isolation correction

The broad suite exposed an existing permission fixture that inherited the operator's
active user profile and persisted its temporary trust grant there. The test now
creates a project profile under its temporary workspace and asserts the grant is
persisted there. Its focused rerun passed and the user-profile hash was unchanged.
This test-only follow-up was validated separately after the complete crate gate.
The test-written local settings were backed up and removed before native relaunch.

Legacy posture parsing and explicit CLI/profile settings remain supported. This
change removes their automatic first-run promotion; it does not remove the internal
posture engine or change saved-profile precedence. Splash first-launch detection and
existing exit-time session/profile persistence remain in place.
