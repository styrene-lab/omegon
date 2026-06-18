//! Renderer-neutral command/menu palette projection.
//!
//! Palette projections describe compact action/object rows for slash-command
//! menus, text command responses, ACP/CLI discovery, and future interactive
//! overlays without binding the data to a specific renderer.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaletteProjection {
    pub title: String,
    pub summary: Option<String>,
    pub groups: Vec<PaletteGroupProjection>,
    pub footer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaletteGroupProjection {
    pub title: String,
    pub description: Option<String>,
    pub rows: Vec<PaletteRowProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaletteRowProjection {
    pub id: String,
    pub label: String,
    pub command: Option<String>,
    pub description: String,
    pub kind: PaletteRowKind,
    pub badges: Vec<PaletteBadgeProjection>,
    pub metadata: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaletteRowKind {
    Action,
    Object,
    Heading,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaletteBadgeTone {
    Neutral,
    Success,
    Warning,
    Danger,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaletteBadgeProjection {
    pub label: String,
    pub tone: PaletteBadgeTone,
}

impl PaletteProjection {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            summary: None,
            groups: Vec::new(),
            footer: None,
        }
    }

    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    pub fn with_group(mut self, group: PaletteGroupProjection) -> Self {
        self.groups.push(group);
        self
    }

    pub fn with_footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = Some(footer.into());
        self
    }

    pub fn render_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("## ");
        out.push_str(&self.title);
        out.push('\n');

        if let Some(summary) = self
            .summary
            .as_deref()
            .filter(|summary| !summary.is_empty())
        {
            out.push_str(summary);
            out.push_str("\n\n");
        } else {
            out.push('\n');
        }

        for group in &self.groups {
            out.push_str("### ");
            out.push_str(&group.title);
            out.push('\n');
            if let Some(description) = group
                .description
                .as_deref()
                .filter(|description| !description.is_empty())
            {
                out.push_str(description);
                out.push('\n');
            }
            for row in &group.rows {
                out.push_str("- ");
                out.push_str(&row.markdown_label());
                let details = row.markdown_details();
                if !details.is_empty() {
                    out.push_str(" — ");
                    out.push_str(&details);
                }
                out.push('\n');
            }
            out.push('\n');
        }

        if let Some(footer) = self.footer.as_deref().filter(|footer| !footer.is_empty()) {
            out.push_str(footer);
        } else if out.ends_with("\n\n") {
            out.pop();
        }
        out
    }
}

impl PaletteGroupProjection {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: None,
            rows: Vec::new(),
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_row(mut self, row: PaletteRowProjection) -> Self {
        self.rows.push(row);
        self
    }

    pub fn with_rows(mut self, rows: Vec<PaletteRowProjection>) -> Self {
        self.rows.extend(rows);
        self
    }
}

impl PaletteRowProjection {
    pub fn action(
        id: impl Into<String>,
        command: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        let command = command.into();
        Self {
            id: id.into(),
            label: command.clone(),
            command: Some(command),
            description: description.into(),
            kind: PaletteRowKind::Action,
            badges: Vec::new(),
            metadata: Vec::new(),
        }
    }

    pub fn object(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            command: None,
            description: String::new(),
            kind: PaletteRowKind::Object,
            badges: Vec::new(),
            metadata: Vec::new(),
        }
    }

    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn with_badge(mut self, label: impl Into<String>, tone: PaletteBadgeTone) -> Self {
        self.badges.push(PaletteBadgeProjection {
            label: label.into(),
            tone,
        });
        self
    }

    pub fn with_metadata(mut self, metadata: impl Into<String>) -> Self {
        let metadata = metadata.into();
        if !metadata.is_empty() {
            self.metadata.push(metadata);
        }
        self
    }

    fn markdown_label(&self) -> String {
        match self.kind {
            PaletteRowKind::Action => {
                format!("`{}`", self.command.as_deref().unwrap_or(&self.label))
            }
            PaletteRowKind::Object => format!("`{}`", self.label),
            PaletteRowKind::Heading => self.label.clone(),
        }
    }

    fn markdown_details(&self) -> String {
        let mut parts = Vec::new();
        parts.extend(self.badges.iter().map(|badge| badge.label.clone()));
        parts.extend(self.metadata.iter().cloned());
        if !self.description.is_empty() {
            parts.push(self.description.clone());
        }
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_renders_action_and_object_rows() {
        let projection = PaletteProjection::new("Demo")
            .with_summary("2 objects")
            .with_group(PaletteGroupProjection::new("Actions").with_row(
                PaletteRowProjection::action("open", "/demo open", "Open demo"),
            ))
            .with_group(
                PaletteGroupProjection::new("Objects")
                    .with_description("`name` · scope · state")
                    .with_row(
                        PaletteRowProjection::object("demo.alpha", "alpha")
                            .with_badge("project", PaletteBadgeTone::Info)
                            .with_badge("active", PaletteBadgeTone::Success)
                            .with_metadata("tags:example")
                            .with_description("Alpha row"),
                    ),
            )
            .with_footer("Use `/demo get <name>` for details.");

        let rendered = projection.render_markdown();

        assert!(rendered.starts_with("## Demo"));
        assert!(rendered.contains("### Actions"));
        assert!(rendered.contains("- `/demo open` — Open demo"));
        assert!(rendered.contains("`name` · scope · state"));
        assert!(rendered.contains("- `alpha` — project · active · tags:example · Alpha row"));
        assert!(rendered.ends_with("Use `/demo get <name>` for details."));
    }
}
