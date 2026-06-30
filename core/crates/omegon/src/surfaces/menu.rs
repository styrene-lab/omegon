//! Renderer-neutral structured menu projections.
//!
//! Menus are command/list/action surfaces: richer than a prose command panel,
//! broader than a one-dimensional selector, and still independent of any TUI,
//! ACP, CLI, or web renderer.

use crate::surfaces::command_menu::{
    CommandAvailabilityProjection, CommandMenuProjection, CommandSafetyProjection,
};
use crate::surfaces::palette::{
    PaletteBadgeProjection, PaletteBadgeTone, PaletteProjection, PaletteRowKind,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuProjection {
    pub id: String,
    pub title: String,
    pub summary: Option<String>,
    pub tabs: Vec<MenuTabProjection>,
    pub actions: Vec<MenuActionProjection>,
    pub footer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuTabProjection {
    pub id: String,
    pub label: String,
    pub groups: Vec<MenuGroupProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuGroupProjection {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    pub rows: Vec<MenuRowProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuRowProjection {
    pub id: String,
    pub label: String,
    pub description: String,
    pub value: Option<String>,
    pub kind: MenuRowKind,
    pub badges: Vec<MenuBadgeProjection>,
    pub metadata: Vec<String>,
    pub primary_action: Option<MenuActionProjection>,
    pub actions: Vec<MenuActionProjection>,
    pub safety: Option<CommandSafetyProjection>,
    pub availability: Option<CommandAvailabilityProjection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuRowKind {
    Action,
    Object,
    Heading,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuBadgeProjection {
    pub label: String,
    pub tone: MenuBadgeTone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuBadgeTone {
    Neutral,
    Success,
    Warning,
    Danger,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuActionProjection {
    pub id: String,
    pub label: String,
    pub key: Option<String>,
    pub command: Option<String>,
    pub target_row_id: Option<String>,
    pub requires_confirmation: bool,
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStatusProjection {
    pub provider_id: String,
    pub display_name: String,
    pub credential_state: String,
    pub credential_available: bool,
    pub availability: ProviderAvailabilityProjection,
    pub remediation_command: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderAvailabilityProjection {
    Available,
    MissingCredentials,
    Unavailable,
}

impl ProviderStatusProjection {
    pub fn from_credential_probe(provider_id: &str) -> Self {
        let credential = crate::route::CredentialLedger.probe(provider_id);
        let display_name = crate::auth::provider_by_id(provider_id)
            .map(|provider| provider.display_name.to_string())
            .unwrap_or_else(|| provider_id.to_string());
        let credential_available = credential.is_valid();
        let availability = if credential_available {
            ProviderAvailabilityProjection::Available
        } else {
            ProviderAvailabilityProjection::MissingCredentials
        };
        let remediation_command = (!credential_available).then(|| format!("/auth login {provider_id}"));
        Self {
            provider_id: provider_id.to_string(),
            display_name,
            credential_state: credential.summary(),
            credential_available,
            availability,
            remediation_command,
        }
    }

    pub fn badge_label(&self) -> &'static str {
        match self.availability {
            ProviderAvailabilityProjection::Available => "valid",
            ProviderAvailabilityProjection::MissingCredentials => "missing",
            ProviderAvailabilityProjection::Unavailable => "unavailable",
        }
    }

    pub fn badge_tone(&self) -> MenuBadgeTone {
        match self.availability {
            ProviderAvailabilityProjection::Available => MenuBadgeTone::Success,
            ProviderAvailabilityProjection::MissingCredentials => MenuBadgeTone::Warning,
            ProviderAvailabilityProjection::Unavailable => MenuBadgeTone::Danger,
        }
    }
}

impl MenuProjection {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            summary: None,
            tabs: Vec::new(),
            actions: Vec::new(),
            footer: None,
        }
    }


    pub fn render_markdown(&self) -> String {
        let mut out = format!("## {}\n", self.title);
        if let Some(summary) = self.summary.as_deref().filter(|summary| !summary.is_empty()) {
            out.push_str("\n");
            out.push_str(summary);
            out.push_str("\n");
        }

        for tab in &self.tabs {
            if self.tabs.len() > 1 {
                out.push_str("\n### ");
                out.push_str(&tab.label);
                out.push_str("\n");
            }
            for group in &tab.groups {
                out.push_str("\n### ");
                out.push_str(&group.label);
                out.push_str("\n");
                if let Some(description) = group.description.as_deref().filter(|value| !value.is_empty()) {
                    out.push_str(description);
                    out.push_str("\n");
                }
                for row in &group.rows {
                    out.push_str("- `");
                    out.push_str(&row.label);
                    out.push('`');
                    if let Some(value) = row.value.as_deref().filter(|value| !value.is_empty()) {
                        out.push_str(" — ");
                        out.push_str(value);
                    }
                    let badges = row.badges.iter().map(|badge| badge.label.as_str());
                    let metadata = row.metadata.iter().map(String::as_str);
                    let extras: Vec<&str> = badges.chain(metadata).collect();
                    if !extras.is_empty() {
                        out.push_str(" · ");
                        out.push_str(&extras.join(" · "));
                    }
                    if let Some(command) = row.primary_action.as_ref().and_then(|action| action.command.as_deref()) {
                        out.push_str(" · Enter: `");
                        out.push_str(command);
                        out.push('`');
                    }
                    for action in &row.actions {
                        if let Some(command) = action.command.as_deref() {
                            out.push_str(" · ");
                            if let Some(key) = action.key.as_deref() {
                                out.push_str(key);
                                out.push_str(": ");
                            }
                            out.push('`');
                            out.push_str(command);
                            out.push('`');
                        }
                    }
                    if !row.description.is_empty() {
                        out.push_str("\n  ");
                        out.push_str(&row.description);
                    }
                    out.push_str("\n");
                }
            }
        }

        if let Some(footer) = self.footer.as_deref().filter(|footer| !footer.is_empty()) {
            out.push_str("\n");
            out.push_str(footer);
            out.push_str("\n");
        }
        out
    }

    pub fn from_palette(id: impl Into<String>, palette: PaletteProjection) -> Self {
        let id = id.into();
        let group = MenuGroupProjection {
            id: "main".into(),
            label: "Items".into(),
            description: None,
            rows: palette
                .groups
                .into_iter()
                .flat_map(|group| {
                    let group_label = group.title;
                    group.rows.into_iter().map(move |row| {
                        let primary_action = row.command.as_ref().map(|command| {
                            MenuActionProjection::command(row.id.clone(), row.label.clone(), command.clone())
                        });
                        MenuRowProjection {
                            id: row.id,
                            label: row.label,
                            description: row.description,
                            value: None,
                            kind: MenuRowKind::from(row.kind),
                            badges: row.badges.into_iter().map(MenuBadgeProjection::from).collect(),
                            metadata: row.metadata.into_iter().chain(std::iter::once(group_label.clone())).collect(),
                            primary_action,
                            actions: Vec::new(),
                            safety: None,
                            availability: None,
                        }
                    })
                })
                .collect(),
        };
        Self {
            id,
            title: palette.title,
            summary: palette.summary,
            tabs: vec![MenuTabProjection {
                id: "main".into(),
                label: "All".into(),
                groups: vec![group],
            }],
            actions: Vec::new(),
            footer: palette.footer,
        }
    }

    pub fn from_command_menu(id: impl Into<String>, title: impl Into<String>, menu: CommandMenuProjection) -> Self {
        let rows = menu
            .rows
            .into_iter()
            .map(|row| {
                let name = row.name;
                let command = row.command;
                let actions = row
                    .subcommands
                    .into_iter()
                    .map(|sub| {
                        MenuActionProjection::command(
                            sub.clone(),
                            sub.clone(),
                            format!("{name} {sub}"),
                        )
                    })
                    .collect();
                MenuRowProjection {
                    id: name.clone(),
                    label: command.clone(),
                    description: row.description,
                    value: Some(name),
                    kind: MenuRowKind::Action,
                    badges: row
                        .badges
                        .into_iter()
                        .map(|label| MenuBadgeProjection {
                            label,
                            tone: MenuBadgeTone::Neutral,
                        })
                        .collect(),
                    metadata: row.metadata,
                    primary_action: Some(MenuActionProjection::command(
                        command.clone(),
                        command.clone(),
                        command,
                    )),
                    actions,
                    safety: Some(row.safety),
                    availability: Some(row.availability),
                }
            })
            .collect();

        Self {
            id: id.into(),
            title: title.into(),
            summary: None,
            tabs: vec![MenuTabProjection {
                id: "commands".into(),
                label: "Commands".into(),
                groups: vec![MenuGroupProjection {
                    id: "commands".into(),
                    label: "Commands".into(),
                    description: None,
                    rows,
                }],
            }],
            actions: Vec::new(),
            footer: Some("↑/↓ navigate · Enter run · / filter · Esc close".into()),
        }
    }
}

impl MenuActionProjection {
    pub fn command(id: impl Into<String>, label: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            key: None,
            command: Some(command.into()),
            target_row_id: None,
            requires_confirmation: false,
        }
    }
}

impl From<PaletteRowKind> for MenuRowKind {
    fn from(value: PaletteRowKind) -> Self {
        match value {
            PaletteRowKind::Action => Self::Action,
            PaletteRowKind::Object => Self::Object,
            PaletteRowKind::Heading => Self::Heading,
        }
    }
}

impl From<PaletteBadgeTone> for MenuBadgeTone {
    fn from(value: PaletteBadgeTone) -> Self {
        match value {
            PaletteBadgeTone::Neutral => Self::Neutral,
            PaletteBadgeTone::Success => Self::Success,
            PaletteBadgeTone::Warning => Self::Warning,
            PaletteBadgeTone::Danger => Self::Danger,
            PaletteBadgeTone::Info => Self::Info,
        }
    }
}

impl From<PaletteBadgeProjection> for MenuBadgeProjection {
    fn from(value: PaletteBadgeProjection) -> Self {
        Self { label: value.label, tone: value.tone.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surfaces::command_menu::{
        CommandAvailabilityProjection, CommandMenuProjection, CommandMenuRowProjection,
        CommandMenuSource, CommandSafetyProjection,
    };
    use crate::surfaces::palette::{PaletteGroupProjection, PaletteRowProjection};

    #[test]
    fn palette_conversion_preserves_commands_badges_and_metadata() {
        let palette = PaletteProjection::new("Skills")
            .with_summary("summary")
            .with_group(
                PaletteGroupProjection::new("Installed").with_row(
                    PaletteRowProjection::object("skill.rust", "rust")
                        .with_command("/skills get rust")
                        .with_badge("project", PaletteBadgeTone::Info)
                        .with_metadata("activation=project"),
                ),
            )
            .with_footer("footer");

        let menu = MenuProjection::from_palette("skills", palette);
        let row = &menu.tabs[0].groups[0].rows[0];
        assert_eq!(menu.title, "Skills");
        assert_eq!(menu.summary.as_deref(), Some("summary"));
        assert_eq!(row.label, "rust");
        assert_eq!(row.primary_action.as_ref().unwrap().command.as_deref(), Some("/skills get rust"));
        assert_eq!(row.badges[0].label, "project");
        assert!(row.metadata.contains(&"activation=project".to_string()));
        assert!(row.metadata.contains(&"Installed".to_string()));
        assert_eq!(menu.footer.as_deref(), Some("footer"));
    }

    #[test]
    fn command_menu_conversion_preserves_safety_availability_and_subcommands() {
        let menu = CommandMenuProjection {
            rows: vec![CommandMenuRowProjection {
                name: "help".into(),
                command: "/help".into(),
                description: "show commands".into(),
                subcommands: vec!["skills".into(), "settings".into()],
                source: CommandMenuSource::Builtin,
                availability: CommandAvailabilityProjection {
                    tui: true,
                    cli: true,
                    acp: true,
                },
                safety: CommandSafetyProjection {
                    class: omegon_traits::CommandSafetyClass::ReadOnly,
                    requires_confirmation: false,
                    prompt_injection_sensitive: false,
                },
                badges: vec!["builtin".into(), "read".into(), "cli+acp".into()],
                metadata: vec!["registry".into()],
            }],
        };

        let projection = MenuProjection::from_command_menu("help", "Commands", menu);
        let row = &projection.tabs[0].groups[0].rows[0];

        assert_eq!(projection.id, "help");
        assert_eq!(projection.title, "Commands");
        assert_eq!(row.id, "help");
        assert_eq!(row.label, "/help");
        assert_eq!(row.value.as_deref(), Some("help"));
        assert_eq!(row.primary_action.as_ref().unwrap().command.as_deref(), Some("/help"));
        assert_eq!(row.actions.len(), 2);
        assert_eq!(row.actions[0].command.as_deref(), Some("help skills"));
        assert_eq!(row.safety.unwrap().class, omegon_traits::CommandSafetyClass::ReadOnly);
        assert!(row.availability.unwrap().cli);
        assert!(row.availability.unwrap().acp);
        assert!(row.metadata.contains(&"registry".to_string()));
        assert_eq!(row.badges[0].label, "builtin");
    }
}
