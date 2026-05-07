+++
id = "62d97a01-b95a-48bf-bd9d-85af03f630d8"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Embedded web dashboard — lightweight localhost UI served from the omegon binary

## Intent

The TUI dashboard panel is constrained to ~36 columns of text. For complex lifecycle operations — dependency graph traversal, spec-to-task traceability, multi-change OpenSpec funnels, cleave timeline inspection — we need a richer interactive surface. The question is how to serve it from the omegon binary without introducing a heavy build pipeline or separate process.

See [design doc](../../../docs/embedded-web-dashboard.md).
