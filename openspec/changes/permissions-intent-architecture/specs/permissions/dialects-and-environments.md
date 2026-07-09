# Permissions Dialects and Environments — Delta Spec

## ADDED Requirements

### Requirement: Path dialects are classified before resolution

Filesystem permission mediation MUST classify raw path strings by dialect before converting them to host `PathBuf` values. Dialect classification MUST preserve the original raw path and MUST NOT silently treat an unrecognized absolute path from another platform as workspace-relative.

#### Scenario: Windows drive absolute path is external
Given shell preflight extracts `C:/Users/alice/secret.txt`
When permissions classify the target
Then the target is classified as a Windows drive absolute path
And it is not treated as workspace-relative.

#### Scenario: Windows drive-relative path is ambiguous
Given shell preflight extracts `C:secret.txt`
When permissions classify the target
Then the target is classified as Windows drive-relative
And diagnostics mark it as ambiguous rather than resolving it under the workspace.

#### Scenario: UNC path is external
Given shell preflight extracts `\\server\share\secret.txt`
When permissions classify the target
Then the target is classified as a UNC path
And it is not treated as workspace-relative.

#### Scenario: Windows verbatim path is high-friction
Given shell preflight extracts `\\?\C:\Users\alice\secret.txt`
When permissions classify the target
Then the target is classified as a Windows verbatim path
And diagnostics preserve the verbatim prefix.

#### Scenario: WSL drive mount is host-bridge context
Given shell preflight extracts `/mnt/c/Users/alice/secret.txt`
When permissions classify the target
Then diagnostics identify it as a WSL Windows-drive mount
And mediation treats it as a potential host bridge outside the workspace unless containment proves otherwise.

### Requirement: Environment and mount context augment workspace relation

Resolved filesystem targets MUST be able to carry best-effort environment and mount context in addition to workspace relation. This context is diagnostic and policy-relevant, but it MUST NOT replace strict workspace-boundary checks.

#### Scenario: Docker bind mount carries host-bridge context
Given the process mount table identifies `/workspace` as a bind mount
When permissions resolve `/workspace/out.txt`
Then the resolved target includes mount context
And diagnostics identify the target as host-bridged storage.

#### Scenario: Container overlay is distinguished from host bind mount
Given the process runs in a container with overlay root
When permissions resolve `/etc/config`
Then diagnostics identify the target as container-local outside-workspace storage
And do not imply host filesystem access unless mount context shows a host bridge.

#### Scenario: VM shared folder is diagnosed
Given the process mount table identifies `/mnt/shared` as `virtiofs`, `9p`, `vboxsf`, or another VM shared filesystem
When permissions resolve `/mnt/shared/out.txt`
Then diagnostics identify the target as a VM shared folder
And mediation treats it as host-bridged storage.

#### Scenario: XDG document portal is host-bridged
Given a path under `/run/user/1000/doc`
When permissions resolve the target
Then diagnostics identify it as a portal-exported host document.

### Requirement: Sensitive infrastructure paths are classified independently of workspace relation

Permission mediation MUST classify known sensitive infrastructure paths even when the path is otherwise syntactically ordinary.

#### Scenario: Kubernetes service account token is sensitive
Given a command reads `/var/run/secrets/kubernetes.io/serviceaccount/token`
When permissions classify the target
Then diagnostics mark it as cluster identity material
And the prompt warns that access may grant Kubernetes API credentials.

#### Scenario: Container runtime socket is high-risk
Given a command targets `/var/run/docker.sock`
When permissions classify the target
Then diagnostics mark it as a container runtime control socket
And persistent broad approval is discouraged.

#### Scenario: Dangerous proc sys and device paths are high-risk
Given a command targets `/proc/1/root`, `/proc/1/fd`, `/sys/kernel`, `/dev/mem`, or `/dev/kmsg`
When permissions classify the target
Then diagnostics mark it as privileged runtime or kernel material
And mediation does not treat it as an ordinary external file.

### Requirement: Trusted external grants retain environment identity

Persistent trusted-directory grants SHOULD record the environment and mount identity observed when the grant was created. If a later request resolves under the same trusted path but a different mount identity, mediation SHOULD surface the change and require renewed operator intent.

#### Scenario: Trusted directory mount identity changes
Given the operator previously trusted `/mnt/data` when it resolved to mount source `A`
And `/mnt/data` later resolves to mount source `B`
When a tool requests `/mnt/data/out.txt`
Then mediation warns that the trusted path's mount identity changed
And it does not silently rely on the old grant without renewed intent.
