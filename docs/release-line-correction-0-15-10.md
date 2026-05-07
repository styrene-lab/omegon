+++
id = "c161fc34-d2cf-4cd9-9159-80602ba2b002"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Release line correction for 0.15.10

## Summary

`v0.15.11-rc.2` was published from an incorrect assumption: that the `0.15.10` line had already closed with a clean stable release.

That assumption was wrong.

The attempted `0.15.10` stable cut exposed UI correctness issues in the TUI engine/inference surfaces, specifically:

- footer state percentage and inference composition bar drifted out of sync
- Codex/OpenAI limit telemetry rendered internal quota field names instead of operator-readable copy

Because those issues were discovered after the attempted stable cut, the `0.15.10` line was not actually complete.

## What happened

The release machinery advanced the workspace to `0.15.11-rc.1`, then later cut `0.15.11-rc.2`, even though `0.15.10` had not been accepted as a clean stable release in practice.

By the time this was corrected, `v0.15.11-rc.2` had already been pushed to origin, so rewriting public history would have created a worse problem.

## Corrective action

The release line was corrected forward rather than rewritten:

- `v0.15.11-rc.2` remains in public history as an accidental RC
- the active candidate line was restored to the `0.15.10` series
- `v0.15.10-rc.30` was cut as the corrective RC

This preserves public git history while restoring honest version semantics.

## Operator guidance

If you are choosing a current RC for validation on this line:

- **Do not treat `v0.15.11-rc.2` as the canonical next candidate**
- **Use `v0.15.10-rc.30` as the corrective active RC for the unfinished `0.15.10` line**

## Scope of the correction

This note is about release bookkeeping honesty only. It does not change the code-level fixes themselves.

The corrective RC includes the TUI fixes that motivated the rollback of the version line:

- footer context state aligned with inference composition
- Codex/OpenAI limit telemetry simplified into operator-readable copy
