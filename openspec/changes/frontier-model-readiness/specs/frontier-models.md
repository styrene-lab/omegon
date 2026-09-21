# Frontier models - Delta Spec

## ADDED Requirements

### Requirement: Named frontier routes use verified capabilities
The harness shall include Fable 5.1 and Astra in their native connected provider choices, with provider-specific limits and unchanged explicit selections.

#### Scenario: Choose a frontier provider
Given an operator connects Anthropic, OpenAI API or native Codex
When the model picker opens with default favorites
Then Fable 5.1 or Astra is included for that provider
And default frontier selection comes from the shared registry
And explicit saved selections and lower-cost grades remain unchanged

#### Scenario: Distinct Astra context limits
Given native OpenAI API and Codex Astra routes
When their metadata is resolved
Then the API route exposes the published 1050000-token context and 128000-token output
And the Codex route exposes its separately verified context ceiling

### Requirement: Astra executes the supported Responses contract
Direct OpenAI Astra tool requests shall use Responses, supported reasoning parameters and existing structured tool results.

#### Scenario: Stream a tool-bearing request
Given a selected direct OpenAI Astra route and a tool schema
When the bridge submits a turn
Then the request targets Responses with model gpt-6-astra
And streamed text and structured tool arguments reach the ordinary loop
And unsupported sampling and logprob fields are absent

#### Scenario: Continue after a tool call
Given a completed Astra response with encrypted reasoning, assistant phase and a tool call
When the tool result is submitted to the same route
Then the complete prior output items are replayed exactly once before the result
And opaque output from another provider or model is not replayed
And extra request fields cannot attach remote conversation history

#### Scenario: Native Codex selection
Given a selected Codex Astra route
When a turn is prepared
Then its model ID remains gpt-6-astra
And account access failures identify the selected route without silently switching models

### Requirement: Frontier effort choices are preserved
The operator shall be able to select xhigh and max. Astra requests shall normalize unsupported low-end reasoning choices to the provider's supported floor.

#### Scenario: Select maximum reasoning
Given an operator requests max thinking
When settings are applied and the Astra request is built
Then settings retain max
And the request uses max reasoning effort

#### Scenario: Select minimal reasoning
Given Astra and minimal thinking
When the request is built
Then the request uses low reasoning effort
And the reasoning-capable route receives the appropriate stall allowance
