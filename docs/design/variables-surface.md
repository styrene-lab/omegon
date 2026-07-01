+++
title = "Variables Surface"
tags = ["design","variables","runtime-config"]
+++

# Variables Surface

## Overview

Introduce `/variables` as the non-secret runtime configuration surface for Omegon. Secrets remain recipe-resolved sensitive values that are never printed; variables are plain, non-secret configuration values that may be displayed, imported, exported, and injected into controlled runtime processes.

## Problem

Operators currently reach for `/secrets` whenever they need to make a value available to the agent or subprocesses. That conflates two different domains:

- credentials and sensitive material, which should remain recipe-resolved and redacted
- ordinary runtime configuration, which should be visible, editable, listable, and injected without depending on the parent terminal environment

This is especially important for OCI deployments, where environment variables are the common configuration surface.

## Design intent

`/secrets` owns sensitive values and recipes:

- `env:VAR`
- `cmd:COMMAND`
- `vault:PATH`
- `file:PATH` when sensitive
- `keyring:NAME` for hidden-input literals stored in the OS keychain

`/variables` owns non-secret configuration:

- session/project/user key-value pairs
- inherited environment imports
- `.env` scan/load workflows
- runtime injection into bounded processes

## Initial MVP

The first implementation slice should add:

- a variable store and resolver
- slash/control commands:
  - `/variables`
  - `/variables list`
  - `/variables get NAME`
  - `/variables set NAME VALUE`
  - `/variables delete NAME`
- session scope first, with project/user persistence reserved for the next slice
- visible values in readouts because variables are non-secret

## Second-order effects

### Runtime reproducibility

Once variables are a first-class surface, runs become easier to reproduce because the non-secret runtime configuration is visible in one place instead of being implicit in the parent shell. This helps OCI and daemon launches, but it also means variables need provenance and scope metadata so an operator can tell whether a value came from session state, project config, user defaults, inherited env, or `.env` import.

### Secret-boundary pressure

A `/variables env import --all` escape hatch will be attractive in containers, but host and CI environments often contain secret-looking values. The variable importer must treat secret-looking keys as suspicious by default and route them toward `/secrets`. The boundary is not “where did the value come from?”; it is “is the value safe to print and log?”

### Process-launch consistency

Once variables are injected into validators, tools, extensions, and package helpers, every process-launch surface needs to consume the same resolved variable map. Hidden per-surface env assembly would recreate the current terminal whack-a-mole problem.

### Debuggability and audit

Because variables are printable, the UI can expose effective values and shadowing. That is useful, but it creates audit obligations: outputs should identify scope/source and make it obvious when a session value is shadowing a project/user/inherited value.

### Configuration sprawl

Project/user/session variables can become a second configuration system. The MVP should avoid complex typed config or policy rules. Keep values as strings first; add typed metadata only when concrete consumers need it.

## Third-order effects

### Deployment posture

OCI-friendly env ingestion makes Omegon easier to deploy as a containerized agent daemon. It also makes the runtime less dependent on interactive shell setup, which is required for supervised launches, web/ACP frontends, and future multi-process modes.

### Tool trust model

If variables are injected into tool subprocesses, operator validators, and extension launches, then the variable resolver becomes part of the trust boundary. Secrets must not be bulk-injected through this path. Secret resolution should remain explicit, consumer-scoped, and redacted.

### Collaboration and project portability

Project variables in `.omegon/variables.toml` will make projects more portable, but only if the file is safe to commit. That reinforces the rule: anything sensitive belongs in `/secrets`, not `/variables`. The UI should label project variables as commit-visible.

### Future policy integration

Variables can become inputs to policy, profiles, and launch modes. For example, a profile could declare required variables or default imports. That is useful later, but the MVP should keep `/variables` operational and avoid binding it prematurely to profiles.

## Future import workflow

Add import convenience once the core surface exists:

- `/variables env` to preview inherited environment
- `/variables env import` to import safe non-secret-looking environment keys into session scope
- `/variables env import --all` as an explicit override
- `/variables dotenv scan`
- `/variables dotenv load .env --session`

Secret-looking names should be skipped by default and directed to `/secrets`.

## Open questions

- [assumption] Session scope is the right default for imported environment values.
- [assumption] Project-persistent variables should live in `.omegon/variables.toml`.
- [assumption] User-persistent variables should live in `~/.omegon/variables.toml`.
- [assumption] The first runtime injection target should be operator validator processes, followed by general tool subprocesses and extension launches.

## Decisions

- `/variables` and `/secrets` must remain separate surfaces.
- Variables are intentionally printable; secrets are intentionally non-printable.
- Hidden-input secrets are still recipes, with keyring-backed resolution, not ordinary variables.
- Session scope is the MVP persistence boundary; project/user persistence and imports come after the command/control surface exists.
- Variable resolution should eventually be shared by all process-launch surfaces rather than reimplemented per subsystem.
