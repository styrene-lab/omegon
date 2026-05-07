+++
id = "041c4ad1-eebd-4e8d-81b8-ac329b8b44f5"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Ollama Model Management Spec

## OllamaManager

### Scenario: List installed models
Given Ollama is running at localhost:11434
When list_models() is called
Then a Vec of OllamaModel with name, size_bytes, and modified_at is returned
And the list matches what /api/tags reports

### Scenario: List running models with VRAM
Given Ollama has a model loaded in VRAM
When list_running() is called
Then a Vec of RunningModel with name, vram_bytes, and expires_at is returned
And the data matches what /api/ps reports

### Scenario: Ollama not running returns empty
Given Ollama is not running
When list_models() is called
Then Ok(empty vec) is returned, not an error
And a warning is logged

### Scenario: Hardware profile detection
Given the system has 64GB unified memory (Apple Silicon)
When hardware_profile() is called
Then total_memory_bytes is approximately 64GB
And recommended_max_model_params is set (e.g. "32B" for 64GB)

### Scenario: is_reachable replaces TCP connect
Given OpenAICompatClient::from_env_ollama previously used bare TCP connect
When OllamaManager::is_reachable() is used instead
Then it checks /api/tags with a 200ms timeout
And returns bool without creating an inference client
