+++
id = "38c4c74a-fa46-4f7d-8b36-2403a5d5c9eb"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Monorepo migration — absorb core into omegon, eliminate submodule

## Intent

The core submodule is the root cause of three entire bug classes: (1) cleave worktree submodule failures, (2) two-level commit dance complexity, (3) ceremony pointer-update commits. Absorbing core into the main repo eliminates all three AND unblocks jj-lib adoption. The core is never used independently — every omegon release pins a specific core SHA. 22 submodule-pointer commits on main are pure noise.

See [design doc](../../../docs/monorepo-migration.md).
