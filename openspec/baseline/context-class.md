+++
id = "667efd86-7650-460c-9fc1-dc6dcc3a921b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# context-class

### Requirement: Context class classification maps token counts to named classes

#### Scenario: Token count below 128k maps to Squad

Given a token count of 100,000
When classifyContextWindow is called
Then the result is `Squad`

#### Scenario: Token count at exactly 128k maps to Squad

Given a token count of 131,072
When classifyContextWindow is called
Then the result is `Squad`

#### Scenario: Token count of 200k maps to Maniple

Given a token count of 200,000
When classifyContextWindow is called
Then the result is `Maniple`

#### Scenario: Token count of 272k maps to Maniple

Given a token count of 278,528
When classifyContextWindow is called
Then the result is `Maniple`

#### Scenario: Token count of 400k maps to Clan

Given a token count of 409,600
When classifyContextWindow is called
Then the result is `Clan`

#### Scenario: Token count of 1M maps to Legion

Given a token count of 1,048,576
When classifyContextWindow is called
Then the result is `Legion`

### Requirement: Route downgrade classification evaluates against context floor

#### Scenario: Compatible route — ceiling meets floor

Given a route envelope with contextCeiling 1,000,000 (Legion)
And a routing state with requiredMinContextWindow 200,000 (Maniple)
When classifyRoute is called
Then the classification is `Compatible`

#### Scenario: Degrading route — ceiling below floor

Given a route envelope with contextCeiling 131,072 (Squad)
And a routing state with requiredMinContextWindow 400,000 (Clan)
When classifyRoute is called
Then the classification is `Degrading`

#### Scenario: Ineligible route — fails tier or thinking constraints

Given a route envelope with tier `retribution`
And a requested tier of `gloriana`
When classifyRoute is called with tier mismatch
Then the classification is `Ineligible`

### Requirement: Downgrade policy enforces safety boundaries

#### Scenario: Auto-reroute when compatible route exists

Given an active route of Legion class
And the session required floor is Maniple
And a compatible candidate exists at Clan class
When evaluateDowngrade is called
Then recommendation is `auto-reroute` with the Clan route as target

#### Scenario: Operator confirmation required for multi-class drop

Given an active route of Legion class
And the only available candidate is Squad class
When evaluateDowngrade is called
Then recommendation is `operator-confirm`
And contextClassDelta is 3 (Legion→Squad)

#### Scenario: Pinned floor prevents compaction

Given a routing state with pinnedFloor of Clan
And the best available route requires compaction to Maniple
When evaluateDowngrade is called
Then recommendation is `operator-confirm`
And reason mentions pinned floor violation

### Requirement: Route matrix provides reviewed provider context ceilings

#### Scenario: Route matrix includes Anthropic and OpenAI routes

Given the checked-in route-matrix.json
When loadRouteMatrix is called
Then the result includes entries for providers `anthropic` and `openai`
And each entry has a numeric contextCeiling and a mapped contextClass

#### Scenario: Dynamic matrix built from registry matches available models

Given a model registry with claude-opus-4-6 (anthropic) and gpt-5.4 (openai)
When buildRouteMatrixFromRegistry is called
Then the result includes envelopes for both models with correct context ceilings

### Requirement: Routing session state tracks active capacity and required floor

#### Scenario: State initialized from resolved model

Given a resolved model on provider `anthropic` with 1M context ceiling
When initRoutingState is called
Then activeContextClass is `Legion`
And requiredMinContextClass defaults to `Squad`
And downgradeSafetyArmed is true

#### Scenario: Usage update adjusts headroom

Given a routing state with activeContextWindow 1,000,000
When updateUsage is called with observedTokens 600,000
Then headroom is approximately 400,000

#### Scenario: Pinning floor raises required minimum

Given a routing state with requiredMinContextClass `Squad`
When pinFloor is called with `Clan`
Then requiredMinContextClass becomes `Clan`
And requiredMinContextWindow is at least 400,000
