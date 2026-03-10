# cleave-checkpoint-parity — Tasks

## 1. cleave_run uses the same dirty-tree preflight as /cleave

- [x] 1.1 Tool path shows preflight instead of bare clean-tree failure
- [x] 1.2 Clean tree still proceeds directly
- [x] 1.3 Write tests for cleave_run uses the same dirty-tree preflight as /cleave

## 2. volatile-only dirty trees are handled separately from substantive drift

- [x] 2.1 facts.jsonl-only drift is classified as volatile
- [x] 2.2 volatile-only resolution does not require substantive checkpointing
- [x] 2.3 Write tests for volatile-only dirty trees are handled separately from substantive drift

## 3. project-memory avoids rewriting facts.jsonl when export content is unchanged

- [x] 3.1 unchanged export leaves facts.jsonl untouched
- [x] 3.2 Write tests for project-memory avoids rewriting facts.jsonl when export content is unchanged

## 4. checkpoint approval uses a single structured confirmation flow

- [x] 4.1 operator approves checkpoint in one confirmation step
- [x] 4.2 operator cancels checkpoint without side effects
- [x] 4.3 Write tests for checkpoint approval uses a single structured confirmation flow

## 5. volatile-only policy default

- [x] 5.1 volatile-only dirty tree can be resolved without repeated manual git choreography
- [x] 5.2 Write tests for volatile-only policy default

## 6. shared confirmation surface across execution modes

- [x] 6.1 /cleave and cleave_run present equivalent checkpoint approval semantics
- [x] 6.2 Write tests for shared confirmation surface across execution modes
