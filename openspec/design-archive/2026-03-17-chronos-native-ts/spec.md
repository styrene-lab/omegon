+++
id = "15d254c5-bd48-4399-b8b5-c49a8e66a825"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Rewrite chronos as pure TypeScript — eliminate BSD/GNU date dependency — Design Spec (extracted)

> Auto-extracted from docs/chronos-native-ts.md at decide-time.

## Decisions

### Pure TypeScript with Date + Intl — no external date library (decided)

All chronos subcommands are simple date arithmetic (add days, week boundaries, quarter math, epoch, ISO week). Node's built-in Date handles arithmetic, Intl.DateTimeFormat handles formatting, and day-of-week/ISO-week can be computed with standard formulas. No need for dayjs/luxon/date-fns — the scope is narrow enough for stdlib.

### Delete chronos.sh entirely — no fallback to shell (decided)

The shell script exists only because chronos was originally a standalone skill. Once the logic is in TypeScript, the bash file is dead weight and a maintenance trap. Clean removal.

### Relative expression parsing: support the same expressions BSD handled plus GNU-style natural language via simple regex patterns (decided)

The BSD handler covered: N days/weeks/months ago, yesterday, tomorrow, next/last Monday/Friday. The TS version should cover at least these plus all weekday names and 'N days from now'. Complex GNU expressions like 'third Thursday of next month' are out of scope — they've never been used.
