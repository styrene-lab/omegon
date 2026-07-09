use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathDialect {
    Posix,
    Windows,
    WslPosix,
    Msys,
    Cygwin,
    Unknown,
}

impl PathDialect {
    pub fn shell_default() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Posix
        }
    }

    pub fn detect_from_env() -> Self {
        Self::detect_from_env_vars(|key| std::env::var(key).ok())
    }

    pub fn detect_from_env_vars(mut get: impl FnMut(&str) -> Option<String>) -> Self {
        if cfg!(windows) {
            return Self::Windows;
        }

        if get("WSL_DISTRO_NAME").is_some()
            || get("WSL_INTEROP").is_some()
            || get("WSLENV").is_some()
        {
            return Self::WslPosix;
        }

        let msystem = get("MSYSTEM").unwrap_or_default();
        let ostype = get("OSTYPE").unwrap_or_default();
        if !msystem.is_empty() || ostype.contains("msys") || ostype.contains("mingw") {
            return Self::Msys;
        }
        if get("CYGWIN").is_some() || ostype.contains("cygwin") {
            return Self::Cygwin;
        }

        Self::shell_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    DynamicShell { raw: String },
    WslDriveMount { raw: String, drive: char },
    MsysDriveMount { raw: String, drive: char },
    CygwinDriveMount { raw: String, drive: char },
    SpecialDevice { raw: String },
    FileDescriptor { raw: String },
    Unknown { raw: String },
}

impl PathTarget {
    pub fn classify(raw: &str) -> Self {
        Self::classify_with_dialect(raw, PathDialect::shell_default())
    }

    pub fn classify_with_dialect(raw: &str, dialect: PathDialect) -> Self {
        if raw.is_empty() {
            return Self::Unknown {
                raw: raw.to_string(),
            };
        }

        if is_dynamic_shell_path(raw) {
            return Self::DynamicShell {
                raw: raw.to_string(),
            };
        }
        if is_posix_special_device(raw) || is_windows_null_device(raw) {
            return Self::SpecialDevice {
                raw: raw.to_string(),
            };
        }
        if raw.starts_with("/dev/fd/") || raw.starts_with("/proc/self/fd/") {
            return Self::FileDescriptor {
                raw: raw.to_string(),
            };
        }
        if is_windows_verbatim(raw) {
            return Self::WindowsVerbatim {
                raw: raw.to_string(),
            };
        }
        if is_windows_device_namespace(raw) || is_windows_reserved_device(raw) {
            return Self::WindowsDevice {
                raw: raw.to_string(),
            };
        }
        if is_windows_unc(raw) {
            return Self::WindowsUnc {
                raw: raw.to_string(),
            };
        }
        if let Some(drive) = windows_drive_absolute(raw) {
            return Self::WindowsDriveAbsolute {
                raw: raw.to_string(),
                drive,
            };
        }
        if let Some(drive) = windows_drive_relative(raw) {
            return Self::WindowsDriveRelative {
                raw: raw.to_string(),
                drive,
            };
        }
        if let Some(drive) = wsl_drive_mount(raw) {
            return Self::WslDriveMount {
                raw: raw.to_string(),
                drive,
            };
        }
        if matches!(dialect, PathDialect::Msys | PathDialect::WslPosix)
            && let Some(drive) = msys_drive_mount(raw)
        {
            return Self::MsysDriveMount {
                raw: raw.to_string(),
                drive,
            };
        }
        if matches!(dialect, PathDialect::Cygwin)
            && let Some(drive) = cygwin_drive_mount(raw)
        {
            return Self::CygwinDriveMount {
                raw: raw.to_string(),
                drive,
            };
        }
        if raw.starts_with('~') {
            return Self::PosixHomeRelative {
                raw: raw.to_string(),
            };
        }
        if raw.starts_with('/') {
            return Self::PosixAbsolute {
                raw: raw.to_string(),
            };
        }
        if matches!(dialect, PathDialect::Windows) && raw.starts_with(['\\', '/']) {
            return Self::WindowsRootRelative {
                raw: raw.to_string(),
            };
        }
        Self::WorkspaceRelative {
            raw: raw.to_string(),
        }
    }

    pub fn raw(&self) -> &str {
        match self {
            Self::WorkspaceRelative { raw }
            | Self::PosixAbsolute { raw }
            | Self::PosixHomeRelative { raw }
            | Self::WindowsDriveAbsolute { raw, .. }
            | Self::WindowsDriveRelative { raw, .. }
            | Self::WindowsRootRelative { raw }
            | Self::WindowsUnc { raw }
            | Self::WindowsVerbatim { raw }
            | Self::WindowsDevice { raw }
            | Self::DynamicShell { raw }
            | Self::WslDriveMount { raw, .. }
            | Self::MsysDriveMount { raw, .. }
            | Self::CygwinDriveMount { raw, .. }
            | Self::SpecialDevice { raw }
            | Self::FileDescriptor { raw }
            | Self::Unknown { raw } => raw,
        }
    }
}

fn is_dynamic_shell_path(raw: &str) -> bool {
    raw.starts_with('$')
        || raw.contains("${")
        || raw.contains("$(")
        || raw.contains('`')
        || raw.contains("<(")
        || raw.contains(">(")
}

fn is_posix_special_device(raw: &str) -> bool {
    matches!(
        raw,
        "/dev/null" | "/dev/stdin" | "/dev/stdout" | "/dev/stderr"
    )
}

fn is_windows_null_device(raw: &str) -> bool {
    raw.eq_ignore_ascii_case("NUL") || raw.eq_ignore_ascii_case("NUL:")
}

fn is_windows_reserved_device(raw: &str) -> bool {
    let stem = raw
        .trim_end_matches(':')
        .split(['.', '/', '\\'])
        .next()
        .unwrap_or(raw);
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX")
        || matches_device_number(&upper, "COM")
        || matches_device_number(&upper, "LPT")
}

fn matches_device_number(value: &str, prefix: &str) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'))
}

fn is_windows_verbatim(raw: &str) -> bool {
    raw.starts_with(r"\\?\") || raw.starts_with(r"//?/")
}

fn is_windows_device_namespace(raw: &str) -> bool {
    raw.starts_with(r"\\.\") || raw.starts_with(r"//./")
}

fn is_windows_unc(raw: &str) -> bool {
    (raw.starts_with(r"\\") && !is_windows_verbatim(raw) && !is_windows_device_namespace(raw))
        || (raw.starts_with("//") && !raw.starts_with("///"))
}

fn windows_drive_absolute(raw: &str) -> Option<char> {
    let bytes = raw.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
    {
        Some(bytes[0].to_ascii_uppercase() as char)
    } else {
        None
    }
}

fn windows_drive_relative(raw: &str) -> Option<char> {
    let bytes = raw.as_bytes();
    if bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && !matches!(bytes.get(2), Some(b'/' | b'\\'))
    {
        Some(bytes[0].to_ascii_uppercase() as char)
    } else {
        None
    }
}

fn wsl_drive_mount(raw: &str) -> Option<char> {
    let rest = raw.strip_prefix("/mnt/")?;
    drive_mount_component(rest)
}

fn msys_drive_mount(raw: &str) -> Option<char> {
    let rest = raw.strip_prefix('/')?;
    drive_mount_component(rest)
}

fn cygwin_drive_mount(raw: &str) -> Option<char> {
    let rest = raw.strip_prefix("/cygdrive/")?;
    drive_mount_component(rest)
}

fn drive_mount_component(rest: &str) -> Option<char> {
    let bytes = rest.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b'/' {
        Some(bytes[0].to_ascii_uppercase() as char)
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntentActor {
    Model,
    Operator,
    ToolRuntime,
    Extension(String),
    NativeCommandShim,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntentSource {
    ToolArgument {
        tool: String,
        field: String,
    },
    NativeCommand {
        command: String,
        argv_index: usize,
    },
    ShellRedirect {
        command_excerpt: String,
        redirect_op: String,
    },
    ShellCommandArgument {
        command_excerpt: String,
        command_name: String,
        argv_index: usize,
    },
    RuntimeInternal {
        subsystem: String,
    },
}

impl IntentSource {
    pub fn description(&self) -> String {
        match self {
            Self::ToolArgument { tool, field } => format!("{tool} tool argument `{field}`"),
            Self::NativeCommand {
                command,
                argv_index,
            } => {
                format!("native command `{command}` argument {argv_index}")
            }
            Self::ShellRedirect {
                command_excerpt,
                redirect_op,
            } => {
                format!("shell redirect `{redirect_op}` from `{command_excerpt}`")
            }
            Self::ShellCommandArgument {
                command_excerpt,
                command_name,
                argv_index,
            } => {
                format!(
                    "shell command `{command_name}` argument {argv_index} from `{command_excerpt}`"
                )
            }
            Self::RuntimeInternal { subsystem } => format!("runtime subsystem `{subsystem}`"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentConfidence {
    Exact,
    Parsed,
    Heuristic,
    Inferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsIntent {
    pub operation: FsOperation,
    pub target: PathTarget,
    pub actor: IntentActor,
    pub source: IntentSource,
    pub confidence: IntentConfidence,
}

impl FsIntent {
    pub fn raw_path(&self) -> &str {
        self.target.raw()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivilegeProgram {
    Sudo,
    Doas,
    Su,
    Pkexec,
}

impl PrivilegeProgram {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sudo => "sudo",
            Self::Doas => "doas",
            Self::Su => "su",
            Self::Pkexec => "pkexec",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivilegeMode {
    InteractivePossible,
    NonInteractive,
    PasswordFromStdin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivilegeIntent {
    pub program: PrivilegeProgram,
    pub mode: PrivilegeMode,
    pub preserve_env: bool,
    pub nested_shell: bool,
    pub command_excerpt: String,
    pub confidence: IntentConfidence,
}

impl PrivilegeIntent {
    pub fn program_name(&self) -> &'static str {
        self.program.as_str()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRelation {
    InsideWorkspace,
    OutsideWorkspace,
    TrustedExternal,
    SpecialAllowed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathWarning {
    RootDotPath {
        suggested_workspace_relative: String,
    },
    ShortRootPath,
    DynamicShellPath,
    WindowsDriveAbsolutePath,
    WindowsDriveRelative,
    WindowsRootRelative,
    WindowsVerbatimPath,
    WindowsUncPath,
    WindowsDeviceName,
    WslWindowsDriveMount,
    MsysWindowsDriveMount,
    CygwinWindowsDriveMount,
    PotentialHostBridge,
    ContainerRuntimeSocket,
    KubernetesServiceAccountToken,
    ProjectedSecretVolume,
    ClusterIdentityMaterial,
    XdgDocumentPortal,
    SandboxPrivateStorage,
    PrivilegedKernelMaterial,
    VmSharedFolder { fs_type: String, mount_point: PathBuf },
    TrustedMountIdentityChanged { mount_point: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentContext {
    pub runtime: RuntimeEnvironment,
    pub detected_by: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountIdentity {
    pub fs_type: String,
    pub source: String,
    pub mount_point: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFsTarget {
    pub raw: String,
    pub expanded: PathBuf,
    pub canonical: PathBuf,
    pub relation: WorkspaceRelation,
    pub warnings: Vec<PathWarning>,
    pub risks: Vec<PathRisk>,
    pub environment: Option<EnvironmentContext>,
    pub mount: Option<MountContext>,
}

pub fn classify_path_warnings(target: &PathTarget, _dialect: PathDialect) -> Vec<PathWarning> {
    let raw = target.raw();
    let mut warnings = Vec::new();
    if let Some(rest) = raw.strip_prefix("/.") {
        if !rest.is_empty() {
            warnings.push(PathWarning::RootDotPath {
                suggested_workspace_relative: format!(".{rest}"),
            });
        }
    }

    if let Some(component) = raw.strip_prefix('/') {
        if !component.is_empty()
            && !component.contains('/')
            && component.chars().count() <= 3
            && component.chars().any(|c| c.is_ascii_uppercase())
        {
            warnings.push(PathWarning::ShortRootPath);
        }
    }

    match target {
        PathTarget::DynamicShell { .. } => warnings.push(PathWarning::DynamicShellPath),
        PathTarget::WindowsDriveRelative { .. } => warnings.push(PathWarning::WindowsDriveRelative),
        PathTarget::WindowsDriveAbsolute { .. } => {
            warnings.push(PathWarning::WindowsDriveAbsolutePath)
        }
        PathTarget::WindowsRootRelative { .. } => warnings.push(PathWarning::WindowsRootRelative),
        PathTarget::WindowsVerbatim { .. } => warnings.push(PathWarning::WindowsVerbatimPath),
        PathTarget::WindowsUnc { .. } => warnings.push(PathWarning::WindowsUncPath),
        PathTarget::WindowsDevice { .. } => warnings.push(PathWarning::WindowsDeviceName),
        PathTarget::WslDriveMount { .. } => warnings.push(PathWarning::WslWindowsDriveMount),
        PathTarget::MsysDriveMount { .. } => warnings.push(PathWarning::MsysWindowsDriveMount),
        PathTarget::CygwinDriveMount { .. } => warnings.push(PathWarning::CygwinWindowsDriveMount),
        _ => {}
    }

    sort_dedup_warnings(&mut warnings);
    warnings
}

fn sort_dedup_warnings(warnings: &mut Vec<PathWarning>) {
    warnings.sort_by_key(|warning| format!("{warning:?}"));
    warnings.dedup();
}

fn sort_dedup_risks(risks: &mut Vec<PathRisk>) {
    risks.sort_by_key(|risk| format!("{risk:?}"));
    risks.dedup();
    if risks.len() > 1 {
        risks.retain(|risk| !matches!(risk, PathRisk::Normal));
    }
}

fn classify_path_risks(
    canonical: &Path,
    target: &PathTarget,
    mount: Option<&MountContext>,
) -> Vec<PathRisk> {
    let mut risks = Vec::new();
    match target {
        PathTarget::DynamicShell { .. } => risks.push(PathRisk::SuspiciousSyntax),
        PathTarget::WindowsDriveRelative { .. }
        | PathTarget::WindowsRootRelative { .. }
        | PathTarget::WindowsVerbatim { .. }
        | PathTarget::WindowsUnc { .. }
        | PathTarget::WindowsDevice { .. } => risks.push(PathRisk::AmbiguousDialect),
        PathTarget::WindowsDriveAbsolute { .. }
        | PathTarget::WslDriveMount { .. }
        | PathTarget::MsysDriveMount { .. }
        | PathTarget::CygwinDriveMount { .. } => risks.push(PathRisk::HostBridge),
        _ => {}
    }
    if let Some(mount) = mount {
        match mount.kind {
            EnvironmentMountKind::KubernetesProjected
            | EnvironmentMountKind::KubernetesSecret
            | EnvironmentMountKind::KubernetesConfigMap
            | EnvironmentMountKind::ServiceAccountToken => risks.push(PathRisk::SecretMaterial),
            EnvironmentMountKind::VirtioFs
            | EnvironmentMountKind::NineP
            | EnvironmentMountKind::VBoxSharedFolder
            | EnvironmentMountKind::VmHgfs
            | EnvironmentMountKind::ParallelsSharedFolder => risks.push(PathRisk::HostBridge),
            EnvironmentMountKind::XdgDocumentPortal => risks.push(PathRisk::SandboxPortal),
            _ => {}
        }
    }
    append_sensitive_path_risks_only(canonical, &mut risks);
    if risks.is_empty() {
        risks.push(PathRisk::Normal);
    }
    sort_dedup_risks(&mut risks);
    risks
}

fn append_sensitive_path_warnings(
    canonical: &Path,
    warnings: &mut Vec<PathWarning>,
    risks: &mut Vec<PathRisk>,
) {
    let path = canonical.to_string_lossy();
    if path.starts_with("/var/run/secrets/kubernetes.io/serviceaccount") {
        warnings.push(PathWarning::KubernetesServiceAccountToken);
        warnings.push(PathWarning::ClusterIdentityMaterial);
        risks.push(PathRisk::SecretMaterial);
    }
    if path.starts_with("/var/run/secrets/tokens") || path.starts_with("/run/secrets") {
        warnings.push(PathWarning::ProjectedSecretVolume);
        risks.push(PathRisk::SecretMaterial);
    }
    if matches!(
        path.as_ref(),
        "/var/run/docker.sock"
            | "/run/docker.sock"
            | "/run/podman/podman.sock"
            | "/run/containerd/containerd.sock"
    ) {
        warnings.push(PathWarning::ContainerRuntimeSocket);
        risks.push(PathRisk::RuntimeControlSocket);
    }
    if path.starts_with("/proc/") && (path.contains("/root") || path.contains("/fd")) {
        warnings.push(PathWarning::PrivilegedKernelMaterial);
        risks.push(PathRisk::PrivilegedKernelMaterial);
    }
    if path.starts_with("/sys/kernel") || matches!(path.as_ref(), "/dev/mem" | "/dev/kmsg") {
        warnings.push(PathWarning::PrivilegedKernelMaterial);
        risks.push(PathRisk::PrivilegedKernelMaterial);
    }
    if is_xdg_document_portal_path(&path) {
        warnings.push(PathWarning::XdgDocumentPortal);
        risks.push(PathRisk::SandboxPortal);
    }
    if path.contains("/.var/app/") || path.starts_with("/var/lib/snapd/") {
        warnings.push(PathWarning::SandboxPrivateStorage);
        risks.push(PathRisk::SandboxPortal);
    }
}

fn append_sensitive_path_risks_only(canonical: &Path, risks: &mut Vec<PathRisk>) {
    let mut warnings = Vec::new();
    append_sensitive_path_warnings(canonical, &mut warnings, risks);
}

fn append_mount_warnings(mount: &MountContext, warnings: &mut Vec<PathWarning>, risks: &mut Vec<PathRisk>) {
    match mount.kind {
        EnvironmentMountKind::XdgDocumentPortal => {
            warnings.push(PathWarning::XdgDocumentPortal);
            risks.push(PathRisk::SandboxPortal);
        }
        EnvironmentMountKind::KubernetesProjected
        | EnvironmentMountKind::KubernetesSecret
        | EnvironmentMountKind::KubernetesConfigMap
        | EnvironmentMountKind::ServiceAccountToken => {
            warnings.push(PathWarning::ProjectedSecretVolume);
            risks.push(PathRisk::SecretMaterial);
        }
        EnvironmentMountKind::VirtioFs
        | EnvironmentMountKind::NineP
        | EnvironmentMountKind::VBoxSharedFolder
        | EnvironmentMountKind::VmHgfs
        | EnvironmentMountKind::ParallelsSharedFolder => {
            warnings.push(PathWarning::VmSharedFolder {
                fs_type: mount.fs_type.clone(),
                mount_point: mount.mount_point.clone(),
            });
            risks.push(PathRisk::HostBridge);
        }
        EnvironmentMountKind::BindMount | EnvironmentMountKind::DockerVolume => {
            warnings.push(PathWarning::PotentialHostBridge);
            risks.push(PathRisk::HostBridge);
        }
        _ => {}
    }
}

fn is_xdg_document_portal_path(path: &str) -> bool {
    path.starts_with("/run/user/") && path.contains("/doc/")
}

pub fn resolve_intent_target(
    intent: &FsIntent,
    cwd: &Path,
    boundary: &crate::tools::WorkspaceBoundary,
) -> ResolvedFsTarget {
    let raw = intent.raw_path().to_string();
    let expanded = expanded_path_for_target(&intent.target, cwd);
    let canonical = crate::tools::canonicalize_existing_parent_for_permissions(&expanded);
    let cwd_canonical = boundary.cwd().canonicalize().unwrap_or_else(|_| boundary.cwd().to_path_buf());
    let expanded_for_relation = if expanded.is_absolute() {
        expanded.clone()
    } else {
        cwd.join(&expanded)
    };
    let relation = if matches!(
        intent.target,
        PathTarget::WindowsDriveAbsolute { .. }
            | PathTarget::WindowsDriveRelative { .. }
            | PathTarget::WindowsRootRelative { .. }
            | PathTarget::WindowsVerbatim { .. }
            | PathTarget::WindowsUnc { .. }
            | PathTarget::WindowsDevice { .. }
            | PathTarget::DynamicShell { .. }
    ) {
        WorkspaceRelation::OutsideWorkspace
    } else if expanded_for_relation.starts_with(&cwd_canonical) || canonical.starts_with(&cwd_canonical) {
        WorkspaceRelation::InsideWorkspace
    } else if boundary.is_trusted_path_for_permissions(&canonical) {
        WorkspaceRelation::TrustedExternal
    } else if crate::tools::is_allowed_special_path_for_permissions(&expanded) {
        WorkspaceRelation::SpecialAllowed
    } else {
        WorkspaceRelation::OutsideWorkspace
    };
    let environment = detect_environment_context();
    let mount = detect_mount_context(&canonical);
    let mut warnings = classify_path_warnings(&intent.target, PathDialect::detect_from_env());
    let mut risks = classify_path_risks(&canonical, &intent.target, mount.as_ref());
    append_sensitive_path_warnings(&expanded, &mut warnings, &mut risks);
    append_sensitive_path_warnings(&canonical, &mut warnings, &mut risks);
    if let Some(mount) = &mount {
        append_mount_warnings(mount, &mut warnings, &mut risks);
    }
    if matches!(relation, WorkspaceRelation::TrustedExternal) && mount.as_ref().and_then(|m| m.identity.as_ref()).is_some() {
        // Persistent trust grants are currently path-prefix based. Surface mount identity
        // on every trusted external resolution so callers/prompts can distinguish a
        // workspace path from an approved host/VM/container mount and renew trust if the
        // observed identity changes in future profile schemas.
        if let Some(mount) = &mount {
            warnings.push(PathWarning::TrustedMountIdentityChanged { mount_point: mount.mount_point.clone() });
        }
    }
    sort_dedup_warnings(&mut warnings);
    sort_dedup_risks(&mut risks);

    ResolvedFsTarget {
        raw,
        expanded,
        canonical,
        relation,
        warnings,
        risks,
        environment,
        mount,
    }
}

pub fn profile_mount_identity_for_path(path: &Path) -> Option<crate::settings::ProfileMountIdentity> {
    detect_mount_context(path)
        .and_then(|mount| mount.identity)
        .map(|identity| crate::settings::ProfileMountIdentity {
            fs_type: identity.fs_type,
            source: identity.source,
            mount_point: identity.mount_point.display().to_string(),
        })
}

pub fn profile_environment_for_current_process() -> Option<String> {
    detect_environment_context().map(|context| format!("{:?}", context.runtime))
}

fn detect_environment_context() -> Option<EnvironmentContext> {
    let mut detected_by = Vec::new();
    let runtime = if std::env::var_os("KUBERNETES_SERVICE_HOST").is_some()
        || Path::new("/var/run/secrets/kubernetes.io/serviceaccount/token").exists()
    {
        detected_by.push("kubernetes-service-account".to_string());
        RuntimeEnvironment::KubernetesPod
    } else if std::env::var_os("DEVCONTAINER").is_some()
        || std::env::var_os("CODESPACES").is_some()
        || Path::new("/workspaces").exists()
    {
        detected_by.push("devcontainer-marker".to_string());
        RuntimeEnvironment::DevContainer
    } else if std::env::var_os("WSL_DISTRO_NAME").is_some()
        || std::fs::read_to_string("/proc/version")
            .is_ok_and(|content| content.contains("Microsoft") || content.contains("WSL"))
    {
        detected_by.push("wsl-marker".to_string());
        RuntimeEnvironment::Wsl
    } else if std::env::var_os("FLATPAK_ID").is_some() || Path::new("/.flatpak-info").exists() {
        detected_by.push("flatpak-marker".to_string());
        RuntimeEnvironment::Flatpak
    } else if std::env::var_os("SNAP").is_some() || std::env::var_os("SNAP_NAME").is_some() {
        detected_by.push("snap-marker".to_string());
        RuntimeEnvironment::Snap
    } else if Path::new("/.dockerenv").exists()
        || Path::new("/run/.containerenv").exists()
        || std::fs::read_to_string("/proc/1/cgroup").is_ok_and(|content| {
            content.contains("kubepods")
                || content.contains("containerd")
                || content.contains("docker")
                || content.contains("podman")
        })
    {
        detected_by.push("container-marker".to_string());
        RuntimeEnvironment::DockerLike
    } else if std::fs::read_to_string("/sys/class/dmi/id/product_name").is_ok_and(|content| {
        let lower = content.to_ascii_lowercase();
        lower.contains("virtualbox")
            || lower.contains("vmware")
            || lower.contains("qemu")
            || lower.contains("parallels")
    }) {
        detected_by.push("dmi-vm-marker".to_string());
        RuntimeEnvironment::VmGuest
    } else {
        detected_by.push("default-host".to_string());
        RuntimeEnvironment::Host
    };
    Some(EnvironmentContext { runtime, detected_by })
}

fn detect_mount_context(path: &Path) -> Option<MountContext> {
    let content = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
    parse_mountinfo(&content, path)
}

fn parse_mountinfo(content: &str, path: &Path) -> Option<MountContext> {
    content
        .lines()
        .filter_map(parse_mountinfo_line)
        .filter(|mount| path.starts_with(&mount.mount_point))
        .max_by(|left, right| {
            left.mount_point
                .components()
                .count()
                .cmp(&right.mount_point.components().count())
                .then_with(|| mount_kind_specificity(left.kind).cmp(&mount_kind_specificity(right.kind)))
        })
}

fn mount_kind_specificity(kind: EnvironmentMountKind) -> u8 {
    match kind {
        EnvironmentMountKind::Ordinary | EnvironmentMountKind::ContainerOverlay => 0,
        EnvironmentMountKind::Fuse | EnvironmentMountKind::UnknownSpecial => 1,
        EnvironmentMountKind::BindMount | EnvironmentMountKind::DockerVolume => 2,
        EnvironmentMountKind::VirtioFs
        | EnvironmentMountKind::NineP
        | EnvironmentMountKind::VBoxSharedFolder
        | EnvironmentMountKind::VmHgfs
        | EnvironmentMountKind::ParallelsSharedFolder => 3,
        EnvironmentMountKind::KubernetesProjected
        | EnvironmentMountKind::KubernetesSecret
        | EnvironmentMountKind::KubernetesConfigMap
        | EnvironmentMountKind::ServiceAccountToken
        | EnvironmentMountKind::XdgDocumentPortal => 4,
    }
}

fn parse_mountinfo_line(line: &str) -> Option<MountContext> {
    let (pre, post) = line.split_once(" - ")?;
    let pre_fields = pre.split_whitespace().collect::<Vec<_>>();
    let post_fields = post.split_whitespace().collect::<Vec<_>>();
    if pre_fields.len() < 6 || post_fields.len() < 3 {
        return None;
    }
    let mount_point = PathBuf::from(unescape_mountinfo_field(pre_fields[4]));
    let options = pre_fields[5].split(',').map(ToOwned::to_owned).collect::<Vec<_>>();
    let fs_type = post_fields[0].to_string();
    let source = post_fields[1].to_string();
    let super_options = post_fields[2].split(',').map(ToOwned::to_owned).collect::<Vec<_>>();
    let read_only = options.iter().any(|o| o == "ro") || super_options.iter().any(|o| o == "ro");
    let kind = classify_mount_kind(&mount_point, &fs_type, &source, &options, &super_options);
    let identity = Some(MountIdentity {
        fs_type: fs_type.clone(),
        source: source.clone(),
        mount_point: mount_point.clone(),
    });
    Some(MountContext { mount_point, fs_type, source, options, super_options, kind, read_only, identity })
}

fn unescape_mountinfo_field(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

fn classify_mount_kind(
    mount_point: &Path,
    fs_type: &str,
    source: &str,
    options: &[String],
    super_options: &[String],
) -> EnvironmentMountKind {
    let mount = mount_point.to_string_lossy();
    let all = options.iter().chain(super_options.iter()).map(String::as_str).collect::<Vec<_>>().join(",");
    if mount.starts_with("/run/user/") && mount.contains("/doc") {
        return EnvironmentMountKind::XdgDocumentPortal;
    }
    if mount.starts_with("/var/run/secrets/kubernetes.io/serviceaccount") {
        return EnvironmentMountKind::ServiceAccountToken;
    }
    if all.contains("kubernetes.io~secret") || source.contains("secret") {
        return EnvironmentMountKind::KubernetesSecret;
    }
    if all.contains("kubernetes.io~configmap") || source.contains("configmap") {
        return EnvironmentMountKind::KubernetesConfigMap;
    }
    if all.contains("kubernetes.io~projected") || source.contains("projected") {
        return EnvironmentMountKind::KubernetesProjected;
    }
    match fs_type {
        "overlay" => EnvironmentMountKind::ContainerOverlay,
        "virtiofs" => EnvironmentMountKind::VirtioFs,
        "9p" => EnvironmentMountKind::NineP,
        "vboxsf" => EnvironmentMountKind::VBoxSharedFolder,
        "fuse.vmhgfs-fuse" => EnvironmentMountKind::VmHgfs,
        "prl_fs" => EnvironmentMountKind::ParallelsSharedFolder,
        ty if ty.starts_with("fuse") => EnvironmentMountKind::Fuse,
        _ if source.starts_with("/dev/") && !mount.starts_with("/dev") => EnvironmentMountKind::BindMount,
        _ if all.contains("bind") => EnvironmentMountKind::BindMount,
        _ if source.starts_with("volume-") || source.contains("docker/volumes") => EnvironmentMountKind::DockerVolume,
        _ => EnvironmentMountKind::Ordinary,
    }
}

fn expanded_path_for_target(target: &PathTarget, cwd: &Path) -> PathBuf {
    match target {
        PathTarget::WorkspaceRelative { raw } => cwd.join(raw),
        PathTarget::PosixHomeRelative { raw } => expand_tilde_for_intent(raw),
        PathTarget::PosixAbsolute { raw }
        | PathTarget::SpecialDevice { raw }
        | PathTarget::FileDescriptor { raw }
        | PathTarget::WslDriveMount { raw, .. }
        | PathTarget::MsysDriveMount { raw, .. }
        | PathTarget::CygwinDriveMount { raw, .. } => PathBuf::from(raw),
        PathTarget::WindowsDriveAbsolute { raw, .. }
        | PathTarget::WindowsDriveRelative { raw, .. }
        | PathTarget::WindowsRootRelative { raw }
        | PathTarget::WindowsUnc { raw }
        | PathTarget::WindowsVerbatim { raw }
        | PathTarget::WindowsDevice { raw }
        | PathTarget::DynamicShell { raw }
        | PathTarget::Unknown { raw } => PathBuf::from(raw),
    }
}

fn expand_tilde_for_intent(path_str: &str) -> PathBuf {
    if let Some(rest) = path_str.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(path_str)
}

pub fn suspicious_low_confidence_shell_path(
    intent: &FsIntent,
    resolved: &ResolvedFsTarget,
) -> bool {
    matches!(
        intent.confidence,
        IntentConfidence::Heuristic | IntentConfidence::Inferred
    ) && matches!(
        intent.source,
        IntentSource::ShellRedirect { .. } | IntentSource::ShellCommandArgument { .. }
    ) && resolved.warnings.iter().any(|w| {
        matches!(
            w,
            PathWarning::ShortRootPath | PathWarning::DynamicShellPath
        )
    })
}

pub fn classify_privilege_intent(command: &str) -> Option<PrivilegeIntent> {
    let tokens = shell_tokens(command);
    let mut idx = first_command_index(&tokens)?;
    while idx < tokens.len() {
        let name = command_basename(&tokens[idx]);
        if let Some(program) = privilege_program(name) {
            return Some(build_privilege_intent(program, &tokens[idx..], command));
        }
        if matches!(name, "sh" | "bash" | "zsh" | "dash") {
            if let Some(nested) = nested_shell_command(&tokens[idx..]) {
                if let Some(mut intent) = classify_privilege_intent(nested) {
                    intent.nested_shell = true;
                    intent.command_excerpt = command.to_string();
                    return Some(intent);
                }
            }
        }
        idx += 1;
    }
    None
}

fn build_privilege_intent(
    program: PrivilegeProgram,
    tokens: &[String],
    command: &str,
) -> PrivilegeIntent {
    let mut mode = PrivilegeMode::InteractivePossible;
    let mut preserve_env = false;
    let mut nested_shell = false;
    for token in tokens.iter().skip(1) {
        if !token.starts_with('-') {
            let name = command_basename(token);
            if matches!(name, "sh" | "bash" | "zsh" | "dash") {
                nested_shell = true;
            }
            continue;
        }
        if token == "-n"
            || token == "--non-interactive"
            || token == "--non-interactive=true"
            || (token.starts_with('-') && !token.starts_with("--") && token.contains('n'))
        {
            mode = PrivilegeMode::NonInteractive;
        }
        if token == "-S"
            || token == "--stdin"
            || (token.starts_with('-') && !token.starts_with("--") && token.contains('S'))
        {
            mode = PrivilegeMode::PasswordFromStdin;
        }
        if token == "-E"
            || token == "--preserve-env"
            || token.starts_with("--preserve-env=")
            || (token.starts_with('-') && !token.starts_with("--") && token.contains('E'))
        {
            preserve_env = true;
        }
    }
    PrivilegeIntent {
        program,
        mode,
        preserve_env,
        nested_shell,
        command_excerpt: command.to_string(),
        confidence: IntentConfidence::Heuristic,
    }
}

fn nested_shell_command(tokens: &[String]) -> Option<&str> {
    let mut expect_command = false;
    for token in tokens.iter().skip(1) {
        if expect_command {
            return Some(token.as_str());
        }
        if token == "-c" || token == "-lc" {
            expect_command = true;
            continue;
        }
        if let Some(short_flags) = token.strip_prefix('-')
            && !token.starts_with("--")
            && short_flags.contains('c')
        {
            expect_command = true;
        }
    }
    None
}

fn first_command_index(tokens: &[String]) -> Option<usize> {
    let mut idx = 0;
    let mut after_env = false;
    while idx < tokens.len() {
        let name = command_basename(&tokens[idx]);
        if matches!(
            name,
            "env" | "exec" | "command" | "builtin" | "time" | "nohup"
        ) {
            after_env = after_env || name == "env";
            idx += 1;
            continue;
        }
        if after_env {
            if matches!(tokens[idx].as_str(), "-u" | "--unset" | "-C" | "--chdir") {
                idx += 2;
                continue;
            }
            if tokens[idx].starts_with("--unset=")
                || tokens[idx].starts_with("--chdir=")
                || tokens[idx].starts_with('-')
            {
                idx += 1;
                continue;
            }
        }
        if looks_like_env_assignment(&tokens[idx]) {
            idx += 1;
            continue;
        }
        return Some(idx);
    }
    None
}

fn shell_tokens(command: &str) -> Vec<String> {
    shlex::split(command).unwrap_or_else(|| {
        command
            .split(|ch: char| ch.is_whitespace() || matches!(ch, ';' | '&' | '|' | '(' | ')'))
            .filter(|token| !token.is_empty())
            .map(|token| {
                token
                    .trim_matches(|ch| matches!(ch, '"' | '\'' | '`'))
                    .to_string()
            })
            .filter(|token| !token.is_empty())
            .collect()
    })
}

fn looks_like_env_assignment(token: &str) -> bool {
    let Some((name, _value)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        && !name.chars().next().is_some_and(|ch| ch.is_ascii_digit())
}

fn command_basename(token: &str) -> &str {
    token.rsplit(['/', '\\']).next().unwrap_or(token)
}

fn privilege_program(name: &str) -> Option<PrivilegeProgram> {
    match name {
        "sudo" => Some(PrivilegeProgram::Sudo),
        "doas" => Some(PrivilegeProgram::Doas),
        "su" => Some(PrivilegeProgram::Su),
        "pkexec" => Some(PrivilegeProgram::Pkexec),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_intent(raw: &str) -> FsIntent {
        FsIntent {
            operation: FsOperation::Write,
            target: PathTarget::classify(raw),
            actor: IntentActor::Model,
            source: IntentSource::ShellCommandArgument {
                command_excerpt: format!("write {raw}"),
                command_name: "write".to_string(),
                argv_index: 1,
            },
            confidence: IntentConfidence::Parsed,
        }
    }

    fn write_intent_with_dialect(raw: &str, dialect: PathDialect) -> FsIntent {
        FsIntent {
            operation: FsOperation::Write,
            target: PathTarget::classify_with_dialect(raw, dialect),
            actor: IntentActor::Model,
            source: IntentSource::ShellCommandArgument {
                command_excerpt: format!("write {raw}"),
                command_name: "write".to_string(),
                argv_index: 1,
            },
            confidence: IntentConfidence::Parsed,
        }
    }

    #[test]
    fn privilege_classifier_detects_shell_compound_c_flag() {
        let intent = classify_privilege_intent("bash -euxc 'sudo true'").unwrap();
        assert_eq!(intent.program, PrivilegeProgram::Sudo);
        assert!(intent.nested_shell);
    }

    #[test]
    fn privilege_classifier_detects_shell_dash_lc_flag() {
        let intent = classify_privilege_intent("zsh -lc 'doas true'").unwrap();
        assert_eq!(intent.program, PrivilegeProgram::Doas);
        assert!(intent.nested_shell);
    }

    #[test]
    fn privilege_classifier_respects_non_interactive_sudo() {
        let intent = classify_privilege_intent("sudo -n true").unwrap();
        assert_eq!(intent.program, PrivilegeProgram::Sudo);
        assert_eq!(intent.mode, PrivilegeMode::NonInteractive);
    }

    #[test]
    fn privilege_classifier_detects_password_from_stdin() {
        let intent = classify_privilege_intent("sudo -S true").unwrap();
        assert_eq!(intent.program, PrivilegeProgram::Sudo);
        assert_eq!(intent.mode, PrivilegeMode::PasswordFromStdin);
    }

    #[test]
    fn classifies_windows_drive_absolute_before_pathbuf_resolution() {
        assert!(matches!(
            PathTarget::classify(r"C:\Users\alice\secret.txt"),
            PathTarget::WindowsDriveAbsolute { drive: 'C', .. }
        ));
        assert!(matches!(
            PathTarget::classify("C:/Users/alice/secret.txt"),
            PathTarget::WindowsDriveAbsolute { drive: 'C', .. }
        ));
    }

    #[test]
    fn classifies_windows_drive_relative_as_ambiguous() {
        let target = PathTarget::classify(r"C:secret.txt");
        assert!(matches!(
            target,
            PathTarget::WindowsDriveRelative { drive: 'C', .. }
        ));
        assert!(
            classify_path_warnings(&target, PathDialect::Posix)
                .contains(&PathWarning::WindowsDriveRelative)
        );
    }

    #[test]
    fn classifies_windows_unc_verbatim_and_devices() {
        assert!(matches!(
            PathTarget::classify(r"\\server\share\secret.txt"),
            PathTarget::WindowsUnc { .. }
        ));
        assert!(matches!(
            PathTarget::classify(r"\\?\C:\Users\alice\secret.txt"),
            PathTarget::WindowsVerbatim { .. }
        ));
        assert!(matches!(
            PathTarget::classify(r"\\.\PhysicalDrive0"),
            PathTarget::WindowsDevice { .. }
        ));
        assert!(matches!(
            PathTarget::classify("CON"),
            PathTarget::WindowsDevice { .. }
        ));
    }

    #[test]
    fn classifies_wsl_msys_and_cygwin_drive_mounts() {
        assert!(matches!(
            PathTarget::classify("/mnt/c/Users/alice/secret.txt"),
            PathTarget::WslDriveMount { drive: 'C', .. }
        ));
        assert!(matches!(
            PathTarget::classify_with_dialect("/c/Users/alice/secret.txt", PathDialect::Msys),
            PathTarget::MsysDriveMount { drive: 'C', .. }
        ));
        assert!(matches!(
            PathTarget::classify_with_dialect(
                "/cygdrive/c/Users/alice/secret.txt",
                PathDialect::Cygwin
            ),
            PathTarget::CygwinDriveMount { drive: 'C', .. }
        ));
    }

    #[test]
    fn windows_drive_absolute_does_not_become_workspace_relative_on_unix_hosts() {
        let cwd = Path::new("/tmp/workspace");
        let boundary = crate::tools::WorkspaceBoundary::new(cwd.to_path_buf());
        let intent = write_intent(r"C:\Users\alice\secret.txt");
        let resolved = resolve_intent_target(&intent, cwd, &boundary);
        assert_eq!(resolved.relation, WorkspaceRelation::OutsideWorkspace);
        assert!(!resolved.expanded.starts_with(cwd));
        assert!(
            resolved
                .warnings
                .contains(&PathWarning::WindowsDriveAbsolutePath)
        );
    }

    #[test]
    fn detects_shell_dialect_from_environment_markers() {
        assert_eq!(
            PathDialect::detect_from_env_vars(|key| match key {
                "WSL_DISTRO_NAME" => Some("Ubuntu".to_string()),
                _ => None,
            }),
            PathDialect::WslPosix
        );
        assert_eq!(
            PathDialect::detect_from_env_vars(|key| match key {
                "MSYSTEM" => Some("MINGW64".to_string()),
                _ => None,
            }),
            PathDialect::Msys
        );
        assert_eq!(
            PathDialect::detect_from_env_vars(|key| match key {
                "OSTYPE" => Some("cygwin".to_string()),
                _ => None,
            }),
            PathDialect::Cygwin
        );
    }

    #[test]
    fn windows_root_relative_is_classified_and_warned_in_windows_dialect() {
        let target = PathTarget::classify_with_dialect(r"\Windows\System32", PathDialect::Windows);
        assert!(matches!(target, PathTarget::WindowsRootRelative { .. }));
        assert!(
            classify_path_warnings(&target, PathDialect::Windows)
                .contains(&PathWarning::WindowsRootRelative)
        );
    }

    #[test]
    fn windows_drive_absolute_has_distinct_warning_from_verbatim() {
        let drive = PathTarget::classify(r"C:\Users\alice\secret.txt");
        let verbatim = PathTarget::classify(r"\\?\C:\Users\alice\secret.txt");
        assert!(
            classify_path_warnings(&drive, PathDialect::Posix)
                .contains(&PathWarning::WindowsDriveAbsolutePath)
        );
        assert!(
            !classify_path_warnings(&drive, PathDialect::Posix)
                .contains(&PathWarning::WindowsVerbatimPath)
        );
        assert!(
            classify_path_warnings(&verbatim, PathDialect::Posix)
                .contains(&PathWarning::WindowsVerbatimPath)
        );
    }

    #[test]
    fn msys_drive_mount_requires_dialect_context() {
        assert!(matches!(
            PathTarget::classify("/c/Users/alice/secret.txt"),
            PathTarget::PosixAbsolute { .. }
        ));
        let intent = write_intent_with_dialect("/c/Users/alice/secret.txt", PathDialect::Msys);
        assert!(matches!(
            intent.target,
            PathTarget::MsysDriveMount { drive: 'C', .. }
        ));
        let warnings = classify_path_warnings(&intent.target, PathDialect::Msys);
        assert!(warnings.contains(&PathWarning::MsysWindowsDriveMount));
    }

    #[test]
    fn wsl_dialect_accepts_msys_style_drive_mount_as_bridge_warning() {
        let target =
            PathTarget::classify_with_dialect("/c/Users/alice/secret.txt", PathDialect::WslPosix);
        assert!(matches!(
            target,
            PathTarget::MsysDriveMount { drive: 'C', .. }
        ));
    }

    #[test]
    fn wsl_drive_mount_carries_host_bridge_warning() {
        let cwd = Path::new("/tmp/workspace");
        let boundary = crate::tools::WorkspaceBoundary::new(cwd.to_path_buf());
        let intent = write_intent("/mnt/c/Users/alice/secret.txt");
        let resolved = resolve_intent_target(&intent, cwd, &boundary);
        assert_eq!(resolved.relation, WorkspaceRelation::OutsideWorkspace);
        assert!(
            resolved
                .warnings
                .contains(&PathWarning::WslWindowsDriveMount)
        );
        assert!(resolved.risks.contains(&PathRisk::HostBridge));
    }

    #[test]
    fn sensitive_paths_carry_risk_warnings() {
        let cwd = Path::new("/tmp/workspace");
        let boundary = crate::tools::WorkspaceBoundary::new(cwd.to_path_buf());
        let token = resolve_intent_target(
            &write_intent("/var/run/secrets/kubernetes.io/serviceaccount/token"),
            cwd,
            &boundary,
        );
        assert!(token.warnings.contains(&PathWarning::KubernetesServiceAccountToken));
        assert!(token.warnings.contains(&PathWarning::ClusterIdentityMaterial));
        assert!(token.risks.contains(&PathRisk::SecretMaterial));

        let socket = resolve_intent_target(&write_intent("/var/run/docker.sock"), cwd, &boundary);
        assert!(socket.warnings.contains(&PathWarning::ContainerRuntimeSocket));
        assert!(socket.risks.contains(&PathRisk::RuntimeControlSocket));

        let kernel = resolve_intent_target(&write_intent("/proc/1/root/etc/shadow"), cwd, &boundary);
        assert!(kernel.warnings.contains(&PathWarning::PrivilegedKernelMaterial));
        assert!(kernel.risks.contains(&PathRisk::PrivilegedKernelMaterial));
    }

    #[test]
    fn parses_mountinfo_and_classifies_special_mounts() {
        let mountinfo = "36 29 0:32 / / rw,relatime - overlay overlay rw,lowerdir=/x\n\
                         37 36 0:33 / /run/user/501/doc rw,nosuid,nodev - fuse.portal portal rw\n\
                         38 36 0:34 / /mnt/shared rw,relatime - virtiofs hostshare rw";
        let portal = parse_mountinfo(mountinfo, Path::new("/run/user/501/doc/file.txt")).unwrap();
        assert_eq!(portal.kind, EnvironmentMountKind::XdgDocumentPortal);
        let shared = parse_mountinfo(mountinfo, Path::new("/mnt/shared/project/file.txt")).unwrap();
        assert_eq!(shared.kind, EnvironmentMountKind::VirtioFs);
        assert_eq!(shared.identity.as_ref().unwrap().source, "hostshare");
    }

    #[test]
    fn trusted_external_relation_is_distinct_from_workspace() {
        let cwd = tempfile::tempdir().unwrap();
        let trusted = tempfile::tempdir().unwrap();
        let settings = std::sync::Arc::new(std::sync::Mutex::new(crate::settings::Settings {
            trusted_directories: vec![trusted.path().display().to_string()],
            ..Default::default()
        }));
        let boundary = crate::tools::WorkspaceBoundary::new(cwd.path().to_path_buf()).with_settings(settings);
        let target = trusted.path().join("out.txt");
        std::fs::write(&target, "ok").unwrap();
        let intent = write_intent(&target.display().to_string());
        let resolved = resolve_intent_target(&intent, cwd.path(), &boundary);
        assert_eq!(resolved.relation, WorkspaceRelation::TrustedExternal);
    }
}
