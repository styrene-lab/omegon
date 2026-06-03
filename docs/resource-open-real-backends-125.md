---
id: resource-open-real-backends-125
title: "Implement real resource.open@1 backends for Flynt, Zed, and terminal reader"
status: seed
tags: [0.27.0, host-actions, extensions, resource-open]
open_questions: []
dependencies: []
related: []
---

# Implement real resource.open@1 backends for Flynt, Zed, and terminal reader

## Overview

Follow-up to issue #83 / commit d392f23d. The resource.open@1 HostAction substrate now exists with SDK contract support, manifest policy validation, secure workspace-root enforcement, backend registry scaffolding, deterministic unavailable fallback, and fake-backend routing tests. This node tracks the remaining operator-visible backend work from GitHub issue #125: route validated resource.open@1 requests to real Flynt, Zed, and terminal/Bookokrat backends while preserving explicit fallback outcomes and auditability.
