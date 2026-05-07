+++
id = "a5057e27-827a-48c9-b359-6312dea32fcf"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# OpenSpec

Spec-driven development artifacts for Omegon.

## Structure

```
openspec/
├── baseline/       # Merged specs from completed changes
├── changes/        # Active changes (proposal → spec → design → tasks)
│   └── <name>/
│       ├── proposal.md
│       ├── specs/<domain>/spec.md
│       ├── design.md
│       ├── tasks.md
│       └── api.yaml          # OpenAPI 3.1 (if change involves an API)
└── archive/        # Completed changes (YYYY-MM-DD-<name>/)
```

## Workflow

See `skills/openspec/SKILL.md` for the full lifecycle and commands.
