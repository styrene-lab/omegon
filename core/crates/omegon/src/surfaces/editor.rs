//! Shared editor/input semantic projection types.
//!
//! These structs describe prompt input state without binding it to Ratatui
//! textarea rendering or keyboard handling.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorProjection {
    pub mode: EditorModeProjection,
    pub intent: EditorIntentProjection,
    pub text: String,
    pub is_empty: bool,
    pub cursor_position: usize,
    pub visual_line_count: usize,
    pub inline_tokens: Vec<EditorInlineTokenProjection>,
    pub kill_ring_present: bool,
}

/// Intent derived from the composer prefix. This is projected rather than stored
/// so renderer and transport surfaces cannot drift from the actual buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorIntentProjection {
    Prompt,
    SlashCommand,
    ShellCommand,
    Context,
    Memory,
}

impl EditorIntentProjection {
    pub fn from_text(text: &str) -> Self {
        match text.trim_start().chars().next() {
            Some('/') => Self::SlashCommand,
            Some('!') => Self::ShellCommand,
            Some('@') => Self::Context,
            Some('*') => Self::Memory,
            _ => Self::Prompt,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorModeProjection {
    Normal,
    ReverseSearch { query: String, has_match: bool },
    SecretInput { label: String, masked_len: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorInlineTokenProjection {
    Attachment { path: PathBuf },
    CollapsedPaste { byte_len: usize, line_count: usize },
}

pub trait ProjectEditorSurface {
    fn project_editor_surface(&self, content_width: u16) -> EditorProjection;
}
