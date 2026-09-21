# Shared inline and fullscreen TUI presentations

## Intent

Give `om` a small inline working surface and `omegon` a fullscreen workspace,
using one application, event loop, input owner, and component library. Default
detail is Active for `om` and Full for `omegon`. Operators can configure detail
independently of terminal presentation.

## Scope

Implement launcher intent, explicit configuration, shared frame preparation and
widgets, two layouts, coordinated terminal ownership, bounded inline publication,
and automated terminal acceptance. Reuse the existing Project browser and rich
surfaces as temporary fullscreen views from inline mode.

This change owns the pending persistent-inline/publication work in
[tui-project-shell](../tui-project-shell/tasks.md). It depends on that change's
navigation and terminal ownership repairs and preserves
[tui-native-usability](../tui-native-usability/tasks.md) behavior. Their unfinished
verification remains unfinished. Extension response transport and new execution
or evidence browsers remain outside this increment.

Do not add a frontend framework, another App, another runtime, a Ratatui fork,
or a second conversation store. Dynamic inline-height animation, interactive
terminal history, and pixel image publication into scrollback are deferred.

## Success criteria

- Fresh `om` starts in the primary buffer with an eight-row live viewport;
  fresh `omegon` starts in the alternate buffer with its workspace layout.
- All four combinations of terminal presentation and Active/Full detail work.
- Inline → Project → permission → Project → inline preserves the draft,
  selection, session, and ongoing turn. The same runtime accepts a second turn.
- Stable streamed output reaches native history before response completion in ordered, bounded batches. Resize,
  detail changes, and temporary fullscreen visits do not replay committed output.
- The agent drives terminal launch and input, checks fixture outcomes, and
  captures attributable evidence without paid inference or operator actions.
- Landing requires completed Rust/script gates and native compatibility results;
  a structurally valid plan alone does not establish runtime acceptance.
