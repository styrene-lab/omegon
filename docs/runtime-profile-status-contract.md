---
id: runtime-profile-status-contract
title: "Runtime profile and autonomy status contract"
status: seed
parent: omega-daemon-runtime
tags: []
open_questions: []
dependencies: []
related: []
---

# Runtime profile and autonomy status contract

## Overview

Define a distinct runtime-profile contract for deployment/autonomy identity, separate from repo preference profiles. Export it through HarnessStatus and IPC/Auspex snapshots so long-running daemons and remote agents can surface policy defaults and autonomous behavior boundaries without overloading settings::Profile.

## Decisions

### Separate runtime profile from repo preference profile

**Status:** accepted

**Rationale:** Repo profile.json is preference persistence and can be shared via git; it is not authoritative deployment identity. IPC currently hardcodes identity.profile as 'primary-interactive', which proves the runtime identity concept exists but is not modeled explicitly. Remote agents and daemons need observable runtime/autonomy defaults independent of repo preference state.
