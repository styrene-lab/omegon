+++
id = "ebc69aa2-a05c-42e0-8beb-cf7c747b5b9c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Plugin loader — TOML manifest discovery, HTTP-backed tools and context — Design Spec (extracted)

> Auto-extracted from docs/plugin-loader.md at decide-time.

## Decisions

### Plugin manifest format: TOML with activation rules, tools, context, events (decided)

TOML is human-readable, widely understood, and already a dep (toml crate via Cargo.toml parsing). The manifest declares what the plugin provides (tools, context) and what it consumes (events). Activation is conditional on marker files or env vars — plugins don't load unless relevant to the current project.
