# Architecture modeling working agreement

## Intent and evidence

The model is authoritative for accepted architecture intent. Distinguish:

- **Observed**: source-backed behavior, with evidence and simplifications stated.
- **Proposed**: an option awaiting a decision.
- **Accepted**: agreed architecture intent, whether implemented or not.

The current model is an observed, source-backed, simplified grouping. Its contents
are not automatically accepted norms. Record any gap between accepted intent and
implemented behavior explicitly, with links to the decision and source evidence.
Examples illustrate the method; they do not establish decisions.

## Concerns, decisions, and closure

Record concerns only when they arise, using the existing design-node workflow.
This agreement creates no concerns and no parallel backlog.
Use the repository's resolved design location: currently `docs/`, as selected by
[`design_docs_dir`](../../../core/crates/omegon/src/paths.rs).
Do not assume every design node belongs under `docs/design/`.

For each concern, record the question, source evidence, rationale or invariant,
and stable model references. Use fully qualified element names, relationship
endpoint names plus purpose, and view IDs. For example,
`omegon.host.memory → omegon.memory: Stores and retrieves retained knowledge`
in `omegonInternals` identifies a relationship without relying on a generated ID.

Bind resolved decisions to OpenSpec requirements, scenarios, and implementation
tasks, using stable IDs where available. Keep these links with the existing
design and specification artifacts. Workbench projects work state; it is not the
task authority.

Implementation closure requires relevant tests, runtime evidence, and
reconciliation of the model, specifications, and implementation. A status label
is not proof. There is currently no automated LikeC4 conformance check against
the implementation.

## Shared runtime review

For this modeling exercise, use only the shared preview at
<http://127.0.0.1:5173>, serving the canonical checkout's `site/diagrams/likec4/`
after promotion: `~/workspace/styrene-lab/omegon/site/diagrams/likec4`.

After changes, verify both the live model and the rendered named view route.
Check relationship ownership and projection, not just HTTP success. Run the
pinned formatter check and full diagram validation as separate checks.
Do not use a parallel preview or a static build as acceptance evidence.

If the shared watcher is stale, a controlled restart of that exact source
preview on the same port is authorized during this exercise. Verify its process
identity and working directory first. Leave the shared preview running for the
operator. Close owned headless review browsers after use. Do not affect unrelated
processes or the operator's desktop.
