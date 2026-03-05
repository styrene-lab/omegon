# pi-kit Project Directives

> Global directives (attribution, completion standards, memory sync, branch hygiene) are defined in `~/.pi/agent/AGENTS.md` and apply to all sessions. This file adds pi-kit-specific context.

## Contributing

This repo follows trunk-based development on `main`. The full policy is in `CONTRIBUTING.md` — read it with the `read` tool if you need branch naming conventions, the memory sync architecture details, or scaling guidance.

Key points for working on pi-kit itself:

- **Direct commits to `main`** for single-file fixes, typos, config tweaks
- **Feature branches** (`feature/<name>`, `refactor/<name>`) for multi-file or multi-session work
- **Conventional commits** required — see `skills/git/SKILL.md` for the spec
- The `.gitattributes` in this repo declares `merge=union` for `.pi/memory/facts.jsonl`
- The `.pi/.gitignore` excludes `memory/*.db` files — only `facts.jsonl` is tracked
