+++
id = "01d9c6e2-67ee-48be-91f2-88e7a206bb15"
kind = "design_node"
title = "Omegon: auspex extension — visualization daemon bridge"
status = "seed"
tags = ["Omegon", "extension", "auspex", "bridge"]
aliases = ["pikit-auspex-extension"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "feature"
open_questions = []
parent = "markdown-viewport"
related = ["omega"]
+++

# Omegon: auspex extension — visualization daemon bridge

## Overview

The Omegon side of the integration. A small extension in this repo (extensions/auspex/) that provides: `/auspex open` — spawns the mdserve binary pointed at the project root, opens the browser to /dashboard; `/auspex stop` — kills the daemon; optionally a footer/widget showing when the daemon is running and the local URL. Checks for the binary on PATH, surfaces a helpful error if not found (points to Nix install instructions). This is the only piece that lives in Omegon rather than the mdserve fork repo.

## Open Questions

*No open questions.*
