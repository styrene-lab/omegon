# Design: Intent-Centered Permission Architecture

## Positioning

Permissions should mediate structured filesystem intents, not raw path strings. The boundary checker remains the final guard, but it should receive intent and provenance from upstream extractors instead of inferring operator meaning from opaque strings.

Current path:

```text
raw path string -> boundary check -> permission prompt
```

Target path:

```text
structured intent -> resolved path -> policy decision -> provenance-rich mediation -> executor
```

## Core Types

### FsIntent

```rust
pub struct FsIntent {
    pub operation: FsOperation,
    pub target: PathTarget,
    pub actor: IntentActor,
    pub source: IntentSource,
    pub confidence: IntentConfidence,
}
```

### FsOperation

```rust
pub enum FsOperation {
    Read,
    Write,
    Append,
    CreateDir,
    Delete,
    Move,
    Copy,
    ExecuteFrom,
    TerminalTranscriptWrite,
}
```

### PathTarget

```rust
pub enum PathTarget {
    WorkspaceRelative { raw: String },
    HostAbsolute { raw: String },
    HomeRelative { raw: String },
    SpecialDevice { raw: String },
    FileDescriptor { raw: String },
    Unknown { raw: String },
}
```

### IntentSource

```rust
pub enum IntentSource {
    ToolArgument { tool: String, field: String },
    NativeCommand { command: String, argv_index: usize },
    ShellRedirect { command_excerpt: String, redirect_op: String },
    ShellCommandArgument { command_excerpt: String, command_name: String, argv_index: usize },
    RuntimeInternal { subsystem: String },
}
```

### IntentConfidence

```rust
pub enum IntentConfidence {
    Exact,
    Parsed,
    Heuristic,
    Inferred,
}
```

## Path Resolution

Path resolution should produce diagnostics, not only allow/deny.

```rust
pub struct ResolvedFsTarget {
    pub raw: String,
    pub expanded: PathBuf,
    pub canonical: PathBuf,
    pub relation: WorkspaceRelation,
    pub warnings: Vec<PathWarning>,
}
```

Warnings include:

- `RootDotPath`: `/.omegon`, `/.git`, `/.cargo`, `/.config` and similar paths that commonly indicate an accidental leading slash.
- `ShortRootPath`: paths like `/Ig` with one short root component, especially from heuristic shell extraction.
- `LooksTruncated`: diagnostic classification for suspicious low-confidence extractions.

Do not auto-correct these paths. The system may suggest `.omegon/...`, but execution must use the path the command actually requested unless the agent rewrites and reruns a corrected command.

## Bash and Terminal Extraction

Replace the direct scanner shape:

```rust
scan_boundary_violations(command, boundary, cwd) -> Vec<String>
```

with an intent extractor:

```rust
extract_shell_fs_intents(command, cwd) -> Vec<FsIntent>
```

The existing scanner can remain as a compatibility wrapper while call sites migrate.

Extraction sources:

- shell redirects: `>`, `>>`, `2>`, etc.
- `tee` and `tee -a` arguments;
- `cp`, `mv`, `install` destination arguments;
- `mkdir` target arguments;
- `rm` target arguments.

Regex-based extraction must mark confidence as `Heuristic` or `Parsed` depending on how much syntax context is known. A later shell parser can upgrade confidence.

## Policy

Policy decisions should be based on intent, resolved target, relation, warning set, and confidence.

Rules for 0.27.8:

1. Inside-workspace targets are allowed.
2. Trusted external targets are allowed according to existing trusted-directory settings.
3. Exact outside-workspace targets still produce permission prompts.
4. Low-confidence suspicious shell-derived paths block with diagnostic text rather than normal approval prompts.
5. Root-dot paths produce correction-oriented diagnostics and should not offer dangerous persistent root grants.

## UX / Mediation

Permission prompts and blocked tool results should name:

- operation;
- raw path;
- resolved/canonical path if safe to show;
- workspace root;
- source/provenance;
- confidence;
- suspicious warnings;
- recommended action.

Example for `/.omegon/runtime`:

```text
Suspicious absolute path outside workspace
Operation: CreateDir
Path: /.omegon/runtime
Source: mkdir argument from: mkdir -p /.omegon/runtime
Warning: This looks like workspace-relative .omegon/runtime with an accidental leading slash.
Recommended: deny and rerun with .omegon/runtime.
```

Example for `/Ig`:

```text
Blocked suspicious filesystem intent
Path: /Ig
Source: heuristic shell scan
Reason: short root path from low-confidence extraction; likely malformed command text.
Action: rewrite the command with an explicit valid path.
```

## Migration Strategy

1. Add types and pure path-warning classifiers.
2. Convert bash scanner to produce structured intents internally; keep wrapper for existing call sites.
3. Update bash and terminal preflight to evaluate intents through a small policy function.
4. Add provenance to permission errors or blocked results.
5. Migrate exact tools (`read`, `write`, `edit`) after bash/terminal behavior is stabilized.
