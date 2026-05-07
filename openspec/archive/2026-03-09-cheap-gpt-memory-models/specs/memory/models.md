+++
id = "6294319b-44c1-4d90-80d0-a605e2c43c5e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# memory/models — Delta Spec

## ADDED Requirements

### Requirement: Default memory extraction uses a cheap GPT cloud model

Project memory background extraction and shutdown episode generation MUST default to a low-cost GPT-class cloud model instead of a local Ollama chat model.

#### Scenario: Default extraction model on fresh session start

Given project-memory starts with default configuration
When the extension initializes its memory config
Then `extractionModel` defaults to `gpt-5.3-codex-spark`
And extraction does not require Ollama to be running

#### Scenario: Effort-tier override still wins

Given project-memory has default extraction configuration
And the active effort tier requests a stronger non-local extraction tier
When extraction config is derived for a cycle
Then the effort-tier-selected extraction model overrides the default

### Requirement: Semantic retrieval uses cloud embeddings by default

Project memory MUST prefer a cheap cloud embedding model for fact and episode vectors so semantic retrieval does not depend on local Ollama embedding models.

#### Scenario: Cloud embeddings available

Given the environment is configured for cloud embeddings
When project-memory checks embedding availability
Then embeddings are reported as available without requiring Ollama
And the selected embedding model is `text-embedding-3-small`

#### Scenario: Cloud embedding writes vectors

Given cloud embeddings are available
When a fact or episode is embedded
Then the resulting vector is stored with the cloud embedding model ID
And semantic recall uses the stored vectors normally

### Requirement: Graceful degradation is preserved

If the configured cloud extraction or cloud embedding path is unavailable, project memory MUST continue operating with degraded behavior instead of failing the session.

#### Scenario: Cloud embeddings unavailable

Given cloud embeddings are not configured or the request fails
When project-memory initializes
Then memory tools remain available
And semantic retrieval falls back to keyword search
And startup does not fail

#### Scenario: Extraction model remains cloud-only

Given the default extraction model is a cloud GPT model
When a background extraction cycle runs
Then project-memory uses the configured cloud model through the existing subprocess path
And it does not attempt direct Ollama chat for extraction first

## MODIFIED Requirements

### Requirement: Concrete default memory models are explicit and configurable

The default cheap GPT choices MUST be explicit in code while still allowing operator overrides through configuration and existing routing behavior.

#### Scenario: Default model constants are visible

Given a developer inspects the project-memory configuration
When they read the default model settings
Then they can see `gpt-5.3-codex-spark` as the extraction default
And they can see `text-embedding-3-small` as the embedding default
And existing per-session overrides can still replace those defaults
