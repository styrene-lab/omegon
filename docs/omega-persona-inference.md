+++
id = "13705307-3f46-4fdf-a398-2b1bfb28a7ea"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omega persona inference — automatic domain detection and persona activation

## Overview

> Parent: [Persona System — domain-expert identities with dedicated mind stores](persona-system.md)
> Spawned from: "How does omega self-injection work — when the harness internalizes its own identity, does it auto-select/adapt persona based on project context?"

*To be explored.*

## Research

### Operator's PCB design scenario

The key scenario: "I am using omegon as a systems engineering harness to design a PCB. Therefore, the main persona I need loaded is a set of XYZ skills/extensions/tools that are relevant to that. That will be a combination of markdown skills, and a mind populated with research in advance when initialized to make sure it's not markdown-pretending-to-be-engineering."

This implies two modes:
1. **Explicit**: operator activates a persona via settings/command (`/persona pcb-designer`)
2. **Inferred**: omega detects project context (KiCad files, `.kicad_pcb`, Gerber outputs) and suggests or auto-loads the relevant persona + skills

The "not markdown-pretending-to-be-engineering" constraint is critical — a PCB persona's mind store must contain actual domain knowledge (IPC standards, common trace width calculations, thermal relief patterns, component datasheets) pre-populated before the session starts. The markdown skill is the behavioral overlay; the mind is the knowledge substrate.

### Inference mechanism: file signature → plugin suggestion

The simplest inference model that doesn't over-reach:

1. During `/init` or first session in a project, scan the working directory for file signatures
2. Match against installed plugins' `plugin.toml` file association declarations:
   ```toml
   [detect]
   file_patterns = ["*.kicad_pcb", "*.kicad_sch", "*.kicad_pro", "fp-lib-table"]
   directories = ["gerbers/", "fabrication/"]
   ```
3. If a match is found, **suggest** — don't auto-activate: "Detected KiCad project files. Activate PCB Designer persona? [Y/n]"
4. Once confirmed, persist the choice in project settings so it auto-activates on future sessions

This is strictly suggest-then-confirm, never silent activation. Operator agency (Lex Imperialis #6) means the harness proposes, the operator decides. Auto-activation only after explicit prior consent for that project.

The deeper "omega self-injection" concept — where the harness develops an internalized sense of domain and adapts automatically — is a stretch goal that depends on the memory system learning from session patterns. For now, file-signature detection covers 80% of the value.

## Open Questions

- How does omega self-injection work — when the harness internalizes its own identity, does it auto-select/adapt persona based on project context?
