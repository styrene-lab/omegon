---
name: likec4
description: Use when creating, reviewing, validating, or debugging C4 architecture models or LikeC4 .c4/.likec4 sources, views, predicates, and CLI workflows. Supports iterative beginner modeling from system context to focused detail.
---

# C4 and LikeC4 modeling

Build one evidence-backed view at a time. Explain the modeling choice before adding detail for a beginner.
Use this skill for architecture modeling and LikeC4 tooling, including incorrect boundaries, missing elements, and unexpected arrows.

## Establish the question

1. Read repository instructions, existing model sources, project configuration, and the package manifest and lockfile.
2. Identify the audience, the system in scope, and the question this view must answer.
3. Locate evidence for responsibilities and interactions. Record assumptions separately from established behavior.
4. Propose the smallest useful view. For a beginner, start with one system, its users, and directly connected external systems.
5. Validate and review that view before expanding another boundary.

Ask a focused question when an uncertain boundary changes the meaning of the diagram.
Do not automatically inventory every crate, generate every C4 level, or publish the model.

## Upstream concepts

### C4 scope and boundaries

- **Level 1: system context** shows the system of interest, people/roles, and directly connected software systems. Describe purpose and interactions.
- A **software system** delivers value to users. Define its cohesive responsibility and what belongs inside it.
- Team ownership, repository layout, and release cadence are evidence about a boundary, not sufficient definitions by themselves.
- A process, network hop, host, or remote service does not automatically establish a software-system boundary.
  For example, a remotely hosted database schema can remain an internal C4 container.
- **Level 2: containers** describes applications and data stores within a system. A C4 container does not mean a Docker container.
  Reserve code modules and implementation details for a suitable deeper view.
- Keep the selected abstraction level consistent. A context view should not become a process or deployment diagram.

### Component decomposition and organizational grouping

Preserve C4's standard decomposition: **software system → container → component → code**.
A component groups related functionality behind a well-defined interface within a container.
Keep system and container boundaries visible where they establish the scope of a view.

Avoid recursive component-within-component hierarchies. They usually signal organizational grouping rather than another architectural abstraction.
This applies Simon Brown's C4 guidance: additional levels often represent organizational constructs, and components describe functional partitioning rather than packaging.
See the [C4 abstraction FAQ](https://c4model.com/abstractions/faq#can-we-add-more-abstraction-levels) and [component FAQ](https://c4model.com/abstractions/component#faq).

If hierarchy is needed for subsystems, bounded contexts, layers, libraries, or folder structure, identify it explicitly as organizational grouping.
Use a labeled grouping in a view or a separate structural view, rather than inventing another component level.
For a genuine nonstandard architectural abstraction, document its rationale and precise definition before using it.
Simon Brown describes adding such levels as an advanced adaptation of C4.

LikeC4 supports custom kinds and arbitrary nesting, including components within components; that capability is not a recommendation to model components recursively.
Use it deliberately, and distinguish custom abstractions and organizational groupings from standard C4 levels.

### Specification, model, and views

| Part | Responsibility |
| --- | --- |
| `specification` | Declare reusable element kinds, relationship kinds, tags, and their defaults. Kind names such as `person` are user-defined. |
| `model` | Define actual elements, containment, responsibilities, and relationships. |
| `views` | Select and present the model for a particular question through ordered predicates. |

LikeC4 merges `.c4` and `.likec4` sources within a project. Inspect project configuration before choosing the CLI source directory.
Separate files do not require file-path imports. Use explicit fully qualified names across files and when short names could be ambiguous.

Keep identifiers stable and independent from display titles: `taskHub` identifies an element; `'Task Hub'` names it for readers.
Identifiers accept letters, digits, hyphens, and underscores, but cannot start with a digit or contain a dot.
Dots express hierarchy in references, such as `taskHub.api`; they are not part of an individual identifier.
Give views stable identifiers too: these affect navigation and exported filenames.

### Relationships

Read each edge as a sentence: **source → target: purpose**.
Use a specific verb phrase, such as “Submits work requests,” instead of “Uses” or a protocol name alone.
Put implementation protocols on detailed views when useful, rather than overwhelming the context view.

Use `->` for an initiated interaction. A request and its response usually need one dependency edge.
Do not add reciprocal arrows mechanically. Add a reverse edge only for a distinct interaction with its own meaning.
C4 notation recommends directed edges; LikeC4's available relationship forms can differ by version.
Check the pinned parser before using model-level bidirectional syntax from current online examples.

Define facts in the model and use view overrides only for presentation.
Containment already expresses “part of”; it does not require a communication arrow between parent and child.

### Predicates and implied relationships

- In an **unscoped** view, `include *` selects top-level elements, not every descendant.
- In `view detail of taskHub`, `include *` starts with the scoped element and its direct children.
  Connected external neighbors can also appear; inspect the result before calling it “internals only.”
- `include taskHub.*` selects direct children. `taskHub.**` selects recursive descendants subject to relationship visibility.
  `taskHub._` includes the parent and expands children connected to visible elements; it is not an “all elements” wildcard.
  Avoid bare `**` or `_`.
- Element inclusion also brings relationships between visible elements. Relationship predicates select endpoints of matching relationships.
- `include a <-> b` is a **view predicate** selecting relationships in either direction; it does not create a model relationship.
- Predicates run in order. An `exclude` removes earlier inclusions; a later `include` can add them again.
- Relationships involving hidden descendants can appear as **implied relationships** between their visible ancestors.
  Trace unexpected arrows to their underlying model relationships before adding duplicate high-level edges.
- Multiple underlying relationships can merge into one connection. A `[...]` label can indicate differing merged titles.
  Inspect those relationships before using a purposeful view label or separate edges.

Prefer explicit inclusion for a small teaching view. Use wildcard projections when their scope matches the intended audience.

## Minimal executable example

This fictional context model is complete. Save it as a `.c4` file in an isolated project directory to try it.
It demonstrates custom kinds, stable identifiers, descriptions, labeled relationships, and one named view.

```likec4
specification {
  element person {
    style {
      shape person
    }
  }
  element softwareSystem
}

model {
  operator = person 'Operator' {
    description 'Requests work and reviews results'
  }
  taskHub = softwareSystem 'Task Hub' {
    description 'Coordinates work requests and records outcomes'
  }
  identity = softwareSystem 'Identity Service' {
    description 'Verifies user identities'
  }

  operator -> taskHub 'Submits work requests and reviews outcomes'
  taskHub -> identity 'Verifies operator identity'
}

views {
  view index {
    title 'Task Hub — System context'
    include operator, taskHub, identity
  }
}
```

Here, `include *` would select the same three top-level elements.
Review the responsibility of `taskHub` before introducing applications or storage beneath it.
When adding notation, explain element kinds, shapes, colors, and line styles in a key; do not rely on color alone.

## Validate and iterate

1. Resolve the repository-pinned executable and run `--version` and the relevant subcommand's `--help`.
2. Run `validate` over the whole selected project after a model change. Use `--no-layout` for a quick syntax/semantic pass.
3. Run full `validate` before handoff. A skipped layout check is not a successful layout check.
4. Use `format --check` for a read-only formatting check. Apply `format` only to intended sources.
5. Preview locally and inspect labels, directions, implied edges, level consistency, and readability.
6. Change one modeling assumption or view rule at a time, then repeat the relevant check.

Use `validate`, not an invented `check` or `lint` subcommand. A static build is a separate artifact check.
File-filtered validation is useful for diagnosis, but cannot establish that the whole project is valid.
Do not infer validation coverage from `filteredFiles`; inspect diagnostics and the unfiltered result.
Parser success does not establish architectural truth or visual readability.

For debugging, first check kind declarations and identifier resolution, then isolate view predicates, then adjust layout.
Prefer reducing a crowded view over changing architecture facts to improve its appearance.
Use `autoLayout` or styles only after the selected elements and relationships are correct.

## Omegon repository conventions

These paths and operating choices are local conventions, not LikeC4 requirements.
The tooling lives in `site/package.json` and `site/package-lock.json`; sources live under `site/diagrams/likec4/`.
Check their current contents before using the commands below, which run from the repository root:

```sh
site/node_modules/.bin/likec4 --version
site/node_modules/.bin/likec4 validate --help
site/node_modules/.bin/likec4 format --help
site/node_modules/.bin/likec4 dev --help
site/node_modules/.bin/likec4 validate site/diagrams/likec4 --no-layout
site/node_modules/.bin/likec4 validate site/diagrams/likec4
site/node_modules/.bin/likec4 format site/diagrams/likec4 --check
```

For an interactive local preview, run:

```sh
site/node_modules/.bin/likec4 dev site/diagrams/likec4 --listen 127.0.0.1 --port 5173 --use-dot=false
```

Keep preview listeners on loopback. Stop owned preview processes after review.
Use bundled Graphviz WASM by default; native `dot` is an optional, deliberate choice for a demonstrated need.
The explicit `--use-dot=false` also avoids the CLI's different default inside containers.

Prefer the installed pinned binary or repository scripts over download-on-demand `npx` commands.
Do not install or upgrade tools, launch public previews, or publish as an incidental modeling step.
PNG/JPEG exports are optional browser-based checks requiring Playwright and a browser; they are not prerequisites for DSL validation.
If an export is requested, inspect `export png --help`, choose an explicit output directory, and report missing browser dependencies.
Use `build --help` before a requested static build. A local build does not publish the site.

## Provenance and compatibility

Verified **2026-10-05** with the installed LikeC4 **1.59.4** pin in `site/package.json`.
The example passed the pinned CLI validator. This version records verification, not a permanent minimum or upgrade policy.
Recheck the manifest, CLI help, and example when the pin changes.

This skill synthesizes the official LikeC4 tutorial, DSL documentation, and `likec4-dsl` skill, plus Simon Brown's C4 guidance.
It adopts model/view separation, explicit kinds and identifiers, predicate awareness, and validation after edits.
It adds the local, iterative workflow above. See [sources and upstream caveats](references/sources.md) for authoritative links.
