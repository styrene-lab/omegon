+++
id = "287c3896-2483-4fca-a53f-a5692701a073"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# models/profile

### Requirement: Operator capability profile persists public role mappings in config

Omegon must support a durable operator capability profile stored in `.omegon/profile.json` that defines the full public capability ladder and maps each role to ordered concrete candidates.

#### Scenario: Default profile is synthesized when none exists
Given a project has no operator capability profile in `.omegon/profile.json`
When Omegon resolves a requested capability role
Then it synthesizes a conservative default profile
And the default profile includes the public roles `archmagos`, `magos`, `adept`, `servitor`, and `servoskull`
And silent fallback from upstream to heavy local candidates is not permitted by default

#### Scenario: Candidate objects preserve explicit thinking ceilings
Given an operator capability profile contains candidates for a role
When Omegon reads the profile
Then each candidate may declare `id`, `provider`, `source`, `weight`, and `maxThinking`
And Omegon preserves the candidate's `maxThinking` ceiling when that candidate is selected

### Requirement: Resolver applies fallback policy using role and source metadata

Omegon must resolve roles through ordered candidates and consult fallback policy before crossing into materially different execution paths.

#### Scenario: Same-role cross-provider fallback happens automatically when allowed
Given the `magos` role lists an Anthropic candidate first and an OpenAI candidate second
And Anthropic is temporarily unavailable
When Omegon resolves `magos`
Then it selects the OpenAI candidate if same-role cross-provider fallback is allowed

#### Scenario: Cross-source fallback requires operator approval when policy asks
Given the remaining viable fallback for a requested role is a local heavy candidate
And the profile policy says cross-source fallback is `ask`
When Omegon loses access to upstream candidates
Then it does not silently switch to the local heavy candidate
And it returns or surfaces an explanation that operator confirmation is required

### Requirement: Transient upstream failures enter runtime cooldown state

Omegon must treat transient provider failures during execution as temporary capability loss and feed them back into resolution.

#### Scenario: Anthropic 429 places the candidate in cooldown
Given a resolved candidate uses the Anthropic provider
When Omegon observes a transient 429 or rate-limit style failure for that candidate
Then Omegon records temporary runtime unavailability for that candidate or provider
And subsequent resolution attempts skip the cooled-down candidate until the cooldown expires
