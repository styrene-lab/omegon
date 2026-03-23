# Release milestone and feature freeze system — Design Spec (extracted)

> Auto-extracted from docs/release-milestone-system.md at decide-time.

## Decisions

### Tags + /milestone command, freeze enforced in implement action (decided)

Zero schema changes. Tags already exist on design nodes. /milestone is a query+display layer. Freeze check is one conditional in design_tree_update(implement). Prove the workflow before promoting to first-class fields.

## Research Summary

### Relationship to OpenSpec lifecycle


