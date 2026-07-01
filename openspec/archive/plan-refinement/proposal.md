# Plan Refinement

## Intent

Thread Omegon's small in-session work plan/tasklist through the lifecycle and OpenSpec design system without turning it into a competing durable task store.

## Problem

The current small plan is useful because it is lightweight and visible during execution, but lifecycle-backed work already has durable state in design-tree nodes and OpenSpec tasks files. Operators need a UX that makes the relationship explicit: when a plan is ephemeral, when it is projected from lifecycle artifacts, and when an action writes through to those artifacts.

Known risk: runtime plan state and plan mode can diverge, producing confusing completed, active, or cleared states.

## Goals

- Preserve lightweight session-local tasklists for simple work.
- Allow spec/design-backed work to surface as the same compact tasklist UX.
- Disclose the plan source and binding in the TUI/dashboard surfaces.
- Prevent silent destructive lifecycle mutations from plan actions.
- Make completion state derive from task item state or a single atomic mutation path.

## Non-goals

- Do not require OpenSpec for every plan.
- Do not create a new durable task database.
- Do not tag or release as part of this change.

## Principle: OpenSpec tracks durable work, not just code

OpenSpec-backed plans may represent any durable, reviewable work with acceptance criteria or lifecycle value: research, design, operations, validation, documentation, review, and implementation. Code changes are one task intent, not the only valid OpenSpec-backed plan type.
