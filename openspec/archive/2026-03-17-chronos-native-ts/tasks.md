+++
id = "477b53b9-d09a-4c31-84de-778a024126f4"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# chronos-native-ts — Tasks

## 1. Implement chronos.ts — pure TS date functions
<!-- specs: chronos -->

- [x] 1.1 Create `extensions/chronos/chronos.ts` with exported functions: `computeWeek(now)`, `computeMonth(now)`, `computeQuarter(now)`, `computeRelative(expression, now)`, `computeIso(now)`, `computeEpoch(now)`, `computeTz(now)`, `computeRange(from, to)`, `computeAll(now)`
- [x] 1.2 Each function returns a string matching the existing output format (DATE_CONTEXT:, MONTH_CONTEXT:, etc.) so the tool output is backward-compatible
- [x] 1.3 `resolveRelative` supports: N days/weeks/months ago, N days/weeks from now, yesterday, tomorrow, next/last {Monday-Sunday}
- [x] 1.4 ISO week uses Thursday-based algorithm; business day counting iterates Mon-Fri
- [x] 1.5 All functions accept an injectable `now?: Date` parameter defaulting to `new Date()`

## 2. Rewrite index.ts — replace shell-out with direct calls
<!-- specs: chronos -->

- [x] 2.1 Remove `CHRONOS_SH` constant and all `existsSync` checks for the shell script
- [x] 2.2 Import functions from `./chronos.ts` and call them directly in `execute()` and the `/chronos` command handler
- [x] 2.3 Remove `pi.exec("bash", ...)` calls — tool no longer spawns a subprocess
- [x] 2.4 Delete `extensions/chronos/chronos.sh`

## 3. Tests
<!-- specs: chronos -->

- [x] 3.1 Create `extensions/chronos/chronos.test.ts` with deterministic tests using fixed dates
- [x] 3.2 Test week boundaries (mid-week, Monday, Friday, weekend edge)
- [x] 3.3 Test month boundaries including Feb (non-leap and leap year), Dec→Jan rollover
- [x] 3.4 Test quarter + fiscal year for each calendar quarter
- [x] 3.5 Test all relative expressions: days ago, weeks ago, months ago, yesterday, tomorrow, next/last each weekday
- [x] 3.6 Test ISO week number, epoch seconds/millis, timezone output format
- [x] 3.7 Test range: calendar days, business days, missing params error
- [x] 3.8 Test "all" returns all section headers
- [x] 3.9 Run `npm run typecheck` and `npm test` — all pass
