+++
id = "0908bb65-1d3f-4173-ab4e-39e09f689a27"
title = "TUI surface architecture decoupling"
tags = []
aliases = []
source_format = "omegon_memory"
source_path = "omegon://memory/TUI architecture"
imported_at = "2026-06-09T14:05:55.855566Z"
imported_reference = true
kind = "memory_fact"
topic = "TUI architecture"

[publication]
enabled = false
visibility = "private"

+++

Omegon TUI rendering is now split into shared semantic surfaces under core/crates/omegon/src/surfaces, ACP DTO/redaction adapters under core/crates/omegon/src/acp/surfaces.rs, and Ratatui rendering modules under core/crates/omegon/src/tui. Segment render bodies live in tui/segment_components and render entrypoints take SegmentRenderContext. tui/mod.rs should remain orchestration, while layout_projection owns slim/full Rect allocation.
