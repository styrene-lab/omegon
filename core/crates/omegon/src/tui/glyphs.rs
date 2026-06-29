//! Semantic glyph matrix for TUI chrome.
//!
//! Renderers ask for semantic glyph roles rather than hardcoding symbols. This
//! keeps visual policy replaceable without coupling independent surfaces.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphProfile {
    Ascii,
    Unicode,
    NerdFont,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleGlyphRole {
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceGlyphRole {
    Repo,
    Directory,
    Branch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStateGlyphRole {
    Running,
    Completed,
    Failed,
    Waiting,
    Cancelled,
    Detail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategoryGlyphRole {
    Shell,
    Read,
    Write,
    Search,
    Design,
    Memory,
    Network,
    Subagent,
    Git,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolGlyphRole {
    Running,
    Completed,
    Failed,
    Detail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineGlyphRole {
    RibbonMark,
    ProviderCloud,
    ProviderLocal,
    Route,
    GradeEmblem,
    ProfileProject,
    ProfileUser,
    ProfileDefault,
    Thinking,
    Context,
    Skill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DividerGlyphRole {
    SegmentRight,
}

#[derive(Debug, Clone, Copy)]
pub struct RuleGlyphMatrix {
    pub horizontal: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct WorkspaceGlyphMatrix {
    pub repo: &'static str,
    pub directory: &'static str,
    pub branch: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct ToolStateGlyphMatrix {
    pub running: &'static str,
    pub completed: &'static str,
    pub failed: &'static str,
    pub waiting: &'static str,
    pub cancelled: &'static str,
    pub detail: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct ToolCategoryGlyphMatrix {
    pub shell: &'static str,
    pub read: &'static str,
    pub write: &'static str,
    pub search: &'static str,
    pub design: &'static str,
    pub memory: &'static str,
    pub network: &'static str,
    pub subagent: &'static str,
    pub git: &'static str,
    pub generic: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct ToolGlyphMatrix {
    pub running: &'static str,
    pub completed: &'static str,
    pub failed: &'static str,
    pub detail: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct EngineGlyphMatrix {
    pub ribbon_mark: &'static str,
    pub provider_cloud: &'static str,
    pub provider_local: &'static str,
    pub route: &'static str,
    pub grade_emblem: &'static str,
    pub profile_project: &'static str,
    pub profile_user: &'static str,
    pub profile_default: &'static str,
    pub thinking: &'static str,
    pub context: &'static str,
    pub skill: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct DividerGlyphMatrix {
    pub segment_right: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct GlyphSet {
    pub profile: GlyphProfile,
    pub rule: RuleGlyphMatrix,
    pub workspace: WorkspaceGlyphMatrix,
    pub tool: ToolGlyphMatrix,
    pub tool_state: ToolStateGlyphMatrix,
    pub tool_category: ToolCategoryGlyphMatrix,
    pub engine: EngineGlyphMatrix,
    pub divider: DividerGlyphMatrix,
}

pub const ASCII_GLYPHS: GlyphSet = GlyphSet {
    profile: GlyphProfile::Ascii,
    rule: RuleGlyphMatrix { horizontal: "-" },
    workspace: WorkspaceGlyphMatrix {
        repo: "repo",
        directory: "$(pwd)",
        branch: "branch",
    },
    tool: ToolGlyphMatrix {
        running: "*",
        completed: "ok",
        failed: "x",
        detail: "=>",
    },
    tool_state: ToolStateGlyphMatrix {
        running: "*",
        completed: "ok",
        failed: "x",
        waiting: "o",
        cancelled: "-",
        detail: "=>",
    },
    tool_category: ToolCategoryGlyphMatrix {
        shell: "$",
        read: "read",
        write: "write",
        search: "find",
        design: "design",
        memory: "mem",
        network: "net",
        subagent: "agent",
        git: "git",
        generic: "*",
    },
    engine: EngineGlyphMatrix {
        ribbon_mark: "eng",
        provider_cloud: "cloud",
        provider_local: "local",
        route: ">",
        grade_emblem: "grade",
        profile_project: "project",
        profile_user: "user",
        profile_default: "default",
        thinking: "think",
        context: "ctx",
        skill: "skill",
    },
    divider: DividerGlyphMatrix { segment_right: ">" },
};

pub const UNICODE_GLYPHS: GlyphSet = GlyphSet {
    profile: GlyphProfile::Unicode,
    rule: RuleGlyphMatrix { horizontal: "─" },
    workspace: WorkspaceGlyphMatrix {
        repo: "⑃",
        directory: "🗀",
        branch: "ᛘ",
    },
    tool: ToolGlyphMatrix {
        running: "⧗",
        completed: "✓",
        failed: "✗",
        detail: "⟹",
    },
    tool_state: ToolStateGlyphMatrix {
        running: "⧗",
        completed: "✓",
        failed: "✗",
        waiting: "◌",
        cancelled: "⊘",
        detail: "⟹",
    },
    tool_category: ToolCategoryGlyphMatrix {
        shell: "$",
        read: "▤",
        write: "✎",
        search: "⌕",
        design: "✦",
        memory: "◎",
        network: "⇄",
        subagent: "⬡",
        git: "⑂",
        generic: "·",
    },
    engine: EngineGlyphMatrix {
        ribbon_mark: "◇",
        provider_cloud: "☁",
        provider_local: "▣",
        route: "›",
        grade_emblem: "◆",
        profile_project: "⌂",
        profile_user: "◉",
        profile_default: "○",
        thinking: "ψ",
        context: "ctx",
        skill: "★",
    },
    divider: DividerGlyphMatrix {
        segment_right: "▶"
    },
};

pub const NERD_FONT_GLYPHS: GlyphSet = GlyphSet {
    profile: GlyphProfile::NerdFont,
    rule: RuleGlyphMatrix { horizontal: "─" },
    workspace: WorkspaceGlyphMatrix {
        repo: "󰊢",
        directory: "",
        branch: "",
    },
    tool: ToolGlyphMatrix {
        running: "",
        completed: "󰄬",
        failed: "",
        detail: "⟹",
    },
    tool_state: ToolStateGlyphMatrix {
        running: "",
        completed: "󰄬",
        failed: "",
        waiting: "",
        cancelled: "",
        detail: "⟹",
    },
    tool_category: ToolCategoryGlyphMatrix {
        shell: "",
        read: "󰈙",
        write: "󰷈",
        search: "󰍉",
        design: "󰙴",
        memory: "󰧑",
        network: "󰖟",
        subagent: "󰚩",
        git: "",
        generic: "·",
    },
    engine: EngineGlyphMatrix {
        ribbon_mark: "󰏉",
        provider_cloud: "",
        provider_local: "󰍹",
        route: "",
        grade_emblem: "󰿃",
        profile_project: "󰊢",
        profile_user: "",
        profile_default: "󰘳",
        thinking: "",
        context: "",
        skill: "󰓎",
    },
    divider: DividerGlyphMatrix {
        segment_right: ""
    },
};

impl GlyphSet {
    pub fn rule(self, role: RuleGlyphRole) -> &'static str {
        match role {
            RuleGlyphRole::Horizontal => self.rule.horizontal,
        }
    }

    pub fn workspace(self, role: WorkspaceGlyphRole) -> &'static str {
        match role {
            WorkspaceGlyphRole::Repo => self.workspace.repo,
            WorkspaceGlyphRole::Directory => self.workspace.directory,
            WorkspaceGlyphRole::Branch => self.workspace.branch,
        }
    }

    pub fn tool(self, role: ToolGlyphRole) -> &'static str {
        match role {
            ToolGlyphRole::Running => self.tool.running,
            ToolGlyphRole::Completed => self.tool.completed,
            ToolGlyphRole::Failed => self.tool.failed,
            ToolGlyphRole::Detail => self.tool.detail,
        }
    }

    pub fn tool_state(self, role: ToolStateGlyphRole) -> &'static str {
        match role {
            ToolStateGlyphRole::Running => self.tool_state.running,
            ToolStateGlyphRole::Completed => self.tool_state.completed,
            ToolStateGlyphRole::Failed => self.tool_state.failed,
            ToolStateGlyphRole::Waiting => self.tool_state.waiting,
            ToolStateGlyphRole::Cancelled => self.tool_state.cancelled,
            ToolStateGlyphRole::Detail => self.tool_state.detail,
        }
    }

    pub fn tool_category(self, role: ToolCategoryGlyphRole) -> &'static str {
        match role {
            ToolCategoryGlyphRole::Shell => self.tool_category.shell,
            ToolCategoryGlyphRole::Read => self.tool_category.read,
            ToolCategoryGlyphRole::Write => self.tool_category.write,
            ToolCategoryGlyphRole::Search => self.tool_category.search,
            ToolCategoryGlyphRole::Design => self.tool_category.design,
            ToolCategoryGlyphRole::Memory => self.tool_category.memory,
            ToolCategoryGlyphRole::Network => self.tool_category.network,
            ToolCategoryGlyphRole::Subagent => self.tool_category.subagent,
            ToolCategoryGlyphRole::Git => self.tool_category.git,
            ToolCategoryGlyphRole::Generic => self.tool_category.generic,
        }
    }

    pub fn engine(self, role: EngineGlyphRole) -> &'static str {
        match role {
            EngineGlyphRole::RibbonMark => self.engine.ribbon_mark,
            EngineGlyphRole::ProviderCloud => self.engine.provider_cloud,
            EngineGlyphRole::ProviderLocal => self.engine.provider_local,
            EngineGlyphRole::Route => self.engine.route,
            EngineGlyphRole::GradeEmblem => self.engine.grade_emblem,
            EngineGlyphRole::ProfileProject => self.engine.profile_project,
            EngineGlyphRole::ProfileUser => self.engine.profile_user,
            EngineGlyphRole::ProfileDefault => self.engine.profile_default,
            EngineGlyphRole::Thinking => self.engine.thinking,
            EngineGlyphRole::Context => self.engine.context,
            EngineGlyphRole::Skill => self.engine.skill,
        }
    }

    pub fn divider(self, role: DividerGlyphRole) -> &'static str {
        match role {
            DividerGlyphRole::SegmentRight => self.divider.segment_right,
        }
    }
}

pub fn tool_category_role_for_category(
    category: crate::surfaces::conversation::ToolCategory,
) -> ToolCategoryGlyphRole {
    match category {
        crate::surfaces::conversation::ToolCategory::CommandExec => ToolCategoryGlyphRole::Shell,
        crate::surfaces::conversation::ToolCategory::FileRead => ToolCategoryGlyphRole::Read,
        crate::surfaces::conversation::ToolCategory::FileMutation => ToolCategoryGlyphRole::Write,
        crate::surfaces::conversation::ToolCategory::DesignTree => ToolCategoryGlyphRole::Design,
        crate::surfaces::conversation::ToolCategory::Memory => ToolCategoryGlyphRole::Memory,
        crate::surfaces::conversation::ToolCategory::Search => ToolCategoryGlyphRole::Search,
        crate::surfaces::conversation::ToolCategory::Subagent => ToolCategoryGlyphRole::Subagent,
        crate::surfaces::conversation::ToolCategory::Network => ToolCategoryGlyphRole::Network,
        crate::surfaces::conversation::ToolCategory::Generic => ToolCategoryGlyphRole::Generic,
    }
}

pub fn tool_category_role_for_identity(
    identity: &crate::surfaces::conversation::ToolVisualIdentity,
) -> ToolCategoryGlyphRole {
    match identity.family {
        crate::surfaces::conversation::ToolFamily::Shell => ToolCategoryGlyphRole::Shell,
        crate::surfaces::conversation::ToolFamily::FileRead => ToolCategoryGlyphRole::Read,
        crate::surfaces::conversation::ToolFamily::FileWrite => ToolCategoryGlyphRole::Write,
        crate::surfaces::conversation::ToolFamily::Git => ToolCategoryGlyphRole::Git,
        crate::surfaces::conversation::ToolFamily::CodebaseSearch
        | crate::surfaces::conversation::ToolFamily::DocumentSearch
        | crate::surfaces::conversation::ToolFamily::WebSearch
        | crate::surfaces::conversation::ToolFamily::BrowserSearch
        | crate::surfaces::conversation::ToolFamily::ShellSearch
        | crate::surfaces::conversation::ToolFamily::ProjectGraph => ToolCategoryGlyphRole::Search,
        crate::surfaces::conversation::ToolFamily::Memory
        | crate::surfaces::conversation::ToolFamily::Context
        | crate::surfaces::conversation::ToolFamily::Time => ToolCategoryGlyphRole::Memory,
        crate::surfaces::conversation::ToolFamily::Delegate
        | crate::surfaces::conversation::ToolFamily::Cleave
        | crate::surfaces::conversation::ToolFamily::Plan
        | crate::surfaces::conversation::ToolFamily::Kanban
        | crate::surfaces::conversation::ToolFamily::Engagement => ToolCategoryGlyphRole::Subagent,
        crate::surfaces::conversation::ToolFamily::DesignTree
        | crate::surfaces::conversation::ToolFamily::Drawing
        | crate::surfaces::conversation::ToolFamily::Diagram
        | crate::surfaces::conversation::ToolFamily::DesignBoard
        | crate::surfaces::conversation::ToolFamily::Flow
        | crate::surfaces::conversation::ToolFamily::FlyntUi => ToolCategoryGlyphRole::Design,
        crate::surfaces::conversation::ToolFamily::Network
        | crate::surfaces::conversation::ToolFamily::Browser
        | crate::surfaces::conversation::ToolFamily::GoogleWorkspace
        | crate::surfaces::conversation::ToolFamily::Forge
        | crate::surfaces::conversation::ToolFamily::Remote => ToolCategoryGlyphRole::Network,
        _ => tool_category_role_for_category(identity.category()),
    }
}

/// Classify a tool name into a visual category glyph role.
///
/// This is intentionally a presentation helper only: it does not import tool
/// implementations or own tool semantics. Callers may use richer local state
/// and bypass this name-based fallback when they have it.
pub fn tool_category_role_for_name(name: &str) -> ToolCategoryGlyphRole {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "bash" | "terminal" | "shell" => ToolCategoryGlyphRole::Shell,
        "read" | "view" => ToolCategoryGlyphRole::Read,
        "write" | "edit" | "change" => ToolCategoryGlyphRole::Write,
        "commit" | "git" | "git_login" => ToolCategoryGlyphRole::Git,
        "codebase_search" | "search_documents" | "web_search" | "browser_search" | "rg" => {
            ToolCategoryGlyphRole::Search
        }
        "design_tree" | "design_tree_update" | "create_drawing" | "create_d2_diagram" => {
            ToolCategoryGlyphRole::Design
        }
        "memory_store" | "memory_recall" | "memory_query" | "store_memory_fact" => {
            ToolCategoryGlyphRole::Memory
        }
        "delegate" | "delegate_result" | "delegate_status" | "delegate_cancel"
        | "cleave_assess" | "cleave_run" => ToolCategoryGlyphRole::Subagent,
        name if name.contains("delegate") || name.contains("cleave") => {
            ToolCategoryGlyphRole::Subagent
        }
        name if name.contains("search") => ToolCategoryGlyphRole::Search,
        name if name.contains("memory") => ToolCategoryGlyphRole::Memory,
        name if name.contains("design") || name.contains("drawing") => {
            ToolCategoryGlyphRole::Design
        }
        name if name.contains("fetch") || name.contains("browser") || name.contains("web") => {
            ToolCategoryGlyphRole::Network
        }
        _ => ToolCategoryGlyphRole::Generic,
    }
}

pub fn tool_state_role_for_status(status: &str) -> ToolStateGlyphRole {
    match status {
        "completed" | "done" | "merged_after_failure" => ToolStateGlyphRole::Completed,
        "running" | "in_progress" => ToolStateGlyphRole::Running,
        "failed" | "error" | "upstream_exhausted" => ToolStateGlyphRole::Failed,
        "cancelled" | "canceled" => ToolStateGlyphRole::Cancelled,
        "waiting" | "queued" | "pending" => ToolStateGlyphRole::Waiting,
        _ => ToolStateGlyphRole::Detail,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphConfidence {
    Explicit,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphCapability {
    pub profile: GlyphProfile,
    pub confidence: GlyphConfidence,
    pub signals: Vec<&'static str>,
}

impl GlyphCapability {
    pub fn should_show_fallback_notice(&self) -> bool {
        self.profile != GlyphProfile::NerdFont && self.confidence == GlyphConfidence::Low
    }

    pub fn summary(&self) -> String {
        if self.signals.is_empty() {
            format!("{:?}/{:?}", self.profile, self.confidence)
        } else {
            format!(
                "{:?}/{:?}: {}",
                self.profile,
                self.confidence,
                self.signals.join(", ")
            )
        }
    }
}

pub fn glyphs() -> &'static GlyphSet {
    static GLYPHS: std::sync::OnceLock<GlyphSet> = std::sync::OnceLock::new();
    GLYPHS.get_or_init(|| glyph_capability().glyphs())
}

impl GlyphCapability {
    pub fn glyphs(&self) -> GlyphSet {
        match self.profile {
            GlyphProfile::Ascii => ASCII_GLYPHS,
            GlyphProfile::Unicode => UNICODE_GLYPHS,
            GlyphProfile::NerdFont => NERD_FONT_GLYPHS,
        }
    }
}

pub fn glyph_capability() -> &'static GlyphCapability {
    static CAPABILITY: std::sync::OnceLock<GlyphCapability> = std::sync::OnceLock::new();
    CAPABILITY.get_or_init(detect_glyph_capability)
}

fn detect_glyph_capability() -> GlyphCapability {
    let mut signals = Vec::new();

    if std::env::var_os("OMEGON_ASCII_GLYPHS").is_some() {
        signals.push("env:OMEGON_ASCII_GLYPHS");
        return GlyphCapability {
            profile: GlyphProfile::Ascii,
            confidence: GlyphConfidence::Explicit,
            signals,
        };
    }
    if std::env::var_os("NO_COLOR").is_some() {
        signals.push("env:NO_COLOR");
        return GlyphCapability {
            profile: GlyphProfile::Ascii,
            confidence: GlyphConfidence::Explicit,
            signals,
        };
    }
    if std::env::var_os("OMEGON_NERD_FONT").is_some() {
        signals.push("env:OMEGON_NERD_FONT");
        return GlyphCapability {
            profile: GlyphProfile::NerdFont,
            confidence: GlyphConfidence::Explicit,
            signals,
        };
    }

    let kitty = terminal_looks_like_kitty();
    if kitty {
        signals.push("terminal:kitty");
    }
    if terminal_font_env_mentions_nerd() {
        signals.push("env:font-mentions-nerd");
    }
    let installed = known_nerd_font_installed();
    if installed {
        signals.push("font:known-nerd-font-installed");
    }
    let kitty_config = kitty_config_mentions_nerd_font();
    if kitty_config {
        signals.push("kitty:config-mentions-nerd-font");
    }

    if kitty && (kitty_config || installed) {
        return GlyphCapability {
            profile: GlyphProfile::NerdFont,
            confidence: GlyphConfidence::High,
            signals,
        };
    }
    if terminal_font_env_mentions_nerd() && installed {
        return GlyphCapability {
            profile: GlyphProfile::NerdFont,
            confidence: GlyphConfidence::High,
            signals,
        };
    }
    if kitty || installed || terminal_font_env_mentions_nerd() {
        return GlyphCapability {
            profile: GlyphProfile::Unicode,
            confidence: GlyphConfidence::Medium,
            signals,
        };
    }

    GlyphCapability {
        profile: GlyphProfile::Unicode,
        confidence: GlyphConfidence::Low,
        signals,
    }
}

pub fn terminal_looks_nerd_font_compatible() -> bool {
    let capability = detect_glyph_capability();
    capability.profile == GlyphProfile::NerdFont
}

fn terminal_looks_like_kitty() -> bool {
    let term = std::env::var("TERM")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let program = std::env::var("TERM_PROGRAM")
        .unwrap_or_default()
        .to_ascii_lowercase();
    std::env::var_os("KITTY_WINDOW_ID").is_some()
        || term.contains("kitty")
        || program.contains("kitty")
}

fn terminal_font_env_mentions_nerd() -> bool {
    std::env::var("KITTY_FONT")
        .or_else(|_| std::env::var("OMEGON_TERMINAL_FONT"))
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains("nerd")
}

fn known_nerd_font_installed() -> bool {
    known_font_dirs()
        .iter()
        .any(|dir| path_tree_contains_nerd_font(dir, 0))
}

fn path_tree_contains_nerd_font(path: &std::path::Path, depth: usize) -> bool {
    if depth > 3 {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let file_name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if file_name.contains("nerdfont") || file_name.contains("nerd font") {
            return true;
        }
        entry.file_type().ok().is_some_and(|ty| ty.is_dir())
            && path_tree_contains_nerd_font(&entry.path(), depth + 1)
    })
}

fn known_font_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("Library/Fonts"));
        dirs.push(home.join(".local/share/fonts"));
        dirs.push(home.join(".fonts"));
    }
    dirs.push(std::path::PathBuf::from("/Library/Fonts"));
    dirs.push(std::path::PathBuf::from("/usr/local/share/fonts"));
    dirs.push(std::path::PathBuf::from("/usr/share/fonts"));
    dirs
}

fn kitty_config_mentions_nerd_font() -> bool {
    kitty_config_paths()
        .iter()
        .any(|path| kitty_config_file_mentions_nerd_font(path, 0))
}

fn kitty_config_paths() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    if let Ok(dir) = std::env::var("KITTY_CONFIG_DIRECTORY") {
        paths.push(std::path::PathBuf::from(dir).join("kitty.conf"));
    }
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".config/kitty/kitty.conf"));
    }
    paths
}

fn kitty_config_file_mentions_nerd_font(path: &std::path::Path, depth: usize) -> bool {
    if depth > 2 {
        return false;
    }
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    contents.lines().any(|line| {
        let trimmed = line.split('#').next().unwrap_or_default().trim();
        if trimmed.is_empty() {
            return false;
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.contains("nerd font") || lower.contains("symbols nerd") {
            return true;
        }
        if !trimmed.starts_with("include ") {
            return false;
        }
        let include = trimmed.trim_start_matches("include ").trim();
        let include_path = expand_kitty_include_path(path, include);
        kitty_config_file_mentions_nerd_font(&include_path, depth + 1)
    })
}

fn expand_kitty_include_path(base: &std::path::Path, include: &str) -> std::path::PathBuf {
    if let Some(rest) = include.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    let path = std::path::PathBuf::from(include);
    if path.is_absolute() {
        return path;
    }
    base.parent()
        .unwrap_or_else(|| std::path::Path::new(""))
        .join(path)
}

pub fn nerd_font_install_help_url() -> &'static str {
    "https://www.nerdfonts.com/font-downloads"
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn structured_tool_identity_maps_to_stable_glyph_roles() {
        let cargo = crate::surfaces::conversation::tool_visual_identity(
            "bash",
            Some("cargo test -p omegon"),
        );
        assert_eq!(
            tool_category_role_for_identity(&cargo),
            ToolCategoryGlyphRole::Shell
        );

        let unknown_shell =
            crate::surfaces::conversation::tool_visual_identity("bash", Some("python3 script.py"));
        assert_eq!(
            tool_category_role_for_identity(&unknown_shell),
            ToolCategoryGlyphRole::Shell
        );

        let codebase = crate::surfaces::conversation::tool_visual_identity("codebase_search", None);
        assert_eq!(
            tool_category_role_for_identity(&codebase),
            ToolCategoryGlyphRole::Search
        );

        let docs = crate::surfaces::conversation::tool_visual_identity("search_documents", None);
        assert_eq!(
            tool_category_role_for_identity(&docs),
            ToolCategoryGlyphRole::Search
        );

        let git = crate::surfaces::conversation::tool_visual_identity("commit", None);
        assert_eq!(
            tool_category_role_for_identity(&git),
            ToolCategoryGlyphRole::Git
        );
    }

    #[test]
    fn tool_name_and_status_helpers_are_decoupled_presentation_fallbacks() {
        assert_eq!(
            tool_category_role_for_name("bash"),
            ToolCategoryGlyphRole::Shell
        );
        assert_eq!(
            tool_category_role_for_name("codebase_search"),
            ToolCategoryGlyphRole::Search
        );
        assert_eq!(
            tool_category_role_for_name("memory_recall"),
            ToolCategoryGlyphRole::Memory
        );
        assert_eq!(
            tool_category_role_for_name("commit"),
            ToolCategoryGlyphRole::Git
        );
        assert_eq!(
            tool_category_role_for_name("unknown"),
            ToolCategoryGlyphRole::Generic
        );
        assert_eq!(
            tool_state_role_for_status("running"),
            ToolStateGlyphRole::Running
        );
        assert_eq!(
            tool_state_role_for_status("completed"),
            ToolStateGlyphRole::Completed
        );
        assert_eq!(
            tool_state_role_for_status("failed"),
            ToolStateGlyphRole::Failed
        );
    }

    #[test]
    fn kitty_config_probe_follows_simple_includes() {
        let temp =
            std::env::temp_dir().join(format!("omegon-kitty-config-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::write(
            temp.join("kitty.conf"),
            "include fonts.conf
",
        )
        .unwrap();
        std::fs::write(
            temp.join("fonts.conf"),
            "symbol_map U+E000-U+F8FF Symbols Nerd Font Mono
",
        )
        .unwrap();

        assert!(kitty_config_file_mentions_nerd_font(
            &temp.join("kitty.conf"),
            0
        ));
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn font_probe_recurses_into_font_subdirectories() {
        let temp =
            std::env::temp_dir().join(format!("omegon-font-probe-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(temp.join("nested/font-family")).unwrap();
        std::fs::write(
            temp.join("nested/font-family/SymbolsNerdFontMono-Regular.ttf"),
            b"",
        )
        .unwrap();

        assert!(path_tree_contains_nerd_font(&temp, 0));
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn kitty_config_probe_ignores_comments_and_expands_relative_includes() {
        let temp =
            std::env::temp_dir().join(format!("omegon-kitty-comment-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(temp.join("conf.d")).unwrap();
        std::fs::write(
            temp.join("kitty.conf"),
            "# font_family Fake Nerd Font
include conf.d/fonts.conf # inline comment
",
        )
        .unwrap();
        std::fs::write(
            temp.join("conf.d/fonts.conf"),
            "font_family Cascadia
",
        )
        .unwrap();

        assert!(!kitty_config_file_mentions_nerd_font(
            &temp.join("kitty.conf"),
            0
        ));
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn core_glyphs_are_non_empty_and_single_cell() {
        let glyphs = glyphs();
        let values = [
            glyphs.rule(RuleGlyphRole::Horizontal),
            glyphs.tool(ToolGlyphRole::Running),
            glyphs.tool(ToolGlyphRole::Completed),
            glyphs.tool(ToolGlyphRole::Failed),
            glyphs.tool(ToolGlyphRole::Detail),
            glyphs.tool_state(ToolStateGlyphRole::Running),
            glyphs.tool_state(ToolStateGlyphRole::Completed),
            glyphs.tool_state(ToolStateGlyphRole::Failed),
            glyphs.tool_state(ToolStateGlyphRole::Waiting),
            glyphs.tool_state(ToolStateGlyphRole::Cancelled),
            glyphs.tool_state(ToolStateGlyphRole::Detail),
            glyphs.tool_category(ToolCategoryGlyphRole::Shell),
            glyphs.tool_category(ToolCategoryGlyphRole::Read),
            glyphs.tool_category(ToolCategoryGlyphRole::Write),
            glyphs.tool_category(ToolCategoryGlyphRole::Search),
            glyphs.tool_category(ToolCategoryGlyphRole::Design),
            glyphs.tool_category(ToolCategoryGlyphRole::Memory),
            glyphs.tool_category(ToolCategoryGlyphRole::Network),
            glyphs.tool_category(ToolCategoryGlyphRole::Git),
            glyphs.tool_category(ToolCategoryGlyphRole::Generic),
        ];
        for value in values {
            assert!(!value.is_empty());
            assert_eq!(UnicodeWidthStr::width(value), 1, "{value:?}");
        }
    }
}
