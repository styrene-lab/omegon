---
id: benchmark-telemetry-capture
title: Telemetry capture — structured logging of per-phase metrics
status: seed
parent: demo-qa-benchmark
open_questions: []
jj_change_id: vkpoqrqrqqvroqyzvtoynxvyukwqotxs
---

# Telemetry capture — structured logging of per-phase metrics

## Overview

Instrument the agent loop to emit structured telemetry events at phase boundaries. The demo prompt marks phases with '═══ PHASE N' headers — the harness detects these and snapshots metrics. In headless mode, instrument intensities are computed but not rendered — their peak/mean values are logged to the results artifact. Requires: phase boundary detection, metric snapshotting, instrument intensity logging without a TUI.

## Open Questions

*No open questions.*
