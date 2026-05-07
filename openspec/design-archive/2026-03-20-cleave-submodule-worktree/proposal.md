+++
id = "e98033e0-d5f6-4c1d-872c-a364f479e3dc"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave worktree submodule failures — root cause and fix

## Intent

Security assessment runs showed 2/4 child failures in both cleave runs. All failures were on children whose scope targeted files inside the `core` git submodule. Root cause analysis below.

See [design doc](../../../docs/cleave-submodule-worktree.md).
