+++
id = "3c259d79-e561-4915-bdc6-80867b51f288"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave Provider Integration Spec

## Per-child provider assignment

### Scenario: Children get individual model assignments
Given a cleave plan with 3 children of varying scope
And ProviderInventory has anthropic (creds) and ollama (8B model)
When the orchestrator dispatches children
Then each child's --model flag reflects its routed assignment
And ChildState.execute_model is populated for each child

### Scenario: Scope-based tier heuristic
Given a child with scope: ["README.md"] (1 file)
When infer_capability_tier(child) is called
Then the result is CapabilityTier::Leaf

Given a child with scope: ["src/main.rs", "src/lib.rs", "src/config.rs"] (3 files)
When infer_capability_tier(child) is called
Then the result is CapabilityTier::Mid

Given a child with scope: 6+ files
When infer_capability_tier(child) is called
Then the result is CapabilityTier::Frontier

### Scenario: Explicit executeModel in plan overrides heuristic
Given a ChildPlan with executeModel: "anthropic:claude-sonnet-4-6"
When the orchestrator resolves the model for this child
Then "anthropic:claude-sonnet-4-6" is used directly
And the router is not consulted

### Scenario: Fallback to global model when router has no candidates
Given ProviderInventory is empty (no credentials, no Ollama)
When the orchestrator tries to route a child
Then it falls back to CleaveConfig.model (the global default)
And the child is dispatched normally
