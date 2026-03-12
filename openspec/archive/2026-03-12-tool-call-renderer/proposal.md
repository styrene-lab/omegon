# Enriched Tool Call Rendering

## Intent

Pi's ToolDefinition interface exposes renderCall(args, theme) and renderResult(result, {expanded, isPartial}, theme) hooks that return pi-tui Component instances. These replace the default bare tool-name + raw-text rendering with structured, colored, contextual display. Currently design_tree_update shows anemic one-liners with no mention of the actual changed content. cleave_run shows a wave-counter in the box that conflicts visually with the done-counter in the tab. Both can be fixed by adding custom renderers to the existing registerTool() calls — no new extension needed.
