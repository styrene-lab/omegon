# Cleave checkpoint parity and volatile memory hygiene

## Intent

Fix the structural gap where cleave_run bypasses dirty-tree preflight and still hard-fails on dirty trees, reduce repeated interruptions from tracked volatile files such as .pi/memory/facts.jsonl, and replace the fragile multi-prompt checkpoint approval flow with a single structured confirmation surface.

## Scope

<!-- Define what is in scope and out of scope -->

## Success Criteria

<!-- How will we know this change is complete and correct? -->
