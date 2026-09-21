# Design

`requires_fullscreen` currently chooses the terminal buffer and viewport. It is
correct for rich widgets to require the alternate screen. `App::draw` incorrectly
uses that choice to select the complete fullscreen workspace composition too.
Keep buffer acquisition unchanged; use `base_terminal` to decide whether to render
the workspace underneath the existing navigation owner.

Share overlay dispatch and precedence between borrowed and explicit fullscreen
frames. A borrowed screen starts with the terminal/theme background, with no
transcript projection, composer, workbench, dashboard, or footer rendering. Clear
hit rectangles for those absent surfaces so previous fullscreen geometry cannot
receive input. The mounted navigation root and responder state retain ownership.

Do not clear conversation data or alter native publication cursors to obtain a
blank backdrop. Returning inline uses existing catch-up rules; old resumed history
stays behind its publication boundary. Explicitly selecting fullscreen continues
to provide access to that history. Test render behavior independently of buffer
transitions, then capture a real resumed-session menu round trip headlessly.

Settings-to-footer synchronization is shared frame preparation, not workspace
rendering. The inline composer consumes these fields too, so accepted menu changes
must refresh model, thinking, and context labels without a new inference event.
Workspace telemetry, layout, and conversation measurement remain in the workspace.
