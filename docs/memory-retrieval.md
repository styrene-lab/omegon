# Identified memory retrieval and index repair

Memory recall combines keyword retrieval with optional identified vectors and a
bounded one-hop relationship expansion. Retrieval scores are ranking signals,
not probabilities that a fact is true.

## Embedding-space contract

A usable vector identifies its model, revision, preprocessing pipeline, and
dimensions. Stored vectors also retain a SHA256 of the exact fact content used
for generation. Matching dimensions or model names alone do not establish
compatibility. Case or whitespace changes can invalidate the content fingerprint
even when the fact-deduplication hash remains unchanged.

- **Ollama:** the host checks the selected model's documented `/api/tags` content
  digest before and after generation. Remote aliases without a verifiable local
  content digest are unavailable for identified retrieval. This assumes an honest,
  stable server during the request; it is not execution attestation. Prefer stable
  model tags during repair and retrieval.
- **Local ONNX:** the host hashes the exact model/tokenizer bytes loaded into the
  session and identifies its masked-mean/L2 preprocessing pipeline. Models must be
  self-contained and provide the supported sentence-transformer output shape;
  implicit external-weight dependencies are not used. Artifact input is bounded.
- **Other implementations:** implement `EmbeddingService::embed_identified` to
  participate. Its default returns unavailable, preserving keyword retrieval.

The space's model identifier is namespaced by the implementation. It is not
necessarily the raw model selector accepted by that provider's HTTP API.

## Readiness and degradation

Per-fact index inspection distinguishes:

| State | Meaning |
|---|---|
| Missing | No vector exists |
| Legacy | Identity or source fingerprint is unknown |
| Incompatible | The vector belongs to another model/revision/pipeline/dimension space |
| Stale | The fact's raw content changed after embedding |
| Ready | Identity, source content, and stored vector representation agree |

`memory_recall` returns typed vector diagnostics with its keyword results. A legacy
or incompatible vector is skipped before similarity comparison. Corrupt identified
vectors and operational storage failures are errors, not successful empty results.

The legacy backend vector-query APIs remain callable but native backends return
`EmbeddingIdentityRequired`. New callers use `search_identified` and
`MemoryMutation::StoreIdentifiedEmbedding`. Legacy managed hybrid requests retain
their result envelope and use keyword fallback without an identity. Modern callers
request diagnostics explicitly.

## Repair the index

```bash
omegon --cwd /path/to/project embedding backfill
```

Backfill honors the selected project directory. It reads bounded keyset pages,
discovers the selected embedding space, skips ready vectors, and checks fact versions
before storing replacements. Generation has a deadline. Provider unavailability or
an embedding-space change ends the run with an incomplete/error result. Source facts
are not reinforced or rewritten by repair.

Each run has a distinct operation namespace; retries within a run remain
payload-bound. A later explicit run can repair a missing index row even if an older
successful operation receipt exists. Concurrent fact changes reject stale writes.
This command is an explicit repair path, not an automatic durable job scheduler.

Schema v10 adds nullable space and source-fingerprint columns to fact vectors.
The current schema-v11 backup/verification migration supports v5–v10 stores and preserves
unknown identity on legacy vectors. Initialized project stores migrate before
startup opens them. Separately managed stores require the explicit migration workflow.

## Scores and relationships

Results expose lexical relevance, cosine similarity, reciprocal-rank fusion, and
graph proximity as separate fields. Tool output names the available signals instead
of presenting one universal match percentage.

Relationship evidence includes the edge, the other fact, the original relationship,
and direction relative to the returned fact. Contradictions label conflicts on both
seeded and expanded results; they do not boost existing seeds or reinforce facts.
Current expansion does not follow supersession toward obsolete facts. Historical
expansion retains status labels and may follow the older history. Unknown relationships
do not imply support and are not used to discover new neighbors.

Vector selection retains only bounded top-k candidates while scanning. Graph
traversal is one hop with fixed seed, edge, neighbor-load, and evidence limits.
These bounds do not constitute a large-corpus latency guarantee; no ANN index or
model-quality benchmark claim is introduced by this change.
