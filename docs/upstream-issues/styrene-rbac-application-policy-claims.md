# Upstream issue: styrene-rbac policy claims need typed operation metadata

## Summary

Omegon/Auspex authorization decisions are attached to operations with structured
metadata: HTTP route, method, session id, assistant profile id, tool name,
mutation class, and whether the operation may expose secrets or host effects.

## Current blocker

`styrene-rbac` currently answers `has_capability(&str)` from role + grants. That
is enough for coarse authorization but not enough to express or audit typed
application decisions without every downstream project inventing its own wrapper.

## Requested upstream shape

Consider adding generic policy-decision DTOs independent of any single app:

- capability string
- action/operation id
- resource id/type
- decision allow/deny
- reason code
- matched role/grant source

This should remain generic and not mention Omegon/Auspex. Downstream crates can
then map their operation metadata into the generic decision envelope.

## Omegon local workaround

`omegon-rbac` will own the application vocabulary and mapping helpers locally.
Omegon will keep route/tool-specific metadata outside `styrene-rbac` until a
generic decision envelope exists upstream.
