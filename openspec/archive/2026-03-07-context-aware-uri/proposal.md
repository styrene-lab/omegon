+++
id = "bff349a7-edb9-4821-92f3-f2473c647186"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Context-Aware OSC 8 URIs — Smart File Links in Terminal Output

## Intent

Add intelligent URI routing to OSC 8 hyperlinks in terminal output. Instead of always using file://, the view tool routes links to the best available handler based on file type: markdown → mdserve (auto-started), code → editor scheme (vscode/cursor/zed per config), .excalidraw → obsidian:// if vault detected, everything else → file://. All schemes optional with file:// fallback. Config lives in .pi/config.json. mdserve auto-starts on session_start if binary is on $PATH.

## Scope

<!-- Define what is in scope and out of scope -->

## Success Criteria

<!-- How will we know this change is complete and correct? -->
