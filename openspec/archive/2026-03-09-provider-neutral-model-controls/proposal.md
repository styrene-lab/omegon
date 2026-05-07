+++
id = "7fe9a8fe-5f8f-4afa-9159-7b68bf32caee"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Provider-neutral model controls and driver persistence

## Intent

Update operator-facing model tier commands and related messaging so /haiku, /sonnet, /opus, and set_model_tier reflect provider-neutral multi-provider routing instead of reading as Anthropic-specific products. Persist the last explicitly selected driver model so new sessions restore the last used model instead of forcing a manual switch back after startup.

## Scope

<!-- Define what is in scope and out of scope -->

## Success Criteria

<!-- How will we know this change is complete and correct? -->
