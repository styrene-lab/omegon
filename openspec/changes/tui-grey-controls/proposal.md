# Grey hierarchy for TUI controls

## Intent
Menus, popups, and composer fixtures currently blend into the conversation. Give
operators a visible hierarchy using neutral greys and shared theme roles.

## Scope
Composer borders and supporting text, slash suggestions, connection/settings
menus, selectors, and command prompts/panels. Retain terminal-default conversation
foreground/background and existing signal colors. Markdown and syntax styling
are a subsequent pass; theme discovery and a theme settings UI are also deferred.

## Success criteria
- Panels, selected rows, labels, descriptions, and hints are visually distinct.
- Moving a selection moves its background treatment, including narrow menus.
- Shared theme roles own every new color. Final-frame cleanup preserves them.
- Captured inline/fullscreen runs demonstrate the hierarchy without inference.
