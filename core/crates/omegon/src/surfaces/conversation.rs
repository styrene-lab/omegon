//! Shared semantic conversation surface projection.
//!
//! This module is renderer- and transport-neutral. It describes conversation
//! segments in terms that TUI renderers, ACP DTO adapters, exports, and future
//! clients can consume without depending on Ratatui, terminal styling, or ACP
//! wire types.
//!
//! Keep this layer semantic: roles, segment payloads, completion state, and tool
//! categories belong here; colors, protocol field names, redaction policy, and
//! widget layout belong in downstream adapters.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentRole {
    Operator,
    Assistant,
    /// Agent-to-agent conversation participants such as cleave children,
    /// delegated workers, or remote peer agents.
    PeerAgent,
    Tool,
    System,
    Lifecycle,
    Media,
    Separator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentEmphasis {
    Strong,
    Normal,
    Muted,
}

/// Semantic presentation hints common to all surface adapters.
///
/// `tool_category` is intentionally not a color/style. Renderers map it to
/// visual treatment; protocol adapters map it to metadata strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentPresentation {
    pub role: SegmentRole,
    pub sigil: &'static str,
    pub emphasis: SegmentEmphasis,
    pub tool_category: Option<ToolCategory>,
}

/// Typed, presentation-ready segment projection.
///
/// The type parameters let callers choose owned (`String`, `PathBuf`) or
/// borrowed (`&str`, `&Path`) payloads. That keeps this projection layer usable
/// both for cheap per-frame views over `SegmentContent` and for durable tests or
/// export snapshots that need owned data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationSegmentProjection<TText = String, TPath = PathBuf>
where
    TText: AsRef<str>,
{
    pub presentation: SegmentPresentation,
    pub kind: ConversationSegmentKind<TText, TPath>,
}

fn text_form(text: &str) -> ContentForm {
    let trimmed = text.trim_start();
    if trimmed.is_empty() {
        ContentForm::Empty
    } else if trimmed.starts_with('#')
        || trimmed.starts_with("```")
        || trimmed.contains(
            "
#",
        )
        || trimmed.contains(
            "
- ",
        )
        || trimmed.contains(
            "
* ",
        )
    {
        ContentForm::Markdown
    } else {
        ContentForm::Prose
    }
}

fn tool_content_form<TText: AsRef<str>>(tool: &ToolSegment<TText>) -> ContentForm {
    let name = tool.name.as_ref();
    let detail = tool
        .detail_result
        .as_ref()
        .map(|t| t.as_ref())
        .unwrap_or("");
    match name {
        "read" | "view" => text_form(detail),
        "bash" => ContentForm::Log,
        "edit" | "change" => ContentForm::Diff,
        _ => {
            let text = tool
                .detail_result
                .as_ref()
                .or(tool.result_summary.as_ref())
                .map(|t| t.as_ref())
                .unwrap_or("");
            if text.trim_start().starts_with(['{', '[']) {
                ContentForm::Json
            } else if text.is_empty() {
                ContentForm::Empty
            } else {
                ContentForm::Structured
            }
        }
    }
}

impl<TText, TPath> ConversationSegmentProjection<TText, TPath>
where
    TText: AsRef<str>,
{
    pub fn new(kind: ConversationSegmentKind<TText, TPath>) -> Self {
        let tool_category = kind.tool_category();
        Self {
            presentation: presentation_for_role(kind.role(), tool_category),
            kind,
        }
    }

    pub fn role(&self) -> SegmentRole {
        self.presentation.role
    }

    pub fn presentation_model(&self) -> SegmentPresentationModel<'_> {
        match &self.kind {
            ConversationSegmentKind::User(user) => SegmentPresentationModel {
                producer: SegmentProducer::Operator,
                state: SegmentState::Completed,
                content: SegmentContentPresentation {
                    form: text_form(user.text.as_ref()),
                    title: None,
                    summary: Some(user.text.as_ref()),
                    body: Some(user.text.as_ref()),
                },
                metrics: Vec::new(),
                affordances: SegmentAffordances {
                    copyable: true,
                    selectable: true,
                    ..Default::default()
                },
                surface: SegmentSurfacePolicy {
                    surface: SegmentSurfaceTreatment::Transcript,
                    copy: SegmentCopyPolicy::Body,
                    selection: SegmentSelectionTreatment::Subtle,
                },
            },
            ConversationSegmentKind::Assistant(assistant) => SegmentPresentationModel {
                producer: SegmentProducer::Assistant,
                state: if assistant.complete {
                    SegmentState::Completed
                } else {
                    SegmentState::Running
                },
                content: SegmentContentPresentation {
                    form: text_form(assistant.text.as_ref()),
                    title: None,
                    summary: Some(assistant.text.as_ref()),
                    body: Some(assistant.text.as_ref()),
                },
                metrics: Vec::new(),
                affordances: SegmentAffordances {
                    copyable: true,
                    selectable: true,
                    ..Default::default()
                },
                surface: SegmentSurfacePolicy {
                    surface: SegmentSurfaceTreatment::Transcript,
                    copy: SegmentCopyPolicy::Body,
                    selection: SegmentSelectionTreatment::Subtle,
                },
            },
            ConversationSegmentKind::PeerAgent(peer) => SegmentPresentationModel {
                producer: SegmentProducer::PeerAgent {
                    label: peer.label.as_ref(),
                    source: peer.source,
                },
                state: match peer.status {
                    PeerAgentStatus::Running => SegmentState::Running,
                    PeerAgentStatus::Completed => SegmentState::Completed,
                    PeerAgentStatus::Failed => SegmentState::Failed,
                    PeerAgentStatus::Cancelled => SegmentState::Cancelled,
                    PeerAgentStatus::Deferred => SegmentState::Pending,
                },
                content: SegmentContentPresentation {
                    form: text_form(peer.text.as_ref()),
                    title: Some(peer.label.as_ref()),
                    summary: Some(peer.text.as_ref()),
                    body: Some(peer.text.as_ref()),
                },
                metrics: vec![
                    SegmentMetric::new(peer.source.as_str()).with_emphasis(MetricEmphasis::Muted),
                ],
                affordances: SegmentAffordances {
                    copyable: true,
                    selectable: true,
                    ..Default::default()
                },
                surface: SegmentSurfacePolicy {
                    surface: SegmentSurfaceTreatment::Transcript,
                    copy: SegmentCopyPolicy::Body,
                    selection: SegmentSelectionTreatment::Subtle,
                },
            },
            ConversationSegmentKind::Tool(tool) => SegmentPresentationModel {
                producer: SegmentProducer::Tool {
                    name: tool.name.as_ref(),
                    category: tool_category_for_name(tool.name.as_ref()),
                },
                state: if tool.is_error {
                    SegmentState::Failed
                } else if tool.complete {
                    SegmentState::Completed
                } else {
                    SegmentState::Running
                },
                content: SegmentContentPresentation {
                    form: tool_content_form(tool),
                    title: Some(tool.name.as_ref()),
                    summary: tool.result_summary.as_ref().map(|t| t.as_ref()),
                    body: tool.detail_result.as_ref().map(|t| t.as_ref()),
                },
                metrics: Vec::new(),
                affordances: SegmentAffordances {
                    detail_available: tool.detail_args.is_some() || tool.detail_result.is_some(),
                    expandable: tool.detail_args.is_some() || tool.detail_result.is_some(),
                    selectable: true,
                    copyable: tool.detail_result.is_some(),
                },
                surface: SegmentSurfacePolicy {
                    surface: SegmentSurfaceTreatment::Card,
                    copy: if tool.detail_result.is_some() {
                        SegmentCopyPolicy::Detail
                    } else if tool.result_summary.is_some() {
                        SegmentCopyPolicy::Summary
                    } else {
                        SegmentCopyPolicy::None
                    },
                    selection: SegmentSelectionTreatment::Explicit,
                },
            },
            ConversationSegmentKind::OperatorCopy(copy) => SegmentPresentationModel {
                producer: SegmentProducer::System,
                state: SegmentState::Informational,
                content: SegmentContentPresentation {
                    form: ContentForm::Code,
                    title: Some(copy.label.as_ref()),
                    summary: Some(copy.text.as_ref()),
                    body: Some(copy.text.as_ref()),
                },
                metrics: Vec::new(),
                affordances: SegmentAffordances {
                    selectable: true,
                    copyable: true,
                    ..Default::default()
                },
                surface: SegmentSurfacePolicy {
                    surface: SegmentSurfaceTreatment::Card,
                    copy: SegmentCopyPolicy::Body,
                    selection: SegmentSelectionTreatment::Explicit,
                },
            },
            ConversationSegmentKind::System(system) => SegmentPresentationModel {
                producer: SegmentProducer::System,
                state: SegmentState::Informational,
                content: SegmentContentPresentation {
                    form: text_form(system.text.as_ref()),
                    title: None,
                    summary: Some(system.text.as_ref()),
                    body: Some(system.text.as_ref()),
                },
                metrics: Vec::new(),
                affordances: SegmentAffordances {
                    selectable: true,
                    ..Default::default()
                },
                surface: SegmentSurfacePolicy {
                    surface: SegmentSurfaceTreatment::Transcript,
                    copy: SegmentCopyPolicy::Body,
                    selection: SegmentSelectionTreatment::Subtle,
                },
            },
            ConversationSegmentKind::Skill(skill) => SegmentPresentationModel {
                producer: SegmentProducer::Skill {
                    active_ref: skill.active_ref.as_ref(),
                },
                state: SegmentState::Informational,
                content: SegmentContentPresentation {
                    form: ContentForm::Structured,
                    title: Some("skill"),
                    summary: Some(skill.active_ref.as_ref()),
                    body: Some(skill.reason.as_ref()),
                },
                metrics: vec![
                    SegmentMetric::labeled("reason", skill.reason.as_ref())
                        .with_emphasis(MetricEmphasis::Muted),
                    SegmentMetric::labeled("resolution", skill.resolution.as_ref())
                        .with_emphasis(MetricEmphasis::Muted),
                ],
                affordances: SegmentAffordances {
                    selectable: true,
                    copyable: true,
                    ..Default::default()
                },
                surface: SegmentSurfacePolicy {
                    surface: SegmentSurfaceTreatment::ChromeOnly,
                    copy: SegmentCopyPolicy::Summary,
                    selection: SegmentSelectionTreatment::Subtle,
                },
            },
            ConversationSegmentKind::Lifecycle(lifecycle) => SegmentPresentationModel {
                producer: SegmentProducer::Lifecycle,
                state: SegmentState::Informational,
                content: SegmentContentPresentation {
                    form: ContentForm::Structured,
                    title: Some(lifecycle.icon.as_ref()),
                    summary: Some(lifecycle.text.as_ref()),
                    body: Some(lifecycle.text.as_ref()),
                },
                metrics: Vec::new(),
                affordances: SegmentAffordances {
                    selectable: true,
                    ..Default::default()
                },
                surface: SegmentSurfacePolicy {
                    surface: SegmentSurfaceTreatment::ChromeOnly,
                    copy: SegmentCopyPolicy::Summary,
                    selection: SegmentSelectionTreatment::Subtle,
                },
            },
            ConversationSegmentKind::Image(image) => SegmentPresentationModel {
                producer: SegmentProducer::Media,
                state: SegmentState::Completed,
                content: SegmentContentPresentation {
                    form: ContentForm::Image,
                    title: Some(image.alt.as_ref()),
                    summary: Some(image.alt.as_ref()),
                    body: None,
                },
                metrics: Vec::new(),
                affordances: SegmentAffordances {
                    detail_available: true,
                    selectable: true,
                    ..Default::default()
                },
                surface: SegmentSurfacePolicy {
                    surface: SegmentSurfaceTreatment::Panel,
                    copy: SegmentCopyPolicy::None,
                    selection: SegmentSelectionTreatment::Explicit,
                },
            },
            ConversationSegmentKind::Separator => SegmentPresentationModel {
                producer: SegmentProducer::Separator,
                state: SegmentState::Informational,
                content: SegmentContentPresentation {
                    form: ContentForm::Separator,
                    title: None,
                    summary: None,
                    body: None,
                },
                metrics: Vec::new(),
                affordances: SegmentAffordances::default(),
                surface: SegmentSurfacePolicy {
                    surface: SegmentSurfaceTreatment::ChromeOnly,
                    copy: SegmentCopyPolicy::None,
                    selection: SegmentSelectionTreatment::None,
                },
            },
        }
    }
}

/// Segment-specific projection payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversationSegmentKind<TText = String, TPath = PathBuf> {
    User(UserSegment<TText>),
    Assistant(AssistantSegment<TText>),
    PeerAgent(PeerAgentSegment<TText>),
    Tool(ToolSegment<TText>),
    OperatorCopy(OperatorCopySegment<TText>),
    System(SystemSegment<TText>),
    Skill(SkillEventSegment<TText>),
    Lifecycle(LifecycleSegment<TText>),
    Image(ImageSegment<TText, TPath>),
    Separator,
}

impl<TText, TPath> ConversationSegmentKind<TText, TPath> {
    pub fn role(&self) -> SegmentRole {
        match self {
            Self::User(_) => SegmentRole::Operator,
            Self::Assistant(_) => SegmentRole::Assistant,
            Self::PeerAgent(_) => SegmentRole::PeerAgent,
            Self::Tool(_) => SegmentRole::Tool,
            Self::OperatorCopy(_) => SegmentRole::System,
            Self::System(_) => SegmentRole::System,
            Self::Skill(_) => SegmentRole::Lifecycle,
            Self::Lifecycle(_) => SegmentRole::Lifecycle,
            Self::Image(_) => SegmentRole::Media,
            Self::Separator => SegmentRole::Separator,
        }
    }
}

impl<TText, TPath> ConversationSegmentKind<TText, TPath>
where
    TText: AsRef<str>,
{
    pub fn tool_category(&self) -> Option<ToolCategory> {
        match self {
            Self::Tool(tool) => Some(tool_category_for_name(tool.name.as_ref())),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentState {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Informational,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentProducer<'a> {
    Operator,
    Assistant,
    PeerAgent {
        label: &'a str,
        source: PeerAgentSource,
    },
    Tool {
        name: &'a str,
        category: ToolCategory,
    },
    Skill {
        active_ref: &'a str,
    },
    System,
    Lifecycle,
    Media,
    Separator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentForm {
    Prose,
    Markdown,
    Code,
    Log,
    Diff,
    Json,
    Structured,
    Image,
    Separator,
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricEmphasis {
    Normal,
    Muted,
    Strong,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentMetric<'a> {
    pub label: Option<&'a str>,
    pub value: &'a str,
    pub emphasis: MetricEmphasis,
}

impl<'a> SegmentMetric<'a> {
    pub fn new(value: &'a str) -> Self {
        Self {
            label: None,
            value,
            emphasis: MetricEmphasis::Normal,
        }
    }

    pub fn labeled(label: &'a str, value: &'a str) -> Self {
        Self {
            label: Some(label),
            value,
            emphasis: MetricEmphasis::Normal,
        }
    }

    pub fn with_emphasis(mut self, emphasis: MetricEmphasis) -> Self {
        self.emphasis = emphasis;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SegmentAffordances {
    pub detail_available: bool,
    pub expandable: bool,
    pub selectable: bool,
    pub copyable: bool,
}

/// Renderer-neutral surface treatment intent for a conversation segment.
///
/// This is semantic presentation policy, not a terminal layout contract: adapters
/// decide whether transcript/card/panel/chrome-only becomes a Ratatui block, a
/// web component, an ACP field, or another frontend-specific representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentSurfaceTreatment {
    Transcript,
    Card,
    Panel,
    ChromeOnly,
}

/// Renderer-neutral copy/export source for a conversation segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentCopyPolicy {
    None,
    Body,
    Summary,
    Detail,
    Full,
}

/// Renderer-neutral selection affordance intensity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentSelectionTreatment {
    None,
    Subtle,
    Explicit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentContentPresentation<'a> {
    pub form: ContentForm,
    pub title: Option<&'a str>,
    pub summary: Option<&'a str>,
    pub body: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentSurfacePolicy {
    pub surface: SegmentSurfaceTreatment,
    pub copy: SegmentCopyPolicy,
    pub selection: SegmentSelectionTreatment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentPresentationModel<'a> {
    pub producer: SegmentProducer<'a>,
    pub state: SegmentState,
    pub content: SegmentContentPresentation<'a>,
    pub metrics: Vec<SegmentMetric<'a>>,
    pub affordances: SegmentAffordances,
    pub surface: SegmentSurfacePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserSegment<TText = String> {
    pub text: TText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantSegment<TText = String> {
    pub text: TText,
    pub thinking: TText,
    pub complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerAgentSource {
    Delegate,
    Cleave,
    A2a,
}

impl PeerAgentSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Delegate => "delegate",
            Self::Cleave => "cleave",
            Self::A2a => "a2a",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerAgentStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
    Deferred,
}

impl PeerAgentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Deferred => "deferred",
        }
    }

    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Running | Self::Deferred)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerAgentSegment<TText = String> {
    pub label: TText,
    pub source: PeerAgentSource,
    pub status: PeerAgentStatus,
    pub text: TText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSegment<TText = String> {
    pub id: TText,
    pub name: TText,
    pub args_summary: Option<TText>,
    pub detail_args: Option<TText>,
    pub result_summary: Option<TText>,
    pub detail_result: Option<TText>,
    pub is_error: bool,
    pub complete: bool,
    pub expanded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemSegment<TText = String> {
    pub text: TText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorCopySegment<TText = String> {
    pub label: TText,
    pub text: TText,
    pub kind: omegon_traits::OperatorCopyKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillEventSegment<TText = String> {
    pub active_ref: TText,
    pub reason: TText,
    pub resolution: TText,
    pub suppressing: Vec<TText>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleSegment<TText = String> {
    pub icon: TText,
    pub text: TText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSegment<TText = String, TPath = PathBuf> {
    pub path: TPath,
    pub alt: TText,
}

pub trait ProjectConversationSegment<'a> {
    type Text: AsRef<str>;
    type Path;

    fn project_conversation_segment(
        &'a self,
    ) -> ConversationSegmentProjection<Self::Text, Self::Path>;
}

pub type BorrowedConversationSegmentProjection<'a> =
    ConversationSegmentProjection<&'a str, &'a Path>;

pub type OwnedConversationSegmentProjection = ConversationSegmentProjection<String, PathBuf>;

/// Semantic category for known tool families.
///
/// This is shared classification, not presentation. TUI maps it to colors and
/// labels; ACP maps it to stable metadata strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    CommandExec,
    FileRead,
    FileMutation,
    DesignTree,
    Memory,
    Search,
    Subagent,
    Network,
    Generic,
}

impl ToolCategory {
    /// Short label for focus-mode display.
    pub fn label(&self) -> &'static str {
        match self {
            Self::CommandExec => "exec",
            Self::FileRead => "read",
            Self::FileMutation => "mutate",
            Self::DesignTree => "design",
            Self::Memory => "memory",
            Self::Search => "search",
            Self::Subagent => "subagent",
            Self::Network => "network",
            Self::Generic => "tool",
        }
    }
}

pub fn tool_category_for_name(name: &str) -> ToolCategory {
    tool_visual_identity(name, None).category()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRealm {
    Execution,
    Filesystem,
    Retrieval,
    Knowledge,
    Orchestration,
    Design,
    Harness,
    External,
    Diagnostics,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolFamily {
    Shell,
    Cargo,
    Git,
    Package,
    Container,
    Kubernetes,
    Remote,
    Build,
    FileRead,
    FileWrite,
    Validate,
    Archive,
    CodebaseSearch,
    DocumentSearch,
    WebSearch,
    BrowserSearch,
    ShellSearch,
    Memory,
    Context,
    ProjectGraph,
    Time,
    Plan,
    Delegate,
    Cleave,
    Kanban,
    Engagement,
    DesignTree,
    Drawing,
    Diagram,
    DesignBoard,
    Flow,
    FlyntUi,
    ToolRegistry,
    ModelRuntime,
    Settings,
    Identity,
    Secrets,
    Nex,
    Reader,
    Network,
    Browser,
    GoogleWorkspace,
    Forge,
    SecurityScan,
    Doctor,
    Status,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolTransport {
    HarnessTool,
    Shell,
    Terminal,
    Browser,
    Extension,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolVisualIdentity {
    pub raw_name: String,
    pub label: String,
    pub realm: ToolRealm,
    pub family: ToolFamily,
    pub transport: ToolTransport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolActivitySummary {
    pub raw_name: String,
    pub args_summary: Option<String>,
}

impl ToolActivitySummary {
    pub fn new(raw_name: impl Into<String>, args_summary: Option<String>) -> Self {
        Self {
            raw_name: raw_name.into(),
            args_summary,
        }
    }

    pub fn visual_identity(&self) -> ToolVisualIdentity {
        tool_visual_identity(&self.raw_name, self.args_summary.as_deref())
    }
}

impl ToolVisualIdentity {
    pub fn category(&self) -> ToolCategory {
        match self.realm {
            ToolRealm::Execution => ToolCategory::CommandExec,
            ToolRealm::Filesystem => match self.family {
                ToolFamily::FileRead => ToolCategory::FileRead,
                ToolFamily::FileWrite => ToolCategory::FileMutation,
                _ => ToolCategory::Generic,
            },
            ToolRealm::Retrieval => ToolCategory::Search,
            ToolRealm::Knowledge => ToolCategory::Memory,
            ToolRealm::Orchestration => ToolCategory::Subagent,
            ToolRealm::Design => ToolCategory::DesignTree,
            ToolRealm::External => ToolCategory::Network,
            _ => ToolCategory::Generic,
        }
    }
}

pub fn tool_visual_identity(name: &str, detail_args: Option<&str>) -> ToolVisualIdentity {
    if matches!(name, "bash" | "terminal" | "shell") {
        return shell_tool_visual_identity(name, detail_args);
    }

    let (realm, family, label) = match name {
        "read" | "view" | "reader_open" | "reader_open_dry_run" => {
            (ToolRealm::Filesystem, ToolFamily::FileRead, "read")
        }
        "write" | "edit" | "change" => (ToolRealm::Filesystem, ToolFamily::FileWrite, "write"),
        "validate" => (ToolRealm::Filesystem, ToolFamily::Validate, "validate"),
        "commit" | "git_login" => (ToolRealm::Execution, ToolFamily::Git, "git"),
        "codebase_search" => (ToolRealm::Retrieval, ToolFamily::CodebaseSearch, "codebase"),
        "search_documents"
        | "list_documents"
        | "find_document_by_slug"
        | "get_document"
        | "get_backlinks" => (ToolRealm::Retrieval, ToolFamily::DocumentSearch, "docs"),
        "web_search" | "web_fetch" => (ToolRealm::Retrieval, ToolFamily::WebSearch, "web"),
        "browser_search"
        | "browser_search_receive"
        | "browser_record_receive"
        | "browser_recipe_draft" => (ToolRealm::Retrieval, ToolFamily::BrowserSearch, "browser"),
        "browser_google_workspace_open" | "browser_google_workspace_probe" => {
            (ToolRealm::External, ToolFamily::GoogleWorkspace, "google")
        }
        "context_status" | "request_context" | "context_compact" | "context_clear" => {
            (ToolRealm::Knowledge, ToolFamily::Context, "context")
        }
        "get_graph" | "get_graph_filtered" | "get_node_neighbors" => {
            (ToolRealm::Knowledge, ToolFamily::ProjectGraph, "graph")
        }
        "chronos" => (ToolRealm::Knowledge, ToolFamily::Time, "time"),
        "plan" => (ToolRealm::Orchestration, ToolFamily::Plan, "plan"),
        "delegate" | "delegate_result" | "delegate_status" | "delegate_cancel" => {
            (ToolRealm::Orchestration, ToolFamily::Delegate, "delegate")
        }
        "cleave_assess" | "cleave_run" => (ToolRealm::Orchestration, ToolFamily::Cleave, "cleave"),
        "list_tasks" | "get_task" | "create_task" | "update_task" | "list_boards" | "get_board"
        | "create_board" | "delete_board" => {
            (ToolRealm::Orchestration, ToolFamily::Kanban, "kanban")
        }
        "design_tree"
        | "design_tree_update"
        | "list_design_nodes"
        | "convert_to_design_node"
        | "openspec_manage"
        | "lifecycle_doctor" => (ToolRealm::Design, ToolFamily::DesignTree, "design"),
        "create_drawing" => (ToolRealm::Design, ToolFamily::Drawing, "drawing"),
        "create_d2_diagram" | "render_diagram" => {
            (ToolRealm::Design, ToolFamily::Diagram, "diagram")
        }
        "get_ui_state" | "flynt_surface_guide" => (ToolRealm::Design, ToolFamily::FlyntUi, "ui"),
        "manage_tools" => (ToolRealm::Harness, ToolFamily::ToolRegistry, "tools"),
        "ask_local_model" | "list_local_models" | "manage_ollama" => {
            (ToolRealm::Harness, ToolFamily::ModelRuntime, "model")
        }
        "whoami" => (ToolRealm::Harness, ToolFamily::Identity, "identity"),
        name if name.starts_with("memory_") || name.contains("memory") => (
            ToolRealm::Knowledge,
            ToolFamily::Memory,
            memory_tool_label(name),
        ),
        name if name.starts_with("drawing_") => (ToolRealm::Design, ToolFamily::Drawing, "drawing"),
        name if name.starts_with("design_board_") => {
            (ToolRealm::Design, ToolFamily::DesignBoard, "board")
        }
        name if name.starts_with("flow_") => (ToolRealm::Design, ToolFamily::Flow, "flow"),
        name if name.starts_with("engagement_") || name.starts_with("forge_") => {
            (ToolRealm::Orchestration, ToolFamily::Engagement, "engage")
        }
        name if name.starts_with("secret_") => (ToolRealm::Harness, ToolFamily::Secrets, "secret"),
        name if name.starts_with("nex_") => (ToolRealm::Harness, ToolFamily::Nex, "nex"),
        name if name.starts_with("reader_") => (ToolRealm::Harness, ToolFamily::Reader, "reader"),
        name if name.starts_with("lipstyk_") => {
            (ToolRealm::Diagnostics, ToolFamily::SecurityScan, "scan")
        }
        name if name.contains("status") || name.contains("doctor") => {
            (ToolRealm::Diagnostics, ToolFamily::Doctor, "status")
        }
        name if name.contains("search") => (ToolRealm::Retrieval, ToolFamily::Generic, "search"),
        _ => (ToolRealm::Generic, ToolFamily::Generic, "tool"),
    };

    ToolVisualIdentity {
        raw_name: name.to_string(),
        label: label.to_string(),
        realm,
        family,
        transport: ToolTransport::HarnessTool,
    }
}

fn memory_tool_label(name: &str) -> &'static str {
    match name {
        "memory_recall" | "memory_query" | "memory_episodes" | "memory_search_archive" => {
            "mem read"
        }
        "memory_store"
        | "memory_archive"
        | "memory_supersede"
        | "memory_connect"
        | "memory_ingest_lifecycle" => "mem write",
        "memory_focus" => "mem pin",
        "memory_release" => "mem unpin",
        "memory_compact" => "mem compact",
        _ => "memory",
    }
}

fn shell_tool_visual_identity(name: &str, detail_args: Option<&str>) -> ToolVisualIdentity {
    let command = detail_args.and_then(shell_command_from_args);
    let first_word = command
        .as_deref()
        .unwrap_or(name)
        .split_whitespace()
        .next()
        .unwrap_or(name);
    let (realm, family, label) = match first_word {
        "grep" | "rg" | "find" => (ToolRealm::Retrieval, ToolFamily::ShellSearch, "search"),
        "ls" | "dir" | "cat" | "head" | "tail" | "bat" => {
            (ToolRealm::Filesystem, ToolFamily::FileRead, "read")
        }
        "sed" | "awk" | "mkdir" | "rm" | "mv" | "cp" | "chmod" | "chown" => {
            (ToolRealm::Filesystem, ToolFamily::FileWrite, "write")
        }
        "git" => (ToolRealm::Execution, ToolFamily::Git, "git"),
        "cargo" => (ToolRealm::Execution, ToolFamily::Cargo, "cargo"),
        "docker" | "podman" => (ToolRealm::Execution, ToolFamily::Container, "container"),
        "kubectl" | "k" => (ToolRealm::Execution, ToolFamily::Kubernetes, "kubectl"),
        "make" | "cmake" => (ToolRealm::Execution, ToolFamily::Build, "build"),
        "curl" | "wget" | "dig" | "nslookup" | "host" => {
            (ToolRealm::External, ToolFamily::Network, "network")
        }
        "ssh" | "scp" | "rsync" => (ToolRealm::Execution, ToolFamily::Remote, "remote"),
        "tar" | "zip" | "unzip" | "gzip" => (ToolRealm::Filesystem, ToolFamily::Archive, "archive"),
        "test" | "[" => (ToolRealm::Diagnostics, ToolFamily::Status, "test"),
        "sh" | "bash" | "zsh" | "shell" | "terminal" => {
            (ToolRealm::Execution, ToolFamily::Shell, "shell")
        }
        other => (ToolRealm::Execution, ToolFamily::Shell, other),
    };

    ToolVisualIdentity {
        raw_name: name.to_string(),
        label: label.to_string(),
        realm,
        family,
        transport: ToolTransport::Shell,
    }
}

fn shell_command_from_args(args: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(args) {
        return value
            .get("command")
            .or_else(|| value.get("cmd"))
            .and_then(serde_json::Value::as_str)
            .map(|command| command.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    let raw = args.lines().next()?.trim();
    (!raw.is_empty()).then(|| raw.to_string())
}

pub fn presentation_for_role(
    role: SegmentRole,
    tool_category: Option<ToolCategory>,
) -> SegmentPresentation {
    match role {
        SegmentRole::Operator => SegmentPresentation {
            role,
            sigil: "OP",
            emphasis: SegmentEmphasis::Strong,
            tool_category: None,
        },
        SegmentRole::Assistant => SegmentPresentation {
            role,
            sigil: "Ω",
            emphasis: SegmentEmphasis::Normal,
            tool_category: None,
        },
        SegmentRole::PeerAgent => SegmentPresentation {
            role,
            sigil: "⬡",
            emphasis: SegmentEmphasis::Normal,
            tool_category: None,
        },
        SegmentRole::Tool => SegmentPresentation {
            role,
            sigil: "⚙",
            emphasis: SegmentEmphasis::Normal,
            tool_category,
        },
        SegmentRole::System => SegmentPresentation {
            role,
            sigil: "ℹ",
            emphasis: SegmentEmphasis::Muted,
            tool_category: None,
        },
        SegmentRole::Lifecycle => SegmentPresentation {
            role,
            sigil: "↯",
            emphasis: SegmentEmphasis::Muted,
            tool_category: None,
        },
        SegmentRole::Media => SegmentPresentation {
            role,
            sigil: "◈",
            emphasis: SegmentEmphasis::Normal,
            tool_category: None,
        },
        SegmentRole::Separator => SegmentPresentation {
            role,
            sigil: "",
            emphasis: SegmentEmphasis::Muted,
            tool_category: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_infers_role_and_presentation_from_kind() {
        let projection = ConversationSegmentProjection::<&str>::new(
            ConversationSegmentKind::Assistant(AssistantSegment {
                text: "answer",
                thinking: "",
                complete: true,
            }),
        );

        assert_eq!(projection.role(), SegmentRole::Assistant);
        assert_eq!(projection.presentation.sigil, "Ω");
        assert_eq!(projection.presentation.emphasis, SegmentEmphasis::Normal);
        assert_eq!(projection.presentation.tool_category, None);
    }

    #[test]
    fn tool_category_for_name_classifies_common_tool_families() {
        let cases = [
            ("bash", ToolCategory::CommandExec),
            ("read", ToolCategory::FileRead),
            ("edit", ToolCategory::FileMutation),
            ("design_tree_update", ToolCategory::DesignTree),
            ("memory_recall", ToolCategory::Memory),
            ("codebase_search", ToolCategory::Search),
            ("search_documents", ToolCategory::Search),
            ("browser_search", ToolCategory::Search),
            ("delegate", ToolCategory::Subagent),
            ("delegate_result", ToolCategory::Subagent),
            ("cleave_assess", ToolCategory::Subagent),
            ("cleave_run", ToolCategory::Subagent),
            ("unknown", ToolCategory::Generic),
        ];

        for (name, expected) in cases {
            assert_eq!(tool_category_for_name(name), expected, "{name}");
        }
    }

    #[test]
    fn tool_visual_identity_resolves_core_and_shell_families() {
        let cargo = tool_visual_identity("bash", Some(r#"{"command":"cargo test -p omegon"}"#));
        assert_eq!(cargo.realm, ToolRealm::Execution);
        assert_eq!(cargo.family, ToolFamily::Cargo);
        assert_eq!(cargo.transport, ToolTransport::Shell);
        assert_eq!(cargo.label, "cargo");

        let rg = tool_visual_identity("bash", Some("rg needle core"));
        assert_eq!(rg.realm, ToolRealm::Retrieval);
        assert_eq!(rg.family, ToolFamily::ShellSearch);
        assert_eq!(rg.label, "search");

        let codebase = tool_visual_identity("codebase_search", None);
        assert_eq!(codebase.realm, ToolRealm::Retrieval);
        assert_eq!(codebase.family, ToolFamily::CodebaseSearch);
        assert_eq!(codebase.transport, ToolTransport::HarnessTool);
        assert_eq!(codebase.label, "codebase");

        let docs = tool_visual_identity("search_documents", None);
        assert_eq!(docs.family, ToolFamily::DocumentSearch);
        assert_eq!(docs.label, "docs");

        let context = tool_visual_identity("context_status", None);
        assert_eq!(context.realm, ToolRealm::Knowledge);
        assert_eq!(context.family, ToolFamily::Context);
        assert_eq!(context.label, "context");

        let unknown_shell = tool_visual_identity("bash", Some("python3 script.py"));
        assert_eq!(unknown_shell.realm, ToolRealm::Execution);
        assert_eq!(unknown_shell.family, ToolFamily::Shell);
        assert_eq!(unknown_shell.transport, ToolTransport::Shell);
        assert_eq!(unknown_shell.label, "python3");

        let unknown_harness = tool_visual_identity("unknown_internal_tool", None);
        assert_eq!(unknown_harness.realm, ToolRealm::Generic);
        assert_eq!(unknown_harness.family, ToolFamily::Generic);
        assert_eq!(unknown_harness.transport, ToolTransport::HarnessTool);
        assert_eq!(unknown_harness.label, "tool");
    }

    #[test]
    fn projection_parameterization_supports_borrowed_tool_payloads() {
        let projection = ConversationSegmentProjection::<&str>::new(ConversationSegmentKind::Tool(
            ToolSegment {
                id: "tool-1",
                name: "bash",
                args_summary: Some("cargo check"),
                detail_args: None,
                result_summary: Some("ok"),
                detail_result: None,
                is_error: false,
                complete: true,
                expanded: false,
            },
        ));

        assert_eq!(projection.role(), SegmentRole::Tool);
        assert_eq!(
            projection.presentation.tool_category,
            Some(ToolCategory::CommandExec)
        );
    }

    #[test]
    fn non_tool_roles_ignore_supplied_tool_category() {
        let presentation =
            presentation_for_role(SegmentRole::Assistant, Some(ToolCategory::Memory));
        assert_eq!(presentation.tool_category, None);
        assert_eq!(presentation.role, SegmentRole::Assistant);
    }

    #[test]
    fn tool_presentation_preserves_supplied_category() {
        let presentation = presentation_for_role(SegmentRole::Tool, Some(ToolCategory::Memory));
        assert_eq!(presentation.tool_category, Some(ToolCategory::Memory));
        assert_eq!(presentation.sigil, "⚙");
    }

    #[test]
    fn projection_parameterization_supports_owned_image_payloads() {
        let projection =
            ConversationSegmentProjection::new(ConversationSegmentKind::Image(ImageSegment {
                path: PathBuf::from("/tmp/screenshot.png"),
                alt: "screenshot".to_string(),
            }));

        assert_eq!(projection.role(), SegmentRole::Media);
        assert_eq!(projection.presentation.sigil, "◈");
    }

    #[test]
    fn peer_agent_projection_uses_peer_role_and_terminal_status() {
        let projection = ConversationSegmentProjection::<&str>::new(
            ConversationSegmentKind::PeerAgent(PeerAgentSegment {
                label: "scout",
                source: PeerAgentSource::Delegate,
                status: PeerAgentStatus::Completed,
                text: "review complete",
            }),
        );

        assert_eq!(projection.role(), SegmentRole::PeerAgent);
        assert_eq!(projection.presentation.sigil, "⬡");
        assert_eq!(projection.presentation.tool_category, None);
        assert_eq!(PeerAgentSource::Delegate.as_str(), "delegate");
        assert_eq!(PeerAgentStatus::Completed.as_str(), "completed");
        assert!(PeerAgentStatus::Completed.is_terminal());
        assert!(!PeerAgentStatus::Running.is_terminal());
    }

    #[test]
    fn presentation_model_separates_assistant_producer_from_markdown_form() {
        let projection = ConversationSegmentProjection::<&str>::new(
            ConversationSegmentKind::Assistant(AssistantSegment {
                text: "# Plan

- ship it",
                thinking: "",
                complete: true,
            }),
        );

        let model = projection.presentation_model();
        assert_eq!(model.producer, SegmentProducer::Assistant);
        assert_eq!(model.state, SegmentState::Completed);
        assert_eq!(model.content.form, ContentForm::Markdown);
        assert_eq!(model.surface.surface, SegmentSurfaceTreatment::Transcript);
        assert_eq!(model.surface.copy, SegmentCopyPolicy::Body);
        assert_eq!(model.surface.selection, SegmentSelectionTreatment::Subtle);
        assert_eq!(
            model.content.body,
            Some(
                "# Plan

- ship it"
            )
        );
    }

    #[test]
    fn presentation_model_uses_same_markdown_form_for_read_tool_output() {
        let projection = ConversationSegmentProjection::<&str>::new(ConversationSegmentKind::Tool(
            ToolSegment {
                id: "tool-1",
                name: "read",
                args_summary: Some("README.md"),
                detail_args: None,
                result_summary: Some("# Plan"),
                detail_result: Some(
                    "# Plan

- ship it",
                ),
                is_error: false,
                complete: true,
                expanded: false,
            },
        ));

        let model = projection.presentation_model();
        assert_eq!(
            model.producer,
            SegmentProducer::Tool {
                name: "read",
                category: ToolCategory::FileRead,
            }
        );
        assert_eq!(model.state, SegmentState::Completed);
        assert_eq!(model.content.form, ContentForm::Markdown);
        assert!(model.affordances.detail_available);
        assert!(model.affordances.copyable);
        assert_eq!(model.surface.surface, SegmentSurfaceTreatment::Card);
        assert_eq!(model.surface.copy, SegmentCopyPolicy::Detail);
        assert_eq!(model.surface.selection, SegmentSelectionTreatment::Explicit);
    }

    #[test]
    fn presentation_model_marks_separator_as_chrome_only_not_copyable() {
        let projection =
            ConversationSegmentProjection::<&str>::new(ConversationSegmentKind::Separator);

        let model = projection.presentation_model();
        assert_eq!(model.surface.surface, SegmentSurfaceTreatment::ChromeOnly);
        assert_eq!(model.surface.copy, SegmentCopyPolicy::None);
        assert_eq!(model.surface.selection, SegmentSelectionTreatment::None);
        assert!(!model.affordances.copyable);
    }

    #[test]
    fn presentation_model_marks_image_as_panel_without_body_copy() {
        let projection =
            ConversationSegmentProjection::new(ConversationSegmentKind::Image(ImageSegment {
                path: PathBuf::from("/tmp/screenshot.png"),
                alt: "screenshot".to_string(),
            }));

        let model = projection.presentation_model();
        assert_eq!(model.surface.surface, SegmentSurfaceTreatment::Panel);
        assert_eq!(model.surface.copy, SegmentCopyPolicy::None);
        assert_eq!(model.surface.selection, SegmentSelectionTreatment::Explicit);
    }

    #[test]
    fn presentation_model_classifies_structured_tool_forms_without_role_coupling() {
        let bash = ConversationSegmentProjection::<&str>::new(ConversationSegmentKind::Tool(
            ToolSegment {
                id: "tool-1",
                name: "bash",
                args_summary: Some("cargo test"),
                detail_args: None,
                result_summary: Some("ok"),
                detail_result: Some("running tests..."),
                is_error: false,
                complete: false,
                expanded: false,
            },
        ));
        let edit = ConversationSegmentProjection::<&str>::new(ConversationSegmentKind::Tool(
            ToolSegment {
                id: "tool-2",
                name: "edit",
                args_summary: Some("file.rs"),
                detail_args: None,
                result_summary: Some("1 file"),
                detail_result: Some(
                    "- old
+ new",
                ),
                is_error: false,
                complete: true,
                expanded: false,
            },
        ));

        assert_eq!(bash.presentation_model().content.form, ContentForm::Log);
        assert_eq!(bash.presentation_model().state, SegmentState::Running);
        assert_eq!(edit.presentation_model().content.form, ContentForm::Diff);
        assert_eq!(edit.presentation_model().state, SegmentState::Completed);
    }
}
