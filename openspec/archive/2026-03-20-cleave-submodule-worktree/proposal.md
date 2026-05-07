+++
id = "2fb457aa-33c2-481f-b09c-e7ca395cbf97"
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
