+++
id = "7cb05ca2-bbd8-437f-ae5a-105f8e597a70"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Provider Routing Spec

## ProviderInventory

### Scenario: Inventory probes all credential sources
Given auth::PROVIDERS has entries for anthropic, openai, groq, ollama
When probe_inventory() runs
Then each provider has a ProviderStatus with has_credentials: bool
And providers with valid env vars or auth.json entries show has_credentials: true
And the probe completes in under 500ms

### Scenario: Inventory includes Ollama model detail
Given Ollama is running at localhost:11434
When probe_inventory() queries /api/tags
Then the Ollama provider entry includes a list of installed model names and sizes
And if /api/ps is reachable, running models with VRAM usage are captured

### Scenario: Inventory refreshes on credential change
Given a ProviderInventory was probed at startup
When /login adds credentials for a new provider
Then refresh_inventory() updates that provider's has_credentials to true

## CapabilityTier and ProviderRouter

### Scenario: Route returns ranked candidates
Given inventory has anthropic (creds), groq (creds), ollama (reachable)
When route(CapabilityRequest { tier: Mid, .. }, inventory) is called
Then result is a Vec of ProviderCandidate sorted by score descending
And at least one candidate is returned

### Scenario: Route respects tier requirements
Given inventory has ollama (8B model only) and anthropic (creds)
When route(CapabilityRequest { tier: Frontier, .. }, inventory) is called
Then anthropic ranks above ollama
And ollama may appear as a fallback but with a lower score

### Scenario: Route with no credentials returns empty
Given inventory has no providers with credentials and Ollama is not running
When route(any request, inventory) is called
Then result is an empty Vec

### Scenario: auto_detect_bridge backward compatibility
Given the existing auto_detect_bridge(model_spec) function signature
When called with "anthropic:claude-sonnet-4-6"
Then it returns the same result as before (Some(AnthropicClient) or None)
And no callers need to change

## BridgeFactory

### Scenario: Factory creates bridge for known provider
Given a BridgeFactory with access to credentials
When create_bridge("groq", "llama-3.3-70b") is called
Then an OpenAICompatClient with base_url "https://api.groq.com/openai" is returned

### Scenario: Factory caches warm bridges
Given a bridge was previously created for provider "groq"
When create_bridge("groq", "llama-3.3-70b") is called again
Then the same bridge instance is returned (no new HTTP client created)
