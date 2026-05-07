+++
id = "a73dac9c-ad4a-4f0e-a4e0-605be45f597c"
kind = "document"
title = "Rewrite chronos as pure TypeScript — eliminate BSD/GNU date dependency"
status = "archived"
tags = ["chronos", "portability", "typescript", "tools"]
aliases = ["chronos-native-ts"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
archive_reason = "superseded"
archived_at = "1775246517"
dependencies = []
open_questions = []
openspec_change = "chronos-native-ts"
parent = "lifecycle-gate-ergonomics"
related = []
superseded_by = "rust-agent-loop"
+++

# Rewrite chronos as pure TypeScript — eliminate BSD/GNU date dependency

## Overview

chronos currently shells out to chronos.sh — a ~350-line bash script with pervasive BSD/GNU `date` branching in every helper function. The BSD `relative` handler is a manual case-match that only covers ~10 expressions and errors on anything else. This is fragile and unnecessary since Node.js Date + Intl APIs provide everything needed cross-platform with zero external dependencies.

Rewrite chronos as pure TypeScript: delete chronos.sh, implement all subcommands (week, month, quarter, relative, iso, epoch, tz, range) using Date arithmetic and Intl.DateTimeFormat. The tool registration and command stay the same — only the backend changes.

## Decisions

### Decision: Pure TypeScript with Date + Intl — no external date library

**Status:** decided

**Rationale:** All chronos subcommands are simple date arithmetic (add days, week boundaries, quarter math, epoch, ISO week). Node's built-in Date handles arithmetic, Intl.DateTimeFormat handles formatting, and day-of-week/ISO-week can be computed with standard formulas. No need for dayjs/luxon/date-fns — the scope is narrow enough for stdlib.

### Decision: Delete chronos.sh entirely — no fallback to shell

**Status:** decided

**Rationale:** The shell script exists only because chronos was originally a standalone skill. Once the logic is in TypeScript, the bash file is dead weight and a maintenance trap. Clean removal.

### Decision: Relative expression parsing: support the same expressions BSD handled plus GNU-style natural language via simple regex patterns

**Status:** decided

**Rationale:** The BSD handler covered: N days/weeks/months ago, yesterday, tomorrow, next/last Monday/Friday. The TS version should cover at least these plus all weekday names and 'N days from now'. Complex GNU expressions like 'third Thursday of next month' are out of scope — they've never been used.
