+++
id = "3ee8fa4a-8500-4245-93e5-a622bb7cc27f"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Simple skills repo addition — open format for community skill packages — Design Spec (extracted)

> Auto-extracted from docs/skills-repo-format.md at decide-time.

## Decisions

### Skills repos are plugins with type=skill in the unified plugin system (decided)

Skills don't need a separate distribution system. A skill is a plugin with type='skill' in plugin.toml, containing a SKILL.md and optionally example files. Same install command, same discovery, same manifest format. The existing SKILL.md convention is preserved — the plugin.toml wraps it with metadata (id, version, description) for discoverability and dependency management.
