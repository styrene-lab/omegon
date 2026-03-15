# 0.6.7 stability step — memory injection budget discipline and Node runtime compatibility — Tasks

## 1. extensions/project-memory/index.ts (modified)

- [ ] 1.1 Reduce default memory injection budget and gate structural/global/episode additions based on need and observed cost.

## 2. extensions/project-memory/injection-metrics.ts (modified)

- [ ] 2.1 Extend or clarify injection telemetry if needed to validate reduced prompt overhead.

## 3. package.json (modified)

- [ ] 3.1 Declare Node engine floor matching vendored runtime requirements.

## 4. scripts/preinstall.sh (modified)

- [ ] 4.1 Fail early with a clear unsupported-Node message before install/build proceeds.

## 5. README.md (modified)

- [ ] 5.1 Document supported Node runtime expectations for operators.

## 6. vendor/pi-mono/packages/tui/src/utils.ts (modified)

- [ ] 6.1 Only if a minimal upstream-compatible guard or comment is needed; avoid a compatibility fork unless forced.

## 7. Cross-cutting constraints

- [ ] 7.1 0.6.7 must reduce routine prompt bloat without removing high-priority working-memory continuity.
- [ ] 7.2 Unsupported Node versions must fail early with a clear message rather than crashing later on /v Unicode regex parsing.
- [ ] 7.3 Prefer aligning Omegon's runtime contract with vendored pi-mono requirements over introducing a Node 18 compatibility fork in a patch release.
