+++
id = "208081b6-a285-4323-8a38-1cb0a0d84e29"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Simple skills repo addition — open format for community skill packages

## Overview

Research open formats for skill distribution (FOSS analog of Claude Code plugins). Replicate the pi extensions install path in UI but for markdown guidances. Skills repos should be installable via URI, discoverable, and composable with personas. A skill is lighter than a persona — it's expertise without identity.

## Decisions

### Decision: Skills repos are plugins with type=skill in the unified plugin system

**Status:** decided
**Rationale:** Skills don't need a separate distribution system. A skill is a plugin with type='skill' in plugin.toml, containing a SKILL.md and optionally example files. Same install command, same discovery, same manifest format. The existing SKILL.md convention is preserved — the plugin.toml wraps it with metadata (id, version, description) for discoverability and dependency management.

## Open Questions

*No open questions.*
