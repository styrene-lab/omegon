+++
id = "8f7feddf-9610-4b6a-9633-7835b4a64c93"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Fractal dashboard ring — living AI state visualization in the dashboard header

## Intent

The fractal state surface (tui/fractal.rs, 335 lines) exists but has never been rendered anywhere. The dashboard header (top-right panel, 36 columns wide) is a stable, always-visible area. Place the fractal there as a living 'AI ring' — a constant visual heartbeat showing the agent's state.

See [design doc](../../../docs/fractal-dashboard-ring.md).
