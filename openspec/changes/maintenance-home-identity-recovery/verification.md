# Verification

The observed installation recovered through the supported debug companion built
from `f1dc5db6`. Dry-run succeeded under the inherited macOS soft descriptor limit;
apply settled request `ef22d4f3-4388-4383-9ec2-540d3a893dfa`. Subsequent inspection
and audit verification succeeded. Installation UUID and record ID were preserved.
All 439 preexisting deny records remained byte-identical. Of the previously
recorded maintenance files, only installation state and the audit checkpoint
changed; recovery added its own intent, journal, continuity and audit evidence.
Replaying the request succeeded without changing any maintenance record hash.

Catalog installation updated six agents. Codescan and the default nex extension
installation recipes completed. A new private PTY session admitted user scope
discovery and loaded 13 skills. Independent trust requirements still block
`auspex-cop` and `omegon-codescan`; recovery did not change trust or deny policy.
Provider credentials and the operator profile were unchanged.

Evidence is outside Git in `../omegon-installation-recovery-evidence-01`:

- `maintenance-before-apply/` and `pre-apply.json`: authority backup and hashes.
- `recovery-companion-identity.json`: exact mutation executable and source.
- `home-recover-dry-run.json`, `home-recover-apply.json`, and `home-recover-replay.json`.
- `recovery-verification.json`, `replay-verification.json`, and `audit-verify-after.json`.
- `before-home-recovery-debug-02/` and `after-home-recovery-debug/`: attributed PTY evidence.
- `logs/omegon-recovery-maint-fd-crate.log`: all 73 companion tests passed.
- `logs/omegon-recovery-maint-contracts-all.log`: all 43 protocol tests passed.
- `logs/omegon-recovery-maint-fd-clippy.log`: companion Clippy and formatting passed.

The first live dry-run exposed descriptor exhaustion before writing any recovery
record. Its result is retained as `home-recover-dry-run-emfile.json`; four isolated
child-process regressions reproduced and then verified the descriptor-budget fix.

Final release handoff requires `logs/just-link-final.log` and
`final-release/manifest.json` in the external evidence directory, tied to the
final source commit. That gate runs after these verification notes are committed.
