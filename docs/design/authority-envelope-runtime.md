+++
title = "Authority Envelope Runtime"
tags = ["design","autonomy","subagents"]
+++

+++
id = "b669e838-9506-4b18-87a6-3d3d10a876b2"
kind = "design_node"

[data]
title = "Authority Envelope Runtime"
status = "exploring"
issue_type = "architecture"
priority = 2
dependencies = []
open_questions = []
+++

## Overview

Subagent authority should eventually be resolved from a typed authority envelope carried with each execution context, not by feature-local settings lookups. The current near-term implementation injects live settings into delegate and cleave features so `/autonomy` changes affect tool gates in the primary runtime. That is an incremental bridge, not the final architecture.

## Future path

- Model session, loop, scheduled-job, and explicit-approval authority as envelopes.
- Resolve envelopes by precedence before prompt assembly and tool execution.
- Carry the resolved envelope through the command/tool bus request context.
- Have delegate, cleave, loop, scheduled jobs, and future OCI child execution consume the same resolved policy.
- Treat `/loop` and cron/scheduled jobs as trigger envelopes, not authority escalation.
- Require explicit approvals to be represented as higher-precedence envelopes with bounded grants.
- Include execution substrate constraints in the envelope so OCI/native extension and subagent execution choices are policy-governed.

## Bridge implementation

Until the bus carries typed authority context, delegate and cleave may read `SharedSettings` directly and map `AutomationLevel` to `SubagentPolicy`. This keeps the primary runtime's live behavior aligned with `/automation status` and prompt rendering while avoiding a wider bus API change.

Known bridge limitation: non-primary or test surfaces that construct features directly, such as ACP command-dispatch fixtures, do not automatically receive `SharedSettings`. They intentionally fall back to the conservative subagent policy. The future envelope path must make these surfaces pass an explicit authority context instead of relying on ambient feature construction.

## Open Questions

- [assumption] The command/tool bus can grow a typed authority-context field without breaking extension SDK compatibility.
- [assumption] Loop and scheduled job runners can attach explicit envelopes at dispatch time.
- [assumption] Approval grants can be serialized through current SDK DTOs and later replayed into envelope resolution.
