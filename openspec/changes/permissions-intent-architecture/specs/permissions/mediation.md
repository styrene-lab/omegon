# Permission Mediation — Delta Spec

## ADDED Requirements

### Requirement: Suspicious low-confidence paths are not ordinary approval prompts

The permission mediator MUST avoid presenting low-confidence suspicious shell scanner artifacts as normal persistent permission requests.

#### Scenario: Suspicious short root scanner hit is blocked diagnostically
Given a model-authored shell command produces a low-confidence extracted path `/Ig`
When the permission policy evaluates the intent
Then the operation is blocked with diagnostic provenance
And the operator is not offered a persistent grant for `/Ig`.

### Requirement: Legitimate external paths still use approval flow

The permission mediator MUST preserve existing external-path approval behavior for exact or non-suspicious requests.

#### Scenario: Legitimate outside-workspace write asks for permission
Given a shell redirect writes to `/etc/example`
When the permission policy evaluates the intent
Then the operator receives a permission request or block according to existing policy
And the prompt includes operation and source provenance.
