# Contribution loading health design

The EventBus owns a shared scope-health handle. Skill registries and dynamic
contribution inventories receive clones, so each discovery owner records its
own result without a renderer polling filesystem state. Snapshots are serialized
in HarnessStatus and formatted through the existing diagnostics command path.

Each record identifies contribution kind, scope, root path, and one of absent,
loaded, or blocked. Blocked records contain a typed error category selected from
actual error types plus the source chain (at most eight causes, each at most
2 KiB; the outer context and deepest cause survive truncation). Maintenance home-identity mismatch and pending recovery have explicit error codes;
other maintenance errors retain their reason; presentation must not guess recovery policy from
error-message substrings. A full discovery replaces only its contribution kind, including its scope and
entry outcomes. A single extension replacement updates only `user/<name>`, so it
cannot clear a separate failed scope or claim that an entire directory was
rescanned. Failed entries coexist with successful siblings and scope counts.
Skill reloads publish health alongside the replaced skill snapshot.

A single aggregate startup notice points to `/status`. Its comparison identity is
the blocked scope set and errors, so duplicate status events do not repeat it.
Recovery removes the failure record through a successful load result; no guard is
relaxed and no recorded failure is cleared merely because a refresh was requested.

Persistent TUI system notifications append as separate bounded records. Consecutive
messages must not merge into a record already published to inline scrollback; this
applies to startup diagnostics, remote control results such as `/status`, and local
slash/error messages through the same conversation method. The existing mutable
plan-progress snapshot remains a separate coalescing contract. Fullscreen retains
one card per persistent record; the existing aggregate and per-record caps apply.
