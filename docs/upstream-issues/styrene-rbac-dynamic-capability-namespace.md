# Upstream issue: styrene-rbac should support application capability namespaces

## Summary

Omegon/Auspex needs precise capabilities such as `omegon.session.action`,
`omegon.surface.stream`, and `omegon.lifecycle.mutate`. These are application
semantics and should not be hard-coded into upstream `styrene-rbac` as core mesh
capabilities.

## Current blocker

`styrene-rbac` 0.1.0 validates explicit grants against a fixed `ALL_CAPABILITIES`
list. That means downstream crates cannot grant custom namespaced capabilities
through `RosterEntry::with_grants()` even when those capabilities are valid for
that application.

## Requested upstream shape

Add a generic extension mechanism, for example one of:

- `CapabilityRegistry` passed to policy/grant validation
- configurable namespace allowlist such as `omegon.*`
- a `CustomCapability(String)` newtype validated by caller-owned policy

The core crate should keep generic Styrene capabilities, while downstream apps
own their domain vocabularies.

## Omegon local workaround

`omegon-rbac` defines precise local capability strings and maps them to current
Styrene base capabilities (`web.read`, `web.write`, `terminal.restricted`, etc.)
for enforcement. This preserves compatibility but loses precision for explicit
per-capability grants until upstream supports custom namespaces.
