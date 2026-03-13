---
id: pikit-vault-extension
title: "pi-kit: /vault serve extension bridge"
status: seed
parent: markdown-viewport
dependencies: [mdserve-lifecycle-backend]
tags: [pi-kit, extension, vault, bridge]
open_questions: []
issue_type: feature
---

# pi-kit: /vault serve extension bridge

## Overview

The pi-kit side of the integration. A small extension in this repo (extensions/vault/) that provides: /vault serve — spawns the mdserve binary pointed at the project root, opens the browser to /dashboard; /vault stop — kills the daemon; optionally a footer/widget showing when the daemon is running and the local URL. Checks for the binary on PATH, surfaces a helpful error if not found (points to Nix install instructions). This is the only piece that lives in pi-kit rather than the mdserve fork repo.

## Open Questions

*No open questions.*
