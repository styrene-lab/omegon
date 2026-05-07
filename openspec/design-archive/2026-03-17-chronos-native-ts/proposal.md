+++
id = "97a696b6-582a-4e86-b4bf-3ad2b341452f"
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

See [design doc](../../../docs/chronos-native-ts.md).
