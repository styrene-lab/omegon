# Fresh launch uses entrypoint defaults without a posture wizard

## Intent

A configuration reset exposes an obsolete pre-TUI wizard advertising Fabricator,
Architect, Explorator, and Devastator as the primary operating modes. Fresh installs
must enter the same quiet interface as configured installs.

## Scope

Remove the blocking posture wizard, tool inventory, and automatic posture profile
write. Keep first-launch detection for splash policy. Retain explicit legacy CLI
and saved-profile compatibility; this change does not remove the behavior engine.

## Success criteria

- Profile-free, non-child `om` reaches inline/Active without setup input.
- Profile-free, non-child `omegon` reaches fullscreen/Full without setup input.
- Startup does not print a named-posture menu or create a posture override.
- Existing explicit profile/CLI precedence and connection setup remain intact.
