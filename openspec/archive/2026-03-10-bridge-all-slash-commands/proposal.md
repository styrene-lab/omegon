+++
id = "f99331cd-aaa7-4ab3-a8ac-8ad352518b1f"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Bridge all pi-kit slash commands through SlashCommandBridge

## Intent

Convert all pi-kit slash commands to use the SlashCommandBridge so the agent can invoke them via execute_slash_command. Currently only /assess is bridged, causing repeated failures when the agent tries to invoke /opsx:verify, /opsx:archive, /opsx:status, etc. Each command gets a structuredExecutor returning a machine-readable result envelope, bridge metadata declaring agentCallable and sideEffectClass, and the interactive handler renders from the structured result. Commands that are interactive-only (/dashboard toggle) can be registered with agentCallable: false.

## Scope

<!-- Define what is in scope and out of scope -->

## Success Criteria

<!-- How will we know this change is complete and correct? -->
