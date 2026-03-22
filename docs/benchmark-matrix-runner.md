---
id: benchmark-matrix-runner
title: Matrix runner — orchestrate permutation runs and collect results
status: seed
parent: demo-qa-benchmark
open_questions: []
jj_change_id: vkpoqrqrqqvroqyzvtoynxvyukwqotxs
---

# Matrix runner — orchestrate permutation runs and collect results

## Overview

A runner that iterates a configuration matrix and launches omegon in headless mode for each permutation. Could be: a /benchmark command within omegon, a standalone CLI tool, or a Justfile/shell script. Each run produces a results JSON. The runner collects all results and produces a comparison report. Key decision: internal vs external orchestration.

## Open Questions

*No open questions.*
