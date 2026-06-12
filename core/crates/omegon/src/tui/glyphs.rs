//! Semantic glyph matrix for TUI chrome.
//!
//! Renderers ask for semantic glyph roles rather than hardcoding symbols. This
//! keeps visual policy replaceable without coupling independent surfaces.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleGlyphRole {
    Horizontal,
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
}

pub const UNICODE_GLYPHS: GlyphSet = GlyphSet {
    rule: RuleGlyphMatrix { horizontal: "─" },
    tool: ToolGlyphMatrix {
        running: "◐",
        completed: "✓",
        failed: "✗",
        detail: "↵",
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
}

pub fn glyphs() -> &'static GlyphSet {
    &UNICODE_GLYPHS
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn core_glyphs_are_non_empty_and_single_cell() {
        let glyphs = glyphs();
        let values = [
            glyphs.rule(RuleGlyphRole::Horizontal),
            glyphs.tool(ToolGlyphRole::Running),
            glyphs.tool(ToolGlyphRole::Completed),
            glyphs.tool(ToolGlyphRole::Failed),
            glyphs.tool(ToolGlyphRole::Detail),
        ];
        for value in values {
            assert!(!value.is_empty());
            assert_eq!(UnicodeWidthStr::width(value), 1, "{value:?}");
        }
    }
}
