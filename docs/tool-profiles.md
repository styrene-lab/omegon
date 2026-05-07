+++
id = "d391161a-f374-4f6f-a7a9-e5b1d6f9efd2"
kind = "document"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
design_docs = ["design/smart-tool-profiles.md"]
last_updated = "2026-03-10"
openspec_baselines = []
subsystem = "tool-profiles"
+++

# Tool Profiles

> Context-aware tool enabling/disabling — reduce prompt bloat by activating only the tools relevant to the current project.

## What It Does

Tool profiles let operators and agents control which tools are visible in the agent's context. Each profile defines a set of enabled/disabled tools tailored to a workflow:

- **Default**: All tools enabled
- **Coding**: Core development tools only (read, write, edit, bash)
- **Research**: Web search, memory, design tree focused
- **Custom**: User-defined via configuration

The `manage_tools` tool provides agent access to list, enable, disable tools and switch profiles. Reducing visible tools saves prompt tokens and reduces hallucination of unavailable capabilities.

## Key Files

| File | Role |
|------|------|
| `extensions/tool-profile/index.ts` | Extension entry — profile management, `/tools` command |
| `extensions/tool-profile/profiles.ts` | Built-in profile definitions |

## Design Decisions

- **Profiles are additive disabling**: Start with all tools, profiles specify which to hide. Simpler than allowlisting.
- **Agent can self-manage**: The `manage_tools` tool lets the agent disable irrelevant tools mid-session to free context.

## Constraints & Known Limitations

- Tool visibility affects prompt injection only — disabled tools are still registered, just hidden from the agent
- No per-project auto-detection yet — profiles must be manually selected

## Related Subsystems

- [Cleave](cleave.md) — child processes inherit the parent's tool profile
