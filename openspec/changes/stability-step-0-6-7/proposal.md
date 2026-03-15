# 0.6.7 stability step — memory/runtime/tooling stability hardening

## Intent

Assess and harden four operator-reported stability issues for 0.6.7:

1. excessive session/token usage likely caused by project-memory context injection volume,
2. runtime failures on unsupported Node versions caused by Unicode regex /v flag usage in the vendored pi-tui dependency,
3. `/assess design` subprocess failures caused by duplicate extension-root loading/runtime-path fragility,
4. `/cleave` dirty-tree preflight blockage from volatile `.pi/runtime/operator-profile.json` churn.
