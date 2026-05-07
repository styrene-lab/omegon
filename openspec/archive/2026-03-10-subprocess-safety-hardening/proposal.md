+++
id = "7e912257-e033-4f53-a3c3-de43d9d79806"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Subprocess safety hardening

## Intent

Narrow the repo-consolidation-hardening effort to a first concrete slice that removes risky shell-string execution and broad process-management patterns in browser/server/process helpers, replacing them with safer process spawning and explicit argument handling.
