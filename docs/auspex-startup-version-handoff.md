---
title: Auspex startup version handoff requirement
status: implementing
tags: [auspex, startup, compatibility, control-plane]
---

# Auspex startup version handoff requirement

Auspex now enforces Omegon compatibility at bootstrap using its declared compatibility manifest.

Current Auspex requirement:
- control-plane schema must match
- startup metadata must report canonical Omegon version identity

At the moment, `/api/startup` returns schema and discovery URLs, but **does not include canonical instance/version identity**. That causes Auspex to reject attach even when Omegon is running correctly.

## Required change

`/api/startup` must include:

```json
instance_descriptor.control_plane.omegon_version
```

with a valid semver string, for example:

```json
"omegon_version": "0.15.10-rc.17"
```

## Minimal acceptable payload shape

This is the smallest addition that unblocks Auspex:

```json
{
  "schema_version": 2,
  "addr": "127.0.0.1:7842",
  "http_base": "http://127.0.0.1:7842",
  "state_url": "http://127.0.0.1:7842/api/state",
  "startup_url": "http://127.0.0.1:7842/api/startup",
  "health_url": "http://127.0.0.1:7842/api/healthz",
  "ready_url": "http://127.0.0.1:7842/api/readyz",
  "ws_url": "ws://127.0.0.1:7842/ws?token=...",
  "token": "...",
  "auth_mode": "ephemeral-bearer",
  "auth_source": "generated",
  "control_plane_state": "ready",
  "instance_descriptor": {
    "control_plane": {
      "schema_version": 2,
      "omegon_version": "0.15.10-rc.17"
    }
  }
}
```

## Recommended change

Do **not** only patch the single leaf if the canonical descriptor is already available internally.

Preferred behavior: emit the **full canonical `instance_descriptor`** in `/api/startup`, matching the same shape used by richer state payloads.

Recommended structure:

```json
"instance_descriptor": {
  "identity": {
    "instance_id": "omg_primary_...",
    "role": "primary-driver",
    "profile": "primary-interactive",
    "status": "ready"
  },
  "control_plane": {
    "schema_version": 2,
    "omegon_version": "0.15.10-rc.17",
    "base_url": "http://127.0.0.1:7842",
    "startup_url": "http://127.0.0.1:7842/api/startup",
    "state_url": "http://127.0.0.1:7842/api/state",
    "health_url": "http://127.0.0.1:7842/api/healthz",
    "ready_url": "http://127.0.0.1:7842/api/readyz",
    "ws_url": "ws://127.0.0.1:7842/ws?token=...",
    "auth_mode": "ephemeral-bearer",
    "token_ref": "secret://...",
    "last_verified_at": "2026-04-05T..."
  },
  "session": {
    "session_id": "session_..."
  },
  "policy": {
    "model": "anthropic:claude-sonnet-4-6",
    "thinking_level": "medium",
    "capability_tier": "victory"
  }
}
```

## Why this is required

Auspex bootstrap now validates:
1. `schema_version`
2. `instance_descriptor.control_plane.omegon_version`

Without startup-reported version identity, Auspex cannot safely enforce RC pinning and refuses attach.

This is not a cosmetic issue. It is the current blocker for local Auspex attach against Omegon RC builds.

## Acceptance checks

After the Omegon change, these commands should succeed:

```bash
curl -s http://127.0.0.1:7842/api/startup | jq '.instance_descriptor.control_plane.omegon_version'
```

Expected:

```json
"0.15.10-rc.17"
```

And:

```bash
curl -s http://127.0.0.1:7842/api/startup | jq '.instance_descriptor.control_plane.schema_version'
```

Expected:

```json
2
```

## One-line implementation target

Add canonical `instance_descriptor` to `/api/startup`, including `control_plane.omegon_version`, so Auspex can enforce RC pinning during bootstrap.
