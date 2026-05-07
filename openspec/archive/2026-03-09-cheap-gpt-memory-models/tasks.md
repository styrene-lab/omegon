+++
id = "378c6ab5-c2a9-4bb0-8885-b3b38e264e3f"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# cheap-gpt-memory-models — Tasks

## 1. Default memory extraction uses a cheap GPT cloud model

- [x] 1.1 Default extraction model on fresh session start
- [x] 1.2 Effort-tier override still wins
- [x] 1.3 Write tests for Default memory extraction uses a cheap GPT cloud model

## 2. Semantic retrieval uses cloud embeddings by default

- [x] 2.1 Cloud embeddings available
- [x] 2.2 Cloud embedding writes vectors
- [x] 2.3 Write tests for Semantic retrieval uses cloud embeddings by default

## 3. Graceful degradation is preserved

- [x] 3.1 Cloud embeddings unavailable
- [x] 3.2 Extraction model remains cloud-only
- [x] 3.3 Write tests for Graceful degradation is preserved

## 4. Concrete default memory models are explicit and configurable

- [x] 4.1 Default model constants are visible
- [x] 4.2 Write tests for Concrete default memory models are explicit and configurable
