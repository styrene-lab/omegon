# ACP HostAction Approval Routing

## Intent

Allow ACP clients such as Flynt to review permitted manual HostAction requests before Omegon executes them, while preserving Omegon as the canonical policy and executor owner.

## Problem

Native extension declarative HostActions are consumed inside Omegon before ACP clients see the original request. ACP receives only post-processed `host_action_outcomes`, which prevents Flynt from owning review.

MCP HostActions are now policy-configurable and can be represented as `needs_approval`, but they also need the same approval route before execution.

## Relationship to `resource.open@1` (#83)

`resource.open@1` is explicitly out of scope for this change. This change provides the generic approval/control plane that `resource.open@1` will use in the next patch.

## Success criteria

- Manual native extension HostActions are sent to ACP as permission requests before execution.
- ACP metadata includes the original HostAction candidate and trusted origin context.
- Approved actions execute through the canonical HostAction executor registry.
- Rejected actions return denied outcomes and do not execute.
- No ACP permission channel deterministically denies manual actions.
- MCP policy-allowed HostActions converge on the same approval route.
