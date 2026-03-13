---
id: tool-card-aesthetics
title: Tool Card Aesthetic Overhaul
status: implemented
parent: tool-call-renderer
tags: [tui, ux, theme, pi-mono]
open_questions: []
---

# Tool Card Aesthetic Overhaul

## Overview

Three tools need aesthetic work: (1) switch_to_offline_driver — flat text, no structure; (2) bash — serviceable but no command syntax highlighting; (3) edit — diff uses chalk.inverse() for intra-line changes which renders as unthemed ANSI reverse-video rather than Alpharius colors. Fixes span pi-mono (theme.ts, diff.ts, tool-execution.ts) and omegon (alpharius.json, offline-driver.ts).

## Open Questions

*No open questions.*
