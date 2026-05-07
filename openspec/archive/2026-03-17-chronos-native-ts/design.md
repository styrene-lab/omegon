+++
id = "d487d209-7358-47a1-9cfd-1ab2f561ff0b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# chronos-native-ts — Design

## Spec-Derived Architecture

All 8 chronos subcommands (week, month, quarter, relative, iso, epoch, tz, range) plus the "all" composite are rewritten as pure TypeScript functions in a new `extensions/chronos/chronos.ts` module. The extension entry point (`extensions/chronos/index.ts`) calls these functions directly instead of shelling out to `chronos.sh`.

### Key design points

- Each subcommand is an exported function that takes a `Date` (defaults to `new Date()`) and returns a formatted string block matching the existing output format exactly (DATE_CONTEXT:, MONTH_CONTEXT:, etc.)
- Functions accept an injectable "now" date for deterministic testing
- `resolveRelative(expression, now)` uses regex matching — no `date` binary
- ISO week calculation uses the standard algorithm (Thursday-based)
- Business day counting iterates day-by-day (range is always small)
- `chronos.sh` is deleted

## File Changes

| Path | Action | Description |
|------|--------|-------------|
| `extensions/chronos/chronos.ts` | new | Pure TS implementation of all subcommand functions |
| `extensions/chronos/chronos.test.ts` | new | Tests for all subcommands with injectable dates |
| `extensions/chronos/index.ts` | modified | Replace shell-out with direct function calls |
| `extensions/chronos/chronos.sh` | deleted | No longer needed |
