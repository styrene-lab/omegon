---
title: Extension SDK Host Compatibility Policy for 0.25
status: exploring
tags: [extension-sdk, compatibility, host-policy, 0.25]
parent: extension-sdk-standalone-0.25-roadmap
issue: 103
---

# Extension SDK Host Compatibility Policy for 0.25

## Purpose

Define how the Omegon host reacts when an extension advertises an SDK contract version.

This belongs in the host repo because compatibility is a runtime policy decision: the SDK can declare its contract, but only the host knows which contract ranges it supports.

## Version relations

| Relation | Host behavior |
|---|---|
| exact supported contract | allow |
| older compatible minor | allow with warning if deprecated |
| older unsupported | refuse unless explicit override exists |
| newer unknown | refuse by default |
| missing | allow only for legacy extensions with warning |

## Inputs

Potential extension sources:

- Manifest `sdk_version` field.
- `initialize`/handshake metadata once v2 handshake stabilizes.
- Contract artifact version consumed by first-party extensions at build time.

## Outputs

Host diagnostics should surface:

- Extension name.
- Advertised SDK contract version.
- Host-supported contract range.
- Compatibility classification.
- Operator remediation: update extension, update host, or enable legacy override.

## Decisions

- Decision: Newer unknown SDK contracts are refused by default because extension protocol features may require host behavior the current host cannot enforce.
- Decision: Missing SDK versions remain legacy-compatible for one stabilization window, but should warn.
- Decision: Compatibility checks happen before tool registration so incompatible tools never enter the model surface.

## Open questions

- [assumption] Existing `manifest.sdk_version` can be repurposed or tightened into contract compatibility semantics.
- What is the first supported range for 0.25: exactly `0.25`, or `>=0.24,<0.26` with warnings?
- Should local development extensions have an override path such as `OMEGON_ALLOW_UNSUPPORTED_EXTENSION_SDK=1`?
- Should incompatible extensions appear in `omegon extension list` with a status reason?

## Required tests

- Exact version allowed.
- Missing version allowed with warning.
- Older compatible version allowed with warning.
- Older unsupported version refused.
- Newer version refused.
- Refused extension tools are not registered in the event bus.
