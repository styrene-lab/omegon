+++
id = "a57b8359-15cd-4e82-9458-07aaa2894329"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Rename pi-kit → Omegon

## Overview

Rename the project from pi-kit to Omegon across all surfaces. Omegon is the hidden primarch of the Alpha Legion — orchestrates through agents, never directly visible, the system that makes everything else work. The rename is a cross-cutting concern touching the GitHub repo, npm package name, terminal title, user-facing strings, bootstrap markers, and the installed package URL in every user's settings.json. Sequencing matters: GitHub repo rename first (triggers automatic redirects), then source changes, then user communication.

## Research

### Full surface inventory

**GitHub / install path (highest risk):**
- Repo: `github.com/cwilson613/pi-kit` → `github.com/cwilson613/omegon` ✓ done
- `~/.pi/agent/settings.json` on every install: was `"packages": ["https://github.com/cwilson613/pi-kit"]` — updated manually by operator. GitHub redirects the old URL automatically but pi's package manager clones to `~/.pi/agent/git/github.com/cwilson613/pi-kit/` — that local path persists until re-cloned.
- All skill paths in the system prompt: `~/.pi/agent/git/github.com/cwilson613/omegon/skills/...` — restored after `pi update`

**package.json:**
- `"name": "Omegon"` → `"omegon"`
- `"description"` — update to reflect actual scope
- `"keywords"` — `"pi-package"` keyword must stay (pi uses it for package detection)

**Extension source (~64 occurrences):**
- `extensions/terminal-title.ts` — `π omegon` in terminal title (most visible surface)
- `extensions/bootstrap/index.ts` — marker file `pi-kit-bootstrap-done` + user-facing strings (migration risk — existing users have old marker)
- `extensions/defaults.ts` — `<!-- managed by pi-kit -->` sentinel in AGENTS.md
- `extensions/local-inference/index.ts` — "Omegon dependencies" in error messages
- `extensions/version-check.ts` — likely references the GitHub repo URL

**Docs / config:**
- `AGENTS.md`, `CONTRIBUTING.md`, `README.md`
- All `docs/*.md` design nodes — pervasive "pi-kit" references
- `.github/workflows/test.yml`
- Skills (`skills/*/SKILL.md`) — reference "Omegon" in context

**Bootstrap marker migration:**
- Existing installs have `~/.pi/agent/pi-kit-bootstrap-done`
- If renamed to `omegon-bootstrap-done`, existing users re-run bootstrap on next session
- Mitigation: check for old marker as fallback, or keep old name, or do a one-time migration at startup

### Sequencing — order of operations matters

1. **GitHub repo rename** — do this first. GitHub creates automatic redirects for clones and API calls. Existing installs continue to work via redirect while we update.
2. **Source changes** — `package.json`, extensions, docs, skills, workflows. Single commit or short feature branch.
3. **Bootstrap marker migration** — check for both old (`pi-kit-bootstrap-done`) and new (`omegon-bootstrap-done`) marker; write new on first run. Silently migrates existing users.
4. **User action required** — update `~/.pi/agent/settings.json` package URL from `cwilson613/pi-kit` to `cwilson613/omegon`. Can't automate this; document clearly in the commit message / CHANGELOG. ✓ done
5. **Re-install** — `pi package update` or full re-install to get the new local clone path. The old `~/.pi/agent/git/github.com/cwilson613/pi-kit/` path remains until cleaned up.

## Decisions

### Decision: Bootstrap marker: check both old and new names

**Status:** decided
**Rationale:** Renaming the marker file would silently re-run bootstrap for all existing users. Instead: on startup check for either pi-kit-bootstrap-done OR omegon-bootstrap-done. Write omegon-bootstrap-done going forward. Zero disruption to existing installs.

### Decision: Keep pi-package keyword in package.json

**Status:** decided
**Rationale:** The pi package manager detects installable packages by the "pi-package" keyword. This must be preserved regardless of the name change or the package becomes invisible to pi install.

### Decision: Terminal title: π omegon (keep π prefix)

**Status:** decided
**Rationale:** The π symbol in the terminal title identifies the pi agent harness — keep it. The project name follows: "π omegon". Clean, consistent with the mathematical identity lineage.

### Decision: AGENTS.md sentinel: check both managed-by strings

**Status:** decided
**Rationale:** defaults.ts checks the sentinel on every session start to determine ownership. Existing deployed AGENTS.md files contain <!-- managed by pi-kit -->. If we only write <!-- managed by omegon --> going forward without checking the old string, existing files lose managed status and never get updated again. Solution: check for either sentinel in the ownership test; write only the new one on deploy. Same pattern as the bootstrap marker.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `package.json` (modified) — name: pi-kit → omegon, description update
- `extensions/terminal-title.ts` (modified) — π omegon → π omegon
- `extensions/bootstrap/index.ts` (modified) — Check both pi-kit-bootstrap-done and omegon-bootstrap-done markers; write new name going forward
- `extensions/defaults.ts` (modified) — <!-- managed by pi-kit --> sentinel → <!-- managed by omegon -->
- `extensions/local-inference/index.ts` (modified) — User-facing Omegon strings → omegon
- `extensions/version-check.ts` (modified) — GitHub repo URL reference if present
- `AGENTS.md` (modified) — pi-kit references → omegon
- `CONTRIBUTING.md` (modified) — pi-kit references → omegon
- `README.md` (modified) — Full rename + description update reflecting actual scope
- `.github/workflows/test.yml` (modified) — Any Omegon references
- `skills/` (modified) — SKILL.md files referencing Omegon
- `docs/` (modified) — Design node docs — bulk sed pass

### Constraints

- pi-package keyword in package.json must be preserved
- Bootstrap marker must check BOTH pi-kit-bootstrap-done and omegon-bootstrap-done
- GitHub repo rename must happen BEFORE source commit is pushed
- User must manually update ~/.pi/agent/settings.json — document in CHANGELOG
- All skill paths injected into system prompt will break until user re-installs — document
