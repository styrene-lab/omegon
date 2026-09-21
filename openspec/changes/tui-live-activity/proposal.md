# Visible live agent activity

## Intent
Inline output currently exposes a generic Working footer and occasionally a bare
tool name. Operators cannot reliably see thinking, response streaming, or which
tool is executing. Reuse the existing event state for a compact action area.

## Scope
A shared grey phase/tool strip for inline and fullscreen, using existing
SlimTurnState and bounded activity_tools. Retain durable tool history separately.
No inference animations, raw thinking disclosure, new state machine, or provider
protocol changes.

## Success criteria
- Waiting, thinking, responding, running tools, and cancellation are distinguishable.
- Inline activity follows the complete visible response tail and precedes input.
- Status updates never enter native scrollback or obscure response text.
- Authoritative completion clears activity; another turn remains usable.
