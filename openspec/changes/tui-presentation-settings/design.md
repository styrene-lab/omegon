# Design: TUI presentation settings and capability remediation

## Architecture

Presentation preferences remain canonical runtime settings and profile fields. `ThemePreference` names a registry entry; `GlyphPreference` records operator intent independently from `GlyphCapability`, and resolution produces an effective `GlyphProfile`. The semantic Settings projection exposes requested/effective state and warning/remediation rows without importing Ratatui types.

The first theme registry uses bundled semantic JSON resources plus the compiled Alpharius fallback. Theme selection is live: the TUI replaces its boxed `Theme` after a selector confirmation and persists the preference through the existing update/profile path.

Glyph rendering resolves through runtime preference rather than a process-global detector-only singleton. Auto follows detection; explicit ASCII and Unicode are always honored; explicit Nerd Font is honored as intent while Settings warns when detection cannot establish support. Remediation is diagnostic and reviewable—it opens help/evidence rather than mutating terminal configuration.

## Constraints

- Preserve requested, detected, and effective glyph values separately.
- Do not install fonts or edit terminal configuration automatically.
- Do not couple semantic settings projections to Ratatui.
- Keep startup capability probes independent from splash visibility.
- Unknown theme names must fail safely to Alpharius.
- Existing profile files without new fields retain current behavior.

## Initial file scope

- `core/crates/omegon/src/settings.rs`
- `core/crates/omegon/src/surfaces/settings.rs`
- `core/crates/omegon/src/tui/theme.rs`
- `core/crates/omegon/src/tui/glyphs.rs`
- `core/crates/omegon/src/tui/settings_menu.rs`
- `core/crates/omegon/src/tui/menu_effects.rs`
- `core/crates/omegon/src/tui/mod.rs`
- `core/crates/omegon/src/tui/tests.rs`
- `themes/*.json`
