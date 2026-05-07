+++
id = "0e72764d-42ff-4d67-826d-428eecc60231"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Knowledge quadrant lifecycle — guide design progression through the Rumsfeld Matrix — Design Spec (extracted)

> Auto-extracted from docs/knowledge-quadrant-lifecycle.md at decide-time.

## Decisions

### Readiness score is a soft gate — advisory, not blocking (decided)

The dashboard shows the readiness score. The /assess design command reports it. But the operator can override — sometimes you need to move forward with acknowledged unknowns. A hard gate would force premature question-closing to satisfy the metric. A soft gate preserves operator agency (Lex Imperialis axiom 6) while making the knowledge state visible. The score is a conversation starter, not a bureaucratic checkpoint.

### Assumptions are open questions tagged [assumption] — not a separate section (decided)

Assumptions ARE unknowns — they're things we think are true but haven't validated. Keeping them in Open Questions with an [assumption] tag means: (1) they naturally block the readiness score the same way questions do, (2) the existing gate logic (zero open questions → can decide) already handles them without new code, (3) answering an assumption is the same workflow as answering a question — research it, then either confirm (→ decision) or deny (→ new question about the alternative). A separate section would create two pools of unknowns that need separate lifecycle management. One pool, two tags: [question] and [assumption].

## Research Summary

### The mapping — design tree artifacts to knowledge quadrants

Each design tree artifact type maps to a specific quadrant:

### Status transitions as quadrant movements

The design tree status machine maps to quadrant migration:

```
  seed ──────► exploring ──────► resolved ──────► decided ──────► implementing
   │               │                │                │                │
   │     surface   │    research    │   validate     │   record       │
   │     unknowns  │    answers     │   assumptions  │   everything   │
   │               │                │                │                │
   ▼               ▼                ▼                ▼               …

### seed → exploring

**Quadrant shift:** Unknown unknowns → Known unknowns
**What happens:** The node is created. Initial questions are articulated. The act of writing the overview surfaces the first known unknowns. Research begins.
**Gate:** At least one open question exists (you know what you don't know).

### exploring → resolved

**Quadrant shift:** Known unknowns → Known knowns
**What happens:** Research answers questions. Open questions get removed as they're answered. Decisions are recorded. The known-knowns column grows.
**Gate:** Zero open questions remain. Everything asked has been answered.

### resolved → decided

**Quadrant shift:** Unknown knowns → Known knowns (the critical step)
**What happens:** This is where assumptions get challenged. The `/assess design` step is an adversarial review that asks: "What are you assuming but haven't stated?" It surfaces unknown knowns (implicit assumptions) and either records them as explicit decisions or creates new open questions.
**Gate:** Decisions cover all identified considerations. Acceptance criteria are falsifiable. No unexamined assumptions.

**This is the s…

### decided → implementing

**Quadrant shift:** All quadrants → Known knowns only
**What happens:** Implementation notes are added (file scope, constraints). The design is complete enough to build against. Any remaining unknowns would create new open questions (reopening the node).
**Gate:** Implementation notes exist. Constraints are documented. The OpenSpec change is scaffolded.

### What about unknown unknowns?

They surface at any stage:
- During exploring: research reveals an unexpected dependency → new open question
- During implementing: the build hits an unforeseen constraint → node reopens to exploring
- During review: adversarial assessment finds a scenario nobody considered → new open question
- Post-implementation: operator testing reveals a gap → new seed node

The design tree's reopen mechanism (implementing → exploring) is how unknown unknowns enter the system. The key property: **the lifecy…

### The missing piece — an Assumptions section for unknown knowns

The design tree has:
- **Open Questions** → Known unknowns ✓
- **Decisions** → Known knowns ✓
- **Research** → Discovery process (◌→?→✓) ✓

It doesn't have:
- **Assumptions** → Unknown knowns ✗

### Proposal: add an Assumptions section to design nodes

```markdown

### How this changes the agent's behavior during design

The quadrant model isn't just a visualization — it changes how the agent approaches design exploration.

### Today's behavior (implicit)

The agent explores a topic, adds research, adds questions when something is unclear, removes questions when answered, adds decisions when converging. The progression is felt but not formalized. The agent doesn't have a structured way to say "I think we're making assumption X" or "I suspect there are unknowns in area Y that we haven't explored."

### Proposed behavior (explicit quadrant awareness)

**During exploring:**
The agent explicitly operates in discovery mode. For each research finding, it asks: "Does this create a new known unknown (question) or surface an unknown known (assumption)?" It populates both sections.

Prompt guidance: "When exploring a design node, actively surface assumptions. For each decision or approach you're considering, state what you're assuming to be true but haven't verified. These become the Assumptions section."

**During the resolve → decide transition:**
…

### The organic progression

The beauty of this model: progression is NATURAL, not forced. A node moves from seed to decided not because someone clicked a button, but because the knowledge distribution shifted:

```
seed:       ◌◌◌◌◌◌◌◌  (mostly unknowns)
exploring:  ◌◌??✓✓⚠⚠  (unknowns surfacing as questions + assumptions)
resolved:   ??✓✓✓✓⚠⚠  (questions answered, assumptions visible)
decided:    ✓✓✓✓✓✓✓⚠  (everything known except acknowledged risks)
```

You can't force a node to decided — the quadrant distribution eithe…

### Integration with memory system

When an assumption is validated (becomes a decision), store it as a memory fact. When an assumption is invalidated, store the invalidation as a fact. This creates an organizational knowledge base of "things we learned were true" and "things we learned were NOT true" — both are valuable for future design work.

The memory system already has sections (Architecture, Decisions, Constraints). Add: **Assumptions** as a section. Facts in this section have a `validated: bool` field. Unvalidated assumpti…
