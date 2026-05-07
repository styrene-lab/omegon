+++
id = "0a523d5d-d531-496a-8476-4f47d35c1e65"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Auto-delete merged feature branches on OpenSpec archive

## Intent

After /opsx:archive completes, the archive handler already transitions design nodes to implemented. It should also delete any git branches recorded in those nodes' branches[] field that are fully merged into the current branch. This closes the loop: spec archived → branches gone, with no manual cleanup needed. The handler already has pi in scope for pi.exec. Safety check: git merge-base --is-ancestor before any deletion. Only deletes local branches; does not touch remotes.
