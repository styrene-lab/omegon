# Design: Intent-Centered Permission Architecture

## Positioning

Permissions should mediate structured filesystem intents, not raw path strings. The boundary checker remains the final guard, but it should receive intent, dialect, environment context, and provenance from upstream extractors instead of inferring operator meaning from opaque strings.

Current path:

```text
raw path string -> boundary check -> permission prompt
```

Target path:

```text
raw target + source context
  -> structured intent
  -> dialect-aware path target
  -> resolved target + mount/environment context
  -> policy decision
  -> provenance-rich mediation
  -> executor
```

The core invariant is:

> A path-like token must never be considered workspace-relative merely because the current host parser does not understand its dialect.

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

### PathDialect

`PathDialect` describes the syntax context used to classify a raw path token. It is source-context, not just host-OS, because a Unix host can inspect Windows-looking command text and a Windows host can run WSL/MSYS shells.

```rust
pub enum PathDialect {
    Posix,
    Windows,
    WslPosix,
    Msys,
    Cygwin,
    Unknown,
}
```

Dialect comes from the producing surface:

- bash on Linux/macOS: `Posix`, while still detecting obvious Windows absolute/UNC/verbatim forms as foreign absolute paths;
- bash inside WSL: `WslPosix`;
- PowerShell/cmd/native Windows tools: `Windows`;
- Git Bash/MSYS: `Msys`;
- Cygwin: `Cygwin`;
- exact tool arguments: host-native unless the tool declares otherwise.

### PathTarget

```rust
pub enum PathTarget {
    WorkspaceRelative { raw: String },
    PosixAbsolute { raw: String },
    PosixHomeRelative { raw: String },

    WindowsDriveAbsolute { raw: String, drive: char },
    WindowsDriveRelative { raw: String, drive: char },
    WindowsRootRelative { raw: String },
    WindowsUnc { raw: String },
    WindowsVerbatim { raw: String },
    WindowsDevice { raw: String },

    WslDriveMount { raw: String, drive: char },
    MsysDriveMount { raw: String, drive: char },
    CygwinDriveMount { raw: String, drive: char },

    SpecialDevice { raw: String },
    FileDescriptor { raw: String },
    Unknown { raw: String },
}
```

Important classifications:

- `C:\Users\alice\file` and `C:/Users/alice/file` are Windows drive-absolute, not workspace-relative.
- `C:file` is Windows drive-relative and ambiguous; it must not be joined to the workspace by default.
- `\\server\share\file`, `//server/share/file`, and `\\?\UNC\server\share\file` are UNC/verbatim paths and external unless explicitly trusted.
- `\\?\C:\...` preserves the verbatim prefix; normalizing it away hides security-relevant syntax.
- `\\.\pipe\docker_engine`, `CON`, `NUL`, `COM1`, and `LPT1` are Windows device namespace targets. Only `NUL` is analogous to `/dev/null`; other devices are not ordinary files.
- `/mnt/c/...` is a WSL Windows-drive mount; `/c/...` and `/cygdrive/c/...` are MSYS/Cygwin drive aliases when the dialect supports them.

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
    pub environment: Option<EnvironmentContext>,
    pub mount: Option<MountContext>,
    pub risks: Vec<PathRisk>,
}
```

Resolution is a pipeline:

```text
raw string + dialect + source
  -> PathTarget
  -> normalization plan
  -> ResolvedFsTarget
  -> MediationDecision
```

Do not auto-correct suspicious paths. The system may suggest `.omegon/...`, but execution must use the path the command actually requested unless the agent rewrites and reruns a corrected command.

### WorkspaceRelation

```rust
pub enum WorkspaceRelation {
    InsideWorkspace,
    TrustedExternal,
    AllowedSpecial,
    OutsideWorkspace,
}
```

`TrustedExternal` must be semantically distinct from `InsideWorkspace`. A trusted host bind mount, VM shared folder, or user-approved external directory is allowed, but still useful for prompts and audit logs.

## Warnings and Risks

Warnings explain syntax or context. Risks explain why the target is security-relevant.

```rust
pub enum PathWarning {
    RootDotPath { suggested_workspace_relative: String },
    ShortRootPath,
    LooksTruncated,
    WindowsDriveRelative,
    WindowsVerbatimPath,
    WindowsUncPath,
    WindowsDeviceName,
    WslWindowsDriveMount,
    MsysWindowsDriveMount,
    CygwinWindowsDriveMount,
    ShellQuotedOrEscapedPath,
    UnexpandedVariable,
    PotentialHostBridge,
    ContainerRuntimeSocket,
    KubernetesServiceAccountToken,
    ProjectedSecretVolume,
    ClusterIdentityMaterial,
    XdgDocumentPortal,
    SandboxPrivateStorage,
    VmSharedFolder { fs_type: String, mount_point: PathBuf },
    SiblingWorkspaceOnSharedMount,
}

pub enum PathRisk {
    Normal,
    SuspiciousSyntax,
    AmbiguousDialect,
    HostBridge,
    SecretMaterial,
    RuntimeControlSocket,
    SandboxPortal,
    PrivilegedKernelMaterial,
}
```

Initial sensitive path patterns:

- Kubernetes and cluster identity:
  - `/var/run/secrets/kubernetes.io/serviceaccount`
  - `/var/run/secrets/tokens`
  - `/run/secrets`
- container runtime control:
  - `/var/run/docker.sock`
  - `/run/docker.sock`
  - `/run/podman/podman.sock`
  - `/run/containerd/containerd.sock`
- dangerous runtime/kernel material:
  - `/proc/<pid>/root`
  - `/proc/<pid>/fd`
  - `/sys/kernel`
  - `/dev/mem`
  - `/dev/kmsg`
- sandbox portals:
  - `/run/user/<uid>/doc`
  - `~/.var/app/<flatpak-id>`

## Environment and Mount Context

VMs and containers do not require a separate permission system, but they require mount/environment provenance. A path can be syntactically ordinary inside the guest/container while semantically crossing into host, cluster, VM-shared, or portal-exported storage.

```rust
pub struct EnvironmentContext {
    pub runtime: RuntimeEnvironment,
    pub detected_by: Vec<String>,
}

pub enum RuntimeEnvironment {
    Host,
    DockerLike,
    KubernetesPod,
    DevContainer,
    Wsl,
    Flatpak,
    Snap,
    VmGuest,
    Unknown,
}

pub struct MountContext {
    pub mount_point: PathBuf,
    pub fs_type: String,
    pub source: String,
    pub options: Vec<String>,
    pub super_options: Vec<String>,
    pub kind: EnvironmentMountKind,
    pub read_only: bool,
    pub identity: Option<MountIdentity>,
}

pub enum EnvironmentMountKind {
    Ordinary,
    ContainerOverlay,
    BindMount,
    DockerVolume,
    KubernetesProjected,
    KubernetesSecret,
    KubernetesConfigMap,
    ServiceAccountToken,
    VirtioFs,
    NineP,
    VBoxSharedFolder,
    VmHgfs,
    ParallelsSharedFolder,
    Fuse,
    XdgDocumentPortal,
    UnknownSpecial,
}
```

Best-effort detection inputs:

- Docker/container:
  - `/.dockerenv`
  - `/run/.containerenv`
  - `/proc/1/cgroup`
- Kubernetes:
  - `KUBERNETES_SERVICE_HOST`
  - `/var/run/secrets/kubernetes.io/serviceaccount`
- WSL:
  - `WSL_DISTRO_NAME`
  - `/proc/version` containing Microsoft/WSL markers
- Flatpak:
  - `FLATPAK_ID`
  - `/.flatpak-info`
- Snap:
  - `SNAP`, `SNAP_NAME`
- Devcontainer/Codespaces:
  - `DEVCONTAINER`, `CODESPACES`, `/workspaces`
- Linux mount context:
  - parse `/proc/self/mountinfo` where available.

Mount kinds of special interest:

- Docker/Podman bind mounts and volumes;
- overlayfs container root;
- Kubernetes projected/secret/configMap/service-account volumes;
- QEMU/KVM `virtiofs` and `9p`;
- VirtualBox `vboxsf`;
- VMware `fuse.vmhgfs-fuse`;
- Parallels shared folders;
- XDG document portal FUSE mounts under `/run/user/<uid>/doc`.

Environment detection is diagnostic and policy-informing. It is not a replacement for workspace-boundary checks.

## Trust Grants and Mount Identity

Path-prefix trust alone is insufficient in container and VM environments because bind propagation, remounts, and shared-folder reconfiguration can change what lies under a previously trusted path.

Target trust shape:

```rust
pub struct TrustGrant {
    pub path: PathBuf,
    pub persistence: PermissionPersistence,
    pub mount_identity: Option<MountIdentity>,
    pub environment: RuntimeEnvironment,
    pub created_at: DateTime<Utc>,
}
```

If a later target resolves under a trusted path but the observed mount identity changes, mediation should warn and require renewed operator intent unless the operator explicitly chose path-only trust across mount changes.

## Bash and Terminal Extraction

Replace the direct scanner shape:

```rust
scan_boundary_violations(command, boundary, cwd) -> Vec<String>
```

with an intent extractor:

```rust
extract_shell_fs_intents(command, cwd, dialect) -> Vec<FsIntent>
```

The existing scanner can remain as a compatibility wrapper while call sites migrate.

Extraction sources:

- shell redirects: `>`, `>>`, `2>`, etc.
- `tee` and `tee -a` arguments;
- `cp`, `mv`, `install` destination arguments;
- `mkdir` target arguments;
- `rm` target arguments.

Regex-based extraction must mark confidence as `Heuristic` or `Parsed` depending on how much syntax context is known. A later shell parser can upgrade confidence.

### Shell Extractor Ownership Boundary

Shell parsing is an evidence-improvement layer, not an execution or sandbox model. Omegon owns the intent extraction contract, confidence labels, provenance spans, and conservative fallback behavior. Omegon does not own full shell evaluation semantics.

Non-goals:

- do not execute shell expansion;
- do not evaluate variables;
- do not resolve globs;
- do not emulate full Bash redirection/control-flow semantics;
- do not infer paths from arbitrary command strings;
- do not claim that absence of extracted intents proves a shell command cannot access the filesystem.

Refinement path:

1. Use the already-present `shlex` crate for quote-aware simple command argument extraction.
2. Add only a tiny redirect-aware lexer for common redirect operands outside quotes/comments/heredoc bodies.
3. Scout `yash-syntax` and `tree-sitter-bash` against Omegon's corpus before adopting either; parser-backed extraction must still emit unresolved/dynamic diagnostics for variables, command substitution, globs, and unsupported constructs.
4. Keep regex extraction as a compatibility fallback until parser-backed extraction is proven by regression tests.

A parser-backed extractor should emit candidates like:

```rust
pub struct ShellFsIntentCandidate {
    pub operation: FsOperation,
    pub raw_operand: String,
    pub span: SourceSpan,
    pub source_kind: ShellSourceKind,
    pub confidence: IntentConfidence,
    pub dynamic: bool,
    pub diagnostics: Vec<ShellIntentDiagnostic>,
}
```

The path dialect classifier runs after shell extraction. The shell extractor preserves raw operands and source spans; it does not decide whether a path is POSIX, Windows, WSL, MSYS, Cygwin, or host-bridged.

## Policy

Policy decisions should be based on intent, resolved target, relation, warning set, risk set, environment/mount context, and confidence.

Rules for 0.27.8:

1. Inside-workspace targets are allowed.
2. Trusted external targets are allowed according to existing trusted-directory settings, but remain distinguishable from workspace targets.
3. Exact outside-workspace targets still produce permission prompts.
4. Low-confidence suspicious shell-derived paths block with diagnostic text rather than normal approval prompts.
5. Root-dot paths produce correction-oriented diagnostics and should not offer dangerous persistent root grants.
6. Windows drive/UNC/verbatim/device paths are never silently treated as workspace-relative.
7. Host-bridge, secret-material, runtime-socket, sandbox-portal, and privileged-kernel risks are surfaced in mediation.

## UX / Mediation

Permission prompts and blocked tool results should name:

- operation;
- raw path;
- resolved/canonical path if safe to show;
- workspace root;
- source/provenance;
- dialect;
- confidence;
- workspace relation;
- mount/environment context when available;
- risk classification;
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

Example for `/mnt/c/Users/alice/project/out.txt`:

```text
Windows drive mount through WSL
Operation: Write
Path: /mnt/c/Users/alice/project/out.txt
Risk: host-bridge storage
Warning: This path targets the Windows C: drive from WSL.
Recommended: approve only if host-drive access is intended.
```

Example for `/var/run/docker.sock`:

```text
Container runtime control socket
Operation: Read/Write
Path: /var/run/docker.sock
Risk: runtime-control socket
Warning: Access may allow controlling containers or host-integrated runtime resources.
Recommended: avoid persistent parent-directory approval.
```

## Migration Strategy

1. Add types and pure path-warning classifiers.
2. Convert bash scanner to produce structured intents internally; keep wrapper for existing call sites.
3. Update bash and terminal preflight to evaluate intents through a small policy function.
4. Add provenance to permission errors or blocked results.
5. Add dialect-aware classification for Windows/WSL/MSYS/Cygwin path shapes.
6. Add sensitive infrastructure path classifiers.
7. Add environment and mount context detection from `/proc/self/mountinfo` and runtime signals.
8. Store mount identity with trusted-directory grants.
9. Migrate exact tools (`read`, `write`, `edit`) after bash/terminal behavior is stabilized.
10. Replace heuristic shell regex extraction with tokenization/parser-backed extraction.
