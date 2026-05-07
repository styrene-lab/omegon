+++
id = "33f9cec5-8044-4f50-a4e5-b98ca7b103c6"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# chronos — Delta Spec

## ADDED Requirements

### Requirement: Week subcommand returns current and previous week boundaries

#### Scenario: Week boundaries on a Wednesday

Given today is 2026-03-18 (Wednesday)
When chronos is called with subcommand "week"
Then the output contains CURR_WEEK_START: 2026-03-16 (Monday)
And the output contains CURR_WEEK_END: 2026-03-20 (Friday)
And the output contains PREV_WEEK_START: 2026-03-09 (Monday)
And the output contains PREV_WEEK_END: 2026-03-13 (Friday)
And the output contains TODAY: 2026-03-18 (Wednesday)

### Requirement: Month subcommand returns current and previous month boundaries

#### Scenario: Month boundaries in March

Given today is 2026-03-15
When chronos is called with subcommand "month"
Then the output contains CURR_MONTH_START: 2026-03-01
And the output contains CURR_MONTH_END: 2026-03-31
And the output contains PREV_MONTH_START: 2026-02-01
And the output contains PREV_MONTH_END: 2026-02-28

### Requirement: Quarter subcommand returns calendar quarter and fiscal year

#### Scenario: Quarter in March (Q1, FQ2)

Given today is 2026-03-15
When chronos is called with subcommand "quarter"
Then the output contains CALENDAR_QUARTER: Q1 2026
And the output contains FISCAL_YEAR: FY2026 (Oct-Sep)
And the output contains FISCAL_QUARTER: FQ2

### Requirement: Relative subcommand resolves date expressions

#### Scenario: "3 days ago" resolves correctly

Given today is 2026-03-18
When chronos is called with subcommand "relative" and expression "3 days ago"
Then RESOLVED date is 2026-03-15

#### Scenario: "next Monday" resolves to upcoming Monday

Given today is 2026-03-18 (Wednesday)
When chronos is called with subcommand "relative" and expression "next Monday"
Then RESOLVED date is 2026-03-23

#### Scenario: "yesterday" resolves correctly

Given today is 2026-03-18
When chronos is called with subcommand "relative" and expression "yesterday"
Then RESOLVED date is 2026-03-17

#### Scenario: "2 months ago" resolves correctly

Given today is 2026-03-18
When chronos is called with subcommand "relative" and expression "2 months ago"
Then RESOLVED date is 2026-01-18

#### Scenario: Missing expression returns error

Given today is any date
When chronos is called with subcommand "relative" and no expression
Then an error is thrown mentioning "expression"

### Requirement: ISO subcommand returns ISO 8601 week info

#### Scenario: ISO week context

Given today is 2026-03-18
When chronos is called with subcommand "iso"
Then the output contains ISO_WEEK: W12
And the output contains DAY_OF_YEAR

### Requirement: Epoch subcommand returns Unix timestamps

#### Scenario: Epoch returns seconds and milliseconds

Given today is any date
When chronos is called with subcommand "epoch"
Then the output contains UNIX_SECONDS as a number
And the output contains UNIX_MILLIS as a number
And UNIX_MILLIS equals UNIX_SECONDS * 1000

### Requirement: Timezone subcommand returns tz info

#### Scenario: Timezone context

Given the system has a configured timezone
When chronos is called with subcommand "tz"
Then the output contains TIMEZONE
And the output contains UTC_OFFSET

### Requirement: Range subcommand counts calendar and business days

#### Scenario: Range across a standard work week

Given two dates 2026-03-16 and 2026-03-20
When chronos is called with subcommand "range" and from_date "2026-03-16" and to_date "2026-03-20"
Then CALENDAR_DAYS is 4
And BUSINESS_DAYS is 4

#### Scenario: Range missing dates returns error

Given from_date or to_date is missing
When chronos is called with subcommand "range"
Then an error is thrown mentioning "from_date" and "to_date"

### Requirement: All subcommand returns combined output

#### Scenario: All returns week + month + quarter + iso + epoch + tz

Given today is any date
When chronos is called with subcommand "all"
Then the output contains DATE_CONTEXT (week)
And the output contains MONTH_CONTEXT
And the output contains QUARTER_CONTEXT
And the output contains ISO_CONTEXT
And the output contains EPOCH_CONTEXT
And the output contains TIMEZONE_CONTEXT

## REMOVED Requirements

### Requirement: chronos.sh shell script

chronos.sh is deleted. All logic moves to pure TypeScript in extensions/chronos/chronos.ts.

## Design Decisions

- **Pure TypeScript with Date + Intl**: No external date library. Node stdlib Date for arithmetic, Intl.DateTimeFormat for formatting.
- **Delete chronos.sh entirely**: No fallback to shell. The bash script is removed from the repo.
- **Relative parsing scope**: N days/weeks/months ago, yesterday, tomorrow, next/last {any weekday}, N days/weeks from now. Complex GNU expressions like "third Thursday of next month" are out of scope.
