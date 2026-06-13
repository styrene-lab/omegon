//! Semantic glyph matrix for TUI chrome.
//!
//! Renderers ask for semantic glyph roles rather than hardcoding symbols. This
//! keeps visual policy replaceable without coupling independent surfaces.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleGlyphRole {
    Horizontal,
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
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolGlyphRole {
    Running,
    Completed,
    Failed,
    Detail,
}

#[derive(Debug, Clone, Copy)]
pub struct RuleGlyphMatrix {
    pub horizontal: &'static str,
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
pub struct GlyphSet {
    pub rule: RuleGlyphMatrix,
    pub tool: ToolGlyphMatrix,
    pub tool_state: ToolStateGlyphMatrix,
    pub tool_category: ToolCategoryGlyphMatrix,
}

pub const UNICODE_GLYPHS: GlyphSet = GlyphSet {
    rule: RuleGlyphMatrix { horizontal: "─" },
    tool: ToolGlyphMatrix {
        running: "◐",
        completed: "✓",
        failed: "✗",
        detail: "↵",
    },
    tool_state: ToolStateGlyphMatrix {
        running: "◐",
        completed: "✓",
        failed: "✗",
        waiting: "◌",
        cancelled: "⊘",
        detail: "↵",
    },
    tool_category: ToolCategoryGlyphMatrix {
        shell: "$",
        read: "◰",
        write: "✎",
        search: "⌕",
        design: "◇",
        memory: "◈",
        network: "⇄",
        generic: "•",
    },
};

impl GlyphSet {
    pub fn rule(self, role: RuleGlyphRole) -> &'static str {
        match role {
            RuleGlyphRole::Horizontal => self.rule.horizontal,
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
            ToolCategoryGlyphRole::Generic => self.tool_category.generic,
        }
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
        "write" | "edit" | "change" | "commit" => ToolCategoryGlyphRole::Write,
        "codebase_search" | "search_documents" | "web_search" | "browser_search" | "rg" => {
            ToolCategoryGlyphRole::Search
        }
        "design_tree" | "design_tree_update" | "create_drawing" | "create_d2_diagram" => {
            ToolCategoryGlyphRole::Design
        }
        "memory_store" | "memory_recall" | "memory_query" | "store_memory_fact" => {
            ToolCategoryGlyphRole::Memory
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

pub fn glyphs() -> &'static GlyphSet {
    &UNICODE_GLYPHS
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

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
            glyphs.tool_category(ToolCategoryGlyphRole::Generic),
        ];
        for value in values {
            assert!(!value.is_empty());
            assert_eq!(UnicodeWidthStr::width(value), 1, "{value:?}");
        }
    }
}
