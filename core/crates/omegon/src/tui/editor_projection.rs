//! Shared editor/input semantic projection types.
//!
//! These structs describe prompt input state without binding it to Ratatui
//! textarea rendering or keyboard handling.

use std::path::PathBuf;

use super::editor::EditorMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorProjection {
    pub mode: EditorModeProjection,
    pub text: String,
    pub is_empty: bool,
    pub cursor_position: usize,
    pub visual_line_count: usize,
    pub inline_tokens: Vec<EditorInlineTokenProjection>,
    pub kill_ring_present: bool,
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

pub fn project_editor_mode(mode: &EditorMode) -> EditorModeProjection {
    match mode {
        EditorMode::Normal => EditorModeProjection::Normal,
        EditorMode::ReverseSearch { query, match_idx } => EditorModeProjection::ReverseSearch {
            query: query.clone(),
            has_match: match_idx.is_some(),
        },
        EditorMode::SecretInput { label, buffer } => EditorModeProjection::SecretInput {
            label: label.clone(),
            masked_len: buffer.chars().count(),
        },
    }
}
