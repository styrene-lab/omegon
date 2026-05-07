+++
id = "7e851505-6d99-4e4b-942a-0edbc0be7063"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# memory-mind-audit-instrumentation — Tasks

## 1. Memory injection metrics are recorded at generation time

- [x] 1.1 Full injection metrics are recorded
- [x] 1.2 Semantic injection metrics are recorded
- [x] 1.3 Write tests for Memory injection metrics are recorded at generation time

## 2. Shared state exposes last memory injection metrics

- [x] 2.1 Shared state receives the last injection snapshot
- [x] 2.2 Write tests for Shared state exposes last memory injection metrics

## 3. Memory stats report last injection metrics

- [x] 3.1 Memory stats include the last injection snapshot
- [x] 3.2 Write tests for Memory stats report last injection metrics

## 4. Injection event records the exact metric set needed for audit

- [x] 4.1 Audit metrics include composition detail
- [x] 4.2 Write tests for Injection event records the exact metric set needed for audit

## 5. Dashboard memory bar continues using estimated tokens initially

- [x] 5.1 Dashboard accounting remains backward compatible
- [x] 5.2 Write tests for Dashboard memory bar continues using estimated tokens initially

## 6. Dashboard exposes memory audit visibility

- [x] 6.1 Raised dashboard footer shows the latest memory injection summary
- [x] 6.2 Write tests for memory audit summary formatting

## 7. Dashboard refreshes from arbitrary on-disk Design Tree and OpenSpec changes

- [x] 7.1 Design Tree disk edits emit dashboard refresh events
- [x] 7.2 OpenSpec disk edits emit dashboard refresh events
- [x] 7.3 Repeated file saves coalesce into bounded refresh emissions
- [x] 7.4 Write tests for file-watch path filtering and refresh semantics helpers
