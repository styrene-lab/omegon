# Design

Add overridable control styles to the existing Theme trait: panel, selection, UI
text, title, secondary text, hint, and border. Default implementations derive from
existing theme slots, preserving legacy/custom implementations. TerminalTheme
uses an indexed neutral palette: panel 235, selection 240, borders/hints 244,
secondary text 248, labels 252, and selection/title text 255. Panels specify both
foreground and background so their contrast does not depend on the terminal's
base colors. The conversation and editor input retain terminal defaults.

Keep these styles separate from Markdown/content styles. The terminal cleanup
pass allows only the new role colors alongside existing ANSI signals, continuing
to strip legacy chromatic RGB styling. Background normalization derives its
allow-list from the panel and selection styles. Selection spans inherit a row
background while warning/error badges retain their signal foreground.

Reuse the same roles in menus, selectors, autocomplete, and command panels.
Composer border and placeholder use the neutral border/hint roles. No new theme
loader, dependency, terminal color probe, or global terminal palette mutation.
