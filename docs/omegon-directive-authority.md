+++
id = "9faf9ed4-4157-4c2a-bc82-5f65c079496e"
kind = "document"
title = "Omegon directive authority — code-level opinions over filesystem discovery"
status = "implemented"
tags = ["architecture", "directives", "system-prompt", "opinions", "authority"]
aliases = ["omegon-directive-authority"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "test-coverage-directive-gap"
+++

# Omegon directive authority — code-level opinions over filesystem discovery

## Overview

Omegon is an opinionated engineering platform, not a flexible markdown tool. Its engineering opinions (testing, spec-first development, branch lifecycle, memory management) should be expressed as code-level authoritative directives, not as filesystem-discovered markdown files that compete with whatever random AGENTS.md or CLAUDE.md exists in a cloned repo.

Pi is the Black Carapace — the flexible neural interface. Omegon is the Power Armor — opinionated, protective, and directive. The armor's opinions should be expressed through the interface, not as additional files sitting alongside the interface.

Near-term: embed critical opinions as promptGuidelines on always-loaded tools.
Medium-term: session_start engineering standards injection via sendMessage.
Long-term (Omega): coordinator owns the system prompt composition with explicit priority layering.

See research in parent node (test-coverage-directive-gap) for the full directive provenance audit.

## Decisions

### Decision: Subsumed by Lex Imperialis in core-directives

**Status:** decided
**Rationale:** The Lex Imperialis (6 constitutional directives embedded at compile time in prompt.rs) is the implementation of this concept. Code-level opinions are now baked into the binary via include_str!(), not discovered from the filesystem. See core-directives node.

## Open Questions

*No open questions.*
