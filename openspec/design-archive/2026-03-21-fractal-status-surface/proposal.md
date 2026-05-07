+++
id = "fc71931f-38c6-490b-a6bd-0161c0d8f570"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Fractal status surface — multi-dimensional state visualization via generative fractal rendering

## Intent

Replace conventional loading bars and status indicators with a living fractal viewport that encodes multi-dimensional harness state into visual properties. Instead of reading \"72% context used\" as text, the operator sees a Mandelbrot region whose zoom depth, color palette, animation speed, and structural features all correspond to real system state.\n\nThe fractal is not decorative — each visual dimension maps to a harness signal:\n- **Zoom depth** → context utilization (deeper = fuller)\n- **Color palette** → cognitive mode (design = cool blues, coding = warm ambers, cleave = split complementary)\n- **Animation speed** → agent activity (fast iteration during tool calls, slow drift during thinking)\n- **Center coordinates** → session progression (drifts through the fractal space over time)\n- **Brightness/contrast** → health (high contrast = all systems nominal, washed out = degraded)\n- **Fractal type** → persona (Mandelbrot = default, Burning Ship = aggressive, Julia = creative)\n\nInspiration: rsfrac (github.com/SkwalExe/rsfrac) demonstrates fractal rendering in ratatui at terminal resolution. The approach here is different — not an explorer, but a generative status surface driven by harness telemetry.

See [design doc](../../../docs/fractal-status-surface.md).
