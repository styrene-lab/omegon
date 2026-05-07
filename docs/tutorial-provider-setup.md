+++
id = "866f95d3-7e7f-4865-a5af-965bb0b3d789"
kind = "document"
title = "Tutorial provider setup widget — 4-path guided onboarding for unconfigured users"
status = "exploring"
tags = ["tutorial", "onboarding", "providers", "ux", "0.15.1"]
aliases = ["tutorial-provider-setup"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = ["openrouter-provider", "startup-systems-check"]
open_questions = []
parent = "free-tier-tutorial"
priority = "1"
+++

# Tutorial provider setup widget — 4-path guided onboarding for unconfigured users

## Overview

When /tutorial launches with no providers configured, present a 4-option choice widget: Local (Ollama), Free (guided OpenRouter signup), Login (OAuth flow), API Key (direct entry). Each path guides the user through setup in 30-60 seconds, then flows into the normal tutorial. Only shows when systems check finds nothing — users with existing providers skip straight to the tutorial.

## Open Questions

*No open questions.*
