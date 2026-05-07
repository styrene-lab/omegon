+++
id = "f408f548-0768-4e07-aa89-33708ba4be83"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# providers/openai-family-routing — Delta Spec

## ADDED Requirements

### Requirement: OpenAI API authentication is distinct from ChatGPT/Codex OAuth

The harness must distinguish OpenAI API credentials from ChatGPT/Codex OAuth credentials in auth probing and operator-facing capability surfaces.

#### Scenario: OpenAI API is not marked authenticated when only ChatGPT OAuth exists
Given stored credentials exist for `openai-codex`
And no `OPENAI_API_KEY` is present
When the harness probes provider authentication state
Then the `openai` provider is reported as unavailable
And the `openai-codex` provider is reported as available
And any operator-facing auth summary distinguishes `OpenAI API` from `ChatGPT/Codex OAuth`

#### Scenario: OpenAI API models are gated by OpenAI API credentials
Given no `OPENAI_API_KEY` is present
And stored credentials exist for `openai-codex`
When the model selector is built
Then it does not advertise `openai:*` models as OpenAI API-backed choices solely because ChatGPT OAuth exists
And it may advertise ChatGPT/Codex-backed GPT-family choices under the concrete `openai-codex` route

### Requirement: GPT-family requests resolve to a viable OpenAI-family provider honestly

When the operator expresses GPT-family model intent, the router may choose a different concrete provider within the OpenAI family if that is the executable route, but it must surface the concrete route honestly.

#### Scenario: GPT-family request falls through to Codex when OpenAI API is unavailable
Given no OpenAI API credentials are available
And ChatGPT/Codex OAuth credentials are available
When the harness resolves a GPT-family request expressed as `openai:gpt-5.4`
Then it selects `openai-codex` as the concrete provider
And it preserves the GPT-family model intent when invoking the executable client
And the active engine surface reports that the request is executing via `openai-codex`

#### Scenario: OpenAI API remains the concrete provider when API credentials are available
Given OpenAI API credentials are available
When the harness resolves a GPT-family request expressed as `openai:gpt-5.4`
Then it selects `openai` as the concrete provider
And the active engine surface reports execution via `openai`

### Requirement: Active engine surfaces show concrete provider identity inside the OpenAI family

The operator must be able to tell which provider, model, and auth method is actually active for the current conversation.

#### Scenario: Active engine display distinguishes OpenAI API from ChatGPT/Codex OAuth
Given the current conversation is executing through an OpenAI-family provider
When the harness renders its active engine or provider status surface
Then it includes the concrete provider identity
And it includes the concrete model identity
And it distinguishes `API key` execution from `OAuth` execution within the OpenAI family

#### Edge Cases
- A bare model such as `gpt-5.4` with only ChatGPT/Codex OAuth available → resolves to the executable OpenAI-family provider without falsely reporting `openai` authentication
- Both OpenAI API and ChatGPT/Codex OAuth available → the preferred concrete route is reported consistently before and after a `/model` switch
- A previously selected OpenAI API model remains visible in history after credentials disappear → the next execution attempt reports the new concrete provider or availability state honestly
