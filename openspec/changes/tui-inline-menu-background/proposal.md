# Clean backgrounds for inline navigation

## Intent
Opening settings from an inline resumed session unexpectedly paints the entire
historical transcript, composer, and workspace chrome behind the menu. Borrowing
terminal space must not imply choosing the fullscreen workspace.

## Scope
Separate the fullscreen workspace composition from shared navigation rendering.
Inline menus, selectors, inspectors, and decisions borrow a clean alternate screen.
Keep existing terminal transitions, navigation ownership, session data, and widgets.

## Success criteria
- Inline navigation shows only the requested surface and applicable notifications.
- Dismissal restores the inline draft and native scrollback without historical replay.
- An explicitly selected fullscreen base retains its transcript and workspace.
