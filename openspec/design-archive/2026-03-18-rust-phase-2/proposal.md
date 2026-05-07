+++
id = "c2991cdd-c08b-46c1-bde0-c0b08c254124"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Phase 2 — Native TUI: Dioxus/ratatui replaces pi-tui bridge subprocess

## Intent

The TUI bridge subprocess disappears. The Rust binary drives the terminal directly via Dioxus terminal renderer or ratatui/crossterm. Dashboard, splash, spinner, tool card rendering — all native Rust. The Node.js LLM bridge is the only remaining subprocess. ~5.7k LoC of TypeScript rendering code migrates to Rust.

See [design doc](../../../docs/rust-phase-2.md).
