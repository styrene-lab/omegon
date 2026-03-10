# Subprocess safety hardening

## Intent

Narrow the repo-consolidation-hardening effort to a first concrete slice that removes risky shell-string execution and broad process-management patterns in browser/server/process helpers, replacing them with safer process spawning and explicit argument handling.
