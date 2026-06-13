//! Renderer-neutral inline row composition primitives.
//!
//! Inline rows describe a single horizontal line as semantic left/right cell
//! groups. Renderers decide glyphs, colors, and exact truncation, but the
//! projection keeps right-side affordances first-class instead of burying them
//! in left-flow text.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineRow<T = String> {
    pub left: Vec<InlineCell<T>>,
    pub right: Vec<InlineCell<T>>,
    pub overflow: InlineOverflowPolicy,
}

impl<T> InlineRow<T> {
    pub fn new(left: Vec<InlineCell<T>>, right: Vec<InlineCell<T>>) -> Self {
        Self {
            left,
            right,
            overflow: InlineOverflowPolicy::PreserveRight,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineCell<T = String> {
    pub text: T,
    pub role: InlineCellRole,
    pub priority: InlinePriority,
}

impl<T> InlineCell<T> {
    pub fn new(text: T, role: InlineCellRole) -> Self {
        Self {
            text,
            role,
            priority: InlinePriority::Normal,
        }
    }

    pub fn with_priority(mut self, priority: InlinePriority) -> Self {
        self.priority = priority;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineCellRole {
    Label,
    Value,
    Metadata,
    Affordance,
    Separator,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InlinePriority {
    Low,
    Normal,
    High,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineOverflowPolicy {
    PreserveRight,
    DropRightWhenCrowded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineAffordance {
    Details,
    Expand,
    Open,
    Select,
}

impl InlineAffordance {
    pub fn label(self) -> &'static str {
        match self {
            Self::Details => "details",
            Self::Expand => "expand",
            Self::Open => "open",
            Self::Select => "select",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyChord {
    Ctrl(char),
    Enter,
    Esc,
    Tab,
    ShiftTab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionHint {
    pub action: InlineAffordance,
    pub key: KeyChord,
}

impl ActionHint {
    pub fn new(action: InlineAffordance, key: KeyChord) -> Self {
        Self { action, key }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_row_keeps_affordances_structured() {
        let row = InlineRow::new(
            vec![InlineCell::new("bash", InlineCellRole::Value)],
            vec![
                InlineCell::new("details", InlineCellRole::Affordance)
                    .with_priority(InlinePriority::Required),
            ],
        );
        assert_eq!(row.left[0].role, InlineCellRole::Value);
        assert_eq!(row.right[0].role, InlineCellRole::Affordance);
        assert_eq!(row.overflow, InlineOverflowPolicy::PreserveRight);
    }
}
