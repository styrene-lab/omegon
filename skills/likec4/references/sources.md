# Authoritative sources

Reviewed **2026-10-05**. Online documentation and upstream `main` can advance beyond a repository pin.
For syntax and flags, confirm compatibility with the installed CLI; the initial local verification used LikeC4 **1.59.4**.

## LikeC4

- [Tutorial](https://likec4.dev/tutorial/): incremental specification → model → relationships → views.
- [Introduction](https://likec4.dev/dsl/intro/): source extensions and project-wide merging.
- [Specification](https://likec4.dev/dsl/specification/): user-defined kinds and reusable defaults.
- [Model](https://likec4.dev/dsl/model/): identifiers, titles, descriptions, and hierarchy.
  The [Structuring Model section](https://likec4.dev/dsl/model/#structuring-model) permits arbitrary containment and demonstrates components nested within components.
  This documents DSL capability, not a C4 recommendation for recursive component decomposition.
- [References](https://likec4.dev/dsl/references/): lexical resolution and fully qualified names.
- [Relationships](https://likec4.dev/dsl/relationships/): directed interaction syntax, purpose labels, and kinds.
- [Views](https://likec4.dev/dsl/views/): model projections without a mandatory C4 hierarchy.
- [Predicates](https://likec4.dev/dsl/views/predicates/): ordered selection, scoped wildcards, implied/merged relationships, and layout.
- [CLI](https://likec4.dev/tooling/cli/): validation, formatting, preview, build, WASM/native layout, and browser exports.
- [Validation guide](https://likec4.dev/guides/validate-your-model/): optional model-API tests for custom architectural rules.
- [AI tools](https://likec4.dev/tooling/ai-tools/): official `likec4-dsl` skill and MCP model queries.
- [Documentation index](https://likec4.dev/llms.txt): discover current documentation links without guessing paths.
- [Upstream repository](https://github.com/likec4/likec4): implementation and release history.
- [Official skill](https://github.com/likec4/likec4/blob/main/skills/likec4-dsl/SKILL.md): inspected for workflow, syntax guardrails, and reference organization.

The official AI tools page recommends the `likec4-dsl` skill through its discovery installer.
This skill uses original prose and a small original example; it does not vendor that skill or run its installer.
Adopted guidance includes explicit project scope, specification reuse, stable identifiers, precise predicates, and CLI validation.
Optional MCP queries can inspect an existing model when an MCP server is already configured; setup is a separate task.

[Upstream issue #3162](https://github.com/likec4/likec4/issues/3162) reports skill/reference inconsistencies against 1.59.2,
including bidirectional model syntax, bare wildcard forms, file imports, and filtered-validation statistics.
Treat those reports as version-specific evidence, not proof of current behavior.
This local skill uses a validated directed-edge example and unfiltered project validation rather than copying those recipes.
The official documentation remains the conceptual reference; installed parser behavior determines compatibility.

## C4 model — Simon Brown

- [Software system](https://c4model.com/abstractions/software-system): user value, responsibility, and qualified ownership heuristics.
- [System context](https://c4model.com/diagrams/system-context): people, the system in scope, and directly connected external systems.
- [Container](https://c4model.com/abstractions/container): applications/data stores, runtime boundaries, and remotely hosted storage.
- [Component](https://c4model.com/abstractions/component): related functionality behind an interface, implemented by code elements within a container.
  The [component FAQ](https://c4model.com/abstractions/component#faq) distinguishes runtime units and functional partitioning from organizational packaging.
  JARs, DLLs, modules, packages, namespaces, and folders are not automatically components, although a one-to-one mapping is possible.
- [Abstraction FAQ — Can we add more abstraction levels?](https://c4model.com/abstractions/faq#can-we-add-more-abstraction-levels):
  extra levels often reflect misunderstanding or organizational constructs/groupings such as subsystems, bounded contexts, layers, and libraries.
  Genuine additional abstractions are allowed, but Brown calls this an advanced manoeuvre requiring precise definitions.
- [Notation](https://c4model.com/diagrams/notation): scope/title, responsibilities, directed labeled relationships, and a diagram key.

The skill's guidance against recursive component-within-component hierarchies synthesizes Simon Brown's distinction between architectural abstractions and organizational grouping.
It preserves standard system → container → component → code decomposition and meaningful boundaries in views.
The cited FAQs support this distinction but do not state the exact nested-components wording used in the skill.
No verbatim statement that nested components indicate organizational structuring was verified in this review; do not present the synthesis as a direct quotation.
Labeled view groupings and separate structural views are the skill's practical application of that distinction.

The C4 site credits Simon Brown and licenses its content under CC BY 4.0.
The guidance here is a concise attributed synthesis, with an original example rather than copied diagrams.
The LikeC4 site identifies its project license as MIT.
