# 0.6.7 stability step — memory injection budget discipline and Node runtime compatibility

## Intent

Assess and harden two operator-reported issues for 0.6.7: (1) excessive session/token usage likely caused by project-memory context injection volume, and (2) runtime failures on unsupported Node versions caused by Unicode regex /v flag usage in the vendored pi-tui dependency.
