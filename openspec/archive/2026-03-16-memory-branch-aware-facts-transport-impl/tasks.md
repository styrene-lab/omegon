# memory-branch-aware-facts-transport-impl — Tasks

## 1. startup import still seeds live memory from tracked transport

- [x] 1.1 startup import seeds an empty or stale DB
- [x] 1.2 Write tests for startup import still seeds live memory from tracked transport

## 2. tracked facts transport is not rewritten on ordinary session shutdown

- [x] 2.1 branch-local session work does not dirty tracked transport by default
- [x] 2.2 Write tests for tracked facts transport is not rewritten on ordinary session shutdown

## 3. memory transport can be exported explicitly

- [x] 3.1 explicit export writes deterministic tracked transport
- [x] 3.2 Write tests for memory transport can be exported explicitly

## 4. memory transport drift is reported separately from lifecycle artifact blockers

- [x] 4.1 incidental memory drift does not masquerade as a lifecycle-doc failure
- [x] 4.2 Write tests for memory transport drift is reported separately from lifecycle artifact blockers
