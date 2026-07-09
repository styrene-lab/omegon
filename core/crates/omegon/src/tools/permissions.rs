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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathTarget {
    WorkspaceRelative { raw: String },
    HostAbsolute { raw: String },
    HomeRelative { raw: String },
    SpecialDevice { raw: String },
    FileDescriptor { raw: String },
    Unknown { raw: String },
}

impl PathTarget {
    pub fn classify(raw: &str) -> Self {
        if raw == "/dev/null" || raw == "/dev/stdin" || raw == "/dev/stdout" || raw == "/dev/stderr"
        {
            Self::SpecialDevice {
                raw: raw.to_string(),
            }
        } else if raw.starts_with("/dev/fd/") || raw.starts_with("/proc/self/fd/") {
            Self::FileDescriptor {
                raw: raw.to_string(),
            }
        } else if raw.starts_with('/') {
            Self::HostAbsolute {
                raw: raw.to_string(),
            }
        } else if raw.starts_with('~') {
            Self::HomeRelative {
                raw: raw.to_string(),
            }
        } else if raw.is_empty() {
            Self::Unknown {
                raw: raw.to_string(),
            }
        } else {
            Self::WorkspaceRelative {
                raw: raw.to_string(),
            }
        }
    }

    pub fn raw(&self) -> &str {
        match self {
            Self::WorkspaceRelative { raw }
            | Self::HostAbsolute { raw }
            | Self::HomeRelative { raw }
            | Self::SpecialDevice { raw }
            | Self::FileDescriptor { raw }
            | Self::Unknown { raw } => raw,
        }
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFsTarget {
    pub raw: String,
    pub expanded: PathBuf,
    pub canonical: PathBuf,
    pub relation: WorkspaceRelation,
    pub warnings: Vec<PathWarning>,
}

pub fn classify_path_warnings(raw: &str) -> Vec<PathWarning> {
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

    warnings
}

pub fn resolve_intent_target(
    intent: &FsIntent,
    cwd: &Path,
    boundary: &crate::tools::WorkspaceBoundary,
) -> ResolvedFsTarget {
    let raw = intent.raw_path().to_string();
    let expanded = if raw.starts_with('/') || raw.starts_with('~') {
        expand_tilde_for_intent(&raw)
    } else {
        cwd.join(&raw)
    };
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
        warnings: classify_path_warnings(intent.raw_path()),
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
