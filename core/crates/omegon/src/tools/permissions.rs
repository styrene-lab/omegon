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
            | Self::WslDriveMount { raw, .. }
            | Self::MsysDriveMount { raw, .. }
            | Self::CygwinDriveMount { raw, .. }
            | Self::SpecialDevice { raw }
            | Self::FileDescriptor { raw }
            | Self::Unknown { raw } => raw,
        }
    }
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
    WindowsDriveAbsolutePath,
    WindowsDriveRelative,
    WindowsRootRelative,
    WindowsVerbatimPath,
    WindowsUncPath,
    WindowsDeviceName,
    WslWindowsDriveMount,
    MsysWindowsDriveMount,
    CygwinWindowsDriveMount,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFsTarget {
    pub raw: String,
    pub expanded: PathBuf,
    pub canonical: PathBuf,
    pub relation: WorkspaceRelation,
    pub warnings: Vec<PathWarning>,
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

    warnings.sort_by_key(|warning| format!("{warning:?}"));
    warnings.dedup();
    warnings
}

pub fn resolve_intent_target(
    intent: &FsIntent,
    cwd: &Path,
    boundary: &crate::tools::WorkspaceBoundary,
) -> ResolvedFsTarget {
    let raw = intent.raw_path().to_string();
    let expanded = expanded_path_for_target(&intent.target, cwd);
    let canonical = crate::tools::canonicalize_existing_parent_for_permissions(&expanded);
    let relation = if crate::tools::is_allowed_special_path_for_permissions(&expanded) {
        WorkspaceRelation::SpecialAllowed
    } else if boundary.is_inside_boundary(&expanded) {
        WorkspaceRelation::InsideWorkspace
    } else {
        WorkspaceRelation::OutsideWorkspace
    };

    ResolvedFsTarget {
        raw,
        expanded,
        canonical,
        relation,
        warnings: classify_path_warnings(&intent.target, PathDialect::detect_from_env()),
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
    ) && resolved
        .warnings
        .iter()
        .any(|w| matches!(w, PathWarning::ShortRootPath))
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
    }
}
