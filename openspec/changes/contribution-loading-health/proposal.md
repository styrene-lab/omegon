# Expose blocked contribution scopes and their recovery

## Intent

An installation-state identity mismatch currently prevents installed extensions,
user plugins, and user skills from loading while the TUI still starts. Empty
inventories hide the distinction between no installation and denied discovery.
Preserve scope loading health as operational state, with a compact startup notice
and actionable details through the existing shared `/status` surface.

## Scope

Record admitted, absent, and failed scope discovery for skills, plugins, and
extensions; retain typed error category, complete cause chain, and owning root.
Refresh health with discovery outcomes and clear recovered failures. Reuse the
existing TUI/CLI/ACP status command and HarnessStatus event pathways. Do not alter
maintenance guard policy, admit denied contributions, or dump provider catalogs.

## Success criteria

- A missing directory is explicitly absent, while an invalid or denied scope is blocked.
- Failed scopes retain their actual cause and root without hiding successful independent scopes.
- Startup emits one compact contribution warning; `/status` exposes scope-level details across frontends.
- Successful reload replaces stale failure state and removes the blocked-health notice.
- Tests exercise guarded filesystem discovery as well as shared status projection.
