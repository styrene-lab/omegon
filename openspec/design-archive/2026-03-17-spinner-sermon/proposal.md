+++
id = "89618c6a-c675-4110-b3a8-a795a5d1a1e8"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Spinner Sermon — Crawler-style scrawling text during long operations

## Intent

During long-running operations (cleave children, extended tool calls), the spinner verb sits static for minutes or hours. Add a second layer beneath the verb: a slowly scrawling sermon inspired by the Crawler's writing in Annihilation — text that crawls character-by-character, giving visual proof-of-life.\n\nThe sermon text should feel alien and procedural, like biological processes masquerading as language. It appears only after a dwell threshold (e.g. 5s without a verb change) and disappears immediately when the next event arrives.

See [design doc](../../../docs/spinner-sermon.md).
