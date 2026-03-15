# 0.6.7 stability step — memory/runtime/tooling stability hardening — Tasks

## 1. extensions/project-memory/index.ts (modified)

- [x] 1.1 Reduce default memory injection budget and gate structural/global/episode additions based on need and observed cost.

## 2. extensions/project-memory/injection-metrics.ts (modified)

- [x] 2.1 Add a reusable budget-policy helper for low-signal vs high-signal turn gating.
- [x] 2.2 Extend tests to validate low-signal suppression and high-signal enablement behavior.

## 3. package.json (modified)

- [x] 3.1 Declare Node engine floor matching vendored runtime requirements.
- [x] 3.2 Bump release version to 0.6.7.

## 4. scripts/preinstall.sh (modified)

- [x] 4.1 Fail early with a clear unsupported-Node message before install/build proceeds.

## 5. README.md (modified)

- [x] 5.1 Document supported Node runtime expectations for operators.

## 6. `/assess design` stability restoration

- [x] 6.1 Replace fragile subprocess JSON dependence with deterministic in-process assessment using design-tree scan + section checks.

## 7. `/cleave` volatile-runtime dirty-tree handling

- [x] 7.1 Add `.pi/runtime/operator-profile.json` to cleave volatile allowlist.
- [x] 7.2 Add regression coverage proving operator-profile churn is treated as volatile and excluded from checkpoint scope.
- [x] 7.3 Ignore `.pi/runtime/` artifacts in `.pi/.gitignore` and untrack the runtime operator profile file.

## 8. Release notes

- [x] 8.1 Add 0.6.7 changelog entry summarizing memory/runtime/tooling stability fixes.

## 9. Cross-cutting constraints

- [x] 9.1 0.6.7 reduces routine prompt bloat without removing high-priority working-memory continuity.
- [x] 9.2 Unsupported Node versions fail early with a clear message rather than crashing later on /v Unicode regex parsing.
- [x] 9.3 Runtime operator-profile churn does not block cleave dispatch preflight.
- [x] 9.4 `/assess design` remains usable without nested subprocess extension-loading fragility.
