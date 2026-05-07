+++
id = "3b525e9c-f4cc-4af4-b693-ebd2d073c122"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave child timeout and idle detection

## Intent

Cleave children currently get a flat 2-hour timeout with no idle detection. When a child has no work (e.g. a sibling already completed it), or gets stuck in a loop, it burns through the full timeout before failing. The chronos-native-ts cleave run had children 1 and 2 hang for 29 minutes before RPC pipe break, consuming API tokens and wall clock time on zero-value work.

See [design doc](../../../docs/cleave-child-timeout.md).
