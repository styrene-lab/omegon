+++
id = "badc8441-9bec-44a7-8897-1bbcd3c623c8"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Rewrite chronos as pure TypeScript — eliminate BSD/GNU date dependency

## Intent

chronos currently shells out to chronos.sh — a ~350-line bash script with pervasive BSD/GNU `date` branching in every helper function. The BSD `relative` handler is a manual case-match that only covers ~10 expressions and errors on anything else. This is fragile and unnecessary since Node.js Date + Intl APIs provide everything needed cross-platform with zero external dependencies.

Rewrite chronos as pure TypeScript: delete chronos.sh, implement all subcommands (week, month, quarter, relative, iso, epoch, tz, range) using Date arithmetic and Intl.DateTimeFormat. The tool registration and command stay the same — only the backend changes.
