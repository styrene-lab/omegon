+++
id = "e2bfaa57-97b9-49e4-bd32-032bd73b4238"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Smart Tool & Skill Profiles for Project Context

## Overview

Omegon ships 25+ extensions and 12 skills. Many are context-inappropriate for a given project — the Rust skill in a Python project, the vault extension when there's no Obsidian vault, the OCI skill in a library repo, etc. We need a system that detects project context and activates an appropriate subset of tools/skills, either automatically at session start or via an explicit `/init` or `/profile` command.

Key tensions:
- **Tools** can be toggled at runtime via `pi.setActiveTools(names[])` / `pi.getActiveTools()`
- **Skills** are glob-matched by pi's resource loader at session_start — no runtime toggle API
- **Extensions** are loaded at startup from package.json — no runtime toggle
- So we can only gate tools dynamically; skills and extensions always load but skills only inject into prompts when their globs match files being worked on

The practical lever is **tool activation** — disable tools for extensions that aren't relevant (e.g., hide `render_diagram`, `render_excalidraw`, `generate_image_local` in a backend-only API project). Skills self-select via globs so they're less of a problem.

## Research

### Tool Inventory & Control Surface

**Omegon registers ~30 tools**, plus MCP bridge tools (scribe: ~30 more). Total: ~60 tools in context window.

**Runtime control levers:**
- `pi.getActiveTools()` / `pi.setActiveTools(names[])` — toggle tool availability at runtime
- `pi.getAllTools()` — get full catalog with descriptions
- Skills: glob-matched, no runtime toggle, but self-selecting (only inject when files match)
- Extensions: load at startup, no runtime toggle, but their tools CAN be toggled

**Tool groupings that are contextually exclusive or optional:**
1. **Visual tools** (generate_image_local, render_diagram, render_excalidraw) — useless in headless/CI/backend projects
2. **Scribe MCP tools** (~30) — only relevant when working on scribe-tracked partnership work
3. **Local inference** (ask_local_model, list_local_models, manage_ollama) — only if Ollama installed
4. **Design tree** (design_tree, design_tree_update) — only useful in design-heavy exploration
5. **OpenSpec** (openspec_manage) — only during spec-driven development
6. **Web search** (web_search) — always useful, but could be optional
7. **Core** (read, write, edit, bash, grep, find, ls, memory_*, chronos, whoami) — always needed

**Skill groupings that are contextually exclusive:**
- python vs rust vs typescript — language-specific
- oci vs vault — deployment vs documentation
- pi-extensions + pi-tui — only when working on pi extensions

### Design Approach: Profile-Based Tool Activation

**Core idea:** A "profile" is a named set of tool activations + any extension-specific config hints. Profiles can be:
1. **Auto-detected** from project signals (package.json, Cargo.toml, pyproject.toml, .git, etc.)
2. **Explicitly set** via `/profile <name>` command or `.omegon/profile.json` config
3. **Layered** — base profile + additive overrides

**Proposed profiles:**

| Profile | Signals | Tools enabled | Tools disabled |
|---------|---------|---------------|----------------|
| `core` | (always) | memory_*, chronos, whoami, set_model_tier, set_thinking_level, switch_to_offline_driver | — |
| `coding` | .git exists | core + cleave_*, openspec, design_tree* | — |
| `visual` | has images/, .excalidraw, d2 files | render_diagram, render_excalidraw, generate_image_local | — |
| `local-ai` | ollama installed | ask_local_model, list_local_models, manage_ollama | — |
| `web` | (always for now) | web_search, view | — |
| `scribe` | scribe MCP configured | all mcp_scribe_* tools | — |
| `pi-dev` | cwd is Omegon or has pi.extensions in pkg.json | all tools | — |

**Detection at session_start:**
1. Scan `cwd` for language/framework markers
2. Check for `.omegon/profile.json` override
3. Compute effective tool set = union of matched profiles
4. Call `pi.setActiveTools(effectiveSet)`

**`.omegon/profile.json` format:**
```json
{
  "include": ["coding", "visual"],
  "exclude": ["scribe"],
  "tools": {
    "enable": ["web_search"],
    "disable": ["generate_image_local"]
  }
}
```

**Key advantage:** This is purely additive — never removes a tool the user explicitly wants. The exclude list and tools.disable are for user overrides only.

## Decisions

### Decision: Tool activation is the primary lever; agent can self-serve

**Status:** decided
**Rationale:** Skills auto-match via globs (self-selecting). Extensions always load (no runtime toggle). Tool activation via pi.setActiveTools() is the only meaningful runtime control. Additionally, the agent can enable tools on demand when asked — user says "I need the view tool" and agent calls setActiveTools to add it, or worst case user does /reload.

### Decision: Auto-detect at session_start with `.omegon/profile.json` override and /profile command

**Status:** decided
**Rationale:** Auto-detection covers 90% of cases by scanning cwd for project markers. `.omegon/profile.json` provides declarative override for projects that need custom config. /profile command allows mid-session inspection and switching. Agent can also toggle individual tools on demand when the user asks.

### Decision: Dedicated manage_tools tool for agent self-serve

**Status:** decided
**Rationale:** The agent needs to call pi.setActiveTools() but has no access to the extension API directly. A lightweight manage_tools tool with actions: list (show all tools + active state), enable/disable (toggle individual tools), profile (switch entire profile) gives the agent the ability to self-serve when the user asks "I need the view tool" or "disable the scribe tools". This is more ergonomic than making the user run /profile.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/tool-profile/index.ts` (new) — Extension entry: session_start auto-detection, /profile command, manage_tools tool registration
- `extensions/tool-profile/profiles.ts` (new) — Profile definitions, detection logic, merge algorithm
- `extensions/tool-profile/profiles.test.ts` (new) — Tests for detection and merge logic
- `package.json` (modified) — Add ./extensions/tool-profile to pi.extensions array

### Constraints

- Must not break existing tool availability — default with no config should enable everything that's currently enabled
- Detection must be fast (no network calls, just fs checks)
- Profile changes must take effect immediately without /reload
