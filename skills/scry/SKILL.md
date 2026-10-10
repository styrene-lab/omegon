---
name: scry
description: Use for explicitly requested Scry image generation or refinement with the admitted Scry extension.
activation: project_detected
profile: [design]
project_signals: [crates/scry/src/tools.rs]
triggers: [scry, generate an image, image generation, refine an image]
---

# Scry image tasks

Apply this guidance only to an authorized image task using the actual Scry
extension. Confirm the admitted tool owner and schemas; a tool named `generate`
alone does not identify Scry. Skill applicability does not grant tools, downloads,
installation, or permission to act.

Before Scry `generate` or `refine`, call its admitted `list_models`. Do not guess
model names. Discover LoRAs with `list_models` using `kind: "lora"`; LoRAs apply
in the supplied order. Follow the actual schema for prompts, dimensions, refinement
strength, and output arguments. Report the returned image output path.

For optional prompt craft and the versioned usage reference, read
`references/usage.md` relative to the **Base directory** reported by `skills_get`.
The reference describes Scry capabilities, not the currently admitted tool set.
