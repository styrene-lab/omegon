# inference/catalog-unification — Delta Spec

## ADDED Requirements

### Requirement: Model catalog projects from the inventory snapshot

The operator-visible model catalog (`ModelCatalog`) MUST be a projection of the active `InventorySnapshot`, not a direct read of the embedded `ModelRegistry`. Auth gating (a provider section appears only when credentials resolve) is preserved. The embedded registry remains the lowest-precedence bootstrap layer inside the inventory; it ceases to be an independent catalog source.

#### Scenario: Catalog reflects discovered offerings
Given a merged snapshot containing 29 Copilot offerings from the Discovery layer
And github-copilot credentials resolve
When the model catalog is built
Then the GitHub Copilot section lists the selectable subset of those 29 offerings
And not the 4-entry static registry list

#### Scenario: Catalog read is non-blocking
Given no discovery fetch has ever completed and no cache exists
When the model catalog is built
Then it returns immediately using bootstrap-layer (embedded registry) offerings
And no network request is made on the catalog build path

#### Scenario: Interface-incompatible offerings are filtered from chat selection
Given the Copilot discovery layer includes embedding and internal model ids (e.g. `text-embedding-3-small-inference`, `trajectory-compaction`)
When the catalog projects chat-selectable models
Then offerings failing chat-modality compatibility are excluded from the selection list

#### Scenario: Ollama local remains dynamic through the same pipeline
Given `ollama list` reports locally installed models
When the discovery refresh runs
Then Ollama offerings are produced as a Discovery-source layer like other providers
And the catalog's Ollama section matches the installed set

### Requirement: Operator-visible freshness and refresh control

The model selection surface MUST expose when each provider's inventory was last confirmed live (discovery timestamp, cached vs fresh) and MUST offer an explicit refresh action that forces TTL-bypassing rediscovery for connected providers.

#### Scenario: Explicit refresh bypasses TTL
Given cached unexpired discovery layers for connected providers
When the operator invokes the model list refresh action
Then all connected enumerable endpoints are refetched regardless of TTL
And the surface reports per-provider success or retained-last-known-good failure

#### Scenario: Stale-cache provenance is visible
Given a provider whose last successful discovery is older than its TTL and whose refresh attempts are failing
When the operator views the model list
Then that provider's section indicates the inventory is cached/stale with its last-confirmed timestamp
